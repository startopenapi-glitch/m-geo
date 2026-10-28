//! Desktop-lifetime offline executor. It has no model, network or publishing transport.
use rusqlite::Connection;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, TryLockError,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

const P0_GAP_LIMIT: Duration = Duration::from_secs(2);

fn observe_gap(last: &mut SystemTime, now: SystemTime) -> bool {
    let gap = now
        .duration_since(*last)
        .map_or(true, |elapsed| elapsed > P0_GAP_LIMIT);
    *last = now;
    gap
}

pub(crate) struct OfflineWorkflowWorker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OfflineWorkflowWorker {
    pub(crate) fn start(db: Arc<Mutex<Connection>>) -> Result<Self, String> {
        Self::start_with_hook(db, || {})
    }

    fn start_with_hook<F: FnMut() + Send + 'static>(
        db: Arc<Mutex<Connection>>,
        mut hook: F,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("mgeo-offline-workflows".into())
            .spawn(move || {
                let mut last = SystemTime::now();
                let mut recovery_pending = false;
                while !stopping.load(Ordering::Acquire) {
                    hook();
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    recovery_pending |= observe_gap(&mut last, SystemTime::now());
                    match db.try_lock() {
                        Ok(mut db) => {
                            // Each task persists its result or failure in a transaction. No
                            // external transport is reachable from this executor.
                            let _ = crate::core::tick_offline_workflows(&mut db);
                            recovery_pending |= observe_gap(&mut last, SystemTime::now());
                            if stopping.load(Ordering::Acquire) {
                                break;
                            }
                            if recovery_pending {
                                if crate::core::recover_p0_workflows(&mut db).is_ok() {
                                    recovery_pending = false;
                                }
                            } else {
                                let _ = crate::core::tick_p0_workflows(&mut db);
                            }
                        }
                        Err(TryLockError::Poisoned(_)) => break,
                        Err(TryLockError::WouldBlock) => {}
                    }
                    thread::sleep(Duration::from_millis(250));
                }
            })
            .map_err(|_| "本地任务执行器未能启动".to_string())?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for OfflineWorkflowWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core, runtime};
    use serde_json::{json, Value};
    use std::time::{Duration, Instant};

    fn wait_status(
        db: &Arc<Mutex<Connection>>,
        brand: &Value,
        task: &Value,
        status: &str,
    ) -> Value {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let current = core::handle(
                &mut db.lock().unwrap(),
                "offlineWorkflow.get",
                json!({
                    "brandId":brand["id"], "taskId":task["id"]
                }),
            )
            .unwrap();
            if current["status"] == status {
                return current;
            }
            assert!(
                Instant::now() < deadline,
                "worker did not reach {status}: {}",
                current["status"]
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn p0_fixture() -> (Arc<Mutex<Connection>>, Value, Value) {
        p0_fixture_at(std::path::Path::new(":memory:"), "success")
    }

    fn p0_fixture_at(
        path: &std::path::Path,
        scenario: &str,
    ) -> (Arc<Mutex<Connection>>, Value, Value) {
        let mut c = runtime::open_database(path).unwrap();
        let b = core::handle(&mut c, "brand.create", json!({"name":"虚构中断验收"})).unwrap();
        let p = core::handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构水杯","sku":"WAKE-021","facts":[{"id":"capacity","label":"容量","value":"500mL","source":"虚构规格","visibility":"public"}]})).unwrap();
        let p = core::handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
        )
        .unwrap();
        let t = runtime::core_dispatch(&mut c,"p0Workflow.create",json!({"brandId":b["id"],"requestId":uuid::Uuid::new_v4().to_string(),"productId":p["id"],"name":"虚构中断","mode":"natural_discovery","question":"如何选择容量？","targetUrl":"https://fixture.invalid/article","scenario":scenario,"maxUnits":20,"maxCalls":20,"transport":"fixture","evidenceMode":"simulated"})).unwrap();
        let t = runtime::core_dispatch(
            &mut c,
            "p0Workflow.resume",
            json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]}),
        )
        .unwrap();
        (Arc::new(Mutex::new(c)), b, t)
    }

    #[test]
    fn suspended_worker_pauses_p0_before_advancing() {
        use std::sync::mpsc;
        let (db, b, t) = p0_fixture();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let mut first = true;
        let worker = OfflineWorkflowWorker::start_with_hook(db.clone(), move || {
            if first {
                first = false;
                ready_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            }
        })
        .unwrap();
        ready_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(2200));
        resume_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let current = runtime::core_dispatch(
            &mut db.lock().unwrap(),
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]}),
        )
        .unwrap();
        assert_eq!(current["status"], "paused");
        assert_eq!(current["budget"]["usedUnits"], 0);
        std::thread::sleep(Duration::from_millis(400));
        let unchanged = runtime::core_dispatch(
            &mut db.lock().unwrap(),
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]}),
        )
        .unwrap();
        assert_eq!(unchanged["budget"]["usedUnits"], 0);
        runtime::core_dispatch(
            &mut db.lock().unwrap(),
            "p0Workflow.resume",
            json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":unchanged["revision"]}),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current = runtime::core_dispatch(
                &mut db.lock().unwrap(),
                "p0Workflow.get",
                json!({"brandId":b["id"],"taskId":t["id"]}),
            )
            .unwrap();
            if current["budget"]["usedUnits"].as_i64().unwrap() > 0 {
                break;
            }
            assert!(Instant::now() < deadline, "explicit resume did not advance");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(worker);
    }

    #[test]
    fn suspended_worker_also_pauses_retry_wait() {
        use std::sync::mpsc;
        let (db, b, t) = p0_fixture_at(std::path::Path::new(":memory:"), "rate_limit_once");
        {
            let mut c = db.lock().unwrap();
            let limited = runtime::core_dispatch(
                &mut c,
                "p0Workflow.step",
                json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]}),
            )
            .unwrap();
            assert_eq!(limited["status"], "retry_wait");
        }
        let (ready_tx, ready_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let mut first = true;
        let worker = OfflineWorkflowWorker::start_with_hook(db.clone(), move || {
            if first {
                first = false;
                ready_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            }
        })
        .unwrap();
        ready_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(2200));
        resume_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current = runtime::core_dispatch(
                &mut db.lock().unwrap(),
                "p0Workflow.get",
                json!({"brandId":b["id"],"taskId":t["id"]}),
            )
            .unwrap();
            if current["status"] == "paused" {
                assert_eq!(current["budget"]["usedUnits"], 0);
                break;
            }
            assert!(Instant::now() < deadline, "retry_wait was not paused");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(worker);
    }

    #[test]
    fn wall_clock_gap_threshold_and_backwards_clock() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let mut last = start;
        assert!(!observe_gap(&mut last, start + Duration::from_millis(250)));
        assert!(!observe_gap(&mut last, start + Duration::from_millis(2250)));
        assert!(observe_gap(&mut last, start + Duration::from_millis(4251)));
        assert!(observe_gap(&mut last, start));
    }

    #[test]
    fn recovery_survives_mutex_contention_and_database_errors() {
        use std::sync::mpsc;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fictional.sqlite");
        let (db, b, t) = p0_fixture_at(&path, "success");
        db.lock().unwrap().busy_timeout(Duration::ZERO).unwrap();
        let blocker = Connection::open(&path).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let mut first = true;
        let worker = OfflineWorkflowWorker::start_with_hook(db.clone(), move || {
            if first {
                first = false;
                ready_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            }
        })
        .unwrap();
        ready_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(2200));
        let guard = db.lock().unwrap();
        resume_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        drop(guard);
        std::thread::sleep(Duration::from_millis(500));
        {
            let c = db.lock().unwrap();
            let data: String = c
                .query_row(
                    "SELECT data FROM m_p0_workflows WHERE id=?1",
                    [t["id"].as_str().unwrap()],
                    |row| row.get(0),
                )
                .unwrap();
            let current: Value = serde_json::from_str(&data).unwrap();
            assert_eq!(current["status"], "running");
            assert_eq!(current["budget"]["usedUnits"], 0);
        }
        blocker.execute_batch("ROLLBACK").unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current = runtime::core_dispatch(
                &mut db.lock().unwrap(),
                "p0Workflow.get",
                json!({"brandId":b["id"],"taskId":t["id"]}),
            )
            .unwrap();
            if current["status"] == "paused" {
                assert_eq!(current["budget"]["usedUnits"], 0);
                break;
            }
            assert!(Instant::now() < deadline, "recovery did not retry");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(worker);
    }

    #[test]
    fn stop_during_suspension_does_not_advance_task() {
        use std::sync::mpsc;
        let (db, b, t) = p0_fixture();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let worker = OfflineWorkflowWorker::start_with_hook(db.clone(), move || {
            ready_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
        })
        .unwrap();
        ready_rx.recv().unwrap();
        let stopped = worker.stop.clone();
        let stopper = std::thread::spawn(move || drop(worker));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !stopped.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "stop was not requested");
            std::thread::yield_now();
        }
        resume_tx.send(()).unwrap();
        stopper.join().unwrap();
        let current = runtime::core_dispatch(
            &mut db.lock().unwrap(),
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]}),
        )
        .unwrap();
        assert_eq!(current["status"], "running");
        assert_eq!(current["budget"]["usedUnits"], 0);
    }

    #[test]
    fn worker_advances_without_a_page_but_requires_native_human_review() {
        let mut c = runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let brand =
            core::handle(&mut c, "brand.create", json!({"name":"虚构自动执行验收"})).unwrap();
        let product = core::handle(&mut c, "product.save", json!({
            "brandId":brand["id"],"name":"虚构水杯","sku":"WORKER-FICTION-1",
            "facts":[{"id":"capacity","label":"容量","value":"500mL","source":"虚构测试规格表","visibility":"public","validUntil":null}]
        })).unwrap();
        let product = core::handle(&mut c, "product.confirm", json!({"brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]})).unwrap();
        let task = core::handle(&mut c, "offlineWorkflow.create", json!({
            "brandId":brand["id"],"requestId":uuid::Uuid::new_v4().to_string(),"name":"虚构原生 worker 验收",
            "productId":product["id"],"questions":["通勤如何选择容量？"],"repetitions":1,"maxSamples":2,"evidenceMode":"simulated"
        })).unwrap();
        core::handle(
            &mut c,
            "offlineWorkflow.resume",
            json!({"brandId":brand["id"],"taskId":task["id"],"expectedRevision":task["revision"]}),
        )
        .unwrap();
        let db = Arc::new(Mutex::new(c));
        let worker = OfflineWorkflowWorker::start(db.clone()).unwrap();
        let waiting = wait_status(&db, &brand, &task, "awaiting_review");
        {
            let mut c = db.lock().unwrap();
            let snapshot =
                core::handle(&mut c, "workspace.snapshot", json!({"brandId":brand["id"]})).unwrap();
            assert_eq!(snapshot["drafts"].as_array().unwrap().len(), 1);
            let draft = &snapshot["drafts"][0];
            assert!(draft["reviewedAt"].is_null());
            assert!(snapshot["observations"].as_array().unwrap().is_empty());
            assert!(snapshot["publications"].as_array().unwrap().is_empty());
            assert!(core::handle(&mut c,"offlineWorkflow.export",json!({"brandId":brand["id"],"taskId":task["id"],"expectedRevision":waiting["revision"]})).is_err());
            core::handle(&mut c,"draft.review",json!({"brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"]})).unwrap();
            core::handle(&mut c,"offlineWorkflow.resume",json!({"brandId":brand["id"],"taskId":task["id"],"expectedRevision":waiting["revision"]})).unwrap();
        }
        let complete = wait_status(&db, &brand, &task, "completed");
        assert_eq!(complete["budget"]["usedSamples"], 2);
        assert_eq!(complete["budget"]["modelCalls"], 0);
        assert_eq!(complete["budget"]["networkRequests"], 0);
        assert_eq!(complete["budget"]["publications"], 0);
        assert_eq!(complete["nodes"][4]["result"]["mentionDelta"], 0);
        drop(worker);
    }

    #[test]
    fn shutting_down_worker_while_database_is_locked_does_not_deadlock() {
        let db = Arc::new(Mutex::new(
            runtime::open_database(std::path::Path::new(":memory:")).unwrap(),
        ));
        let _guard = db.lock().unwrap();
        let worker = OfflineWorkflowWorker::start(db.clone()).unwrap();
        let started = Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
