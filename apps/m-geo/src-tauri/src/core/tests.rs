use super::*;
use serde_json::json;
fn db() -> Connection {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    c
}
fn call(c: &mut Connection, a: &str, p: Value) -> Value {
    handle(c, a, p).unwrap()
}
fn brand(c: &mut Connection, n: &str) -> Value {
    call(c, "brand.create", json!({"name":n}))
}
fn product(c: &mut Connection, b: &Value) -> Value {
    call(
        c,
        "product.save",
        json!({"brandId":b["id"],"name":"电水壶","sku":"SKU-1","category":"家电","officialUrl":"https://example.com/item","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"f1","label":"容量","value":"1.5L","source":"官方参数表","visibility":"public","validUntil":null},{"id":"f2","label":"成本","value":"PRIVATE_SECRET","source":"内部","visibility":"private","validUntil":null}]}),
    )
}
fn confirm(c: &mut Connection, p: &Value) -> Value {
    call(
        c,
        "product.confirm",
        json!({"brandId":p["brandId"],"id":p["id"],"expectedRevision":p["revision"]}),
    )
}
fn draft(c: &mut Connection, p: &Value) -> Value {
    call(
        c,
        "draft.generateLocal",
        json!({"brandId":p["brandId"],"productId":p["id"],"kind":"description"}),
    )
}
fn review(c: &mut Connection, d: &Value) -> Value {
    call(
        c,
        "draft.review",
        json!({"brandId":d["brandId"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
}
#[test]
fn confirmed_public_flow_and_immutable_publication() {
    let mut c = db();
    let b = brand(&mut c, "壹品牌");
    let p = product(&mut c, &b);
    assert!(handle(
        &mut c,
        "draft.generateLocal",
        json!({"brandId":b["id"],"productId":p["id"],"kind":"description"})
    )
    .is_err());
    let p = confirm(&mut c, &p);
    let d = draft(&mut c, &p);
    let body = d["body"].as_str().unwrap();
    assert!(body.contains("1.5L"));
    for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
        assert!(!body.contains(secret));
    }
    let d = review(&mut c, &d);
    let publication = call(
        &mut c,
        "publication.add",
        json!({"brandId":b["id"],"draftId":d["id"],"expectedRevision":d["revision"],"channel":"官网","url":"https://example.com/article","publishedAt":"2026-10-17T01:00:00Z","note":"人工发布"}),
    );
    let edited = call(
        &mut c,
        "draft.save",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"title":"编辑","body":"<script>alert(1)</script>新正文"}),
    );
    assert!(edited["reviewedAt"].is_null());
    assert!(handle(&mut c,"draft.export",json!({"brandId":b["id"],"id":edited["id"],"expectedRevision":edited["revision"],"format":"html"})).is_err());
    let edited = review(&mut c, &edited);
    let exported = call(
        &mut c,
        "draft.export",
        json!({"brandId":b["id"],"id":edited["id"],"expectedRevision":edited["revision"],"format":"html"}),
    );
    assert!(!exported["content"].as_str().unwrap().contains("<script>"));
    let snap = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(
        snap["publications"][0]["contentSnapshot"],
        publication["contentSnapshot"]
    );
    assert!(publication["contentSnapshot"]
        .as_str()
        .unwrap()
        .contains("1.5L"));
}
#[test]
fn isolation_sku_and_revision_guards() {
    let mut c = db();
    let a = brand(&mut c, "A");
    let b = brand(&mut c, "B");
    let p = product(&mut c, &a);
    product(&mut c, &b);
    assert!(handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":1})
    )
    .is_err());
    let mut update = p.clone();
    update["expectedRevision"] = json!(0);
    assert!(handle(&mut c, "product.save", update).is_err());
    let mut update = p.clone();
    update["expectedRevision"] = p["revision"].clone();
    let updated = call(&mut c, "product.save", update);
    assert_eq!(updated["revision"], 2);
    assert!(updated["confirmedAt"].is_null());
    assert!(handle(
        &mut c,
        "product.confirm",
        json!({"brandId":a["id"],"id":p["id"],"expectedRevision":1})
    )
    .is_err());
}
#[test]
fn questions_dedup_and_observation_ownership() {
    let mut c = db();
    let a = brand(&mut c, "A");
    let p = product(&mut c, &a);
    let q = call(
        &mut c,
        "question.save",
        json!({"brandId":a["id"],"productId":p["id"],"text":"  怎么  使用？ ","intent":"usage","origin":"customer","source":"客服","branded":false,"confirmed":true}),
    );
    let q2 = call(
        &mut c,
        "question.save",
        json!({"brandId":a["id"],"productId":p["id"],"text":"怎么 使用？","intent":"usage","origin":"customer","source":"客服","branded":false,"confirmed":true}),
    );
    assert_eq!(q["id"], q2["id"]);
    let input = json!({"brandId":a["id"],"productId":p["id"],"questionId":q["id"],"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":"未知","searchMode":"联网","testedAt":"2026-10-17T00:00:00Z","answer":"实测回答","mentioned":true,"accuracy":"unknown","citedUrl":"","evidenceNote":"截图编号1"});
    let o = call(&mut c, "observation.add", input.clone());
    assert_eq!(o["querySnapshot"], q["text"]);
    assert_eq!(o["branded"], false);
    assert_eq!(call(&mut c, "observation.add", input)["id"], o["id"]);
}
#[test]
fn ai_context_excludes_secrets_and_rechecks_revision() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let p = confirm(&mut c, &p);
    let ctx = call(
        &mut c,
        "internal.ai.prepare",
        json!({"brandId":b["id"],"productId":p["id"],"kind":"faq"}),
    );
    let serialized = ctx.to_string();
    for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
        assert!(!serialized.contains(secret));
    }
    let mut forged = ctx.clone();
    forged["title"] = json!("篡改");
    assert!(handle(
        &mut c,
        "internal.ai.commit",
        json!({"context":forged,"body":"模型正文"})
    )
    .is_err());
    let d = call(
        &mut c,
        "internal.ai.commit",
        json!({"context":ctx,"body":"模型正文"}),
    );
    assert_eq!(d["generationMode"], "ai");
    let ctx = call(
        &mut c,
        "internal.ai.prepare",
        json!({"brandId":b["id"],"productId":p["id"],"kind":"guide"}),
    );
    let mut update = p.clone();
    update["expectedRevision"] = p["revision"].clone();
    call(&mut c, "product.save", update);
    assert!(handle(
        &mut c,
        "internal.ai.commit",
        json!({"context":ctx,"body":"迟到正文"})
    )
    .is_err());
    assert!(handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]})
    )
    .is_err());
}
#[test]
fn expired_public_facts_block_confirmation() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let mut update = p.clone();
    update["facts"][0]["validUntil"] = json!("2000-01-01");
    update["expectedRevision"] = json!(1);
    let saved = call(&mut c, "product.save", update);
    assert!(handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":saved["id"],"expectedRevision":saved["revision"]})
    )
    .is_err());
}
#[test]
fn csv_rejects_whole_cross_brand_batch_and_skips_sku_duplicates() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let bad="品牌,商品名称,SKU,类目,商品链接\nA,商品1,1,家电,https://example.com/1\nB,商品2,2,家电,https://example.com/2\n";
    assert!(handle(
        &mut c,
        "import.csv",
        json!({"brandId":b["id"],"content":bad,"confirm":true})
    )
    .is_err());
    assert_eq!(
        call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}))["products"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let good = bad.replace("B,商品2", "A,商品2");
    assert_eq!(
        call(
            &mut c,
            "import.csv",
            json!({"brandId":b["id"],"content":good,"confirm":true})
        )["created"],
        2
    );
    assert_eq!(
        call(
            &mut c,
            "import.csv",
            json!({"brandId":b["id"],"content":good,"confirm":true})
        )["skipped"],
        2
    );
}
#[test]
fn cnvp_import_is_atomic_and_idempotent_with_preserved_snapshots() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let export = json!({"schemaVersion":1,"brandName":"A","products":[{"id":"old-p","name":"壶","sku":"old-sku","category":"家电","officialUrl":"https://example.com/p","sourceText":"资料","privateNotes":"内部","facts":[],"revision":3,"confirmedAt":null}],"questions":[{"id":"old-q","productId":"old-p","text":"怎么用","intent":"usage","origin":"customer","source":"客服","branded":false,"confirmed":true}],"drafts":[{"id":"old-d","productId":"old-p","questionId":"old-q","kind":"faq","title":"旧文","body":"旧正文","sourceRevision":3,"revision":2,"generationMode":"local","reviewedAt":null,"createdAt":"2026-10-17T00:00:00Z"}],"publications":[{"id":"old-pub","draftId":"old-d","draftRevision":1,"channel":"官网","url":"https://example.com/a","publishedAt":"2026-10-17T00:00:00Z","note":"旧记录","contentSnapshot":"真实旧快照"}],"observations":[]});
    let payload = json!({"brandId":b["id"],"sourceInstance":"cnvp-export-test","content":export.to_string(),"confirm":true});
    let r = call(&mut c, "import.cnvp", payload.clone());
    assert_eq!(r["created"], 4);
    assert_eq!(call(&mut c, "import.cnvp", payload)["skipped"], 4);
    let s = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(s["publications"][0]["contentSnapshot"], "真实旧快照");
    assert_eq!(s["products"][0]["revision"], 3);
    assert!(s["drafts"][0]["reviewedAt"].is_null());
    let mut invalid = export;
    invalid["products"][0]["sku"] = json!("new");
    invalid["products"][0]["id"] = json!("new-p");
    invalid["questions"][0]["productId"] = json!("missing");
    assert!(handle(&mut c,"import.cnvp",json!({"brandId":b["id"],"sourceInstance":"second","content":invalid.to_string(),"confirm":true})).is_err());
    assert_eq!(
        call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}))["products"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn cnvp_sku_conflict_skips_descendants_without_mixing_existing_facts() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let existing = product(&mut c, &b);
    let export = json!({"schemaVersion":1,"brandName":"A","products":[{"id":"p","name":"另一个旧商品","sku":"SKU-1","category":"旧","officialUrl":"","facts":[],"revision":99,"confirmedAt":null}],"questions":[{"id":"q","productId":"p","text":"旧商品的问题","intent":"usage","origin":"customer","source":"历史","branded":true,"confirmed":true}],"drafts":[{"id":"d","productId":"p","questionId":"q","kind":"faq","title":"无关旧稿","body":"不同商品旧事实","sourceRevision":1,"revision":1,"generationMode":"ai","reviewedAt":null,"createdAt":"2026-10-17T00:00:00Z"}],"publications":[],"observations":[]});
    let r = call(
        &mut c,
        "import.cnvp",
        json!({"brandId":b["id"],"sourceInstance":"conflict","content":export.to_string(),"confirm":true}),
    );
    assert_eq!(r["created"], 0);
    assert_eq!(r["skipped"], 3);
    let s = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(s["products"][0], existing);
    assert_eq!(s["questions"], json!([]));
    assert_eq!(s["drafts"], json!([]));
}
#[test]
fn observations_require_current_brand_product_and_confirmed_question() {
    let mut c = db();
    let a = brand(&mut c, "A");
    let b = brand(&mut c, "B");
    let pa = product(&mut c, &a);
    let pb = product(&mut c, &b);
    let q = call(
        &mut c,
        "question.save",
        json!({"brandId":b["id"],"productId":pb["id"],"text":"怎么用","intent":"usage","origin":"customer","source":"客服","branded":true,"confirmed":true}),
    );
    let input = json!({"brandId":a["id"],"productId":pa["id"],"questionId":q["id"],"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":"未知","searchMode":"联网","testedAt":"2026-10-17T00:00:00Z","answer":"回答","mentioned":true,"accuracy":"correct","citedUrl":"","evidenceNote":""});
    assert!(handle(&mut c, "observation.add", input.clone()).is_err());
    let mut correct = input;
    correct["brandId"] = b["id"].clone();
    correct["productId"] = pb["id"].clone();
    call(
        &mut c,
        "question.confirm",
        json!({"brandId":b["id"],"id":q["id"],"confirmed":false}),
    );
    assert!(handle(&mut c, "observation.add", correct).is_err());
}
#[test]
fn expired_confirmed_product_cannot_export_or_publish_old_draft() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let mut p = confirm(&mut c, &p);
    let d = draft(&mut c, &p);
    let d = review(&mut c, &d);
    p["facts"][0]["validUntil"] = json!("2000-01-01");
    c.execute(
        "UPDATE m_products SET data=?1 WHERE id=?2",
        params![p.to_string(), p["id"].as_str()],
    )
    .unwrap();
    assert!(handle(
        &mut c,
        "draft.export",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"],"format":"markdown"})
    )
    .is_err());
    assert!(handle(&mut c,"publication.add",json!({"brandId":b["id"],"draftId":d["id"],"expectedRevision":d["revision"],"channel":"官网","url":"https://example.com/a","publishedAt":"2026-10-17T00:00:00Z","note":""})).is_err());
}
#[test]
fn csv_late_invalid_or_201st_row_rolls_back_every_insert() {
    for mode in ["invalid", "overflow"] {
        let mut c = db();
        let b = brand(&mut c, "A");
        let mut input = "品牌,商品名称,SKU,类目,商品链接\n".to_string();
        for n in 0..(if mode == "invalid" { 199 } else { 200 }) {
            input.push_str(&format!("A,商品{n},{n},类目,https://example.com/{n}\n"));
        }
        if mode == "invalid" {
            input.push_str("A,错误,late,类目,javascript:alert(1)\n")
        } else {
            input.push_str("A,超量,201,类目,https://example.com/201\n")
        }
        assert!(handle(
            &mut c,
            "import.csv",
            json!({"brandId":b["id"],"content":input,"confirm":true})
        )
        .is_err());
        assert_eq!(
            call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}))["products"],
            json!([])
        );
    }
}
#[test]
fn draft_limit_and_invalid_urls_do_not_mutate_records() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let p = confirm(&mut c, &p);
    let d = draft(&mut c, &p);
    assert!(handle(&mut c,"draft.save",json!({"brandId":b["id"],"id":d["id"],"expectedRevision":1,"title":"长稿","body":"字".repeat(30001)})).is_err());
    let d = review(&mut c, &d);
    for url in [
        "file:///tmp/a",
        "javascript:alert(1)",
        "https://localhost/a",
        "https://user:pass@example.com/a",
        "https://[::ffff:127.0.0.1]/a",
    ] {
        assert!(handle(&mut c,"publication.add",json!({"brandId":b["id"],"draftId":d["id"],"expectedRevision":1,"channel":"官网","url":url,"publishedAt":"2026-10-17T00:00:00Z","note":""})).is_err(),"accepted {url}");
    }
}
#[test]
fn native_database_persists_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("core.sqlite");
    let b;
    {
        let mut c = Connection::open(&path).unwrap();
        initialize(&mut c).unwrap();
        b = brand(&mut c, "持久化");
        product(&mut c, &b);
    }
    let mut c = Connection::open(&path).unwrap();
    initialize(&mut c).unwrap();
    let s = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(s["products"].as_array().unwrap().len(), 1);
    assert_eq!(s["products"][0]["sku"], "SKU-1");
}
#[test]
fn freeform_source_and_draft_preserve_exact_whitespace() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let mut input = p.clone();
    input["expectedRevision"] = p["revision"].clone();
    input["sourceText"] = json!("  原始文本\n\n");
    input["privateNotes"] = json!("  内部备注\n");
    let p = call(&mut c, "product.save", input);
    assert_eq!(p["sourceText"], "  原始文本\n\n");
    assert_eq!(p["privateNotes"], "  内部备注\n");
    let p = confirm(&mut c, &p);
    let d = draft(&mut c, &p);
    let d = call(
        &mut c,
        "draft.save",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":1,"title":"缩进正文","body":"    代码段\n\n"}),
    );
    assert_eq!(d["body"], "    代码段\n\n");
}

#[test]
fn audit_records_mutation_metadata_without_user_content() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let table_exists: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='m_audit')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        table_exists,
        "native mutations must retain content-free audit metadata"
    );
    let before: i64 = c
        .query_row("SELECT count(*) FROM m_audit", [], |r| r.get(0))
        .unwrap();
    assert!(handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":99})
    )
    .is_err());
    let mut statement = c
        .prepare("SELECT action,brand_id,object_id FROM m_audit ORDER BY rowid")
        .unwrap();
    let entries: Vec<(String, Option<String>, Option<String>)> = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(entries.len() as i64, before);
    assert!(entries
        .iter()
        .any(|(action, brand, object)| action == "product.save"
            && brand.as_deref() == b["id"].as_str()
            && object.as_deref() == p["id"].as_str()));
    let encoded = format!("{entries:?}");
    for forbidden in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET", "电水壶"] {
        assert!(!encoded.contains(forbidden));
    }
}

#[test]
fn cnvp_keeps_historical_query_cohorts_and_does_not_confirm_questions() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let export = json!({"schemaVersion":1,"brandName":"A","products":[{"id":"p","name":"壶","sku":"p","category":"","officialUrl":"","facts":[],"revision":1}],"questions":[{"id":"q","productId":"p","text":"现在的问题","intent":"usage","origin":"customer","source":"历史","branded":false,"confirmed":false}],"drafts":[],"publications":[],"observations":[{"id":"o","productId":"p","questionId":"q","querySnapshot":"旧品牌问题","branded":true,"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":"原模型","searchMode":"联网","testedAt":"2026-10-17T00:00:00Z","answer":"历史回答","mentioned":false,"accuracy":"unknown","citedUrl":"","evidenceNote":"原截图"}]});
    let r = call(
        &mut c,
        "import.cnvp",
        json!({"brandId":b["id"],"sourceInstance":"history","content":export.to_string(),"confirm":true}),
    );
    assert_eq!(r["created"], 3);
    let s = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(s["questions"][0]["confirmed"], false);
    assert_eq!(s["observations"][0]["querySnapshot"], "旧品牌问题");
    assert_eq!(s["observations"][0]["branded"], true);
    assert_eq!(s["observations"][0]["mentioned"], false);
}

#[test]
fn snapshot_marks_truncation_and_future_schema_refuses_writes() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    for n in 0..1000 {
        let mut row = p.clone();
        row["id"] = json!(Uuid::new_v4().to_string());
        row["sku"] = json!(format!("extra-{n}"));
        c.execute(
            "INSERT INTO m_products(id,brand_id,sku,data)VALUES(?1,?2,?3,?4)",
            params![
                row["id"].as_str(),
                b["id"].as_str(),
                row["sku"].as_str(),
                row.to_string()
            ],
        )
        .unwrap();
    }
    let s = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(s["products"].as_array().unwrap().len(), 1000);
    assert_eq!(s["truncated"], true);
    let mut future = Connection::open_in_memory().unwrap();
    future.execute_batch("CREATE TABLE m_core_meta(id INTEGER PRIMARY KEY,schema_version INTEGER);INSERT INTO m_core_meta VALUES(1,6)").unwrap();
    assert!(initialize(&mut future).is_err());
    let count: i64 = future
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn migration_retains_review_history_and_snapshots_without_reapproving() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let reviewed = "2026-10-16T03:00:00+08:00";
    let source = json!({"name":"旧壶","brand":"A","sku":"p","facts":[{"label":"容量","value":"1L","source":"旧表","visibility":"public"}]});
    let review_snapshot = json!({"title":"旧审稿","body":"  原审核正文\n","revision":4});
    let export = json!({"schemaVersion":1,"brandName":"A","products":[{"id":"p","name":"壶","sku":"p","category":"","officialUrl":"","facts":[],"revision":9,"confirmedAt":reviewed,"ownerLabel":"原归属者"}],"questions":[],"drafts":[{"id":"d","productId":"p","questionId":null,"kind":"description","title":"旧稿","body":"正文","sourceRevision":8,"revision":4,"generationMode":"local","reviewedAt":reviewed,"createdAt":"2026-10-15T00:00:00Z","reviewerLabel":"原审核者","sourceSnapshot":source,"reviewSnapshot":review_snapshot}],"publications":[],"observations":[]});
    let result = handle(
        &mut c,
        "import.cnvp",
        json!({"brandId":b["id"],"sourceInstance":"snapshots","content":export.to_string(),"confirm":true}),
    );
    assert!(result.is_ok(), "{result:?}");
    let history:String=c.query_row("SELECT data FROM m_migration_history WHERE source='snapshots' AND kind='draft' AND old_id='d'",[],|r|r.get(0)).unwrap();
    let history: Value = serde_json::from_str(&history).unwrap();
    assert_eq!(history["reviewedAt"], reviewed);
    assert_eq!(history["reviewerLabel"], "原审核者");
    assert_eq!(history["sourceSnapshot"], source);
    assert_eq!(history["reviewSnapshot"], review_snapshot);
    let s = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert!(s["products"][0]["confirmedAt"].is_null());
    assert!(s["drafts"][0]["reviewedAt"].is_null());
    assert_eq!(s["drafts"][0]["revision"], 4);
    assert_eq!(s["drafts"][0]["sourceRevision"], 8);
}

#[test]
fn independent_ai_generations_can_save_distinct_results_for_same_product_version() {
    let mut c = db();
    let b = brand(&mut c, "A");
    let p = product(&mut c, &b);
    let p = confirm(&mut c, &p);
    let input = json!({"brandId":b["id"],"productId":p["id"],"kind":"description"});
    let first = call(&mut c, "internal.ai.prepare", input.clone());
    let second = call(&mut c, "internal.ai.prepare", input);
    let d1 = call(
        &mut c,
        "internal.ai.commit",
        json!({"context":first,"body":"第一份独立模型结果"}),
    );
    let d2 = call(
        &mut c,
        "internal.ai.commit",
        json!({"context":second,"body":"第二份独立模型结果"}),
    );
    assert_ne!(d1["id"], d2["id"]);
    let duplicate = call(
        &mut c,
        "internal.ai.commit",
        json!({"context":first,"body":"第一份独立模型结果"}),
    );
    assert_eq!(duplicate["id"], d1["id"]);
}

fn restore_fixture() -> (Connection, Value) {
    let mut c = db();
    let a = brand(&mut c, "壹品牌");
    let b = brand(&mut c, "贰品牌");
    let p = product(&mut c, &a);
    let p = confirm(&mut c, &p);
    let other = product(&mut c, &b);
    let q = call(
        &mut c,
        "question.save",
        json!({"brandId":a["id"],"productId":p["id"],"text":"怎么用","intent":"usage","origin":"customer","source":"客服","branded":false,"confirmed":true}),
    );
    let d = draft(&mut c, &p);
    let d = review(&mut c, &d);
    let publication = call(
        &mut c,
        "publication.add",
        json!({"brandId":a["id"],"draftId":d["id"],"expectedRevision":d["revision"],"channel":"官网","url":"https://example.com/a","publishedAt":"2026-10-17T00:00:00Z","note":""}),
    );
    let observation = call(
        &mut c,
        "observation.add",
        json!({"brandId":a["id"],"productId":p["id"],"questionId":q["id"],"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":"未知","searchMode":"联网","testedAt":"2026-10-17T00:00:00Z","answer":"观察回答","mentioned":true,"accuracy":"correct","citedUrl":"","evidenceNote":""}),
    );
    let ctx = call(
        &mut c,
        "internal.ai.prepare",
        json!({"brandId":a["id"],"productId":p["id"],"kind":"faq","questionId":q["id"]}),
    );
    (
        c,
        json!({"brand":a,"otherBrand":b,"product":p,"otherProduct":other,"question":q,"draft":d,"publication":publication,"observation":observation,"context":ctx}),
    )
}

#[test]
fn database_validation_rejects_bad_json_identity_fields_choices_and_limits() {
    let cases = [
        ("m_brands", "brand", "name", Value::Null),
        ("m_products", "product", "brandId", json!("OTHER")),
        ("m_products", "product", "sku", json!("SQL-SKU-MISMATCH")),
        ("m_products", "product", "revision", json!(0)),
        ("m_products", "product", "sourceText", Value::Null),
        (
            "m_questions",
            "question",
            "productId",
            json!(Uuid::new_v4().to_string()),
        ),
        ("m_questions", "question", "intent", json!("invalid")),
        ("m_drafts", "draft", "body", json!("字".repeat(30001))),
        (
            "m_drafts",
            "draft",
            "questionId",
            json!(Uuid::new_v4().to_string()),
        ),
        (
            "m_publications",
            "publication",
            "draftId",
            json!(Uuid::new_v4().to_string()),
        ),
        (
            "m_publications",
            "publication",
            "url",
            json!("javascript:alert(1)"),
        ),
        (
            "m_observations",
            "observation",
            "questionId",
            json!(Uuid::new_v4().to_string()),
        ),
        (
            "m_observations",
            "observation",
            "platform",
            json!("imaginary"),
        ),
        ("m_observations", "observation", "mentioned", json!("yes")),
    ];
    for (table, key, field, bad) in cases {
        let (c, fixture) = restore_fixture();
        let mut row = fixture[key].clone();
        row[field] = bad;
        c.execute(
            &format!("UPDATE {table} SET data=?1 WHERE id=?2"),
            params![row.to_string(), fixture[key]["id"].as_str()],
        )
        .unwrap();
        assert!(
            validate_database(&c).is_err(),
            "accepted corrupted {table}.{field}"
        );
    }
    for (table, key) in [
        ("m_brands", "brand"),
        ("m_products", "product"),
        ("m_questions", "question"),
        ("m_drafts", "draft"),
        ("m_publications", "publication"),
        ("m_observations", "observation"),
    ] {
        let (c, fixture) = restore_fixture();
        c.execute(
            &format!("UPDATE {table} SET data='not json' WHERE id=?1"),
            [fixture[key]["id"].as_str().unwrap()],
        )
        .unwrap();
        assert!(
            validate_database(&c).is_err(),
            "accepted invalid JSON in {table}"
        );
    }
}

#[test]
fn database_validation_is_readonly_and_allows_expired_stale_history() {
    let (mut c, fixture) = restore_fixture();
    assert!(validate_database(&c).is_ok());
    let mut p = fixture["product"].clone();
    p["facts"][0]["validUntil"] = json!("2000-01-01");
    p["revision"] = json!(5);
    p["confirmedAt"] = Value::Null;
    c.execute(
        "UPDATE m_products SET data=?1 WHERE id=?2",
        params![p.to_string(), p["id"].as_str()],
    )
    .unwrap();
    call(
        &mut c,
        "question.confirm",
        json!({"brandId":fixture["brand"]["id"],"id":fixture["question"]["id"],"confirmed":false}),
    );
    let before = c.total_changes();
    assert!(validate_database(&c).is_ok());
    let after = c.total_changes();
    assert_eq!(before, after);
    let saved = load(
        &c,
        "m_products",
        fixture["brand"]["id"].as_str().unwrap(),
        p["id"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(saved, p);
}

#[test]
fn database_validation_rejects_unindexed_normalization_and_preparation_tampering() {
    let (c, fixture) = restore_fixture();
    c.execute(
        "UPDATE m_questions SET normalized='WRONG' WHERE id=?1",
        [fixture["question"]["id"].as_str().unwrap()],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
    let (c, fixture) = restore_fixture();
    let mut ctx = fixture["context"].clone();
    ctx["brandId"] = fixture["otherBrand"]["id"].clone();
    c.execute(
        "UPDATE m_ai_preparations SET hash=?1,context=?2 WHERE hash=?3",
        params![
            fingerprint(&ctx),
            ctx.to_string(),
            fingerprint(&fixture["context"])
        ],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
}

#[test]
fn suggested_questions_mark_actual_brand_mentions() {
    let mut c = db();
    let b = brand(&mut c, "壹品牌");
    let p = product(&mut c, &b);
    let suggestions = call(
        &mut c,
        "question.suggest",
        json!({"brandId":b["id"],"productId":p["id"]}),
    );
    assert!(suggestions
        .as_array()
        .unwrap()
        .iter()
        .all(|q| q["branded"] == false));
    let mut change = p;
    change["name"] = json!("壹品牌电水壶");
    change["expectedRevision"] = json!(1);
    let p = call(&mut c, "product.save", change);
    let suggestions = call(
        &mut c,
        "question.suggest",
        json!({"brandId":b["id"],"productId":p["id"]}),
    );
    assert!(suggestions
        .as_array()
        .unwrap()
        .iter()
        .all(|q| q["branded"] == true));
}

#[test]
fn database_validation_checks_existing_cross_brand_ids_and_migration_audit_records() {
    let (mut c, fixture) = restore_fixture();
    let mut p = fixture["product"].clone();
    p["brandId"] = fixture["otherBrand"]["id"].clone();
    c.execute(
        "UPDATE m_products SET data=?1 WHERE id=?2",
        params![p.to_string(), p["id"].as_str()],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
    for damaged in ["history", "mapping", "skip", "audit"] {
        let (fresh, f) = restore_fixture();
        c = fresh;
        let mut legacy = f["product"].clone();
        legacy.as_object_mut().unwrap().remove("brandId");
        legacy["id"] = json!("old");
        legacy["sku"] = json!("imported");
        let export = json!({"schemaVersion":1,"brandName":"壹品牌","products":[legacy],"questions":[],"drafts":[],"publications":[],"observations":[]});
        call(
            &mut c,
            "import.cnvp",
            json!({"brandId":f["brand"]["id"],"sourceInstance":"validation","content":export.to_string(),"confirm":true}),
        );
        assert!(validate_database(&c).is_ok());
        match damaged {
            "history" => {
                c.execute("UPDATE m_migration_history SET data='[]'", [])
                    .unwrap();
            }
            "mapping" => {
                c.execute(
                    "UPDATE m_migration_map SET new_id=?1",
                    [f["otherProduct"]["id"].as_str().unwrap()],
                )
                .unwrap();
            }
            "skip" => {
                c.execute("INSERT INTO m_migration_skips(source,kind,old_id,brand_id) VALUES('validation','product','old',?1)",[f["otherBrand"]["id"].as_str().unwrap()]).unwrap();
            }
            "audit" => {
                c.execute(
                    "UPDATE m_audit SET object_id=?1 WHERE action='product.save' AND brand_id=?2",
                    params![f["otherProduct"]["id"].as_str(), f["brand"]["id"].as_str()],
                )
                .unwrap();
            }
            _ => unreachable!(),
        };
        assert!(
            validate_database(&c).is_err(),
            "accepted corrupted {damaged}"
        );
    }
}

#[test]
fn database_validation_reads_beyond_ui_snapshot_limits() {
    let mut c = db();
    let b = brand(&mut c, "全量");
    let p = product(&mut c, &b);
    for n in 0..1001 {
        let mut row = p.clone();
        row["id"] = json!(Uuid::new_v4().to_string());
        row["sku"] = json!(format!("tail-{n}"));
        if n == 1000 {
            row["facts"] = json!([{"id":"bad","label":"坏事实","value":"值","source":"表","visibility":"invalid","validUntil":null}]);
        }
        c.execute(
            "INSERT INTO m_products(id,brand_id,sku,data)VALUES(?1,?2,?3,?4)",
            params![
                row["id"].as_str(),
                b["id"].as_str(),
                row["sku"].as_str(),
                row.to_string()
            ],
        )
        .unwrap();
    }
    assert!(validate_database(&c).is_err());
}

#[test]
fn database_validation_accepts_legitimate_large_prepared_context() {
    let mut c = db();
    let b = brand(&mut c, "大快照");
    let mut fs = vec![];
    for n in 0..100 {
        fs.push(json!({"id":format!("f-{n}"),"label":"有效大资料","value":"参".repeat(3000),"source":"据".repeat(2000),"visibility":"public","validUntil":null}));
    }
    let p = call(
        &mut c,
        "product.save",
        json!({"brandId":b["id"],"name":"大商品","sku":"large","category":"","officialUrl":"","sourceText":"","privateNotes":"","facts":fs}),
    );
    let p = confirm(&mut c, &p);
    let context = call(
        &mut c,
        "internal.ai.prepare",
        json!({"brandId":b["id"],"productId":p["id"],"kind":"description"}),
    );
    assert!(context.to_string().len() > 2_100_000);
    assert!(
        validate_database(&c).is_ok(),
        "valid request produced a restorable historical context"
    );
}

#[test]
fn database_validation_rejects_noncanonical_stored_enum_values() {
    for case in ["fact", "draft"] {
        let (c, f) = restore_fixture();
        if case == "fact" {
            let mut p = f["product"].clone();
            p["facts"][0]["visibility"] = json!(" public ");
            c.execute(
                "UPDATE m_products SET data=?1 WHERE id=?2",
                params![p.to_string(), p["id"].as_str()],
            )
            .unwrap();
        } else {
            let mut d = f["draft"].clone();
            d["kind"] = json!(" description ");
            c.execute(
                "UPDATE m_drafts SET data=?1 WHERE id=?2",
                params![d.to_string(), d["id"].as_str()],
            )
            .unwrap();
        }
        assert!(
            validate_database(&c).is_err(),
            "accepted noncanonical {case} choice"
        );
    }
}

#[test]
fn commerce_structured_facts_reject_invalid_values_and_require_fresh_prices() {
    let mut c = db();
    let b = brand(&mut c, "购买档案测试");
    let p = product(&mut c, &b);
    for (key, value) in [
        ("price", "-1"),
        ("price", "1e9"),
        ("currency", "人民币"),
        ("availability", "假库存"),
        ("purchase", "javascript:alert(1)"),
        ("image", "http://localhost/a.png"),
    ] {
        let mut input = p.clone();
        input["expectedRevision"] = p["revision"].clone();
        input["facts"] = json!([{"id":format!("commerce:{key}"),"label":key,"value":value,"source":"虚构测试","visibility":"public","validUntil":null}]);
        assert!(
            handle(&mut c, "product.save", input).is_err(),
            "must reject {key}: {value}"
        );
    }
    let mut input = p.clone();
    input["expectedRevision"] = p["revision"].clone();
    input["facts"] = json!([{"id":"commerce:price","label":"价格","value":"19.90","source":"虚构价目表","visibility":"public","validUntil":null}]);
    let saved = call(&mut c, "product.save", input);
    assert!(handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":saved["revision"]})
    )
    .is_err());
}

fn experiment_fixture(c: &mut Connection, b: &Value, p: &Value) -> Value {
    let q = call(
        c,
        "question.save",
        json!({"brandId":b["id"],"productId":p["id"],"text":"通勤选哪种水壶？","intent":"selection","origin":"customer","source":"虚构测试问题","branded":false,"confirmed":true}),
    );
    call(
        c,
        "campaign.create",
        json!({"brandId":b["id"],"name":"虚构测试批次","questionIds":[q["id"]],"platforms":["doubao","yuanbao"],"modelLabel":"平台未显示","searchMode":"联网","context":"中国大陆；新对话；测试账号","repetitions":3}),
    )
}
#[test]
fn experiment_freezes_questions_and_rejects_duplicate_cells_or_wrong_conditions() {
    let mut c = db();
    let b = brand(&mut c, "实验测试");
    let p = product(&mut c, &b);
    let campaign = experiment_fixture(&mut c, &b, &p);
    let q = &campaign["questions"][0];
    let input = json!({"brandId":b["id"],"productId":p["id"],"questionId":q["id"],"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":campaign["modelLabel"],"searchMode":campaign["searchMode"],"testedAt":"2026-10-23T00:00:00Z","answer":"虚构回答推荐该商品","mentioned":true,"accuracy":"correct","citedUrl":"","evidenceNote":"模拟验收","assessment":{"campaignId":campaign["id"],"repetition":1,"context":campaign["context"],"recommendation":"yes","rank":1,"competitors":"虚构竞品"}});
    let o = call(&mut c, "observation.add", input.clone());
    assert_eq!(
        call(&mut c, "observation.add", input.clone())["id"],
        o["id"]
    );
    let mut changed = input.clone();
    changed["answer"] = json!("另一份回答");
    assert!(handle(&mut c, "observation.add", changed).is_err());
    let mut changed = input.clone();
    changed["assessment"]["repetition"] = json!(4);
    assert!(handle(&mut c, "observation.add", changed).is_err());
    let mut changed = input.clone();
    changed["surface"] = json!("api");
    assert!(handle(&mut c, "observation.add", changed).is_err());
    let mut changed = input.clone();
    changed["mentioned"] = json!(false);
    assert!(handle(&mut c, "observation.add", changed).is_err());
    let other = brand(&mut c, "其他实验品牌");
    let mut changed = input.clone();
    changed["brandId"] = other["id"].clone();
    assert!(handle(&mut c, "observation.add", changed).is_err());
    validate_database(&c).unwrap();
    call(
        &mut c,
        "question.confirm",
        json!({"brandId":b["id"],"id":q["id"],"confirmed":false}),
    );
    assert!(handle(&mut c, "observation.add", input).is_err());
    validate_database(&c).unwrap();
}
#[test]
fn experiment_rejects_brand_leaking_and_cross_brand_question_plans() {
    let mut c = db();
    let b = brand(&mut c, "实验测试");
    let p = product(&mut c, &b);
    let q = call(
        &mut c,
        "question.save",
        json!({"brandId":b["id"],"productId":p["id"],"text":"实验测试电水壶值得买吗？","intent":"selection","origin":"customer","source":"测试","branded":false,"confirmed":true}),
    );
    assert!(handle(&mut c,"campaign.create",json!({"brandId":b["id"],"name":"实验","questionIds":[q["id"]],"platforms":["doubao"],"modelLabel":"x","searchMode":"x","context":"x","repetitions":3})).is_err());
}

#[test]
fn commerce_v1_backup_migrates_without_losing_rows_and_v2_roundtrips() {
    let mut legacy = db();
    crate::ai::initialize(&mut legacy).unwrap();
    let b = brand(&mut legacy, "旧版资料");
    let p = product(&mut legacy, &b);
    legacy.execute_batch("DROP TABLE m_optimization_plans; DROP TABLE m_local_jobs; DROP TABLE m_attribution_datasets; DROP TABLE m_campaigns; UPDATE m_core_meta SET schema_version=1; PRAGMA user_version=1;").unwrap();
    let bytes = crate::backup::encrypt_snapshot(&legacy, "fictional-test-password").unwrap();
    let mut target = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut target, &bytes, "fictional-test-password").unwrap();
    assert_eq!(
        call(
            &mut target,
            "workspace.snapshot",
            json!({"brandId":b["id"]})
        )["products"][0],
        p
    );
    let campaign = experiment_fixture(&mut target, &b, &p);
    let bytes = crate::backup::encrypt_snapshot(&target, "fictional-test-password").unwrap();
    let mut other = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut other, &bytes, "fictional-test-password").unwrap();
    assert_eq!(
        call(&mut other, "workspace.snapshot", json!({"brandId":b["id"]}))["campaigns"][0],
        campaign
    );
}

#[test]
fn commerce_site_is_review_gated_private_safe_and_preview_not_publish() {
    let mut c = db();
    let b = brand(&mut c, "虚构品牌");
    let p = product(&mut c, &b);
    let p = confirm(&mut c, &p);
    let d = draft(&mut c, &p);
    let input =
        json!({"brandId":b["id"],"baseUrl":"","drafts":[{"id":d["id"],"expectedRevision":1}]});
    assert!(handle(&mut c, "site.prepare", input.clone()).is_err());
    review(&mut c, &d);
    let site = call(&mut c, "site.prepare", input.clone());
    let all = site.to_string();
    for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
        assert!(!all.contains(secret))
    }
    let files = site["files"].as_array().unwrap();
    assert!(files.iter().any(|f| f["path"] == "catalog.json"));
    assert!(files.iter().any(
        |f| f["path"] == "robots.txt" && f["content"].as_str().unwrap().contains("Disallow: /")
    ));
    assert!(!files.iter().any(|f| f["path"] == "sitemap.xml"));
    let html = files
        .iter()
        .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
        .unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(html.contains("noindex"));
    assert!(html.contains("<article>"));
    assert!(html.contains("application/ld+json"));
    assert!(!html.contains("\"offers\""));
    let mut live = input.clone();
    live["baseUrl"] = json!("https://example.com/store/");
    let site = call(&mut c, "site.prepare", live);
    let all = site.to_string();
    assert!(all.contains("sitemap.xml"));
    assert!(all.contains("canonical"));
    assert!(!all.contains("noindex"));
    let mut stale = p.clone();
    stale["expectedRevision"] = p["revision"].clone();
    call(&mut c, "product.save", stale);
    assert!(handle(&mut c, "site.prepare", input).is_err());
}
#[test]
fn commerce_site_escapes_script_delimiters_in_jsonld_and_body() {
    let mut c = db();
    let b = brand(&mut c, "虚构品牌");
    let p = product(&mut c, &b);
    let mut input = p.clone();
    input["expectedRevision"] = json!(1);
    input["name"] = json!("</script><script>alert(1)</script>");
    let p = call(&mut c, "product.save", input);
    let p = confirm(&mut c, &p);
    let d = draft(&mut c, &p);
    review(&mut c, &d);
    let site = call(
        &mut c,
        "site.prepare",
        json!({"brandId":b["id"],"baseUrl":"https://example.com/","drafts":[{"id":d["id"],"expectedRevision":1}]}),
    );
    for f in site["files"].as_array().unwrap() {
        if f["path"].as_str().unwrap().ends_with(".html") {
            assert!(!f["content"].as_str().unwrap().contains("<script>alert"));
        }
    }
}

#[test]
fn commerce_complete_fixture_loop_with_240_observations_export_and_restore() {
    let mut c = db();
    crate::ai::initialize(&mut c).unwrap();
    c.pragma_update(None, "user_version", 5).unwrap();
    let b = brand(&mut c, "虚构商品验收（非真实效果）");
    let mut products = vec![];
    let mut drafts = vec![];
    let mut qids = vec![];
    let queries = [
        "通勤水杯怎么选？",
        "小容量水杯适合什么场景？",
        "办公桌水杯应注意哪些参数？",
        "随身水杯有哪些使用限制？",
        "买水杯怎样核对售后政策？",
        "水杯价格比较要看哪些条件？",
        "怎样确认杯盖是否方便清洗？",
        "轻便水杯购买前需要哪些信息？",
        "如何比较不同水杯的容量？",
        "选购水杯怎样查看真实参数依据？",
    ];
    for n in 0..3 {
        let mut fs = vec![
            json!({"id":"spec-capacity","label":"容量","value":format!("{}mL",350+n*100),"source":"虚构参数表，仅验收软件","visibility":"public","validUntil":null}),
            json!({"id":"private","label":"内部信息","value":"PRIVATE_SECRET","source":"内部","visibility":"private","validUntil":null}),
        ];
        for (k, label, value) in [
            ("price", "价格", "19.90"),
            ("currency", "币种", "CNY"),
            ("availability", "库存状态", "InStock"),
            ("audience", "适用人群", "虚构办公人群"),
            ("scenario", "使用场景", "虚构通勤场景"),
            ("limits", "使用限制", "仅作测试，不出售"),
            ("purchase", "购买链接", "https://example.com/fictional"),
            ("afterSales", "售后说明", "虚构测试，无真实售后承诺"),
            ("image", "商品图片", "https://example.com/fictional.png"),
        ] {
            fs.push(json!({"id":format!("commerce:{k}"),"label":label,"value":value,"source":"虚构说明书，仅软件验收","visibility":"public","validUntil":if ["price","availability"].contains(&k){json!("2099-01-01")}else{Value::Null}}));
        }
        let p = call(
            &mut c,
            "product.save",
            json!({"brandId":b["id"],"name":format!("虚构通勤杯{}（非在售）",n+1),"sku":format!("FICTION-{}",n+1),"category":"水杯","officialUrl":"https://example.com/fictional","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":fs}),
        );
        let p = confirm(&mut c, &p);
        for kind in ["description", "faq", "guide", "comparison"] {
            let d = call(
                &mut c,
                "draft.generateLocal",
                json!({"brandId":b["id"],"productId":p["id"],"kind":kind}),
            );
            review(&mut c, &d);
            drafts.push(json!({"id":d["id"],"expectedRevision":1}));
        }
        products.push(p);
    }
    for (n, query) in queries.iter().enumerate() {
        let q = call(
            &mut c,
            "question.save",
            json!({"brandId":b["id"],"productId":products[n%3]["id"],"text":query,"intent":"selection","origin":"suggestion","source":"虚构验收问题，不代表真实搜索量","branded":false,"confirmed":true}),
        );
        qids.push(q["id"].clone());
    }
    let campaign = call(
        &mut c,
        "campaign.create",
        json!({"brandId":b["id"],"name":"模拟基线与复测：10题 × 4平台 × 3次","questionIds":qids,"platforms":["doubao","yuanbao","qianwen","baidu"],"modelLabel":"模拟数据，无模型调用","searchMode":"模拟联网状态，未请求平台","context":"虚构验收；非真实平台表现","repetitions":3}),
    );
    for phase in ["baseline", "followup"] {
        for (n, q) in campaign["questions"].as_array().unwrap().iter().enumerate() {
            for platform in ["doubao", "yuanbao", "qianwen", "baidu"] {
                for repetition in 1..=3 {
                    let recommended = (n + repetition) % 3 == 0;
                    let input = json!({"brandId":b["id"],"productId":q["productId"],"questionId":q["id"],"platform":platform,"surface":"consumer","phase":phase,"modelLabel":campaign["modelLabel"],"searchMode":campaign["searchMode"],"testedAt":"2026-10-23T00:00:00Z","answer":"虚构软件验收回答，并非平台真实回答。","mentioned":recommended,"accuracy":"unknown","citedUrl":"","evidenceNote":"人工构造夹具，未向外部平台提问","assessment":{"campaignId":campaign["id"],"repetition":repetition,"context":campaign["context"],"recommendation":if recommended{"yes"}else{"no"},"rank":if recommended{json!(2)}else{Value::Null},"competitors":"虚构竞品记录"}});
                    call(&mut c, "observation.add", input);
                }
            }
        }
    }
    let snapshot = call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}));
    assert_eq!(snapshot["observations"].as_array().unwrap().len(), 240);
    assert_eq!(snapshot["publications"].as_array().unwrap().len(), 0);
    let site = call(
        &mut c,
        "site.prepare",
        json!({"brandId":b["id"],"baseUrl":"","drafts":drafts}),
    );
    assert_eq!(site["productCount"], 3);
    let bytes = crate::files::site_zip(&site).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    use std::io::Read;
    let mut catalog = String::new();
    zip.by_name("catalog.json")
        .unwrap()
        .read_to_string(&mut catalog)
        .unwrap();
    let catalog: Value = serde_json::from_str(&catalog).unwrap();
    assert_eq!(catalog["products"].as_array().unwrap().len(), 3);
    assert_eq!(catalog["products"][0]["offer"]["price"], "19.90");
    assert!(!site.to_string().contains("PRIVATE_SECRET"));
    let backup = crate::backup::encrypt_snapshot(&c, "fictional-test-password").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &backup, "fictional-test-password").unwrap();
    assert_eq!(
        call(
            &mut restored,
            "workspace.snapshot",
            json!({"brandId":b["id"]})
        ),
        snapshot
    );
    if let Ok(dir) = std::env::var("MGEO_FIXTURE_OUTPUT") {
        assert!(dir.contains("mgeo-e2e-"));
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let mut db = Connection::open(dir.join("fictional.sqlite3")).unwrap();
        rusqlite::backup::Backup::new(&c, &mut db)
            .unwrap()
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .unwrap();
        drop(db);
        let mut reopened = crate::runtime::open_database(&dir.join("fictional.sqlite3")).unwrap();
        assert_eq!(
            call(
                &mut reopened,
                "workspace.snapshot",
                json!({"brandId":b["id"]})
            ),
            snapshot
        );
        std::fs::write(dir.join("fictional-website.zip"), bytes).unwrap();
        std::fs::write(dir.join("fixture-result.json"),serde_json::to_string_pretty(&json!({"passed":true,"brandId":b["id"],"products":3,"questions":10,"platforms":4,"repetitions":3,"baseline":120,"followup":120,"drafts":12,"publications":0,"modelCalls":0,"realPlatformCalls":0,"backupRestored":true,"reopened":true,"note":"All products and observations are synthetic fixtures, not measured platform results."})).unwrap()).unwrap();
    }
}

#[test]
fn review_fix_padded_fact_ids_cannot_bypass_commerce_validation() {
    let mut c = db();
    let b = brand(&mut c, "字段校验");
    let p = product(&mut c, &b);
    for (key, value) in [
        (" commerce:price", "-9"),
        ("commerce:purchase ", "javascript:alert(1)"),
    ] {
        let mut input = p.clone();
        input["expectedRevision"] = json!(1);
        input["facts"] = json!([{"id":key,"label":"测试","value":value,"source":"虚构","visibility":"public","validUntil":"2099-01-01"}]);
        assert!(handle(&mut c, "product.save", input).is_err());
    }
}
#[test]
fn review_fix_multibyte_campaign_never_breaks_backup() {
    let mut c = db();
    crate::ai::initialize(&mut c).unwrap();
    c.pragma_update(None, "user_version", 5).unwrap();
    let b = brand(&mut c, "长问题测试");
    let p = product(&mut c, &b);
    let mut ids = vec![];
    for n in 0..100 {
        let q = call(
            &mut c,
            "question.save",
            json!({"brandId":b["id"],"productId":p["id"],"text":format!("{}选择条件{n}","选".repeat(970)),"intent":"selection","origin":"suggestion","source":"测试","branded":false,"confirmed":true}),
        );
        ids.push(q["id"].clone());
    }
    let result = handle(
        &mut c,
        "campaign.create",
        json!({"brandId":b["id"],"name":"大计划","questionIds":ids,"platforms":["doubao"],"modelLabel":"测试","searchMode":"测试","context":"测试","repetitions":1}),
    );
    if result.is_err() {
        assert_eq!(
            call(&mut c, "workspace.snapshot", json!({"brandId":b["id"]}))["campaigns"],
            json!([])
        );
    }
    assert!(crate::backup::encrypt_snapshot(&c, "fictional-test-password").is_ok());
}
#[test]
fn review_fix_annotations_can_change_without_changing_raw_evidence_or_submission_dedup() {
    let mut c = db();
    let b = brand(&mut c, "补评测试");
    let p = product(&mut c, &b);
    let plan = experiment_fixture(&mut c, &b, &p);
    let q = &plan["questions"][0];
    let input = json!({"brandId":b["id"],"productId":p["id"],"questionId":q["id"],"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":plan["modelLabel"],"searchMode":plan["searchMode"],"testedAt":"2026-10-23T00:00:00Z","answer":"虚构推荐回答，原文固定","mentioned":false,"accuracy":"unknown","citedUrl":"","evidenceNote":"待核对","assessment":{"campaignId":plan["id"],"repetition":1,"context":plan["context"],"recommendation":"unknown","rank":null,"competitors":""}});
    let o = call(&mut c, "observation.add", input.clone());
    let edit = json!({"brandId":b["id"],"id":o["id"],"expectedRevision":1,"mentioned":true,"accuracy":"correct","citedUrl":"https://example.com/source","evidenceNote":"人工核对虚构夹具","recommendation":"yes","rank":2,"competitors":"虚构竞品"});
    let updated = call(&mut c, "observation.assess", edit.clone());
    assert_eq!(updated["assessmentRevision"], 2);
    assert_eq!(
        updated["assessmentHistory"][0]["assessment"]["recommendation"],
        "unknown"
    );
    for key in [
        "answer",
        "querySnapshot",
        "testedAt",
        "productId",
        "questionId",
        "platform",
        "surface",
        "phase",
        "modelLabel",
        "searchMode",
    ] {
        assert_eq!(updated[key], o[key]);
    }
    assert_eq!(updated["assessment"]["campaignId"], plan["id"]);
    assert!(handle(&mut c, "observation.assess", edit.clone()).is_err());
    assert_eq!(call(&mut c, "observation.add", input)["id"], o["id"]);
    validate_database(&c).unwrap();
    let mut second = edit.clone();
    second["expectedRevision"] = json!(2);
    second["rank"] = json!(3);
    let second = call(&mut c, "observation.assess", second);
    assert_eq!(second["assessmentRevision"], 3);
    assert_eq!(second["assessmentHistory"].as_array().unwrap().len(), 2);
    validate_database(&c).unwrap();
    crate::ai::initialize(&mut c).unwrap();
    c.pragma_update(None, "user_version", 5).unwrap();
    let bytes = crate::backup::encrypt_snapshot(&c, "fictional-test-password").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-test-password").unwrap();
    assert_eq!(
        call(
            &mut restored,
            "workspace.snapshot",
            json!({"brandId":b["id"]})
        )["observations"][0],
        second
    );
    let other = brand(&mut c, "另一个补评品牌");
    let mut edit = edit;
    edit["brandId"] = other["id"].clone();
    edit["expectedRevision"] = json!(2);
    assert!(handle(&mut c, "observation.assess", edit).is_err());
}

#[test]
fn phase3_evidence_mode_survives_backup_and_reports_without_reclassifying_history() {
    let mut c = db();
    let b = brand(&mut c, "第三阶段虚构模式验收");
    let p = product(&mut c, &b);
    let historical = experiment_fixture(&mut c, &b, &p);
    assert_eq!(historical["evidenceMode"], "unspecified");
    let mut input = json!({"brandId":b["id"],"name":"虚构模拟批次","questionIds":[historical["questions"][0]["id"]],"platforms":["doubao"],"modelLabel":"模拟","searchMode":"不联网","context":"虚构工作流验收","repetitions":1,"evidenceMode":"simulated"});
    let simulated = call(&mut c, "campaign.create", input.clone());
    assert_eq!(simulated["evidenceMode"], "simulated");
    input["evidenceMode"] = json!("verified");
    assert!(handle(&mut c, "campaign.create", input).is_err());
    let report = call(
        &mut c,
        "campaign.report",
        json!({"brandId":b["id"],"campaignId":simulated["id"]}),
    );
    assert_eq!(report["summary"]["evidenceMode"], "simulated");
    assert!(report["files"][0]["content"]
        .as_str()
        .unwrap()
        .contains("模拟样本"));
    let mut old = historical.clone();
    old.as_object_mut().unwrap().remove("evidenceMode");
    update(&c, "m_campaigns", &old).unwrap();
    let legacy_report = call(
        &mut c,
        "campaign.report",
        json!({"brandId":b["id"],"campaignId":historical["id"]}),
    );
    assert_eq!(legacy_report["summary"]["evidenceMode"], "unspecified");
    validate_database(&c).unwrap();
    let mut restored = db();
    rusqlite::backup::Backup::new(&c, &mut restored)
        .unwrap()
        .run_to_completion(16, std::time::Duration::ZERO, None)
        .unwrap();
    validate_database(&restored).unwrap();
    let snap = call(
        &mut restored,
        "workspace.snapshot",
        json!({"brandId":b["id"]}),
    );
    assert_eq!(
        snap["campaigns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == simulated["id"])
            .unwrap()["evidenceMode"],
        "simulated"
    );
    assert!(snap["campaigns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == old["id"])
        .unwrap()
        .get("evidenceMode")
        .is_none());
}
