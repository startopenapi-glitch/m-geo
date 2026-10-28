use super::*;

fn database() -> (Connection, String) {
    let mut connection = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    connection.execute_batch(automation::TABLE_SQL).unwrap();
    let brand = handle(
        &mut connection,
        "brand.create",
        json!({"name":"虚构巡检品牌"}),
    )
    .unwrap();
    (connection, brand["id"].as_str().unwrap().to_owned())
}

fn nodes() -> Value {
    json!([
        {"id":"facts","kind":"fact_expiry_scan","dependsOn":[]},
        {"id":"readback","kind":"publication_readback_due","dependsOn":["facts"]},
        {"id":"retest","kind":"visibility_retest_reminder","dependsOn":["facts"]}
    ])
}

fn request(brand_id: &str) -> Value {
    json!({"brandId":brand_id,"requestId":id(),"name":"虚构每日只读巡检","intervalMinutes":15,"nodes":nodes()})
}

fn create_task(c: &Connection, brand_id: &str) -> Value {
    automation::create(c, brand_id, &request(brand_id)).unwrap()
}

fn at(task: &Value, key: &str, extra_minutes: i64) -> String {
    (DateTime::parse_from_rfc3339(task[key].as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::minutes(extra_minutes))
    .to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn reload(c: &Connection, brand_id: &str, task: &Value) -> Value {
    automation::list(c, brand_id, &json!({"brandId":brand_id})).unwrap()["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == task["id"])
        .unwrap()
        .clone()
}

fn mutation_payload(task: &Value) -> Value {
    json!({"brandId":task["brandId"],"taskId":task["id"],"expectedRevision":task["revision"]})
}

fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn rich_fixture(c: &mut Connection, brand_id: &str) {
    let product = handle(c, "product.save", json!({
        "brandId":brand_id,"name":"虚构保温杯","sku":"AUTO-1","facts":[
            {"id":"expired","label":"虚构容量","value":"500mL","source":"虚构资料","visibility":"public","validUntil":"2999-01-01"},
            {"id":"future","label":"虚构保修","value":"一年","source":"虚构资料","visibility":"public","validUntil":"2999-01-01"}
        ]
    })).unwrap();
    let question = handle(c, "question.save", json!({
        "brandId":brand_id,"productId":product["id"],"text":"通勤怎样选择保温杯？","intent":"selection",
        "origin":"customer","source":"虚构测试","branded":false,"confirmed":true
    })).unwrap();
    handle(c, "campaign.create", json!({
        "brandId":brand_id,"name":"虚构模拟复测计划","questionIds":[question["id"]],"platforms":["doubao"],
        "modelLabel":"虚构本地模型","searchMode":"无请求","context":"隔离测试","repetitions":1,"evidenceMode":"simulated"
    })).unwrap();
    let product = handle(
        c,
        "product.confirm",
        json!({
            "brandId":brand_id,"id":product["id"],"expectedRevision":product["revision"]
        }),
    )
    .unwrap();
    let draft = handle(
        c,
        "draft.generateLocal",
        json!({
            "brandId":brand_id,"productId":product["id"],"kind":"description"
        }),
    )
    .unwrap();
    let draft = handle(
        c,
        "draft.review",
        json!({
            "brandId":brand_id,"id":draft["id"],"expectedRevision":draft["revision"]
        }),
    )
    .unwrap();
    handle(c, "publication.add", json!({
        "brandId":brand_id,"draftId":draft["id"],"expectedRevision":draft["revision"],"channel":"虚构登记",
        "url":"https://example.com/readonly-fixture","publishedAt":"2020-01-01T00:00:00Z","note":"仅虚构登记，未真实发布"
    })).unwrap();
    // Expire the fact only after the separate publication fixture has been reviewed and recorded.
    let product = handle(c, "product.save", json!({
        "brandId":brand_id,"id":product["id"],"expectedRevision":product["revision"],"name":"虚构保温杯","sku":"AUTO-1","facts":[
            {"id":"expired","label":"虚构容量","value":"500mL","source":"虚构资料","visibility":"public","validUntil":"2020-01-01"},
            {"id":"future","label":"虚构保修","value":"一年","source":"虚构资料","visibility":"public","validUntil":"2999-01-01"}
        ]
    })).unwrap();
    assert_eq!(product["facts"][0]["reviewStatus"], "pending");
    // Explicitly review the edited expiry; a changed fact must not approve itself.
    handle(c, "product.save", json!({
        "brandId":brand_id,"id":product["id"],"expectedRevision":product["revision"],"name":"虚构保温杯","sku":"AUTO-1","facts":[
            {"id":"expired","label":"虚构容量","value":"500mL","source":"虚构资料","visibility":"public","validUntil":"2020-01-01","reviewStatus":"approved"},
            {"id":"future","label":"虚构保修","value":"一年","source":"虚构资料","visibility":"public","validUntil":"2999-01-01"}
        ]
    })).unwrap();
}

#[test]
fn create_is_idempotent_and_rejects_invalid_dags_and_limits() {
    let (c, brand_id) = database();
    let input = request(&brand_id);
    let first = automation::create(&c, &brand_id, &input).unwrap();
    assert_eq!(first["alreadyExists"], false);
    assert_eq!(first["status"], "active");
    assert_eq!(first["nextRunAt"], at(&first, "createdAt", 15));
    let replay = automation::create(&c, &brand_id, &input).unwrap();
    assert_eq!(replay["id"], first["id"]);
    assert_eq!(replay["alreadyExists"], true);
    assert_eq!(count(&c, "m_automation_tasks"), 1);
    let mut conflicting = input.clone();
    conflicting["name"] = json!("不同内容");
    assert!(automation::create(&c, &brand_id, &conflicting)
        .unwrap_err()
        .contains("请求标识"));

    for bad in [json!([]), json!(vec![nodes()[0].clone(); 9])] {
        let mut invalid = request(&brand_id);
        invalid["nodes"] = bad;
        assert!(automation::create(&c, &brand_id, &invalid).is_err());
    }
    for kind in [
        "publish",
        "ai.generate",
        "shell.exec",
        "sql.query",
        "url.fetch",
    ] {
        let mut invalid = request(&brand_id);
        invalid["nodes"] = json!([{"id":"forbidden","kind":kind,"dependsOn":[]}]);
        assert!(
            automation::create(&c, &brand_id, &invalid).is_err(),
            "accepted {kind}"
        );
    }
    let mut cycle = request(&brand_id);
    cycle["nodes"] = json!([
        {"id":"a","kind":"fact_expiry_scan","dependsOn":["b"]},
        {"id":"b","kind":"visibility_retest_reminder","dependsOn":["a"]}
    ]);
    assert!(automation::create(&c, &brand_id, &cycle)
        .unwrap_err()
        .contains("无环"));
    let mut duplicate = request(&brand_id);
    duplicate["nodes"][0]["dependsOn"] = json!(["readback", "readback"]);
    assert!(automation::create(&c, &brand_id, &duplicate).is_err());
    let mut missing = request(&brand_id);
    missing["nodes"][0]["dependsOn"] = json!(["missing"]);
    assert!(automation::create(&c, &brand_id, &missing).is_err());
    let mut duplicate_id = request(&brand_id);
    duplicate_id["nodes"][1]["id"] = json!("facts");
    assert!(automation::create(&c, &brand_id, &duplicate_id).is_err());
    for interval in [0, 1, 14, 525_601] {
        let mut invalid = request(&brand_id);
        invalid["intervalMinutes"] = json!(interval);
        assert!(automation::create(&c, &brand_id, &invalid).is_err());
    }
    assert!(automation::create(&c, &brand_id, &json!({"brandId":brand_id})).is_err());
    assert_eq!(count(&c, "m_automation_tasks"), 1);
}

#[test]
fn due_run_generates_only_local_reminders_and_never_fetches_or_publishes() {
    let (mut c, brand_id) = database();
    rich_fixture(&mut c, &brand_id);
    let task = create_task(&c, &brand_id);
    let before = json!({
        "products":count(&c,"m_products"),"drafts":count(&c,"m_drafts"),
        "publications":count(&c,"m_publications"),"observations":count(&c,"m_observations"),
        "ai":count(&c,"m_ai_preparations"),"audit":count(&c,"m_audit")
    });
    let publication_before: String = c
        .query_row("SELECT data FROM m_publications", [], |row| row.get(0))
        .unwrap();
    let output = automation::run_due_at(
        &c,
        &brand_id,
        Some(task["id"].as_str().unwrap()),
        &at(&task, "nextRunAt", 0),
    )
    .unwrap();
    assert_eq!(output["reports"].as_array().unwrap().len(), 1);
    for key in [
        "modelCalls",
        "modelFeeMinor",
        "networkRequests",
        "publicationWrites",
    ] {
        assert_eq!(output[key], 0);
        assert_eq!(output["reports"][0][key], 0);
    }
    let report_nodes = output["reports"][0]["nodes"].as_array().unwrap();
    assert_eq!(
        report_nodes
            .iter()
            .map(|node| node["nodeId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["facts", "readback", "retest"]
    );
    assert!(report_nodes[0]["reminders"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "fact_expired"));
    assert!(report_nodes[1]["reminders"]
        .as_array()
        .unwrap()
        .iter()
        .all(
            |item| item["kind"] == "publication_readback_due" && item["refs"].get("url").is_none()
        ));
    assert!(report_nodes[1]["reminders"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("没有请求 URL"));
    assert_eq!(
        report_nodes[2]["reminders"][0]["refs"]["expectedSamples"],
        2
    );
    assert_eq!(
        report_nodes[2]["reminders"][0]["refs"]["evidenceMode"],
        "simulated"
    );
    assert!(!output.to_string().contains("example.com"));
    let after = json!({
        "products":count(&c,"m_products"),"drafts":count(&c,"m_drafts"),
        "publications":count(&c,"m_publications"),"observations":count(&c,"m_observations"),
        "ai":count(&c,"m_ai_preparations"),"audit":count(&c,"m_audit")
    });
    assert_eq!(before, after);
    let publication_after: String = c
        .query_row("SELECT data FROM m_publications", [], |row| row.get(0))
        .unwrap();
    assert_eq!(publication_before, publication_after);
    automation::validate_database(&c).unwrap();
}

#[test]
fn missed_runs_are_counted_once_without_catchup_storm() {
    let (c, brand_id) = database();
    let mut task = create_task(&c, &brand_id);
    let late = at(&task, "nextRunAt", 155);
    let first = automation::run_due_at(&c, &brand_id, None, &late).unwrap();
    assert_eq!(first["reports"].as_array().unwrap().len(), 1);
    task = first["tasks"][0].clone();
    assert_eq!(task["totalRuns"], 1);
    assert_eq!(task["missedRuns"], 10);
    assert_eq!(task["nextRunAt"], at(&task, "lastRunAt", 15));
    let repeated = automation::run_due_at(&c, &brand_id, None, &late).unwrap();
    assert_eq!(repeated["reports"].as_array().unwrap().len(), 0);
    assert_eq!(repeated["skipped"], 1);
    assert_eq!(reload(&c, &brand_id, &task), task);
    for _ in 0..5 {
        let next = at(&task, "nextRunAt", 0);
        task = automation::run_due_at(&c, &brand_id, Some(task["id"].as_str().unwrap()), &next)
            .unwrap()["tasks"][0]
            .clone();
    }
    assert_eq!(task["totalRuns"], 6);
    assert_eq!(task["reports"].as_array().unwrap().len(), 5);
    assert_eq!(task["lastRunAt"], task["reports"][4]["ranAt"]);
    automation::validate_database(&c).unwrap();
}

#[test]
fn pause_resume_cancel_are_scoped_versioned_and_do_not_run() {
    let (c, brand_id) = database();
    let mut task = create_task(&c, &brand_id);
    let before_run =
        automation::run_due_at(&c, &brand_id, None, &at(&task, "nextRunAt", 1000)).unwrap();
    assert_eq!(before_run["reports"].as_array().unwrap().len(), 1);
    task = before_run["tasks"][0].clone();
    let paused = automation::pause(&c, &brand_id, &mutation_payload(&task)).unwrap();
    assert_eq!(paused["status"], "paused");
    let skipped =
        automation::run_due_at(&c, &brand_id, None, &at(&paused, "nextRunAt", 1000)).unwrap();
    assert_eq!(skipped["reports"].as_array().unwrap().len(), 0);
    assert_eq!(
        automation::pause(&c, &brand_id, &mutation_payload(&paused)).unwrap(),
        paused
    );
    let resumed = automation::resume(&c, &brand_id, &mutation_payload(&paused)).unwrap();
    assert_eq!(resumed["status"], "active");
    assert!(
        DateTime::parse_from_rfc3339(resumed["nextRunAt"].as_str().unwrap()).unwrap() > Utc::now()
    );
    let cancelled = automation::cancel(&c, &brand_id, &mutation_payload(&resumed)).unwrap();
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(
        automation::cancel(&c, &brand_id, &mutation_payload(&cancelled)).unwrap(),
        cancelled
    );
    assert!(automation::resume(&c, &brand_id, &mutation_payload(&cancelled)).is_err());
    assert!(automation::pause(&c, &brand_id, &mutation_payload(&resumed)).is_err());
    let other = id();
    assert!(automation::list(&c, &other, &json!({"brandId":other})).is_err());
    assert!(
        automation::run_due_at(&c, &other, Some(cancelled["id"].as_str().unwrap()), &now())
            .is_err()
    );
}

#[test]
fn task_capacity_and_stored_side_effect_tampering_are_rejected() {
    let (c, brand_id) = database();
    let original = create_task(&c, &brand_id);
    for _ in 1..50 {
        create_task(&c, &brand_id);
    }
    assert!(automation::create(&c, &brand_id, &request(&brand_id)).is_err());
    assert_eq!(automation::create(&c,&brand_id,&json!({"brandId":brand_id,"requestId":original["requestId"],"name":original["name"],"intervalMinutes":15,"nodes":nodes()})).unwrap()["alreadyExists"], true);
    assert_eq!(
        automation::list(&c, &brand_id, &json!({"brandId":brand_id})).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .len(),
        50
    );
    let ran = automation::run_due_at(
        &c,
        &brand_id,
        Some(original["id"].as_str().unwrap()),
        &at(&original, "nextRunAt", 0),
    )
    .unwrap()["tasks"][0]
        .clone();
    for (key, value) in [
        ("networkRequests", 1),
        ("modelCalls", 1),
        ("publicationWrites", 1),
    ] {
        let mut bad = ran.clone();
        bad["reports"][0][key] = json!(value);
        c.execute(
            "UPDATE m_automation_tasks SET data=?1 WHERE id=?2",
            params![bad.to_string(), bad["id"].as_str()],
        )
        .unwrap();
        assert!(automation::validate_database(&c).is_err());
        assert!(automation::list(&c, &brand_id, &json!({"brandId":brand_id})).is_err());
    }
    c.execute(
        "UPDATE m_automation_tasks SET data=?1 WHERE id=?2",
        params![ran.to_string(), ran["id"].as_str()],
    )
    .unwrap();
    automation::validate_database(&c).unwrap();
}

#[test]
fn persisted_tasks_survive_encrypted_backup_restore_without_auto_execution() {
    let (c, brand_id) = database();
    let task = create_task(&c, &brand_id);
    let bytes = crate::backup::encrypt_snapshot(&c, "fictional-automation-password").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-automation-password")
        .unwrap();
    let loaded = reload(&restored, &brand_id, &task);
    assert_eq!(loaded["status"], "active");
    assert_eq!(loaded["totalRuns"], 0);
    assert_eq!(loaded["reports"], json!([]));
    let due = automation::run_due_at(
        &restored,
        &brand_id,
        Some(task["id"].as_str().unwrap()),
        &at(&task, "nextRunAt", 0),
    )
    .unwrap();
    assert_eq!(due["reports"].as_array().unwrap().len(), 1);
    assert_eq!(due["networkRequests"], 0);
    assert_eq!(count(&restored, "m_ai_preparations"), 0);
    assert_eq!(count(&restored, "m_publications"), 0);
}
