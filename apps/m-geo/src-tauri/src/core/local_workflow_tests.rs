//! Fictional, isolated offline workflow boundary tests.
use super::*;
fn fixture() -> (Connection, Value, Value) {
    let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let b = handle(&mut c, "brand.create", json!({"name":"星岸虚构品牌"})).unwrap();
    let p=handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构晨露杯","sku":"OFF-1","facts":[{"id":"capacity","label":"容量","value":"500mL","source":"虚构产品资料","visibility":"public"}]})).unwrap();
    let p = handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
    )
    .unwrap();
    (c, b, p)
}
fn request(b: &Value, p: &Value) -> Value {
    json!({"brandId":b["id"],"productId":p["id"],"requestId":id(),"name":"虚构离线流程","questions":["通勤怎样选择水杯？"],"repetitions":1,"maxSamples":2,"evidenceMode":"simulated"})
}
fn mutate(c: &mut Connection, a: &str, t: &Value) -> Value {
    handle(
        c,
        a,
        json!({"brandId":t["brandId"],"taskId":t["id"],"expectedRevision":t["revision"]}),
    )
    .unwrap()
}
fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
#[test]
fn offline_workflow_create_is_paused_idempotent_and_validates_neutral_budget() {
    let (mut c, b, p) = fixture();
    let r = request(&b, &p);
    let t = handle(&mut c, "offlineWorkflow.create", r.clone()).unwrap();
    assert_eq!(t["status"], "paused");
    assert_eq!(t["nodes"].as_array().unwrap().len(), 5);
    assert_eq!(t["budget"]["networkRequests"], 0);
    let duplicate = handle(&mut c, "offlineWorkflow.create", r.clone()).unwrap();
    assert_eq!(t["id"], duplicate["id"]);
    assert_eq!(count(&c, "m_offline_workflows"), 1);
    for changes in [
        json!({"questions":["星岸虚构品牌好吗？"]}),
        json!({"questions":["虚构晨露杯怎样？"]}),
        json!({"maxSamples":1}),
        json!({"repetitions":4}),
        json!({"evidenceMode":"real"}),
        json!({"nodes":[]}),
    ] {
        let mut invalid = request(&b, &p);
        for (k, v) in changes.as_object().unwrap() {
            invalid[k] = v.clone();
        }
        assert!(handle(&mut c, "offlineWorkflow.create", invalid).is_err());
    }
    validate_database(&c).unwrap();
}
#[test]
fn offline_workflow_native_review_gate_and_zero_delta_export() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    for _ in 0..4 {
        t = mutate(&mut c, "offlineWorkflow.step", &t);
    }
    assert_eq!(t["status"], "awaiting_review");
    assert_eq!(count(&c, "m_drafts"), 1);
    assert_eq!(count(&c, "m_observations"), 0);
    assert_eq!(count(&c, "m_publications"), 0);
    assert!(handle(
        &mut c,
        "offlineWorkflow.export",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    let unchanged = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(unchanged, t);
    assert_eq!(count(&c, "m_drafts"), 1);
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":t["draftId"],"expectedRevision":1}),
    )
    .unwrap();
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(t["status"], "completed");
    assert_eq!(t["nodes"][4]["result"]["mentionDelta"], 0);
    assert_eq!(t["budget"]["usedSamples"], 2);
    let out = mutate(&mut c, "offlineWorkflow.export", &t);
    assert!(out["files"].as_array().unwrap().len() >= 3);
    validate_database(&c).unwrap();
}
#[test]
fn offline_workflow_pause_cancel_stale_revision_and_brand_isolation() {
    let (mut c, b, p) = fixture();
    let t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    let running = mutate(&mut c, "offlineWorkflow.resume", &t);
    assert!(handle(
        &mut c,
        "offlineWorkflow.step",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    let paused = mutate(&mut c, "offlineWorkflow.pause", &running);
    assert_eq!(paused["status"], "paused");
    let cancelled = mutate(&mut c, "offlineWorkflow.cancel", &paused);
    assert_eq!(cancelled["status"], "cancelled");
    assert!(handle(
        &mut c,
        "offlineWorkflow.resume",
        json!({"brandId":b["id"],"taskId":cancelled["id"],"expectedRevision":cancelled["revision"]})
    )
    .is_err());
    let b2 = handle(&mut c, "brand.create", json!({"name":"另一个虚构品牌"})).unwrap();
    assert!(handle(
        &mut c,
        "offlineWorkflow.get",
        json!({"brandId":b2["id"],"taskId":t["id"]})
    )
    .is_err());
    assert_eq!(
        handle(&mut c, "offlineWorkflow.list", json!({"brandId":b2["id"]})).unwrap()["tasks"],
        json!([])
    );
}
#[test]
fn offline_workflow_fact_changes_fail_without_duplicate_drafts() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    handle(&mut c,"product.save",json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"],"name":p["name"],"sku":p["sku"],"facts":p["facts"]})).unwrap();
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(t["status"], "failed");
    assert_eq!(count(&c, "m_drafts"), 1);
    assert!(handle(
        &mut c,
        "offlineWorkflow.retry",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
}
#[test]
fn offline_workflow_tick_and_reopen_recovery_preserve_single_candidate() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    assert_eq!(tick_offline_workflows(&mut c).unwrap(), 1);
    assert_eq!(tick_offline_workflows(&mut c).unwrap(), 1);
    assert_eq!(count(&c, "m_drafts"), 1);
    assert_eq!(recover_offline_workflows(&mut c).unwrap(), 1);
    assert_eq!(tick_offline_workflows(&mut c).unwrap(), 0);
    t = handle(
        &mut c,
        "offlineWorkflow.get",
        json!({"brandId":b["id"],"taskId":t["id"]}),
    )
    .unwrap();
    assert_eq!(t["status"], "paused");
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    let _ = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(count(&c, "m_drafts"), 1);
    validate_database(&c).unwrap();
}
#[test]
fn offline_workflow_schema9_backup_roundtrip_and_tamper_rejection() {
    let (mut c, b, p) = fixture();
    let t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    let bytes = crate::backup::encrypt_snapshot(&c, "offline-fictional-password").unwrap();
    let inspection = crate::backup::inspect_snapshot(&bytes, "offline-fictional-password").unwrap();
    assert_eq!(inspection["targetVersion"], 10);
    assert_eq!(inspection["counts"]["offlineWorkflows"], 1);
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "offline-fictional-password").unwrap();
    assert_eq!(count(&restored, "m_offline_workflows"), 1);
    for mutation in [
        json!({"status":"completed"}),
        json!({"budget":{"maxSamples":2,"usedSamples":0,"networkRequests":1,"modelCalls":0,"publications":0}}),
        json!({"draftId":id()}),
        json!({"secret":"forbidden"}),
    ] {
        let mut bad = t.clone();
        for (k, v) in mutation.as_object().unwrap() {
            bad[k] = v.clone();
        }
        update(&c, "m_offline_workflows", &bad).unwrap();
        assert!(validate_database(&c).is_err());
        assert!(crate::backup::encrypt_snapshot(&c, "offline-fictional-password").is_err());
        update(&c, "m_offline_workflows", &t).unwrap();
    }
    validate_database(&c).unwrap();
}
#[test]
fn offline_workflow_schema8_migration_snapshot_and_reopen_pause() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.sqlite3");
    let c = crate::runtime::open_database(&path).unwrap();
    c.execute_batch("DROP TABLE m_p0_workflows; DROP TABLE m_p0_policy; DROP TABLE m_offline_workflows; UPDATE m_core_meta SET schema_version=8; PRAGMA user_version=8;").unwrap();
    drop(c);
    let mut c = crate::runtime::open_database(&path).unwrap();
    assert_eq!(count(&c, "m_offline_workflows"), 0);
    let snapshots: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("before-offline-v9-")
        })
        .collect();
    assert_eq!(snapshots.len(), 1);
    let prior = Connection::open(snapshots[0].path()).unwrap();
    assert_eq!(
        prior
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    let old_backup = crate::backup::encrypt_snapshot(&prior, "fictional-legacy-eight").unwrap();
    let old_inspection =
        crate::backup::inspect_snapshot(&old_backup, "fictional-legacy-eight").unwrap();
    assert_eq!(old_inspection["sourceVersion"], 8);
    assert_eq!(old_inspection["targetVersion"], 10);
    assert_eq!(old_inspection["counts"]["offlineWorkflows"], 0);
    let b = handle(&mut c, "brand.create", json!({"name":"虚构重开品牌"})).unwrap();
    let p=handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构重开商品","sku":"REOPEN","facts":[{"id":"f","label":"材质","value":"玻璃","source":"虚构资料","visibility":"public"}]})).unwrap();
    let p = handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
    )
    .unwrap();
    let t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    mutate(&mut c, "offlineWorkflow.resume", &t);
    drop(c);
    let mut c = crate::runtime::open_database(&path).unwrap();
    let t = handle(
        &mut c,
        "offlineWorkflow.get",
        json!({"brandId":b["id"],"taskId":t["id"]}),
    )
    .unwrap();
    assert_eq!(t["status"], "paused");
}
#[test]
fn offline_workflow_alias_questions_and_cross_brand_products_are_rejected() {
    let (mut c, b, p) = fixture();
    handle(&mut c,"brand.profile.save",json!({"brandId":b["id"],"aliases":["Star Shore"],"officialUrl":"","manufacturer":"","category":"","description":""})).unwrap();
    let mut r = request(&b, &p);
    r["questions"] = json!(["is STAR SHORE good?"]);
    assert!(handle(&mut c, "offlineWorkflow.create", r).is_err());
    let b2 = handle(&mut c, "brand.create", json!({"name":"另一个虚构公司"})).unwrap();
    let mut r = request(&b2, &p);
    assert!(handle(&mut c, "offlineWorkflow.create", r.clone()).is_err());
    r["nodes"] = json!([{"kind":"command","command":"curl"}]);
    assert!(handle(&mut c, "offlineWorkflow.create", r).is_err());
    assert_eq!(count(&c, "m_offline_workflows"), 0);
}
#[test]
fn offline_workflow_restored_running_work_is_paused() {
    let (mut c, b, p) = fixture();
    let t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    mutate(&mut c, "offlineWorkflow.resume", &t);
    let bytes = crate::backup::encrypt_snapshot(&c, "fictional-running-backup").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-running-backup").unwrap();
    let t = handle(
        &mut restored,
        "offlineWorkflow.get",
        json!({"brandId":b["id"],"taskId":t["id"]}),
    )
    .unwrap();
    assert_eq!(t["status"], "paused");
    assert_eq!(tick_offline_workflows(&mut restored).unwrap(), 0);
}
#[test]
fn offline_workflow_restore_rejects_running_destination_without_mutation() {
    let (mut c, b, p) = fixture();
    let bytes = crate::backup::encrypt_snapshot(&c, "fictional-running-target").unwrap();
    let t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    mutate(&mut c, "offlineWorkflow.resume", &t);
    assert!(crate::backup::restore_snapshot(&mut c, &bytes, "fictional-running-target").is_err());
    assert_eq!(count(&c, "m_offline_workflows"), 1);
}
#[test]
fn offline_workflow_cannot_forge_review_or_export_edited_draft() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    for _ in 0..4 {
        t = mutate(&mut c, "offlineWorkflow.step", &t);
    }
    let mut forged = t.clone();
    forged["status"] = json!("completed");
    forged["nodes"][3]["status"] = json!("completed");
    forged["nodes"][3]["result"] =
        json!({"draftId":t["draftId"],"draftRevision":1,"reviewedAt":now()});
    forged["nodes"][4]["status"] = json!("completed");
    let mut result = forged["nodes"][0]["result"].clone();
    result["mentionDelta"] = json!(0);
    result["comparisonLabel"] = json!("模拟夹具");
    forged["nodes"][4]["result"] = result;
    forged["budget"]["usedSamples"] = json!(2);
    update(&c, "m_offline_workflows", &forged).unwrap();
    assert!(
        validate_database(&c).is_err(),
        "forged workflow review must fail backup validation"
    );
    assert!(handle(
        &mut c,
        "offlineWorkflow.export",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    update(&c, "m_offline_workflows", &t).unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":t["draftId"],"expectedRevision":1}),
    )
    .unwrap();
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    let draft = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    handle(&mut c,"draft.save",json!({"brandId":b["id"],"id":draft["id"],"expectedRevision":draft["revision"],"title":draft["title"],"body":draft["body"]})).unwrap();
    assert!(handle(
        &mut c,
        "offlineWorkflow.export",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
}
#[test]
fn offline_workflow_failed_node_retry_rolls_back_and_preserves_completed_nodes() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    let baseline = t["nodes"][0].clone();
    c.execute_batch("CREATE TRIGGER fixture_block_draft BEFORE INSERT ON m_drafts BEGIN SELECT RAISE(ABORT,'fictional storage failure'); END;").unwrap();
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(t["status"], "failed");
    assert_eq!(count(&c, "m_drafts"), 0);
    assert_eq!(t["budget"]["usedSamples"], 1);
    c.execute_batch("DROP TRIGGER fixture_block_draft;")
        .unwrap();
    t = mutate(&mut c, "offlineWorkflow.retry", &t);
    assert_eq!(t["status"], "paused");
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(t["nodes"][0], baseline);
    assert_eq!(count(&c, "m_drafts"), 1);
    validate_database(&c).unwrap();
}
#[test]
fn offline_workflow_worker_ticks_have_persistent_native_audit() {
    let (mut c, b, p) = fixture();
    let t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    mutate(&mut c, "offlineWorkflow.resume", &t);
    assert_eq!(tick_offline_workflows(&mut c).unwrap(), 1);
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM m_audit WHERE action='offlineWorkflow.step' AND object_id=?1",
            [t["id"].as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn offline_html_boundary_is_readonly_and_rejects_oversized_utf8() {
    let (mut c, b, _) = fixture();
    let before = c.total_changes();
    let result=crate::runtime::core_dispatch(&mut c,"offlineGeo.audit",json!({"brandId":b["id"],"html":"<title>虚构商品</title><h1>虚构商品</h1><p>虚构公开说明</p><script>fetch('https://example.invalid')</script>"})).unwrap();
    assert_eq!(result["scope"], "offline_html");
    assert_eq!(result["networkRequests"], 0);
    assert_eq!(c.total_changes(), before);
    assert!(crate::runtime::core_dispatch(
        &mut c,
        "offlineGeo.audit",
        json!({"brandId":b["id"],"html":"杯".repeat(90_000)})
    )
    .is_err());
    assert_eq!(c.total_changes(), before);
}

#[test]
fn offline_workflow_review_fix1_profile_edit_keeps_frozen_fixture_backup_and_reopen_valid() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    for _ in 0..4 {
        t = mutate(&mut c, "offlineWorkflow.step", &t);
    }
    let baseline = t["nodes"][0]["result"].clone();
    handle(&mut c, "brand.profile.save", json!({"brandId":b["id"],"aliases":["虚构新别名"],"officialUrl":"","manufacturer":"","category":"","description":""})).unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":t["draftId"],"expectedRevision":1}),
    )
    .unwrap();
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(t["status"], "completed");
    let get = json!({"brandId":b["id"],"taskId":t["id"]});
    assert_eq!(
        handle(&mut c, "offlineWorkflow.get", get.clone()).unwrap(),
        t
    );
    assert_eq!(t["nodes"][4]["result"]["samples"], baseline["samples"]);
    assert_eq!(t["nodes"][4]["result"]["mentionDelta"], 0);
    validate_database(&c).unwrap();
    let bytes = crate::backup::encrypt_snapshot(&c, "fictional-profile-roundtrip").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fictional-reopened.sqlite3");
    let mut target = crate::runtime::open_database(&path).unwrap();
    crate::backup::restore_snapshot(&mut target, &bytes, "fictional-profile-roundtrip").unwrap();
    drop(target);
    let mut reopened = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        handle(&mut reopened, "offlineWorkflow.get", get).unwrap(),
        t
    );
    validate_database(&reopened).unwrap();
}

#[test]
fn offline_workflow_review_fix1_rejects_non_fixture_citation_authorities() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    let get = json!({"brandId":b["id"],"taskId":t["id"]});
    for citation in [
        "https://example.invalid@real.example.com/path",
        "https://example.invalid.real.example.com/path",
        "https://example.com/path",
        "https://example.invalid:444/path",
        "https://example.invalid:443/path",
        "https://user:secret@example.invalid/path",
        "http://example.invalid/path",
    ] {
        let mut bad = t.clone();
        bad["nodes"][0]["result"]["samples"][0]["citations"][0] = json!(citation);
        update(&c, "m_offline_workflows", &bad).unwrap();
        assert!(
            handle(&mut c, "offlineWorkflow.get", get.clone()).is_err(),
            "accepted {citation}"
        );
        assert!(
            validate_database(&c).is_err(),
            "backup content validator accepted {citation}"
        );
        assert!(crate::backup::encrypt_snapshot(&c, "fictional-citation-rejection").is_err());
    }
    update(&c, "m_offline_workflows", &t).unwrap();
    assert_eq!(handle(&mut c, "offlineWorkflow.get", get).unwrap(), t);
    validate_database(&c).unwrap();
    crate::backup::encrypt_snapshot(&c, "fictional-citation-rejection").unwrap();
}

#[test]
fn offline_workflow_final_fix_repeat_review_is_idempotent_before_and_after_followup() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "offlineWorkflow.create", request(&b, &p)).unwrap();
    t = mutate(&mut c, "offlineWorkflow.resume", &t);
    for _ in 0..4 {
        t = mutate(&mut c, "offlineWorkflow.step", &t);
    }
    let review_request = json!({"brandId":b["id"],"id":t["draftId"],"expectedRevision":1});
    let first = handle(&mut c, "draft.review", review_request.clone()).unwrap();
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let repeated = handle(&mut c, "draft.review", review_request.clone()).unwrap();
    assert_eq!(
        repeated, first,
        "valid review of unchanged revision must preserve approval identity"
    );
    t = mutate(&mut c, "offlineWorkflow.step", &t);
    assert_eq!(t["status"], "completed");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        handle(&mut c, "draft.review", review_request).unwrap(),
        first
    );
    let get = json!({"brandId":b["id"],"taskId":t["id"]});
    assert_eq!(
        handle(&mut c, "offlineWorkflow.get", get.clone()).unwrap(),
        t
    );
    validate_database(&c).unwrap();
    let package = mutate(&mut c, "offlineWorkflow.export", &t);
    assert_eq!(package["files"].as_array().unwrap().len(), 4);
    let bytes = crate::backup::encrypt_snapshot(&c, "fictional-repeat-review").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("repeat-review.sqlite3");
    let mut target = crate::runtime::open_database(&path).unwrap();
    crate::backup::restore_snapshot(&mut target, &bytes, "fictional-repeat-review").unwrap();
    drop(target);
    let mut reopened = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        handle(&mut reopened, "offlineWorkflow.get", get).unwrap(),
        t
    );
    validate_database(&reopened).unwrap();
}
