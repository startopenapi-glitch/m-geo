use crate::runtime::{core_dispatch, open_database};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;

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
        json!({"brandId":brand_id,"name":"虚构星云杯","sku":sku,"facts":[]}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn research_payload(brand_id: &str, product_id: Option<&str>, question: &str) -> Value {
    json!({
        "brandId":brand_id,
        "productId":product_id,
        "question":question,
        "intent":"usage",
        "sourceKind":"manual",
        "sourceLabel":"虚构客服访谈记录",
        "sourceUrl":"https://example.com/research/interview-1",
        "observedAt":"2026-01-02T03:04:05Z",
        "clusterKey":"清洁保养"
    })
}

fn save(c: &mut Connection, payload: Value) -> Value {
    core_dispatch(c, "research.save", payload).unwrap()
}

#[test]
fn brand_research_profile_is_optional_merges_brand_and_deduplicates_aliases() {
    let mut c = db();
    let b = brand(&mut c, "虚构星河品牌");
    let before = super::brand(&c, &b).unwrap();
    assert!(before.get("profile").is_none());
    let saved = core_dispatch(
        &mut c,
        "brand.profile.save",
        json!({
            "brandId":b,
            "aliases":[" 星河 ","星河","STAR RIVER","star river","".repeat(0)],
            "officialUrl":"https://example.com/brand",
            "manufacturer":"虚构星河制造有限公司",
            "category":"虚构家居用品",
            "description":"仅用于测试的品牌介绍"
        }),
    )
    .unwrap();
    assert_eq!(saved["id"], b);
    assert_eq!(saved["name"], "虚构星河品牌");
    assert_eq!(saved["createdAt"], before["createdAt"]);
    assert_eq!(saved["profile"]["aliases"], json!(["星河", "STAR RIVER"]));
    assert_eq!(saved["profile"]["officialUrl"], "https://example.com/brand");
    assert!(saved["profile"]["updatedAt"].as_str().is_some());
    assert_eq!(super::brand(&c, &b).unwrap(), saved);

    for mut invalid in [
        json!({"brandId":b,"aliases":[],"officialUrl":"http://localhost/brand"}),
        json!({"brandId":b,"aliases":[],"officialUrl":"https://user:pass@example.com"}),
        json!({"brandId":b,"aliases":["x"],"unknown":true}),
        json!({"brandId":b,"aliases":(0..21).map(|n|format!("别名{n}")).collect::<Vec<_>>()}),
    ] {
        assert!(
            core_dispatch(&mut c, "brand.profile.save", invalid.clone()).is_err(),
            "accepted {invalid}"
        );
        invalid["brandId"] = json!("not-a-uuid");
        assert!(core_dispatch(&mut c, "brand.profile.save", invalid).is_err());
    }
}

#[test]
fn brand_research_normalizes_duplicate_questions_within_product_and_isolates_brands() {
    let mut c = db();
    let b = brand(&mut c, "虚构研究品牌");
    let other = brand(&mut c, "虚构其他研究品牌");
    let p = product(&mut c, &b, "RESEARCH-A");
    let p2 = product(&mut c, &b, "RESEARCH-B");
    let foreign = product(&mut c, &other, "RESEARCH-FOREIGN");
    let first = save(
        &mut c,
        research_payload(&b, Some(&p), " 请问 Ｎｅｂｕｌａ 怎么清洗吗？ "),
    );
    assert_eq!(first["status"], "candidate");
    assert_eq!(first["revision"], 1);
    assert!(core_dispatch(
        &mut c,
        "research.save",
        research_payload(&b, Some(&p), "nebula 怎么清洗")
    )
    .is_err());
    let other_product = save(&mut c, research_payload(&b, Some(&p2), "nebula怎么清洗"));
    assert_ne!(other_product["id"], first["id"]);
    let generic = save(&mut c, research_payload(&b, None, "NEBULA怎么清洗呢"));
    assert!(generic["productId"].is_null());
    assert!(core_dispatch(
        &mut c,
        "research.save",
        research_payload(&b, None, "nebula怎么清洗")
    )
    .is_err());
    let other_brand = save(
        &mut c,
        research_payload(&other, Some(&foreign), "nebula怎么清洗"),
    );
    assert_ne!(other_brand["brandId"], first["brandId"]);
    assert!(core_dispatch(
        &mut c,
        "research.save",
        research_payload(&b, Some(&foreign), "另一个问题")
    )
    .is_err());
    assert!(core_dispatch(
        &mut c,
        "research.archive",
        json!({"brandId":other,"id":first["id"],"expectedRevision":1})
    )
    .is_err());
    assert!(core_dispatch(
        &mut c,
        "research.list",
        json!({"brandId":other,"productId":p})
    )
    .is_ok());
    let visible = core_dispatch(&mut c, "research.list", json!({"brandId":other})).unwrap();
    assert_eq!(visible["items"].as_array().unwrap().len(), 1);
    assert_eq!(visible["items"][0]["id"], other_brand["id"]);
}

#[test]
fn brand_research_save_revision_status_and_atomic_stale_updates() {
    let mut c = db();
    let b = brand(&mut c, "虚构修订研究品牌");
    let p = product(&mut c, &b, "RESEARCH-REV");
    let first = save(&mut c, research_payload(&b, Some(&p), "虚构商品如何保养？"));
    let id = first["id"].as_str().unwrap();
    let mut edit = research_payload(&b, Some(&p), "虚构商品如何清洁与保养？");
    edit["id"] = json!(id);
    edit["expectedRevision"] = json!(1);
    edit["sourceKind"] = json!("suggested");
    edit["sourceLabel"] = json!("M- GEO 本地模板建议；非真实搜索数据");
    let revised = core_dispatch(&mut c, "research.save", edit.clone()).unwrap();
    assert_eq!(revised["revision"], 2);
    assert_eq!(revised["status"], "candidate");
    assert_eq!(revised["createdAt"], first["createdAt"]);
    edit["expectedRevision"] = json!(1);
    edit["question"] = json!("过期修订不应保存");
    assert!(core_dispatch(&mut c, "research.save", edit).is_err());
    let current = core_dispatch(&mut c, "research.list", json!({"brandId":b})).unwrap();
    assert_eq!(current["items"][0], revised);

    let confirmed = core_dispatch(
        &mut c,
        "research.confirm",
        json!({"brandId":b,"id":id,"expectedRevision":2}),
    )
    .unwrap();
    assert_eq!(confirmed["status"], "confirmed");
    assert_eq!(confirmed["revision"], 3);
    assert!(core_dispatch(
        &mut c,
        "research.confirm",
        json!({"brandId":b,"id":id,"expectedRevision":2})
    )
    .is_err());
    let archived = core_dispatch(
        &mut c,
        "research.archive",
        json!({"brandId":b,"id":id,"expectedRevision":3}),
    )
    .unwrap();
    assert_eq!(archived["status"], "archived");
    assert_eq!(archived["revision"], 4);
    let mut archived_edit = research_payload(&b, Some(&p), "归档后不可编辑");
    archived_edit["id"] = json!(id);
    archived_edit["expectedRevision"] = json!(4);
    assert!(core_dispatch(&mut c, "research.save", archived_edit).is_err());
}

#[test]
fn brand_research_confirm_to_question_reuses_native_question_semantics() {
    let mut c = db();
    let b = brand(&mut c, "虚构星云品牌");
    let p = product(&mut c, &b, "RESEARCH-QUESTION");
    let candidate = save(
        &mut c,
        research_payload(&b, Some(&p), "虚构星云品牌的星云杯如何清洗？"),
    );
    assert!(core_dispatch(
        &mut c,
        "research.confirmToQuestion",
        json!({"brandId":b,"id":candidate["id"],"expectedRevision":1})
    )
    .is_err());
    let confirmed = core_dispatch(
        &mut c,
        "research.confirm",
        json!({"brandId":b,"id":candidate["id"],"expectedRevision":1}),
    )
    .unwrap();
    let result = core_dispatch(
        &mut c,
        "research.confirmToQuestion",
        json!({"brandId":b,"id":candidate["id"],"expectedRevision":2}),
    )
    .unwrap();
    assert_eq!(result["research"], confirmed);
    assert_eq!(result["question"]["brandId"], b);
    assert_eq!(result["question"]["productId"], p);
    assert_eq!(result["question"]["origin"], "customer");
    assert_eq!(result["question"]["confirmed"], true);
    assert_eq!(result["question"]["branded"], true);
    assert_eq!(result["question"]["intent"], "usage");
    assert!(result["question"]["source"]
        .as_str()
        .unwrap()
        .contains("https://example.com/research/interview-1"));
    let replay = core_dispatch(
        &mut c,
        "research.confirmToQuestion",
        json!({"brandId":b,"id":candidate["id"],"expectedRevision":2}),
    )
    .unwrap();
    assert_eq!(replay["question"]["id"], result["question"]["id"]);
    let count: i64 = c
        .query_row("SELECT count(*) FROM m_questions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn brand_research_generic_or_suggested_entries_have_explicit_limits_and_origin() {
    let mut c = db();
    let b = brand(&mut c, "虚构建议研究品牌");
    let generic = save(&mut c, research_payload(&b, None, "没有关联商品的问题"));
    let confirmed = core_dispatch(
        &mut c,
        "research.confirm",
        json!({"brandId":b,"id":generic["id"],"expectedRevision":1}),
    )
    .unwrap();
    assert!(core_dispatch(
        &mut c,
        "research.confirmToQuestion",
        json!({"brandId":b,"id":confirmed["id"],"expectedRevision":2})
    )
    .is_err());

    let p = product(&mut c, &b, "RESEARCH-SUGGESTED");
    let mut input = research_payload(&b, Some(&p), "虚构本地模板建议问题");
    input["sourceKind"] = json!("suggested");
    input["intent"] = json!("after_sales");
    input["sourceLabel"] = json!("M- GEO 本地模板建议；非真实搜索数据");
    let suggested = save(&mut c, input);
    assert_eq!(suggested["intent"], "after_sales");
    core_dispatch(
        &mut c,
        "research.confirm",
        json!({"brandId":b,"id":suggested["id"],"expectedRevision":1}),
    )
    .unwrap();
    let result = core_dispatch(
        &mut c,
        "research.confirmToQuestion",
        json!({"brandId":b,"id":suggested["id"],"expectedRevision":2}),
    )
    .unwrap();
    assert_eq!(result["question"]["origin"], "suggestion");
    assert_eq!(result["question"]["intent"], "after_sales");
}

#[test]
fn brand_research_list_filters_dates_sources_status_and_reports_clusters() {
    let mut c = db();
    let b = brand(&mut c, "虚构筛选研究品牌");
    let p = product(&mut c, &b, "RESEARCH-FILTER");
    let first = save(&mut c, research_payload(&b, Some(&p), "虚构如何清洗杯盖"));
    let mut second_input = research_payload(&b, Some(&p), "虚构如何保养杯身");
    second_input["sourceKind"] = json!("imported");
    second_input["observedAt"] = json!("2026-02-03T00:00:00+08:00");
    let second = save(&mut c, second_input);
    let mut third_input = research_payload(&b, None, "虚构独立来源问题");
    third_input["clusterKey"] = json!("其他");
    third_input["observedAt"] = json!("2026-03-01T00:00:00Z");
    save(&mut c, third_input);
    let listed = core_dispatch(
        &mut c,
        "research.list",
        json!({"brandId":b,"clusterKey":"清洁保养","observedFrom":"2026-01-01","observedTo":"2026-02-28","limit":10}),
    )
    .unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        listed["clusters"],
        json!([{"clusterKey":"清洁保养","count":2,"researchIds":[second["id"],first["id"]]}])
    );
    assert_eq!(listed["truncated"], false);
    let imported = core_dispatch(
        &mut c,
        "research.list",
        json!({"brandId":b,"sourceKind":"imported","query":"杯身"}),
    )
    .unwrap();
    assert_eq!(imported["items"].as_array().unwrap().len(), 1);
    assert_eq!(imported["items"][0]["id"], second["id"]);
    let limited = core_dispatch(&mut c, "research.list", json!({"brandId":b,"limit":1})).unwrap();
    assert_eq!(limited["items"].as_array().unwrap().len(), 1);
    assert_eq!(limited["truncated"], true);
    assert!(core_dispatch(
        &mut c,
        "research.list",
        json!({"brandId":b,"observedFrom":"2026-03-01","observedTo":"2026-01-01"})
    )
    .is_err());
}

#[test]
fn brand_research_records_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("brand-research.sqlite3");
    let mut c = open_database(&path).unwrap();
    let b = brand(&mut c, "虚构重开研究品牌");
    let p = product(&mut c, &b, "RESEARCH-REOPEN");
    let saved = save(
        &mut c,
        research_payload(&b, Some(&p), "虚构重开后仍保留的问题"),
    );
    core_dispatch(
        &mut c,
        "brand.profile.save",
        json!({"brandId":b,"aliases":["重开别名"],"officialUrl":"","manufacturer":"虚构厂商","category":"虚构分类","description":"虚构介绍"}),
    )
    .unwrap();
    let profile = super::brand(&c, &b).unwrap()["profile"].clone();
    drop(c);
    let mut reopened = open_database(&path).unwrap();
    let listed = core_dispatch(&mut reopened, "research.list", json!({"brandId":b})).unwrap();
    assert_eq!(listed["items"], json!([saved]));
    assert_eq!(super::brand(&reopened, &b).unwrap()["profile"], profile);
}
