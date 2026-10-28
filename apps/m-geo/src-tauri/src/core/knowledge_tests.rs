use super::*;
use crate::runtime::{core_dispatch, open_database};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;

fn db() -> Connection {
    open_database(Path::new(":memory:")).unwrap()
}

fn brand(c: &mut Connection, name: &str) -> String {
    core_dispatch(c, "brand.create", json!({"name":name})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn product(c: &mut Connection, brand_id: &str, sku: &str) -> String {
    core_dispatch(
        c,
        "product.save",
        json!({"brandId":brand_id,"name":"虚构知识商品","sku":sku,"facts":[]}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn import_payload(brand_id: &str, product_id: Option<&str>, request_id: &str) -> Value {
    json!({
        "brandId":brand_id,
        "productId":product_id,
        "title":"虚构产品知识",
        "source":"虚构内部测试资料",
        "format":"markdown",
        "effectiveAt":"2020-01-01",
        "risk":"medium",
        "content":"第一段虚构知识。\n\n第二段包含中文🙂与原始顺序。",
        "requestId":request_id
    })
}

fn import(c: &mut Connection, payload: Value) -> Value {
    core_dispatch(c, "knowledge.import", payload).unwrap()
}

fn revise_payload(
    brand_id: &str,
    product_id: Option<&str>,
    document_id: &str,
    expected_revision: u64,
    request_id: &str,
) -> Value {
    json!({
        "brandId":brand_id,
        "id":document_id,
        "expectedRevision":expected_revision,
        "productId":product_id,
        "title":" 修订后的虚构产品知识 ",
        "source":" 修订后的虚构内部资料 ",
        "format":"text",
        "effectiveAt":null,
        "risk":"high",
        "content":"修订后的第一段虚构知识。\n\n第二段已重新确定性切片🙂。",
        "requestId":request_id
    })
}

fn revise(c: &Connection, brand_id: &str, payload: Value) -> Value {
    super::knowledge::revise(c, brand_id, &payload).unwrap()
}

fn detail(c: &mut Connection, brand_id: &str, document_id: &str) -> Value {
    core_dispatch(
        c,
        "knowledge.list",
        json!({"brandId":brand_id,"documentId":document_id}),
    )
    .unwrap()
}

fn approve(c: &mut Connection, brand_id: &str, document: &Value) -> Value {
    core_dispatch(
        c,
        "knowledge.review",
        json!({"brandId":brand_id,"id":document["id"],"expectedRevision":document["revision"],"decision":"approved","note":"虚构人工审核"}),
    )
    .unwrap()
}

fn import_approved(c: &mut Connection, brand_id: &str, payload: Value) -> Value {
    let document = import(c, payload);
    approve(c, brand_id, &document)
}

fn recall(c: &mut Connection, brand_id: &str, query: &str, product_id: Option<&str>) -> Value {
    core_dispatch(
        c,
        "knowledge.recall",
        json!({"brandId":brand_id,"query":query,"productId":product_id,"limit":20}),
    )
    .unwrap()
}

#[test]
fn knowledge_recall_is_brand_product_and_review_scoped_with_traceable_minimal_hits() {
    let mut c = db();
    let b = brand(&mut c, "虚构召回品牌");
    let other = brand(&mut c, "虚构召回其他品牌");
    let p = product(&mut c, &b, "RECALL-CURRENT");
    let other_product = product(&mut c, &b, "RECALL-OTHER");
    let foreign_product = product(&mut c, &other, "RECALL-FOREIGN");
    let mut generic_input = import_payload(&b, None, &Uuid::new_v4().to_string());
    generic_input["title"] = json!("通用保温杯资料");
    generic_input["content"] = json!("通用容量为虚构 500 毫升。\n\n清洗时使用软布。");
    let generic = import_approved(&mut c, &b, generic_input);
    let mut current_input = import_payload(&b, Some(&p), &Uuid::new_v4().to_string());
    current_input["content"] = json!("当前商品容量为虚构 500 毫升。");
    let current = import_approved(&mut c, &b, current_input);
    let mut other_input = import_payload(&b, Some(&other_product), &Uuid::new_v4().to_string());
    other_input["content"] = json!("其他商品绝密标记 OTHER-PRODUCT-CANARY 容量 500。");
    import_approved(&mut c, &b, other_input);
    let pending = import(
        &mut c,
        import_payload(&b, Some(&p), &Uuid::new_v4().to_string()),
    );
    assert_eq!(pending["reviewStatus"], "pending");
    let mut rejected_input = import_payload(&b, Some(&p), &Uuid::new_v4().to_string());
    rejected_input["content"] = json!("驳回文档包含虚构 500 毫升。");
    let rejected = import(&mut c, rejected_input);
    core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":rejected["id"],"expectedRevision":1,"decision":"rejected","note":"虚构资料不充分"}),
    )
    .unwrap();
    let mut foreign_input = import_payload(&other, None, &Uuid::new_v4().to_string());
    foreign_input["content"] = json!("其他品牌绝密标记 FOREIGN-BRAND-CANARY 容量 500。");
    import_approved(&mut c, &other, foreign_input);

    let hits = recall(&mut c, &b, "容量 500", Some(&p));
    let hits = hits.as_array().unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits
        .iter()
        .all(|hit| { hit["documentId"] == generic["id"] || hit["documentId"] == current["id"] }));
    assert!(!hits
        .iter()
        .any(|hit| hit["text"].as_str().unwrap().contains("CANARY")));
    let hit = hits
        .iter()
        .find(|hit| hit["documentId"] == current["id"])
        .unwrap();
    let current_detail = detail(&mut c, &b, current["id"].as_str().unwrap());
    let chunk = current_detail["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|chunk| chunk["id"] == hit["chunkId"])
        .unwrap();
    assert_eq!(hit["documentRevision"], current_detail["revision"]);
    assert_eq!(hit["documentTitle"], current_detail["title"]);
    assert_eq!(hit["source"], current_detail["source"]);
    assert_eq!(hit["reviewedAt"], current_detail["reviewedAt"]);
    assert_eq!(hit["chunkOrdinal"], chunk["ordinal"]);
    assert_eq!(hit["text"], chunk["text"]);
    assert_eq!(hit["sha256"], chunk["sha256"]);
    let mut keys: Vec<_> = hit.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "chunkId",
            "chunkOrdinal",
            "documentId",
            "documentRevision",
            "documentTitle",
            "effectiveAt",
            "productId",
            "reviewedAt",
            "risk",
            "score",
            "sha256",
            "source",
            "text"
        ]
    );
    assert!(hit.get("content").is_none());
    assert!(hit.get("reviews").is_none());
    assert!(hit.get("brandId").is_none());
    assert!(recall(&mut c, &b, "第一段", Some(&p))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(recall(&mut c, &b, "FOREIGN-BRAND-CANARY", None)
        .as_array()
        .unwrap()
        .is_empty());
    assert!(core_dispatch(
        &mut c,
        "knowledge.recall",
        json!({"brandId":b,"query":"容量","productId":foreign_product})
    )
    .is_err());
    assert!(core_dispatch(
        &mut c,
        "knowledge.recall",
        json!({"brandId":other,"query":"容量","productId":p})
    )
    .is_err());
}

#[test]
fn knowledge_recall_revision_invalidation_reapproval_and_read_only_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge-recall-reopen.sqlite3");
    let mut c = open_database(&path).unwrap();
    let b = brand(&mut c, "虚构召回重开品牌");
    let p = product(&mut c, &b, "RECALL-REOPEN");
    let mut original_input = import_payload(&b, Some(&p), &Uuid::new_v4().to_string());
    original_input["content"] = json!("旧版已审核关键词 OLD-APPROVED。");
    let original = import(&mut c, original_input);
    let id = original["id"].as_str().unwrap().to_owned();
    approve(&mut c, &b, &original);
    assert_eq!(
        recall(&mut c, &b, "OLD-APPROVED", Some(&p))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut revised_input = revise_payload(&b, Some(&p), &id, 2, &Uuid::new_v4().to_string());
    revised_input["content"] = json!("新版待审核关键词 NEW-PENDING。");
    let revised = revise(&c, &b, revised_input);
    assert_eq!(revised["revision"], 3);
    assert!(recall(&mut c, &b, "OLD-APPROVED", Some(&p))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(recall(&mut c, &b, "NEW-PENDING", Some(&p))
        .as_array()
        .unwrap()
        .is_empty());
    let reapproved = approve(&mut c, &b, &revised);
    let before_document = detail(&mut c, &b, &id);
    let audit_before: i64 = c
        .query_row("SELECT count(*) FROM m_audit", [], |row| row.get(0))
        .unwrap();
    let changes = c.total_changes();
    let first = recall(&mut c, &b, "NEW-PENDING", Some(&p));
    let second = recall(&mut c, &b, "NEW-PENDING", Some(&p));
    assert_eq!(first, second);
    assert_eq!(first.as_array().unwrap()[0]["documentRevision"], 4);
    assert_eq!(
        first.as_array().unwrap()[0]["reviewedAt"],
        reapproved["reviewedAt"]
    );
    assert_eq!(detail(&mut c, &b, &id), before_document);
    assert_eq!(c.total_changes(), changes);
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_audit", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        audit_before
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_drafts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(c);
    let mut reopened = open_database(&path).unwrap();
    assert_eq!(recall(&mut reopened, &b, "NEW-PENDING", Some(&p)), first);
    crate::core::validate_database(&reopened).unwrap();
}

#[test]
fn knowledge_recall_validates_contract_without_writes() {
    let mut c = db();
    let b = brand(&mut c, "虚构召回输入品牌");
    let changes = c.total_changes();
    let valid = json!({"brandId":b,"query":"保温"});
    for mut invalid in [
        json!({"brandId":b,"query":"保温","unexpected":true}),
        json!({"brandId":b,"query":""}),
        json!({"brandId":b,"query":"   "}),
        json!({"brandId":b,"query":"bad\0query"}),
        json!({"brandId":b,"query":"超".repeat(201)}),
        json!({"brandId":b,"query":"保温","productId":"bad"}),
        json!({"brandId":b,"query":"保温","limit":0}),
        json!({"brandId":b,"query":"保温","limit":21}),
        json!({"brandId":b,"query":"保温","limit":-1}),
        json!({"brandId":b,"query":"保温","limit":"20"}),
    ] {
        assert!(
            core_dispatch(&mut c, "knowledge.recall", invalid.clone()).is_err(),
            "accepted {invalid}"
        );
        invalid.as_object_mut().unwrap().remove("query");
        assert!(core_dispatch(&mut c, "knowledge.recall", invalid).is_err());
    }
    assert_eq!(
        core_dispatch(&mut c, "knowledge.recall", valid).unwrap(),
        json!([])
    );
    assert_eq!(c.total_changes(), changes);
}

#[test]
fn knowledge_recall_matches_chinese_case_whitespace_and_ranks_deterministically() {
    let mut c = db();
    let b = brand(&mut c, "虚构召回排序品牌");
    let p = product(&mut c, &b, "RECALL-RANK");
    let mut documents = Vec::new();
    for (title, content) in [
        ("虚构 CUP 保养", "保温性能仅作虚构测试，容量 500。"),
        ("虚构保温资料", "CUP 容量 500，保温仅作虚构测试。"),
        ("虚构同分资料", "CUP 容量 500，保温仅作虚构测试。"),
    ] {
        let mut input = import_payload(&b, Some(&p), &Uuid::new_v4().to_string());
        input["title"] = json!(title);
        input["content"] = json!(content);
        documents.push(import_approved(&mut c, &b, input));
    }
    let hits = recall(&mut c, &b, "  保温   cup 500 ", Some(&p));
    let hits = hits.as_array().unwrap();
    assert_eq!(hits.len(), 3);
    assert!(hits[0]["score"].as_u64().unwrap() >= hits[1]["score"].as_u64().unwrap());
    assert!(hits[1]["score"].as_u64().unwrap() >= hits[2]["score"].as_u64().unwrap());
    for pair in hits.windows(2) {
        if pair[0]["score"] == pair[1]["score"] {
            assert!(pair[0]["documentId"].as_str() < pair[1]["documentId"].as_str());
        }
    }
    assert_eq!(
        recall(&mut c, &b, "保温 cup 500", Some(&p)),
        Value::Array(hits.clone())
    );
    assert_eq!(recall(&mut c, &b, "保温 500 缺失词", Some(&p)), json!([]));
    let limited = core_dispatch(
        &mut c,
        "knowledge.recall",
        json!({"brandId":b,"query":"保温 cup 500","productId":p,"limit":1}),
    )
    .unwrap();
    assert_eq!(limited.as_array().unwrap().len(), 1);
    assert_eq!(limited[0], hits[0]);
    let unique: std::collections::HashSet<_> = hits
        .iter()
        .map(|hit| hit["chunkId"].as_str().unwrap())
        .collect();
    assert_eq!(unique.len(), hits.len());
    assert_eq!(documents.len(), 3);
    assert_eq!(
        recall(&mut c, &b, "保温 cup 500 cup 保温", Some(&p)),
        Value::Array(hits.clone())
    );
    assert_eq!(recall(&mut c, &b, "CU", Some(&p)), json!([]));
    assert_eq!(recall(&mut c, &b, "50", Some(&p)), json!([]));
}

#[test]
fn knowledge_recall_is_bounded_and_deterministic_at_document_and_chunk_limits() {
    let mut c = db();
    let b = brand(&mut c, "虚构召回容量品牌");
    for document_index in 0..100 {
        let content = (0..200)
            .map(|chunk_index| format!("{document_index:03}-{chunk_index:03}-{}", "知".repeat(592)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut input = import_payload(&b, None, &Uuid::new_v4().to_string());
        input["content"] = json!(content);
        let document = import_approved(&mut c, &b, input);
        assert_eq!(document["chunks"].as_array().unwrap().len(), 200);
    }
    let changes = c.total_changes();
    let first = recall(&mut c, &b, "知", None);
    let second = recall(&mut c, &b, "知", None);
    assert_eq!(first, second);
    let hits = first.as_array().unwrap();
    assert_eq!(hits.len(), 20);
    assert!(hits.windows(2).all(|pair| {
        pair[0]["documentId"].as_str() < pair[1]["documentId"].as_str()
            || pair[0]["documentId"] == pair[1]["documentId"]
                && pair[0]["chunkOrdinal"].as_u64() < pair[1]["chunkOrdinal"].as_u64()
    }));
    assert_eq!(c.total_changes(), changes);
}

#[test]
fn knowledge_recall_survives_backup_restore_without_model_settings() {
    let mut source = db();
    let b = brand(&mut source, "虚构召回备份品牌");
    let p = product(&mut source, &b, "RECALL-BACKUP");
    let mut input = import_payload(&b, Some(&p), &Uuid::new_v4().to_string());
    input["content"] = json!("虚构可恢复关键词 RESTORE-RECALL。");
    let approved = import_approved(&mut source, &b, input);
    let before = recall(&mut source, &b, "RESTORE-RECALL", Some(&p));
    assert_eq!(before.as_array().unwrap().len(), 1);
    source
        .execute("UPDATE ai_settings SET key_ref='local-only-secret-ref'", [])
        .unwrap();
    let bytes = crate::backup::encrypt_snapshot(&source, "recall-test-password").unwrap();
    let mut restored = db();
    crate::backup::restore_snapshot(&mut restored, &bytes, "recall-test-password").unwrap();
    assert_eq!(
        recall(&mut restored, &b, "RESTORE-RECALL", Some(&p)),
        before
    );
    assert_eq!(
        detail(&mut restored, &b, approved["id"].as_str().unwrap()),
        approved
    );
    assert_eq!(
        restored
            .query_row("SELECT count(*) FROM ai_settings", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    crate::core::validate_database(&restored).unwrap();
}

#[test]
fn knowledge_import_list_summary_and_detail_follow_contract() {
    let mut c = db();
    let b = brand(&mut c, "虚构知识品牌");
    let p = product(&mut c, &b, "KNOW-1");
    let first = import(
        &mut c,
        import_payload(&b, Some(&p), &Uuid::new_v4().to_string()),
    );
    let second = import(
        &mut c,
        import_payload(&b, None, &Uuid::new_v4().to_string()),
    );
    assert_eq!(first["reviewStatus"], "pending");
    assert_eq!(first["reviews"], json!([]));
    assert_eq!(first["productId"], p);
    assert!(first["content"].as_str().unwrap().contains("第一段"));
    assert_eq!(first["chunks"].as_array().unwrap().len(), 1);
    assert_eq!(first["chunks"][0]["ordinal"], 0);
    assert_eq!(first["chunks"][0]["sha256"].as_str().unwrap().len(), 64);

    let list = core_dispatch(&mut c, "knowledge.list", json!({"brandId":b})).unwrap();
    let summaries = list.as_array().unwrap();
    assert_eq!(summaries.len(), 2);
    let mut expected = [first.clone(), second.clone()];
    expected.sort_by(|left, right| {
        right["createdAt"]
            .as_str()
            .cmp(&left["createdAt"].as_str())
            .then_with(|| right["id"].as_str().cmp(&left["id"].as_str()))
    });
    assert_eq!(summaries[0]["id"], expected[0]["id"]);
    assert_eq!(summaries[1]["id"], expected[1]["id"]);
    let keys: Vec<_> = summaries[0].as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        keys,
        vec![
            "brandId",
            "chunkCount",
            "createdAt",
            "effectiveAt",
            "format",
            "id",
            "productId",
            "reviewStatus",
            "reviewedAt",
            "revision",
            "risk",
            "source",
            "title",
            "updatedAt"
        ]
    );
    assert_eq!(summaries[0]["chunkCount"], 1);
    assert!(summaries[0].get("content").is_none());
    assert!(summaries[0].get("chunks").is_none());
    assert!(summaries[0].get("reviews").is_none());
    assert_eq!(detail(&mut c, &b, first["id"].as_str().unwrap()), first);
    crate::core::validate_database(&c).unwrap();
}

#[test]
fn knowledge_import_is_idempotent_per_brand_and_checks_product_ownership() {
    let mut c = db();
    let b = brand(&mut c, "虚构幂等品牌");
    let other = brand(&mut c, "虚构其他品牌");
    let p = product(&mut c, &b, "OWNED");
    let foreign_product = product(&mut c, &other, "FOREIGN");
    let request = Uuid::new_v4().to_string();
    let payload = import_payload(&b, Some(&p), &request);
    let first = import(&mut c, payload.clone());
    let changes = c.total_changes();
    let replay = import(&mut c, payload.clone());
    assert_eq!(replay, first);
    assert_eq!(c.total_changes(), changes);
    let count: i64 = c
        .query_row("SELECT count(*) FROM m_knowledge_documents", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    let mut changed = payload.clone();
    changed["title"] = json!("不同完整载荷");
    assert!(core_dispatch(&mut c, "knowledge.import", changed).is_err());
    assert!(core_dispatch(
        &mut c,
        "knowledge.import",
        import_payload(&b, Some(&foreign_product), &Uuid::new_v4().to_string())
    )
    .is_err());
    let other_document = import(&mut c, import_payload(&other, None, &request));
    assert_ne!(other_document["id"], first["id"]);
    assert!(core_dispatch(
        &mut c,
        "knowledge.list",
        json!({"brandId":other,"documentId":first["id"]})
    )
    .is_err());
    assert!(core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":other,"id":first["id"],"expectedRevision":1,"decision":"approved","note":""})
    )
    .is_err());
}

#[test]
fn knowledge_revise_preserves_identity_history_and_invalidates_prior_review() {
    let mut c = db();
    let b = brand(&mut c, "虚构修订品牌");
    let p = product(&mut c, &b, "REVISE");
    let document = import(
        &mut c,
        import_payload(&b, Some(&p), &Uuid::new_v4().to_string()),
    );
    let id = document["id"].as_str().unwrap().to_owned();
    let product_before = super::load(&c, "m_products", &b, &p).unwrap();
    let approved = core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":1,"decision":"approved","note":"原审核保留"}),
    )
    .unwrap();
    let revised = revise(
        &c,
        &b,
        revise_payload(&b, Some(&p), &id, 2, &Uuid::new_v4().to_string()),
    );
    assert_eq!(revised["id"], id);
    assert_eq!(revised["brandId"], b);
    assert_eq!(revised["createdAt"], document["createdAt"]);
    assert_eq!(revised["productId"], p);
    assert_eq!(revised["title"], "修订后的虚构产品知识");
    assert_eq!(revised["source"], "修订后的虚构内部资料");
    assert_eq!(revised["format"], "text");
    assert_eq!(revised["effectiveAt"], Value::Null);
    assert_eq!(revised["risk"], "high");
    assert_eq!(revised["revision"], 3);
    assert_eq!(revised["reviewStatus"], "pending");
    assert_eq!(revised["reviewedAt"], Value::Null);
    assert_eq!(revised["reviews"], approved["reviews"]);
    assert_ne!(revised["chunks"], document["chunks"]);
    let expected_chunks = serde_json::to_value(
        super::knowledge::chunk_content(&id, revised["content"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(revised["chunks"], expected_chunks);
    crate::core::validate_database(&c).unwrap();

    let rejected = core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":3,"decision":"rejected","note":"修订后重新审核"}),
    )
    .unwrap();
    assert_eq!(rejected["revision"], 4);
    assert_eq!(rejected["reviews"].as_array().unwrap().len(), 2);
    let revised_again = revise(
        &c,
        &b,
        revise_payload(&b, None, &id, 4, &Uuid::new_v4().to_string()),
    );
    assert_eq!(revised_again["revision"], 5);
    assert_eq!(revised_again["reviewStatus"], "pending");
    assert_eq!(revised_again["reviews"], rejected["reviews"]);
    assert_eq!(revised_again["reviews"][0], approved["reviews"][0]);
    assert_eq!(revised_again["productId"], Value::Null);
    assert_eq!(
        super::load(&c, "m_products", &b, &p).unwrap(),
        product_before
    );
    crate::core::validate_database(&c).unwrap();
}

#[test]
fn knowledge_revise_and_history_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge-revise.sqlite3");
    let mut c = open_database(&path).unwrap();
    let b = brand(&mut c, "虚构修订重开品牌");
    let document = import(
        &mut c,
        import_payload(&b, None, &Uuid::new_v4().to_string()),
    );
    let id = document["id"].as_str().unwrap().to_owned();
    core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":1,"decision":"approved","note":""}),
    )
    .unwrap();
    let revised = revise(
        &c,
        &b,
        revise_payload(&b, None, &id, 2, &Uuid::new_v4().to_string()),
    );
    drop(c);
    let mut reopened = open_database(&path).unwrap();
    assert_eq!(detail(&mut reopened, &b, &id), revised);
    assert_eq!(revised["reviews"].as_array().unwrap().len(), 1);
    crate::core::validate_database(&reopened).unwrap();
}

#[test]
fn knowledge_revise_replay_conflicts_and_stale_revision_are_atomic() {
    let mut c = db();
    let b = brand(&mut c, "虚构修订幂等品牌");
    let import_request = Uuid::new_v4().to_string();
    let document = import(&mut c, import_payload(&b, None, &import_request));
    let id = document["id"].as_str().unwrap().to_owned();
    let payload = revise_payload(&b, None, &id, 1, &Uuid::new_v4().to_string());
    assert!(!super::knowledge::revise_is_replay(&c, &b, &payload).unwrap());
    let revised = core_dispatch(&mut c, "knowledge.revise", payload.clone()).unwrap();
    assert_eq!(revised["revision"], 2);
    assert!(super::knowledge::revise_is_replay(&c, &b, &payload).unwrap());
    let changes = c.total_changes();
    assert_eq!(
        core_dispatch(&mut c, "knowledge.revise", payload.clone()).unwrap(),
        revised
    );
    assert_eq!(c.total_changes(), changes);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM m_audit WHERE action='knowledge.revise'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );

    for (field, replacement) in [
        ("title", json!("不同修订载荷")),
        ("expectedRevision", json!(0)),
        ("id", json!(Uuid::new_v4().to_string())),
    ] {
        let mut conflict = payload.clone();
        conflict[field] = replacement;
        assert!(super::knowledge::revise_is_replay(&c, &b, &conflict).is_err());
        assert!(super::knowledge::revise(&c, &b, &conflict).is_err());
        assert_eq!(c.total_changes(), changes);
    }
    core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":2,"decision":"approved","note":"后续审核"}),
    )
    .unwrap();
    let current = detail(&mut c, &b, &id);
    let changes = c.total_changes();
    assert_eq!(current["revision"], 3);
    assert_eq!(revise(&c, &b, payload), current);
    assert_eq!(c.total_changes(), changes);
    let stale = revise_payload(&b, None, &id, 1, &Uuid::new_v4().to_string());
    assert!(!super::knowledge::revise_is_replay(&c, &b, &stale).unwrap());
    assert!(super::knowledge::revise(&c, &b, &stale).is_err());
    assert_eq!(detail(&mut c, &b, &id), current);
    assert_eq!(c.total_changes(), changes);
    crate::core::validate_database(&c).unwrap();
}

#[test]
fn knowledge_revise_rejects_scope_product_and_invalid_input_without_writes() {
    let mut c = db();
    let b = brand(&mut c, "虚构修订归属品牌");
    let other = brand(&mut c, "虚构修订其他品牌");
    let foreign_product = product(&mut c, &other, "FOREIGN-REVISE");
    let document = import(
        &mut c,
        import_payload(&b, None, &Uuid::new_v4().to_string()),
    );
    let id = document["id"].as_str().unwrap().to_owned();
    let before = detail(&mut c, &b, &id);
    let changes = c.total_changes();

    let cross_brand = revise_payload(&other, None, &id, 1, &Uuid::new_v4().to_string());
    assert!(super::knowledge::revise(&c, &other, &cross_brand).is_err());
    let mismatched_brand = revise_payload(&b, None, &id, 1, &Uuid::new_v4().to_string());
    assert!(super::knowledge::revise(&c, &other, &mismatched_brand).is_err());
    let missing = revise_payload(
        &b,
        None,
        &Uuid::new_v4().to_string(),
        1,
        &Uuid::new_v4().to_string(),
    );
    assert!(super::knowledge::revise(&c, &b, &missing).is_err());
    let foreign = revise_payload(
        &b,
        Some(&foreign_product),
        &id,
        1,
        &Uuid::new_v4().to_string(),
    );
    assert!(super::knowledge::revise(&c, &b, &foreign).is_err());

    for (field, replacement) in [
        ("id", json!("bad")),
        ("expectedRevision", json!("1")),
        ("title", json!(" ")),
        ("source", json!("bad\0source")),
        ("format", json!("html")),
        ("risk", json!("critical")),
        ("effectiveAt", json!("2027-02-29")),
        ("content", json!("")),
        ("requestId", json!("bad")),
        ("productId", json!("bad")),
        ("unexpected", json!(true)),
    ] {
        let mut invalid = revise_payload(&b, None, &id, 1, &Uuid::new_v4().to_string());
        invalid[field] = replacement;
        assert!(super::knowledge::revise_is_replay(&c, &b, &invalid).is_err());
        assert!(super::knowledge::revise(&c, &b, &invalid).is_err());
    }
    for field in [
        "brandId",
        "id",
        "expectedRevision",
        "title",
        "source",
        "format",
        "risk",
        "content",
        "requestId",
    ] {
        let mut invalid = revise_payload(&b, None, &id, 1, &Uuid::new_v4().to_string());
        invalid.as_object_mut().unwrap().remove(field);
        assert!(super::knowledge::revise(&c, &b, &invalid).is_err());
    }
    assert_eq!(detail(&mut c, &b, &id), before);
    assert_eq!(c.total_changes(), changes);
    crate::core::validate_database(&c).unwrap();
}

#[test]
fn knowledge_review_requires_rejection_reason_and_appends_history_only() {
    let mut c = db();
    let b = brand(&mut c, "虚构审核品牌");
    let p = product(&mut c, &b, "REVIEW");
    let document = import(
        &mut c,
        import_payload(&b, Some(&p), &Uuid::new_v4().to_string()),
    );
    let id = document["id"].as_str().unwrap();
    let product_before = super::load(&c, "m_products", &b, &p).unwrap();
    for note in [json!(null), json!(""), json!("  ")] {
        assert!(core_dispatch(
            &mut c,
            "knowledge.review",
            json!({"brandId":b,"id":id,"expectedRevision":1,"decision":"rejected","note":note})
        )
        .is_err());
    }
    assert_eq!(detail(&mut c, &b, id)["revision"], 1);

    let rejected = core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":1,"decision":"rejected","note":"虚构来源需要补充"}),
    )
    .unwrap();
    assert_eq!(rejected["reviewStatus"], "rejected");
    assert_eq!(rejected["revision"], 2);
    assert_eq!(rejected["reviews"].as_array().unwrap().len(), 1);
    assert_eq!(rejected["reviews"][0]["revision"], 2);
    assert_eq!(rejected["reviews"][0]["decision"], "rejected");
    assert_eq!(rejected["reviews"][0]["note"], "虚构来源需要补充");
    assert_eq!(rejected["reviews"][0]["documentId"], id);
    let review_keys: Vec<_> = rejected["reviews"][0]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        review_keys,
        vec![
            "createdAt",
            "decision",
            "documentId",
            "id",
            "note",
            "revision"
        ]
    );

    let approved = core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":2,"decision":"approved","note":null}),
    )
    .unwrap();
    assert_eq!(approved["reviewStatus"], "approved");
    assert_eq!(approved["revision"], 3);
    assert_eq!(approved["reviews"].as_array().unwrap().len(), 2);
    assert_eq!(approved["reviews"][1]["note"], "");
    let approved_again = core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":3,"decision":"approved","note":"人工复核通过"}),
    )
    .unwrap();
    assert_eq!(approved_again["revision"], 4);
    assert_eq!(approved_again["reviews"].as_array().unwrap().len(), 3);
    assert_eq!(approved_again["reviews"][0], rejected["reviews"][0]);
    assert!(core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":2,"decision":"rejected","note":"旧版本驳回"})
    )
    .is_err());
    assert_eq!(
        super::load(&c, "m_products", &b, &p).unwrap(),
        product_before
    );
    assert_eq!(approved_again["effectiveAt"], "2020-01-01");
    crate::core::validate_database(&c).unwrap();
}

#[test]
fn knowledge_validation_rejects_chunk_and_review_tampering() {
    let mut c = db();
    let b = brand(&mut c, "虚构只读校验品牌");
    let document = import(
        &mut c,
        import_payload(&b, None, &Uuid::new_v4().to_string()),
    );
    let id = document["id"].as_str().unwrap();
    let reviewed = core_dispatch(
        &mut c,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":1,"decision":"approved","note":""}),
    )
    .unwrap();
    let original: String = c
        .query_row("SELECT data FROM m_knowledge_documents", [], |r| r.get(0))
        .unwrap();
    let mut corrupted: Value = serde_json::from_str(&original).unwrap();
    corrupted["chunks"][0]["sha256"] = json!("0".repeat(64));
    c.execute(
        "UPDATE m_knowledge_documents SET data=?1",
        [corrupted.to_string()],
    )
    .unwrap();
    assert!(crate::core::validate_database(&c).is_err());
    c.execute("UPDATE m_knowledge_documents SET data=?1", [original])
        .unwrap();
    crate::core::validate_database(&c).unwrap();

    let review_id = reviewed["reviews"][0]["id"].as_str().unwrap();
    let original: String = c
        .query_row("SELECT data FROM m_knowledge_reviews", [], |r| r.get(0))
        .unwrap();
    let mut corrupted: Value = serde_json::from_str(&original).unwrap();
    corrupted["documentId"] = json!(Uuid::new_v4().to_string());
    c.execute(
        "UPDATE m_knowledge_reviews SET data=?1 WHERE id=?2",
        params![corrupted.to_string(), review_id],
    )
    .unwrap();
    assert!(crate::core::validate_database(&c).is_err());
}

#[test]
fn schema5_upgrade_preserves_existing_rows_and_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema5.sqlite3");
    let mut old = open_database(&path).unwrap();
    brand(&mut old, "虚构schema5品牌");
    let before: String = old
        .query_row("SELECT data FROM m_brands", [], |r| r.get(0))
        .unwrap();
    old.execute_batch("DROP TABLE m_p0_workflows; DROP TABLE m_p0_policy; DROP TABLE m_offline_workflows; DROP TABLE m_question_research; DROP TABLE m_media; DROP TABLE m_automation_tasks; DROP TABLE m_assets; DROP TABLE m_knowledge_reviews; DROP TABLE m_knowledge_documents; UPDATE m_core_meta SET schema_version=5; PRAGMA user_version=5;")
        .unwrap();
    drop(old);
    let upgraded = open_database(&path).unwrap();
    assert_eq!(
        upgraded
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        10
    );
    assert_eq!(
        upgraded
            .query_row("SELECT data FROM m_brands", [], |r| r.get::<_, String>(0))
            .unwrap(),
        before
    );
    assert_eq!(
        upgraded
            .query_row("SELECT count(*) FROM m_knowledge_documents", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        upgraded
            .query_row("SELECT count(*) FROM m_knowledge_reviews", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let snapshots: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("before-optimization-v5-")
        })
        .collect();
    assert_eq!(snapshots.len(), 1);
    let prior = Connection::open(snapshots[0].path()).unwrap();
    assert_eq!(
        prior
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        prior
            .query_row("SELECT data FROM m_brands", [], |r| r.get::<_, String>(0))
            .unwrap(),
        before
    );
}

#[test]
fn backup_restore_preserves_knowledge_history_and_excludes_model_settings() {
    let mut source = db();
    let b = brand(&mut source, "虚构知识备份品牌");
    let document = import(
        &mut source,
        import_payload(&b, None, &Uuid::new_v4().to_string()),
    );
    let id = document["id"].as_str().unwrap();
    let reviewed = core_dispatch(
        &mut source,
        "knowledge.review",
        json!({"brandId":b,"id":id,"expectedRevision":1,"decision":"rejected","note":"备份中的虚构驳回理由"}),
    )
    .unwrap();
    source
        .execute("UPDATE ai_settings SET key_ref='local-only-secret-ref'", [])
        .unwrap();
    let bytes = crate::backup::encrypt_snapshot(&source, "knowledge-test-password").unwrap();
    let inspection = crate::backup::inspect_snapshot(&bytes, "knowledge-test-password").unwrap();
    assert_eq!(inspection["targetVersion"], 10);
    assert_eq!(inspection["counts"]["knowledgeDocuments"], 1);
    assert_eq!(inspection["counts"]["knowledgeReviews"], 1);
    let mut restored = db();
    crate::backup::restore_snapshot(&mut restored, &bytes, "knowledge-test-password").unwrap();
    assert_eq!(detail(&mut restored, &b, id), reviewed);
    assert_eq!(
        restored
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        10
    );
    assert_eq!(
        restored
            .query_row("SELECT count(*) FROM ai_settings", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    crate::core::validate_database(&restored).unwrap();
}
