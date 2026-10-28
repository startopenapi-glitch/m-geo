//! P0 native boundary tests use only isolated fictional data.
use super::*;
fn fixture() -> (Connection, Value, Value) {
    let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    c.execute_batch(p0_workflow::TABLE_SQL).unwrap();
    let b = handle(&mut c, "brand.create", json!({"name":"星岸虚构品牌"})).unwrap();
    let p=handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构晨露杯","sku":"P0-1","facts":[{"id":"capacity","label":"容量","value":"500mL","source":"虚构资料","visibility":"public"}]})).unwrap();
    let p = handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
    )
    .unwrap();
    (c, b, p)
}
fn request(b: &Value, p: &Value) -> Value {
    json!({"brandId":b["id"],"productId":p["id"],"requestId":id(),"name":"虚构全链路演练","mode":"natural_discovery","question":"通勤怎样选择水杯？","targetUrl":"https://fiction.invalid/article","scenario":"success","maxUnits":20,"maxCalls":20,"transport":"fixture","evidenceMode":"simulated"})
}
fn call(c: &mut Connection, a: &str, p: Value) -> Result<Value> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    let out = p0_workflow::dispatch(&tx, p["brandId"].as_str().unwrap_or(""), a, &p)?;
    tx.commit().map_err(db_error)?;
    Ok(out)
}
fn mutate(c: &mut Connection, a: &str, t: &Value) -> Value {
    let mut p = json!({"brandId":t["brandId"],"taskId":t["id"],"expectedRevision":t["revision"]});
    if [
        "p0Workflow.approvePublish",
        "p0Workflow.reconcile",
        "p0Workflow.rollback",
    ]
    .contains(&a)
    {
        p["confirm"] = json!(true);
    }
    call(c, a, p).unwrap()
}
fn create(c: &mut Connection, b: &Value, p: &Value, scenario: &str) -> Value {
    let mut r = request(b, p);
    r["scenario"] = json!(scenario);
    call(c, "p0Workflow.create", r).unwrap()
}
fn reach_review(c: &mut Connection, t: &Value) -> Value {
    let mut t = mutate(c, "p0Workflow.resume", t);
    for _ in 0..4 {
        t = mutate(c, "p0Workflow.step", &t);
    }
    t
}
fn approve(c: &mut Connection, t: &Value) -> Value {
    handle(
        c,
        "draft.review",
        json!({"brandId":t["brandId"],"id":t["draftId"],"expectedRevision":1}),
    )
    .unwrap();
    let t = mutate(c, "p0Workflow.step", t);
    assert_eq!(t["status"], "awaiting_publish_approval");
    mutate(c, "p0Workflow.approvePublish", &t)
}
fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn tamper(c: &Connection, t: &Value) {
    c.execute(
        "UPDATE m_p0_workflows SET data=?1 WHERE id=?2",
        params![t.to_string(), t["id"].as_str()],
    )
    .unwrap();
}
#[test]
fn p0_workflow_creation_rejects_real_transport_and_cross_brand() {
    let (mut c, b, p) = fixture();
    let r = request(&b, &p);
    let t = call(&mut c, "p0Workflow.create", r.clone()).unwrap();
    assert_eq!(t["status"], "paused");
    assert_eq!(t["nodes"].as_array().unwrap().len(), 7);
    assert_eq!(call(&mut c, "p0Workflow.create", r).unwrap()["id"], t["id"]);
    for delta in [
        json!({"transport":"real"}),
        json!({"evidenceMode":"real"}),
        json!({"targetUrl":"https://example.com/a"}),
        json!({"maxUnits":0}),
        json!({"nodes":[]}),
    ] {
        let mut r = request(&b, &p);
        for (k, v) in delta.as_object().unwrap() {
            r[k] = v.clone();
        }
        assert!(call(&mut c, "p0Workflow.create", r).is_err());
    }
    let b2 = handle(&mut c, "brand.create", json!({"name":"另一个虚构品牌"})).unwrap();
    assert!(call(
        &mut c,
        "p0Workflow.get",
        json!({"brandId":b2["id"],"taskId":t["id"]})
    )
    .is_err());
    p0_workflow::validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_review_and_publish_approval_are_independent() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let mut t = reach_review(&mut c, &t);
    assert_eq!(t["status"], "awaiting_review");
    assert!(t["approval"].is_null());
    assert!(call(
        &mut c,
        "p0Workflow.approvePublish",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"],"confirm":true})
    )
    .is_err());
    assert_eq!(count(&c, "m_ai_preparations"), 0);
    t = approve(&mut c, &t);
    assert!(!t["approval"].is_null());
    for _ in 0..3 {
        t = mutate(&mut c, "p0Workflow.step", &t);
    }
    assert_eq!(t["status"], "completed");
    assert_eq!(t["budget"]["usedUnits"], 6);
    assert_eq!(t["budget"]["usedCalls"], 6);
    assert_eq!(count(&c, "m_drafts"), 1);
    assert_eq!(count(&c, "m_observations"), 0);
    assert_eq!(count(&c, "m_publications"), 0);
    assert_eq!(
        t["realEffects"],
        json!({"networkRequests":0,"modelCalls":0,"publications":0})
    );
    let out = mutate(&mut c, "p0Workflow.export", &t);
    let files = out["files"].as_array().unwrap();
    assert!(files.len() >= 3);
    assert!(files
        .iter()
        .any(|f| f["path"] == "p0-workflow/capabilities.json"));
    p0_workflow::validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_timeout_reconciles_original_operation_without_double_charge() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "publish_timeout");
    let t = reach_review(&mut c, &t);
    let t = approve(&mut c, &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "awaiting_reconcile");
    assert_eq!(t["budget"]["usedUnits"], 4);
    let receipt = t["remote"]["receipt"].clone();
    let ledger = t["ledger"].as_array().unwrap().len();
    assert!(call(
        &mut c,
        "p0Workflow.retry",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    let mut t = mutate(&mut c, "p0Workflow.reconcile", &t);
    assert_eq!(t["remote"]["receipt"], receipt);
    assert_eq!(t["ledger"].as_array().unwrap().len(), ledger);
    assert_eq!(t["budget"]["usedUnits"], 4);
    for _ in 0..2 {
        t = mutate(&mut c, "p0Workflow.step", &t);
    }
    assert_eq!(t["status"], "completed");
    let rolled = mutate(&mut c, "p0Workflow.rollback", &t);
    assert!(rolled["remote"]["published"].is_null());
    assert_eq!(rolled["budget"]["usedUnits"], 7);
    let again = mutate(&mut c, "p0Workflow.rollback", &rolled);
    assert_eq!(again, rolled);
    p0_workflow::validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_utc_workspace_caps_and_global_halt_stop_all_tasks() {
    let (mut c, b, p) = fixture();
    call(&mut c,"p0Workflow.policyUpdate",json!({"brandId":b["id"],"expectedRevision":1,"dailyMaxUnits":1,"dailyMaxCalls":1,"halted":false})).unwrap();
    let t = create(&mut c, &b, &p, "success");
    let t = mutate(&mut c, "p0Workflow.resume", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "budget_blocked");
    assert_eq!(count(&c, "m_drafts"), 0);
    let policy = call(&mut c, "p0Workflow.policyGet", json!({"brandId":b["id"]})).unwrap();
    assert_eq!(policy["usedUnits"], 1);
    assert_eq!(policy["usedCalls"], 1);
    let other = create(&mut c, &b, &p, "success");
    let other = mutate(&mut c, "p0Workflow.resume", &other);
    call(
        &mut c,
        "p0Workflow.haltAll",
        json!({"brandId":b["id"],"expectedRevision":policy["revision"]}),
    )
    .unwrap();
    assert_eq!(p0_workflow::tick(&mut c, false).unwrap(), 0);
    let stopped = call(
        &mut c,
        "p0Workflow.get",
        json!({"brandId":b["id"],"taskId":other["id"]}),
    )
    .unwrap();
    assert_eq!(stopped["status"], "paused");
    assert!(call(
        &mut c,
        "p0Workflow.resume",
        json!({"brandId":b["id"],"taskId":stopped["id"],"expectedRevision":stopped["revision"]})
    )
    .is_err());
}
#[test]
fn p0_workflow_stale_revisions_and_draft_changes_cannot_publish() {
    let (mut c, b, p) = fixture();
    let original = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &original);
    assert!(call(
        &mut c,
        "p0Workflow.step",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":original["revision"]})
    )
    .is_err());
    let t = approve(&mut c, &t);
    let mut d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    d["body"] = json!("已变更正文");
    d["revision"] = json!(3);
    d["reviewedAt"] = Value::Null;
    update(&c, "m_drafts", &d).unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "failed");
    assert!(t["remote"]["published"].is_null());
}
#[test]
fn p0_workflow_restore_rejects_budget_receipt_and_origin_forgery() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &t);
    let t = approve(&mut c, &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    p0_workflow::validate_database(&c).unwrap();
    for which in 0..7 {
        let mut bad = t.clone();
        match which {
            0 => bad["budget"]["usedUnits"] = json!(0),
            1 => bad["transport"] = json!("https"),
            2 => bad["realEffects"]["networkRequests"] = json!(1),
            3 => bad["remote"]["receipt"]["operationId"] = json!(id()),
            4 => bad["approval"]["draftRevision"] = json!(99),
            5 => bad["ledger"][0]["calls"] = json!(0),
            _ => bad["remote"]["published"]["fixture"]["html"] = json!("forged"),
        };
        tamper(&c, &bad);
        assert!(
            p0_workflow::validate_database(&c).is_err(),
            "mutation {which}"
        );
    }
    tamper(&c, &t);
    p0_workflow::validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_recovery_pauses_running_and_retry_wait() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "rate_limit_once");
    let t = mutate(&mut c, "p0Workflow.resume", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "retry_wait");
    assert_eq!(t["budget"]["usedUnits"], 0);
    assert_eq!(t["budget"]["usedCalls"], 1);
    assert!(call(
        &mut c,
        "p0Workflow.retry",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    assert_eq!(p0_workflow::tick(&mut c, true).unwrap(), 1);
    assert_eq!(p0_workflow::tick(&mut c, false).unwrap(), 0);
    p0_workflow::validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_readback_failure_cannot_be_reported_as_completed() {
    for scenario in ["readback_mismatch", "robots_blocked"] {
        let (mut c, b, p) = fixture();
        let t = create(&mut c, &b, &p, scenario);
        let t = reach_review(&mut c, &t);
        if scenario == "robots_blocked" {
            assert_eq!(t["status"], "failed");
            continue;
        }
        let t = approve(&mut c, &t);
        let t = mutate(&mut c, "p0Workflow.step", &t);
        let t = mutate(&mut c, "p0Workflow.step", &t);
        assert_eq!(t["status"], "failed");
        assert!(call(
            &mut c,
            "p0Workflow.export",
            json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
        )
        .is_err());
        p0_workflow::validate_database(&c).unwrap();
    }
}
#[test]
fn p0_workflow_public_dispatch_completed_backup_preserves_audit_and_evidence() {
    let (mut c, b, p) = fixture();
    let mut t = handle(&mut c, "p0Workflow.create", request(&b, &p)).unwrap();
    for a in [
        "p0Workflow.resume",
        "p0Workflow.step",
        "p0Workflow.step",
        "p0Workflow.step",
        "p0Workflow.step",
    ] {
        t = handle(
            &mut c,
            a,
            json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]}),
        )
        .unwrap();
    }
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":t["draftId"],"expectedRevision":1}),
    )
    .unwrap();
    t = handle(
        &mut c,
        "p0Workflow.step",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]}),
    )
    .unwrap();
    t = handle(
        &mut c,
        "p0Workflow.approvePublish",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"],"confirm":true}),
    )
    .unwrap();
    for _ in 0..3 {
        t = handle(
            &mut c,
            "p0Workflow.step",
            json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]}),
        )
        .unwrap();
    }
    assert_eq!(t["status"], "completed");
    validate_database(&c).unwrap();
    let approval_events:i64=c.query_row("SELECT count(*) FROM m_audit WHERE action='p0Workflow.approvePublish' AND object_id=?1",[t["id"].as_str()],|r|r.get(0)).unwrap();
    assert_eq!(approval_events, 1);
    let bytes = crate::backup::encrypt_snapshot(&c, "p0-fictional-roundtrip").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "p0-fictional-roundtrip").unwrap();
    let loaded = handle(
        &mut restored,
        "p0Workflow.get",
        json!({"brandId":b["id"],"taskId":t["id"]}),
    )
    .unwrap();
    assert_eq!(loaded, t);
    assert_eq!(
        mutate(&mut restored, "p0Workflow.export", &loaded),
        mutate(&mut c, "p0Workflow.export", &t)
    );
}
#[test]
fn p0_workflow_paused_step_and_halted_unknown_reconcile_never_publish_again() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "publish_timeout");
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "paused");
    assert_eq!(t["nodes"][0]["status"], "completed");
    assert_eq!(t["budget"]["usedUnits"], 1);
    let t = reach_review(&mut c, &t);
    let t = approve(&mut c, &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let policy = call(
        &mut c,
        "p0Workflow.haltAll",
        json!({"brandId":b["id"],"expectedRevision":1}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.reconcile", &t);
    assert_eq!(t["status"], "paused");
    assert_eq!(t["budget"]["usedUnits"], 4);
    assert_eq!(t["ledger"].as_array().unwrap().len(), 4);
    assert!(call(
        &mut c,
        "p0Workflow.step",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    call(&mut c,"p0Workflow.policyUpdate",json!({"brandId":b["id"],"expectedRevision":policy["revision"],"dailyMaxUnits":100,"dailyMaxCalls":100,"halted":false})).unwrap();
    assert_eq!(p0_workflow::tick(&mut c, false).unwrap(), 0);
    let t = mutate(&mut c, "p0Workflow.rollback", &t);
    assert_eq!(t["budget"]["usedUnits"], 5);
    assert!(t["remote"]["published"].is_null());
    let _ = call(
        &mut c,
        "p0Workflow.haltAll",
        json!({"brandId":b["id"],"expectedRevision":3}),
    )
    .unwrap();
    assert_eq!(mutate(&mut c, "p0Workflow.rollback", &t), t);
    validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_429_retry_reuses_operation_and_max_three_attempts_are_enforced() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "rate_limit_once");
    let t = mutate(&mut c, "p0Workflow.resume", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(p0_workflow::tick(&mut c, false).unwrap(), 0);
    let t = mutate(&mut c, "p0Workflow.retry", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["budget"]["usedUnits"], 1);
    assert_eq!(t["budget"]["usedCalls"], 2);
    assert_eq!(t["ledger"][0]["operationId"], t["ledger"][1]["operationId"]);
    assert_eq!(t["ledger"][1]["attempt"], 2);
    let t = create(&mut c, &b, &p, "robots_blocked");
    let mut t = reach_review(&mut c, &t);
    assert_eq!(t["status"], "failed");
    for _ in 0..2 {
        t = mutate(&mut c, "p0Workflow.retry", &t);
        t = mutate(&mut c, "p0Workflow.step", &t);
        assert_eq!(t["status"], "failed");
    }
    let entries = t["ledger"].as_array().unwrap();
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[2]["operationId"], entries[4]["operationId"]);
    assert_eq!(entries[4]["attempt"], 3);
    assert!(call(
        &mut c,
        "p0Workflow.retry",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    validate_database(&c).unwrap();
}
#[test]
fn p0_workflow_transaction_failure_rolls_back_candidate_and_budget_together() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = mutate(&mut c, "p0Workflow.resume", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    c.execute_batch("CREATE TEMP TRIGGER reject_p0_write BEFORE UPDATE ON m_p0_workflows BEGIN SELECT RAISE(ABORT,'fictional failure'); END;").unwrap();
    assert!(handle(
        &mut c,
        "p0Workflow.step",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    assert_eq!(count(&c, "m_drafts"), 0);
    assert_eq!(count(&c, "m_ai_preparations"), 0);
    let loaded = call(
        &mut c,
        "p0Workflow.get",
        json!({"brandId":b["id"],"taskId":t["id"]}),
    )
    .unwrap();
    assert_eq!(loaded, t);
}
#[test]
fn p0_workflow_task_caps_apply_before_creating_a_draft() {
    let (mut c, b, p) = fixture();
    let mut r = request(&b, &p);
    r["maxUnits"] = json!(1);
    r["maxCalls"] = json!(1);
    let t = call(&mut c, "p0Workflow.create", r).unwrap();
    let t = mutate(&mut c, "p0Workflow.resume", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "budget_blocked");
    assert_eq!(t["budget"]["usedUnits"], 1);
    assert_eq!(count(&c, "m_drafts"), 0);
    let bytes = crate::backup::encrypt_snapshot(&c, "p0-fictional-cap").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "p0-fictional-cap").unwrap();
    assert_eq!(
        call(
            &mut restored,
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]})
        )
        .unwrap()["status"],
        "budget_blocked"
    );
}
#[test]
fn p0_workflow_restore_rejects_generated_content_forgery_even_with_rehashed_ledger() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = mutate(&mut c, "p0Workflow.resume", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let mut t = mutate(&mut c, "p0Workflow.step", &t);
    t["nodes"][1]["result"]["body"] = json!("伪造的模拟内容");
    t["ledger"][1]["resultHash"] = json!(fingerprint(&t["nodes"][1]["result"]));
    tamper(&c, &t);
    assert!(p0_workflow::validate_database(&c).is_err());
}
#[test]
fn p0_workflow_restore_rejects_missing_keys_duplicate_keys_and_policy_injection() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    for key in [
        "approval",
        "ledger",
        "remote",
        "realEffects",
        "draftId",
        "error",
    ] {
        let mut bad = t.clone();
        bad.as_object_mut().unwrap().remove(key);
        tamper(&c, &bad);
        assert!(p0_workflow::validate_database(&c).is_err(), "missing {key}");
    }
    tamper(&c, &t);
    let duplicate = t.to_string().replacen('{', "{\"transport\":\"real\",", 1);
    c.execute(
        "UPDATE m_p0_workflows SET data=?1 WHERE id=?2",
        params![duplicate, t["id"].as_str()],
    )
    .unwrap();
    assert!(p0_workflow::validate_database(&c).is_err());
    tamper(&c, &t);
    c.execute("UPDATE m_p0_policy SET data=?1",[json!({"revision":1,"dailyMaxUnits":100,"dailyMaxCalls":100,"halted":false,"transport":"real"}).to_string()]).unwrap();
    assert!(p0_workflow::validate_database(&c).is_err());
}

#[test]
fn p0_workflow_stale_product_unknown_outcome_can_reconcile_and_rollback_without_republish() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "publish_timeout");
    let t = reach_review(&mut c, &t);
    let t = approve(&mut c, &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    handle(&mut c,"product.save",json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"],"name":p["name"],"sku":p["sku"],"facts":p["facts"]})).unwrap();
    let receipt = t["remote"]["receipt"].clone();
    let t = mutate(&mut c, "p0Workflow.reconcile", &t);
    assert_eq!(t["remote"]["receipt"], receipt);
    assert_eq!(t["budget"]["usedUnits"], 4);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "failed");
    assert_eq!(t["budget"]["usedUnits"], 4);
    let t = mutate(&mut c, "p0Workflow.rollback", &t);
    assert_eq!(t["status"], "cancelled");
    assert!(t["remote"]["published"].is_null());
    validate_database(&c).unwrap();
}

#[test]
fn p0_workflow_idle_tick_does_not_validate_inactive_evidence() {
    let (mut c, b, p) = fixture();
    let mut t = create(&mut c, &b, &p, "success");
    // Full validation remains mandatory at open/restore/read boundaries. The
    // idle scheduler only inspects runnable task identities, not report bodies.
    t["realEffects"]["modelCalls"] = json!(1);
    tamper(&c, &t);
    assert!(p0_workflow::validate_database(&c).is_err());
    assert_eq!(p0_workflow::tick(&mut c, false).unwrap(), 0);
    assert!(call(
        &mut c,
        "p0Workflow.get",
        json!({"brandId":b["id"],"taskId":t["id"]})
    )
    .is_err());
}

#[test]
fn p0_workflow_reviewed_edits_are_reaudited_and_publish_only_with_new_approval() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &t);
    let generated = t["nodes"][1]["result"].clone();
    let initial_preview = t["remote"]["preview"].clone();
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"虚构修订正文，适合通勤使用。","claims":[{"id":id(),"text":"虚构修订正文，适合通勤使用。","factId":"capacity"}]})).unwrap();
    assert_eq!(
        mutate(&mut c, "p0Workflow.step", &t)["status"],
        "awaiting_review"
    );
    let d = handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "awaiting_publish_approval");
    assert_eq!(t["nodes"][1]["result"], generated);
    assert_ne!(t["remote"]["preview"], initial_preview);
    assert!(t["remote"]["preview"]["fixture"]["html"]
        .as_str()
        .unwrap()
        .contains("虚构修订正文"));
    assert!(t["approval"].is_null());
    assert_eq!(t["budget"]["usedUnits"], 4);
    let mut t = mutate(&mut c, "p0Workflow.approvePublish", &t);
    assert_eq!(t["approval"]["draftRevision"], d["revision"]);
    assert_eq!(
        t["approval"]["previewHash"],
        t["remote"]["preview"]["contentHash"]
    );
    for _ in 0..3 {
        t = mutate(&mut c, "p0Workflow.step", &t);
    }
    assert_eq!(t["status"], "completed");
    assert_eq!(
        t["remote"]["published"]["contentHash"],
        t["remote"]["preview"]["contentHash"]
    );
    p0_workflow::validate_database(&c).unwrap();
    let bytes = crate::backup::encrypt_snapshot(&c, "edited-fictional").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "edited-fictional").unwrap();
    assert_eq!(
        call(
            &mut restored,
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]})
        )
        .unwrap(),
        t
    );
}

#[test]
fn p0_workflow_edited_review_audit_requires_budget_and_new_approval_stales_after_save() {
    let (mut c, b, p) = fixture();
    let mut req = request(&b, &p);
    req["maxUnits"] = json!(3);
    let t = call(&mut c, "p0Workflow.create", req).unwrap();
    let t = reach_review(&mut c, &t);
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"虚构改稿","claims":[{"id":id(),"text":"虚构改稿","factId":"capacity"}]})).unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "budget_blocked");
    assert_eq!(t["budget"]["usedUnits"], 3);
    assert!(t["approval"].is_null());
    assert!(call(
        &mut c,
        "p0Workflow.approvePublish",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"],"confirm":true})
    )
    .is_err());
    p0_workflow::validate_database(&c).unwrap();

    let t = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &t);
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"第一次虚构改稿","claims":[{"id":id(),"text":"第一次虚构改稿","factId":"capacity"}]})).unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.approvePublish", &t);
    handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"第二次虚构改稿","claims":[{"id":id(),"text":"第二次虚构改稿","factId":"capacity"}]})).unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "failed");
    assert!(t["remote"]["published"].is_null());
    p0_workflow::validate_database(&c).unwrap();
}

#[test]
fn p0_workflow_edited_review_rejects_preview_and_ledger_forgery_on_restore() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &t);
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"虚构改稿","claims":[{"id":id(),"text":"虚构改稿","factId":"capacity"}]})).unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.approvePublish", &t);
    for which in 0..4 {
        let mut bad = t.clone();
        match which {
            0 => bad["remote"]["preview"]["fixture"]["html"] = json!("伪造页面"),
            1 => bad["approval"]["previewHash"] = json!("0".repeat(64)),
            2 => bad["nodes"][3]["result"]["content"]["body"] = json!("伪造正文"),
            _ => {
                bad["ledger"].as_array_mut().unwrap().pop();
            }
        }
        tamper(&c, &bad);
        assert!(
            p0_workflow::validate_database(&c).is_err(),
            "mutation {which}"
        );
    }
    tamper(&c, &t);
    p0_workflow::validate_database(&c).unwrap();
}

#[test]
fn p0_workflow_edit_before_initial_audit_preserves_generation_and_requires_review() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "paused");
    let generated = t["nodes"][1]["result"].clone();
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"审计前虚构修订正文","claims":[{"id":id(),"text":"审计前虚构修订正文","factId":"capacity"}]})).unwrap();
    let other = handle(&mut c, "brand.create", json!({"name":"另一虚构品牌"})).unwrap();
    assert!(handle(
        &mut c,
        "draft.review",
        json!({"brandId":other["id"],"id":d["id"],"expectedRevision":d["revision"]})
    )
    .is_err());
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "paused");
    assert_eq!(t["nodes"][1]["result"], generated);
    assert_eq!(t["nodes"][2]["status"], "completed");
    assert_eq!(t["budget"]["usedUnits"], 3);
    let initial = t["remote"]["preview"].clone();
    assert!(!initial["fixture"]["html"]
        .as_str()
        .unwrap()
        .contains("审计前虚构修订正文"));
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "awaiting_review");
    assert!(t["approval"].is_null());
    assert!(call(
        &mut c,
        "p0Workflow.approvePublish",
        json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"],"confirm":true})
    )
    .is_err());
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "awaiting_publish_approval");
    assert_eq!(t["nodes"][1]["result"], generated);
    assert_ne!(t["remote"]["preview"], initial);
    assert!(t["remote"]["preview"]["fixture"]["html"]
        .as_str()
        .unwrap()
        .contains("审计前虚构修订正文"));
    assert_eq!(t["budget"]["usedUnits"], 4);
    assert_eq!(t["ledger"][3]["nodeId"], "review_audit");
    let t = mutate(&mut c, "p0Workflow.approvePublish", &t);
    assert_eq!(
        t["approval"]["previewHash"],
        t["remote"]["preview"]["contentHash"]
    );
    assert!(call(
        &mut c,
        "p0Workflow.step",
        json!({"brandId":other["id"],"taskId":t["id"],"expectedRevision":t["revision"]})
    )
    .is_err());
    let mut bad = t.clone();
    bad["remote"]["preview"]["fixture"]["html"] = json!("篡改预览");
    tamper(&c, &bad);
    assert!(p0_workflow::validate_database(&c).is_err());
    tamper(&c, &t);
    let mut t = t;
    for _ in 0..3 {
        t = mutate(&mut c, "p0Workflow.step", &t);
    }
    assert_eq!(t["status"], "completed");
    p0_workflow::validate_database(&c).unwrap();
}

#[test]
fn p0_workflow_edit_after_review_prompts_new_task_preserves_evidence_and_allows_restart() {
    let (mut c, b, p) = fixture();
    let t = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &t);
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "awaiting_publish_approval");
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"审核节点后再改稿","claims":[{"id":id(),"text":"审核节点后再改稿","factId":"capacity"}]})).unwrap();
    for reviewed in [false, true] {
        if reviewed {
            handle(
                &mut c,
                "draft.review",
                json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
            )
            .unwrap();
        }
        let err = call(&mut c,"p0Workflow.approvePublish",json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"],"confirm":true})).unwrap_err();
        assert!(
            err.contains("取消旧任务") && err.contains("新建演练"),
            "{err}"
        );
        let loaded = call(
            &mut c,
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]}),
        )
        .unwrap();
        assert_eq!(loaded, t);
    }
    let cancelled = mutate(&mut c, "p0Workflow.cancel", &t);
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["nodes"], t["nodes"]);
    assert_eq!(cancelled["ledger"], t["ledger"]);
    let t = create(&mut c, &b, &p, "success");
    let t = reach_review(&mut c, &t);
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c, "draft.save", json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"新任务审核前改稿","claims":[{"id":id(),"text":"新任务审核前改稿","factId":"capacity"}]})).unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "awaiting_publish_approval");
    let mut t = mutate(&mut c, "p0Workflow.approvePublish", &t);
    for _ in 0..3 {
        t = mutate(&mut c, "p0Workflow.step", &t);
    }
    assert_eq!(t["status"], "completed");
    p0_workflow::validate_database(&c).unwrap();
}

#[test]
fn p0_workflow_edit_before_audit_still_blocks_when_review_audit_exceeds_budget() {
    let (mut c, b, p) = fixture();
    let mut req = request(&b, &p);
    req["maxUnits"] = json!(3);
    let t = call(&mut c, "p0Workflow.create", req).unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let d = load(
        &c,
        "m_drafts",
        b["id"].as_str().unwrap(),
        t["draftId"].as_str().unwrap(),
    )
    .unwrap();
    let d = handle(&mut c,"draft.save",json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"提前修改的虚构正文","claims":[{"id":id(),"text":"提前修改的虚构正文","factId":"capacity"}]})).unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    let t = mutate(&mut c, "p0Workflow.step", &t);
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let t = mutate(&mut c, "p0Workflow.step", &t);
    assert_eq!(t["status"], "budget_blocked");
    assert_eq!(t["budget"]["usedUnits"], 3);
    assert_eq!(t["ledger"].as_array().unwrap().len(), 3);
    assert!(t["approval"].is_null());
    p0_workflow::validate_database(&c).unwrap();
}
