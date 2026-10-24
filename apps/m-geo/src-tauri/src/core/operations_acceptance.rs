use super::*;
fn fixture() -> (Connection, String, Value) {
    let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let b = handle(&mut c, "brand.create", json!({"name":"虚构运营验收"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    handle(
        &mut c,
        "product.save",
        json!({"brandId":b,"name":"虚构商品","sku":"SIM-A","facts":[]}),
    )
    .unwrap();
    let content = handle(&mut c, "attribution.template", json!({"brandId":b})).unwrap()["exchange"]
        .to_string();
    let pre = handle(
        &mut c,
        "attribution.preview",
        json!({"brandId":b,"name":"虚构项目","content":content}),
    )
    .unwrap();
    let job=handle(&mut c,"localJobs.create",json!({"brandId":b,"name":"虚构队列","entries":[{"name":"虚构项目","content":content,"expectedHash":pre["contentHash"]}]})).unwrap();
    (c, b, job)
}
fn request(b: &str, j: &Value) -> Value {
    json!({"brandId":b,"jobId":j["id"],"expectedRevision":j["revision"]})
}
#[test]
fn phase6_restore_guard_requires_pause_but_allows_damaged_current_queue_recovery() {
    let (mut c, b, j) = fixture();
    assert!(crate::runtime::no_active_jobs(&c).is_ok());
    let ready = handle(&mut c, "localJobs.resume", request(&b, &j)).unwrap();
    assert!(crate::runtime::no_active_jobs(&c).is_err());
    handle(&mut c, "localJobs.pause", request(&b, &ready)).unwrap();
    assert!(crate::runtime::no_active_jobs(&c).is_ok());
    c.execute("UPDATE m_local_jobs SET data='broken'", [])
        .unwrap();
    assert!(crate::runtime::no_active_jobs(&c).is_ok());
}
#[test]
fn phase6_root_audit_ignore_rolls_back_entire_step() {
    let (mut c, b, j) = fixture();
    let ready = handle(&mut c, "localJobs.resume", request(&b, &j)).unwrap();
    let before: i64 = c
        .query_row("SELECT count(*) FROM m_audit", [], |r| r.get(0))
        .unwrap();
    c.execute_batch("CREATE TRIGGER reject_root_audit BEFORE INSERT ON m_audit WHEN NEW.action='localJobs.step' BEGIN SELECT RAISE(IGNORE); END;").unwrap();
    assert!(handle(&mut c, "localJobs.step", request(&b, &ready)).is_err());
    assert_eq!(
        handle(
            &mut c,
            "localJobs.get",
            json!({"brandId":b,"jobId":j["id"]})
        )
        .unwrap(),
        ready
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_attribution_datasets", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_audit", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
}

#[test]
fn phase6_fictional_csv_queue_reopen_backup_and_bulk_flow() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("fictional.sqlite3");
    let mut c = crate::runtime::open_database(&db_path).unwrap();
    let b = handle(
        &mut c,
        "brand.create",
        json!({"name":"第六阶段 · 虚构批量运营"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (sku, name) in [
        ("SIM-A", "虚构水壶"),
        ("SIM-B,XL", "虚构背包加大号"),
        ("SIM-C\n蓝色", "虚构蓝色台灯"),
    ] {
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"sku":sku,"name":name,"facts":[]}),
        )
        .unwrap();
    }
    let template = handle(&mut c, "attribution.csvTemplate", json!({"brandId":b})).unwrap();
    let mut entries = Vec::new();
    for index in 1..=3 {
        let mut metadata = template["metadata"].clone();
        metadata["sourceLabel"] = json!(format!("模拟独立来源{index}，不可相加"));
        let name = format!("虚构演练{index}");
        let mapped=handle(&mut c,"attribution.csvPrepare",json!({"brandId":b,"name":name,"csv":template["csv"],"delimiter":template["delimiter"],"mapping":template["mapping"],"metadata":metadata})).unwrap();
        assert_eq!(mapped["preview"]["counts"]["duplicates"], 1);
        entries.push(json!({"name":name,"content":mapped["content"],"expectedHash":mapped["preview"]["contentHash"]}));
    }
    let creation = json!({"brandId":b,"name":"三个独立模拟数据集","entries":entries});
    let mut j = handle(&mut c, "localJobs.create", creation.clone()).unwrap();
    j = handle(&mut c, "localJobs.resume", request(&b, &j)).unwrap();
    j = handle(&mut c, "localJobs.step", request(&b, &j)).unwrap();
    j = handle(&mut c, "localJobs.pause", request(&b, &j)).unwrap();
    let id = j["id"].clone();
    drop(c);
    let mut c = crate::runtime::open_database(&db_path).unwrap();
    let get = json!({"brandId":b,"jobId":id});
    assert_eq!(handle(&mut c, "localJobs.get", get.clone()).unwrap(), j);
    assert_eq!(j["entries"][0]["status"], "succeeded");
    assert_eq!(j["entries"][1]["status"], "pending");
    let password = "phase6-fictional-only";
    let backup = crate::backup::encrypt_snapshot(&c, password).unwrap();
    let changes = c.total_changes();
    let inspection = crate::backup::inspect_snapshot(&backup, password).unwrap();
    assert_eq!(
        inspection["counts"],
        json!({"brands":1,"products":3,"datasets":1,"localJobs":1,"optimizationPlans":0})
    );
    assert_eq!(c.total_changes(), changes);
    let restored_path = temp.path().join("restored.sqlite3");
    let mut restored = crate::runtime::open_database(&restored_path).unwrap();
    crate::backup::restore_snapshot(&mut restored, &backup, password).unwrap();
    assert_eq!(
        handle(&mut restored, "localJobs.get", get.clone()).unwrap(),
        j
    );
    assert_eq!(
        handle(&mut restored, "localJobs.create", creation).unwrap()["alreadyExists"],
        true
    );
    j = handle(&mut c, "localJobs.resume", request(&b, &j)).unwrap();
    let stale = request(&b, &j);
    j = handle(&mut c, "localJobs.step", stale.clone()).unwrap();
    assert!(handle(&mut c, "localJobs.step", stale).is_err());
    j = handle(&mut c, "localJobs.cancel", request(&b, &j)).unwrap();
    assert_eq!(j["entries"][2]["status"], "cancelled");
    assert_eq!(j["entries"][2]["attempts"], 0);
    let report_params = json!({"brandId":b,"datasetId":j["entries"][0]["datasetId"],"from":"2026-10-01T00:00:00Z","to":"2026-10-23T00:00:00Z","lookbackDays":7});
    let report = handle(&mut c, "attribution.report", report_params).unwrap();
    assert_eq!(report["summary"]["grossMinor"], 81000);
    assert_eq!(report["summary"]["refundMinor"], 22000);
    assert_eq!(report["summary"]["netMinor"], 59000);
    let export = handle(&mut c, "localJobs.export", get).unwrap();
    let job_zip = crate::files::site_zip(&export).unwrap();
    let report_zip = crate::files::site_zip(&report).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&job_zip)).unwrap();
    assert_eq!(archive.len(), 3);
    use std::io::Read;
    let mut exported = String::new();
    archive
        .by_name("job.json")
        .unwrap()
        .read_to_string(&mut exported)
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&exported).unwrap(), j);
    // Two independent 1,000-row inputs exercise queue throughput without relaxing
    // the per-dataset ceiling or adding overlapping attribution totals.
    let mut bulk = Vec::new();
    for batch in 0..2 {
        let visits=(0..500).map(|i|json!({"id":format!("V{i}"),"sessionId":format!("S{i}"),"sku":"SIM-A","occurredAt":"2026-10-10T00:00:00Z","source":"doubao","sourceEvidence":"tagged"})).collect::<Vec<_>>();
        let orders=(0..500).map(|i|json!({"id":format!("O{i}"),"sessionId":format!("S{i}"),"sku":"SIM-A","occurredAt":"2026-10-11T00:00:00Z","status":"paid","amountMinor":100})).collect::<Vec<_>>();
        let content=json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":format!("独立千行模拟批次{batch}"),"coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-10-23T00:00:00Z","currency":"CNY","visits":visits,"orders":orders,"refunds":[]}).to_string();
        let name = format!("千行模拟批次{batch}");
        let pre = handle(
            &mut c,
            "attribution.preview",
            json!({"brandId":b,"name":name,"content":content}),
        )
        .unwrap();
        bulk.push(json!({"name":name,"content":content,"expectedHash":pre["contentHash"]}));
    }
    let mut bj = handle(
        &mut c,
        "localJobs.create",
        json!({"brandId":b,"name":"两份千行模拟队列","entries":bulk}),
    )
    .unwrap();
    bj = handle(&mut c, "localJobs.resume", request(&b, &bj)).unwrap();
    for _ in 0..2 {
        bj = handle(&mut c, "localJobs.step", request(&b, &bj)).unwrap();
    }
    assert_eq!(bj["status"], "completed");
    for entry in bj["entries"].as_array().unwrap() {
        let r=handle(&mut c,"attribution.report",json!({"brandId":b,"datasetId":entry["datasetId"],"from":"2026-10-01T00:00:00Z","to":"2026-10-23T00:00:00Z","lookbackDays":7})).unwrap();
        assert_eq!(r["summary"]["visits"], 500);
        assert_eq!(r["summary"]["paidOrders"], 500);
        assert_eq!(r["summary"]["netMinor"], 50000);
    }
    validate_database(&c).unwrap();
    assert_eq!(
        handle(&mut c, "workspace.snapshot", json!({"brandId":b})).unwrap()["publications"],
        json!([])
    );
    if let Ok(path) = std::env::var("MGEO_PHASE6_OUTPUT") {
        assert!(path.contains("mgeo-e2e-phase6-"));
        let dir = std::path::Path::new(&path);
        std::fs::create_dir_all(dir).unwrap();
        let output = dir.join("fictional.sqlite3");
        assert!(!output.exists());
        let mut dest = Connection::open(&output).unwrap();
        rusqlite::backup::Backup::new(&c, &mut dest)
            .unwrap()
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .unwrap();
        std::fs::write(
            dir.join("simulated-input.csv"),
            template["csv"].as_str().unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("simulated-job.zip"), job_zip).unwrap();
        std::fs::write(dir.join("simulated-report.zip"), report_zip).unwrap();
        std::fs::write(dir.join("paused-workspace.mgeobak"), backup).unwrap();
        std::fs::write(dir.join("fixture-result.json"),serde_json::to_string_pretty(&json!({"passed":true,"brandId":b,"products":3,"csvDuplicates":1,"partialReopen":true,"pauseBackupRestore":true,"staleStepRejected":true,"job":j,"bulkJob":bj,"bulkRows":2000,"reportSummary":report["summary"],"inspection":inspection,"modelCalls":0,"publications":0})).unwrap()).unwrap();
    }
}

#[test]
fn phase6_actual_schema3_fictional_file_migration() {
    let Ok(source) = std::env::var("MGEO_PHASE6_LEGACY") else {
        return;
    };
    let prior =
        Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        prior
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("legacy-copy.sqlite3");
    let mut copy = Connection::open(&path).unwrap();
    rusqlite::backup::Backup::new(&prior, &mut copy)
        .unwrap()
        .run_to_completion(128, std::time::Duration::from_millis(1), None)
        .unwrap();
    drop(copy);
    let migrated = crate::runtime::open_database(&path).unwrap();
    let mut st=prior.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name<>'m_core_meta' ORDER BY name").unwrap();
    let tables = st
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for table in &tables {
        let sql = format!("SELECT * FROM \"{table}\" ORDER BY rowid");
        let rows = |c: &Connection| {
            let mut st = c.prepare(&sql).unwrap();
            let cols = st.column_count();
            st.query_map([], |r| {
                (0..cols)
                    .map(|i| r.get::<_, rusqlite::types::Value>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
        };
        assert_eq!(rows(&prior), rows(&migrated), "{table}");
    }
    assert_eq!(
        migrated
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        migrated
            .query_row("SELECT count(*) FROM m_local_jobs", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("before-optimization-v5-"))
            .count(),
        1
    );
    if let Ok(path) = std::env::var("MGEO_PHASE6_OUTPUT") {
        std::fs::write(std::path::Path::new(&path).join("legacy-file-migration.json"),serde_json::to_string_pretty(&json!({"passed":true,"sourceVersion":3,"targetVersion":5,"preservedTables":tables,"preUpgradeSnapshot":true,"sourceReadOnly":true})).unwrap()).unwrap();
    }
}
