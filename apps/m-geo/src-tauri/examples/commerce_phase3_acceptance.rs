//! Disposable fictional pilot workflow; no network or model operations.
use m_geo_lib::{backup, files, runtime};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{fs, path::Path};
fn call(db: &mut Connection, action: &str, payload: Value) -> Value {
    runtime::core_dispatch(db, action, payload).unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn zip(output: &Path, name: &str, package: &Value) {
    fs::write(output.join(name), files::site_zip(package).unwrap()).unwrap();
}
fn clone_db(source: &Connection) -> Connection {
    let mut target = Connection::open_in_memory().unwrap();
    rusqlite::backup::Backup::new(source, &mut target)
        .unwrap()
        .run_to_completion(128, std::time::Duration::ZERO, None)
        .unwrap();
    target
}
fn count(db: &Connection) -> i64 {
    db.query_row("SELECT count(*) FROM m_observations", [], |r| r.get(0))
        .unwrap()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "fictional source and new mgeo-e2e output required"
    );
    let source = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    assert!(source.to_string_lossy().contains("mgeo-e2e-"));
    assert!(output.to_string_lossy().contains("mgeo-e2e-"));
    fs::create_dir_all(output).unwrap();
    let path = output.join("fictional.sqlite3");
    assert!(
        !path.exists(),
        "do not overwrite existing acceptance database"
    );
    fs::copy(source, &path).unwrap();
    let mut db = runtime::open_database(&path).unwrap();
    let brands = call(&mut db, "brand.list", json!({}));
    assert_eq!(brands.as_array().unwrap().len(), 1);
    assert!(brands[0]["name"].as_str().unwrap().contains("虚构"));
    let b = brands[0]["id"].clone();
    let snapshot = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    let products = snapshot["products"].as_array().unwrap();
    assert_eq!(products.len(), 3);
    let old_count = count(&db);
    let ids: Vec<_> = products.iter().map(|p| p["id"].clone()).collect();
    let selection = json!({"brandId":b,"productIds":ids});
    let pilot = call(&mut db, "pilot.prepare", selection.clone());
    assert_eq!(pilot["candidates"].as_array().unwrap().len(), 10);
    for secret in [
        "PHASE2_PRIVATE_SENTINEL",
        "RAW_SECRET",
        "NOTE_SECRET",
        "PRIVATE_SECRET",
    ] {
        assert!(!pilot.to_string().contains(secret));
    }
    zip(output, "fictional-pilot.zip", &pilot);
    let save = json!({"brandId":b,"productIds":ids,"token":pilot["token"],"confirm":true});
    let questions = call(&mut db, "pilot.createQuestions", save.clone());
    assert_eq!(questions["created"], 10);
    assert_eq!(call(&mut db, "pilot.createQuestions", save)["skipped"], 10);
    let mut stale = clone_db(&db);
    let mut changed = products[0].clone();
    changed["expectedRevision"] = changed["revision"].clone();
    changed["privateNotes"] = json!("PHASE3_PRIVATE_SENTINEL");
    call(&mut stale, "product.save", changed);
    assert!(runtime::core_dispatch(
        &mut stale,
        "pilot.createQuestions",
        json!({"brandId":b,"productIds":ids,"token":pilot["token"],"confirm":true})
    )
    .is_err());
    let mut qids = vec![];
    for q in questions["questions"].as_array().unwrap() {
        assert_eq!(q["confirmed"], false);
        call(
            &mut db,
            "question.confirm",
            json!({"brandId":b,"id":q["id"],"confirmed":true}),
        );
        qids.push(q["id"].clone());
    }
    let campaign = call(
        &mut db,
        "campaign.create",
        json!({"brandId":b,"name":"第三阶段虚构模拟试点（非真实平台结果）","questionIds":qids,"platforms":["doubao","yuanbao","qianwen","baidu"],"repetitions":3,"modelLabel":"虚构验收，无模型调用","searchMode":"模拟，不联网","context":"虚构商品软件流程演练，未访问消费者平台","evidenceMode":"simulated"}),
    );
    assert_eq!(campaign["evidenceMode"], "simulated");
    for phase in ["baseline", "followup"] {
        let request = json!({"brandId":b,"campaignId":campaign["id"],"phase":phase});
        let package = call(&mut db, "collection.prepare", request.clone());
        assert_eq!(package["summary"]["remaining"], 120);
        assert_eq!(package["summary"]["evidenceMode"], "simulated");
        zip(output, &format!("{phase}-blank-collection.zip"), &package);
        let content = package["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "collection-001.json")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        let mut packet: Value = serde_json::from_str(content).unwrap();
        let blank = call(
            &mut db,
            "collection.preview",
            json!({"brandId":b,"campaignId":campaign["id"],"phase":phase,"content":content}),
        );
        assert_eq!(blank["canApply"], false);
        assert_eq!(blank["skippedCount"], 120);
        for (i, row) in packet["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            assert_eq!(row["answer"], "");
            row["testedAt"] = json!(if phase == "baseline" {
                "2026-10-23T01:00:00Z"
            } else {
                "2026-10-23T02:00:00Z"
            });
            row["answer"] = json!(format!(
                "  【模拟演练；非真实平台回答】第{i}条\n包含\"原文引号\"与换行。  "
            ));
            row["mentioned"] = json!(i % 2 == 0);
            row["recommendation"] = json!(if i % 2 == 0 { "yes" } else { "no" });
            row["rank"] = if i % 2 == 0 { json!(2) } else { Value::Null };
            row["evidenceNote"] = json!("虚构商品本地夹具；未访问平台，不代表推荐效果");
        }
        fs::write(
            output.join(format!("{phase}-filled-simulated.json")),
            serde_json::to_vec_pretty(&packet).unwrap(),
        )
        .unwrap();
        let filled = json!({"brandId":b,"campaignId":campaign["id"],"phase":phase,"content":packet.to_string()});
        let preview = call(&mut db, "collection.preview", filled.clone());
        assert_eq!(preview["canApply"], true);
        assert_eq!(preview["readyCount"], 120);
        let mut apply = filled.clone();
        apply["token"] = preview["token"].clone();
        apply["confirm"] = json!(true);
        let before = count(&db);
        let mut invalid_packet = packet.clone();
        invalid_packet["rows"][119]["testedAt"] = json!("not-a-time");
        let invalid = json!({"brandId":b,"campaignId":campaign["id"],"phase":phase,"content":invalid_packet.to_string()});
        let invalid_preview = call(&mut db, "collection.preview", invalid.clone());
        assert_eq!(invalid_preview["canApply"], false);
        let mut invalid_apply = invalid;
        invalid_apply["token"] = invalid_preview["token"].clone();
        invalid_apply["confirm"] = json!(true);
        assert!(runtime::core_dispatch(&mut db, "collection.apply", invalid_apply).is_err());
        assert_eq!(count(&db), before);
        let mut concurrent = clone_db(&db);
        let mut one = packet.clone();
        one["rows"] = json!([packet["rows"][0]]);
        let mut single = json!({"brandId":b,"campaignId":campaign["id"],"phase":phase,"content":one.to_string()});
        let first_preview = call(&mut concurrent, "collection.preview", single.clone());
        single["token"] = first_preview["token"].clone();
        single["confirm"] = json!(true);
        assert_eq!(
            call(&mut concurrent, "collection.apply", single)["created"],
            1
        );
        assert!(
            runtime::core_dispatch(&mut concurrent, "collection.apply", apply.clone()).is_err()
        );
        assert_eq!(count(&concurrent), before + 1);
        assert_eq!(
            call(&mut db, "collection.apply", apply.clone())["created"],
            120
        );
        assert!(runtime::core_dispatch(&mut db, "collection.apply", apply).is_err());
        assert_eq!(count(&db), before + 120);
        assert_eq!(
            call(&mut db, "collection.prepare", request)["summary"]["remaining"],
            0
        );
    }
    let mut refs = vec![];
    for p in products {
        for kind in ["description", "faq", "guide", "comparison"] {
            let q = questions["questions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|q| q["productId"] == p["id"])
                .unwrap();
            let d = call(
                &mut db,
                "draft.generateLocal",
                json!({"brandId":b,"productId":p["id"],"kind":kind,"questionId":q["id"]}),
            );
            let d = call(
                &mut db,
                "draft.review",
                json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
            );
            refs.push(json!({"id":d["id"],"expectedRevision":d["revision"]}));
        }
    }
    let site_request = json!({"brandId":b,"baseUrl":"","drafts":refs});
    let site = call(&mut db, "site.prepare", site_request.clone());
    zip(output, "fictional-website.zip", &site);
    let audit = call(&mut db, "site.audit", site_request);
    assert_eq!(audit["summary"]["blocked"], 0, "{audit}");
    assert!(audit["remoteChecks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["status"] == "pending"));
    zip(output, "preview-site-audit.zip", &audit);
    let configured = call(
        &mut db,
        "site.audit",
        json!({"brandId":b,"baseUrl":"https://fictional.example/geo/","drafts":refs}),
    );
    assert_eq!(configured["summary"]["blocked"], 0, "{configured}");
    zip(output, "configured-domain-local-audit.zip", &configured);
    let request = json!({"brandId":b,"campaignId":campaign["id"]});
    let report = call(&mut db, "campaign.report", request.clone());
    assert_eq!(report["summary"]["evidenceMode"], "simulated");
    assert_eq!(report["summary"]["baselineCount"], 120);
    assert_eq!(report["summary"]["followupCount"], 120);
    assert_eq!(report["summary"]["missingCount"], 0);
    zip(output, "fictional-experiment-report.zip", &report);
    let encrypted = backup::encrypt_snapshot(&db, "fictional-phase3-only-passphrase").unwrap();
    fs::write(output.join("fictional-backup.mgeo"), &encrypted).unwrap();
    let mut restored = runtime::open_database(&output.join("restored.sqlite3")).unwrap();
    backup::restore_snapshot(
        &mut restored,
        &encrypted,
        "fictional-phase3-only-passphrase",
    )
    .unwrap();
    assert_eq!(
        call(&mut restored, "campaign.report", request.clone())["summary"],
        report["summary"]
    );
    drop(db);
    let mut reopened = runtime::open_database(&path).unwrap();
    assert_eq!(
        call(&mut reopened, "campaign.report", request)["summary"],
        report["summary"]
    );
    let snapshot = call(&mut reopened, "workspace.snapshot", json!({"brandId":b}));
    assert_eq!(snapshot["publications"].as_array().unwrap().len(), 0);
    assert_eq!(count(&reopened), old_count + 240);
    let original = snapshot["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["assessment"]["campaignId"] == campaign["id"])
        .unwrap();
    assert!(original["answer"].as_str().unwrap().starts_with("  【模拟"));
    assert!(original["answer"].as_str().unwrap().ends_with("  "));
    let result = json!({"status":"passed","fixture":"fictional-only-not-real-platform-evidence","brandId":b,"campaignId":campaign["id"],"products":3,"newCandidates":10,"newReviewedDrafts":12,"newSimulatedObservations":240,"priorUnclassifiedObservations":old_count,"publications":0,"paidModelCalls":0,"summary":report["summary"],"siteAudit":audit["summary"],"remoteChecks":audit["remoteChecks"],"checks":["public-intake-no-private-leak","ten-unconfirmed-candidates","dedup-stale-preview","explicit-question-confirmation","simulated-mode-frozen","blank-packet-not-negative","invalid-late-row-atomic","concurrent-slot-stale-rejected","replay-rejected","raw-answer-whitespace-preserved","reviewed-website","local-semantic-audit","configured-domain-not-fetched","encrypted-backup-restore","database-reopen"]});
    fs::write(
        output.join("fixture-result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("Phase3 acceptance passed: 3 fictional products, 10 candidates, 240 new simulated observations, 12 reviewed drafts; no publishing/paid calls.");
}
