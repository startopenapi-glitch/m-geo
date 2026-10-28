use super::*;
#[test]
fn phase7_schema4_upgrade_and_inspection_keep_old_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.sqlite3");
    let mut c = crate::runtime::open_database(&path).unwrap();
    let b = handle(&mut c, "brand.create", json!({"name":"第七阶段虚构迁移"})).unwrap();
    c.execute_batch("DROP TABLE IF EXISTS m_p0_workflows; DROP TABLE IF EXISTS m_p0_policy; DROP TABLE IF EXISTS m_offline_workflows; DROP TABLE IF EXISTS m_question_research; DROP TABLE IF EXISTS m_media; DROP TABLE IF EXISTS m_automation_tasks; DROP TABLE IF EXISTS m_assets; DROP TABLE IF EXISTS m_knowledge_reviews; DROP TABLE IF EXISTS m_knowledge_documents; DROP TABLE IF EXISTS m_optimization_plans; UPDATE m_core_meta SET schema_version=4; PRAGMA user_version=4;").unwrap();
    drop(c);
    let c = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        10
    );
    assert_eq!(brand(&c, b["id"].as_str().unwrap()).unwrap(), b);
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_optimization_plans", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with("before-optimization-v5-")));
    let backup = crate::backup::encrypt_snapshot(&c, "fictional-phase7-only").unwrap();
    let checked = crate::backup::inspect_snapshot(&backup, "fictional-phase7-only").unwrap();
    assert_eq!(checked["targetVersion"], 10);
    assert_eq!(checked["counts"]["optimizationPlans"], 0);
}
fn setup(c: &mut Connection) -> (String, Vec<Value>, Value) {
    let b = handle(c, "brand.create", json!({"name":"第七阶段 · 虚构优化演练"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut products = vec![];
    let mut qs = vec![];
    for (i, (sku, name)) in [
        ("SIM-A", "虚构轻量水壶"),
        ("SIM-B,XL", "虚构通勤背包"),
        ("SIM-C\n蓝色", "虚构阅读台灯"),
    ]
    .iter()
    .enumerate()
    {
        let p=handle(c,"product.save",json!({"brandId":b,"sku":sku,"name":name,"facts":[],"privateNotes":"PRIVATE-NOTES-PHASE7","sourceText":"PRIVATE-SOURCE-PHASE7"})).unwrap();
        let q=handle(c,"question.save",json!({"brandId":b,"productId":p["id"],"text":(["单人租房如何选烧水设备？","日常通勤怎样选择随身收纳装备？","夜晚阅读怎样选择照明设备？"][i]),"intent":"selection","origin":"suggestion","source":"虚构流程样本","branded":false,"confirmed":true})).unwrap();
        products.push(p);
        qs.push(q["id"].clone());
    }
    let campaign=handle(c,"campaign.create",json!({"brandId":b,"name":"虚构同条件双平台演练","evidenceMode":"simulated","questionIds":qs,"platforms":["doubao","yuanbao"],"repetitions":2,"modelLabel":"模拟模型，无真实调用","searchMode":"模拟关闭联网","context":"虚构测试，无历史会话"})).unwrap();
    (b, products, campaign)
}
fn samples(c: &mut Connection, b: &str, campaign: &Value, phase: &str) {
    for (i, q) in campaign["questions"].as_array().unwrap().iter().enumerate() {
        for (j, platform) in ["doubao", "yuanbao"].iter().enumerate() {
            for rep in 1..=if phase == "baseline" { 1 } else { 2 } {
                let index = i * 2 + j;
                let rec = if phase == "followup" {
                    "yes"
                } else {
                    ["unknown", "yes", "no", "yes", "no", "no"][index]
                };
                handle(c,"observation.add",json!({"brandId":b,"productId":q["productId"],"questionId":q["id"],"platform":platform,"surface":"consumer","phase":phase,"modelLabel":campaign["modelLabel"],"searchMode":campaign["searchMode"],"testedAt":if phase=="baseline"{"2026-10-01T08:00:00Z"}else{"2026-10-22T08:00:00Z"},"answer":format!("PRIVATE-ANSWER-PHASE7 虚构回答{phase}{index}{rep}"),"mentioned":rec=="yes","accuracy":if phase=="baseline"&&index==0{"unknown"}else if phase=="baseline"&&index==2{"incorrect"}else{"correct"},"citedUrl":if phase=="followup"{"https://fictional.example/evidence"}else{""},"evidenceNote":"模拟原始记录，不代表平台表现","assessment":{"campaignId":campaign["id"],"repetition":rep,"context":campaign["context"],"recommendation":rec,"rank":if rec=="yes"{json!(2)}else{Value::Null},"competitors":"PRIVATE-COMPETITOR-PHASE7"}})).unwrap();
            }
        }
    }
}
#[test]
fn phase7_fictional_correction_retest_export_backup_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fictional.sqlite3");
    let mut c = crate::runtime::open_database(&path).unwrap();
    let (b, products, campaign) = setup(&mut c);
    samples(&mut c, &b, &campaign, "baseline");
    let before = c.total_changes();
    let diag = handle(
        &mut c,
        "optimization.diagnose",
        json!({"brandId":b,"campaignId":campaign["id"],"phase":"baseline"}),
    )
    .unwrap();
    assert_eq!(c.total_changes(), before);
    assert_eq!(diag["metrics"]["expectedPerPhase"], 12);
    assert_eq!(diag["metrics"]["baseline"]["samples"], 6);
    assert_eq!(diag["metrics"]["baseline"]["assessed"], 5);
    assert_eq!(diag["metrics"]["baseline"]["recommended"], 2);
    assert!(!diag["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["code"] == "recommendation_gap"));
    let create = json!({"brandId":b,"name":"虚构商品优化闭环","campaignId":campaign["id"],"diagnosisFingerprint":diag["fingerprint"],"requestId":id()});
    let saved = handle(&mut c, "optimization.create", create.clone()).unwrap();
    let pid = saved["id"].clone();
    assert_eq!(
        handle(&mut c, "optimization.create", create).unwrap()["id"],
        pid
    );
    let old = handle(
        &mut c,
        "optimization.get",
        json!({"brandId":b,"planId":pid}),
    )
    .unwrap();
    let key = old["actions"][0]["key"].clone();
    let upd = json!({"brandId":b,"planId":pid,"expectedRevision":old["revision"],"actionKey":key,"status":"done","note":"虚构处理记录，等待同条件复测"});
    c.execute_batch("CREATE TRIGGER reject_phase7_audit BEFORE INSERT ON m_audit WHEN NEW.action='optimization.update' BEGIN SELECT RAISE(IGNORE); END;").unwrap();
    assert!(handle(&mut c, "optimization.update", upd.clone()).is_err());
    assert_eq!(
        handle(
            &mut c,
            "optimization.get",
            json!({"brandId":b,"planId":pid})
        )
        .unwrap(),
        old
    );
    c.execute_batch("DROP TRIGGER reject_phase7_audit").unwrap();
    let updated = handle(&mut c, "optimization.update", upd.clone()).unwrap();
    assert!(handle(&mut c, "optimization.update", upd).is_err());
    let mut factlist = vec![];
    for (field, value) in [
        ("price", "129.00"),
        ("currency", "CNY"),
        ("availability", "InStock"),
        ("audience", "单人用户"),
        ("scenario", "虚构租房烧水"),
        ("limits", "不得干烧"),
        ("purchase", "https://fictional.example/buy"),
        ("afterSales", "虚构测试售后"),
        ("image", "https://fictional.example/image.png"),
    ] {
        factlist.push(json!({"id":format!("commerce:{field}"),"label":field,"value":value,"source":"虚构商品说明书","visibility":"public","validUntil":"2099-12-31"}));
    }
    factlist.push(json!({"id":"private:cost","label":"内部价格","value":"PRIVATE-FACT-PHASE7","source":"PRIVATE-PROOF-PHASE7","visibility":"private","validUntil":null}));
    let mut change = products[0].clone();
    change["expectedRevision"] = change["revision"].clone();
    change["facts"] = json!(factlist);
    change["name"] = json!("虚构轻量水壶修订");
    change["sku"] = json!("SIM-A-V2");
    let p = handle(&mut c, "product.save", change).unwrap();
    handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b,"id":p["id"],"expectedRevision":p["revision"]}),
    )
    .unwrap();
    let d = handle(
        &mut c,
        "draft.generateLocal",
        json!({"brandId":b,"productId":p["id"],"kind":"description","questionId":null}),
    )
    .unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    samples(&mut c, &b, &campaign, "followup");
    let result = handle(
        &mut c,
        "optimization.check",
        json!({"brandId":b,"planId":pid,"expectedRevision":updated["revision"]}),
    )
    .unwrap();
    let latest = &result["checkpoints"][1]["diagnosis"];
    assert_eq!(result["checkpoints"][0], old["checkpoints"][0]);
    assert_eq!(latest["metrics"]["matchedPairs"], 6);
    assert_eq!(latest["metrics"]["matchedAssessedPairs"], 5);
    assert_eq!(latest["metrics"]["deltaPercentagePoints"], 60.0);
    assert!(!latest["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["productId"] == p["id"]
            && ["facts_missing", "facts_unconfirmed", "description_pending"]
                .contains(&f["code"].as_str().unwrap())));
    assert_eq!(
        result["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == key)
            .unwrap()["status"],
        "done"
    );
    validate_database(&c).unwrap();
    let report = handle(
        &mut c,
        "optimization.export",
        json!({"brandId":b,"planId":pid}),
    )
    .unwrap();
    let encoded = report.to_string();
    for secret in [
        "PRIVATE-NOTES-PHASE7",
        "PRIVATE-SOURCE-PHASE7",
        "PRIVATE-ANSWER-PHASE7",
        "PRIVATE-COMPETITOR-PHASE7",
        "PRIVATE-FACT-PHASE7",
        "PRIVATE-PROOF-PHASE7",
    ] {
        assert!(!encoded.contains(secret), "{secret}");
    }
    let zip = crate::files::site_zip(&report).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&zip)).unwrap();
    assert_eq!(archive.len(), 3);
    let mut exported = String::new();
    use std::io::Read;
    archive
        .by_name("plan.json")
        .unwrap()
        .read_to_string(&mut exported)
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&exported).unwrap(), result);
    let backup = crate::backup::encrypt_snapshot(&c, "fictional-phase7-only").unwrap();
    let n = c.total_changes();
    assert_eq!(
        crate::backup::inspect_snapshot(&backup, "fictional-phase7-only").unwrap()["counts"]
            ["optimizationPlans"],
        1
    );
    assert_eq!(c.total_changes(), n);
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &backup, "fictional-phase7-only").unwrap();
    assert_eq!(
        handle(
            &mut restored,
            "optimization.get",
            json!({"brandId":b,"planId":pid})
        )
        .unwrap(),
        result
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_publications", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM ai_jobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(c);
    let mut reopened = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        handle(
            &mut reopened,
            "optimization.get",
            json!({"brandId":b,"planId":pid})
        )
        .unwrap(),
        result
    );
    if let Ok(out) = std::env::var("MGEO_PHASE7_OUTPUT") {
        assert!(out.contains("mgeo-e2e-phase7-"));
        let out = std::path::Path::new(&out);
        std::fs::create_dir_all(out).unwrap();
        assert!(!out.join("fictional.sqlite3").exists());
        let mut copy = Connection::open(out.join("fictional.sqlite3")).unwrap();
        rusqlite::backup::Backup::new(&reopened, &mut copy)
            .unwrap()
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .unwrap();
        for (name, bytes) in [
            ("optimization-plan.zip", zip),
            ("fictional-workspace.mgeobak", backup),
            (
                "baseline-diagnosis.json",
                serde_json::to_vec_pretty(&diag).unwrap(),
            ),
            (
                "final-plan.json",
                serde_json::to_vec_pretty(&result).unwrap(),
            ),
        ] {
            std::fs::write(out.join(name), bytes).unwrap();
        }
    }
}
#[test]
fn phase7_actual_schema4_file_copy_migration() {
    let Ok(source) = std::env::var("MGEO_PHASE7_LEGACY") else {
        return;
    };
    let prior =
        Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        prior
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    let old_backup = crate::backup::encrypt_snapshot(&prior, "fictional-phase7-legacy").unwrap();
    let inspection =
        crate::backup::inspect_snapshot(&old_backup, "fictional-phase7-legacy").unwrap();
    assert_eq!(inspection["sourceVersion"], 4);
    assert_eq!(inspection["targetVersion"], 10);
    assert_eq!(inspection["counts"]["optimizationPlans"], 0);
    let mut old_restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut old_restored, &old_backup, "fictional-phase7-legacy")
        .unwrap();
    validate_database(&old_restored).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("schema4-copy.sqlite3");
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
        7
    );
    assert_eq!(
        migrated
            .query_row("SELECT count(*) FROM m_optimization_plans", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    validate_database(&migrated).unwrap();
}
