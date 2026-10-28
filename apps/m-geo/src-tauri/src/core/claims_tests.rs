use super::*;

fn fixture() -> (Connection, Value, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let brand = handle(&mut c, "brand.create", json!({"name":"虚构证据品牌"})).unwrap();
    let product = handle(&mut c, "product.save", json!({
        "brandId":brand["id"],"name":"虚构水杯","sku":"SIM-CUP","category":"测试",
        "officialUrl":"https://fictional.example/cup","sourceText":"PRIVATE_RAW",
        "privateNotes":"PRIVATE_NOTE","facts":[
            {"id":"capacity","label":"容量","value":"500mL","source":"https://fictional.example/spec","visibility":"public","validUntil":null},
            {"id":"cost","label":"成本","value":"PRIVATE_COST","source":"内部","visibility":"private","validUntil":null}
        ]
    })).unwrap();
    let product = handle(
        &mut c,
        "product.confirm",
        json!({"brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]}),
    )
    .unwrap();
    (c, brand, product)
}

fn local_draft(c: &mut Connection, brand: &Value, product: &Value) -> Value {
    handle(
        c,
        "draft.generateLocal",
        json!({"brandId":brand["id"],"productId":product["id"],"kind":"description"}),
    )
    .unwrap()
}

fn review(c: &mut Connection, draft: &Value) -> Result<Value> {
    handle(
        c,
        "draft.review",
        json!({"brandId":draft["brandId"],"id":draft["id"],"expectedRevision":draft["revision"]}),
    )
}

#[test]
fn local_claims_trace_to_public_facts_and_survive_export_readback() {
    let (mut c, brand, product) = fixture();
    let draft = local_draft(&mut c, &brand, &product);
    assert_eq!(draft["claims"].as_array().unwrap().len(), 1);
    assert_eq!(draft["claims"][0]["text"], "容量：500mL");
    assert_eq!(draft["claims"][0]["factId"], "capacity");
    let reviewed = review(&mut c, &draft).unwrap();
    for format in ["markdown", "html"] {
        let exported = handle(&mut c, "draft.export", json!({"brandId":brand["id"],"id":reviewed["id"],"expectedRevision":reviewed["revision"],"format":format})).unwrap();
        let content = exported["content"].as_str().unwrap();
        assert!(content.contains("主张依据（审核记录）"));
        assert!(content.contains("容量：500mL"));
        assert!(content.contains("https://fictional.example/spec"));
        for private in [
            "PRIVATE_RAW",
            "PRIVATE_NOTE",
            "PRIVATE_COST",
            "/Users/",
            "apiKey",
        ] {
            assert!(!content.contains(private));
        }
    }
    validate_database(&c).unwrap();
}

#[test]
fn review_blocks_missing_mismatched_private_and_non_body_evidence() {
    let (mut c, brand, product) = fixture();
    let ctx = handle(
        &mut c,
        "internal.ai.prepare",
        json!({"brandId":brand["id"],"productId":product["id"],"kind":"description"}),
    )
    .unwrap();
    let ai = handle(
        &mut c,
        "internal.ai.commit",
        json!({"context":ctx,"body":"容量：500mL"}),
    )
    .unwrap();
    assert!(review(&mut c, &ai).unwrap_err().contains("至少登记一条"));
    let mut current = ai;
    for (claim, expected) in [
        (
            json!({"id":id(),"text":"容量：500mL","factId":""}),
            "缺少事实依据",
        ),
        (
            json!({"id":id(),"text":"容量：500mL","factId":"missing"}),
            "不存在或不是公开事实",
        ),
        (
            json!({"id":id(),"text":"容量：500mL","factId":"cost"}),
            "不存在或不是公开事实",
        ),
        (
            json!({"id":id(),"text":"未写入正文的功效","factId":"capacity"}),
            "不在正文中",
        ),
    ] {
        current = handle(&mut c, "draft.save", json!({"brandId":brand["id"],"id":current["id"],"expectedRevision":current["revision"],"title":current["title"],"body":current["body"],"claims":[claim]})).unwrap();
        assert!(review(&mut c, &current).unwrap_err().contains(expected));
    }
    let saved = handle(&mut c, "draft.save", json!({"brandId":brand["id"],"id":current["id"],"expectedRevision":current["revision"],"title":current["title"],"body":current["body"],"claims":[{"id":id(),"text":"容量：500mL","factId":"capacity"}]})).unwrap();
    assert!(review(&mut c, &saved).unwrap()["reviewedAt"].is_string());
}

#[test]
fn legacy_draft_is_preserved_but_requires_claim_mapping_before_review() {
    let (mut c, brand, product) = fixture();
    let mut draft = local_draft(&mut c, &brand, &product);
    draft.as_object_mut().unwrap().remove("claims");
    c.execute(
        "UPDATE m_drafts SET data=?1 WHERE id=?2",
        params![draft.to_string(), draft["id"].as_str()],
    )
    .unwrap();
    validate_database(&c).unwrap();
    assert!(review(&mut c, &draft)
        .unwrap_err()
        .contains("缺少关键主张依据"));
    let saved = handle(&mut c, "draft.save", json!({"brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"],"title":draft["title"],"body":draft["body"],"claims":[{"id":id(),"text":"容量：500mL","factId":"capacity"}]})).unwrap();
    assert!(review(&mut c, &saved).is_ok());
}

#[test]
fn pending_or_rejected_facts_block_claim_review_and_export() {
    let (mut c, brand, product) = fixture();
    let draft = local_draft(&mut c, &brand, &product);
    for status in ["pending", "rejected"] {
        let mut blocked = product.clone();
        blocked["facts"][0]["reviewStatus"] = json!(status);
        blocked["facts"][0]["reviewedAt"] = Value::Null;
        assert!(claims::review(&draft, &blocked)
            .unwrap_err()
            .contains("尚未审核通过"));
    }

    let reviewed = review(&mut c, &draft).unwrap();
    let mut blocked = product.clone();
    blocked["facts"][0]["reviewStatus"] = json!("pending");
    blocked["facts"][0]["reviewedAt"] = Value::Null;
    c.execute(
        "UPDATE m_products SET data=?1 WHERE id=?2",
        params![blocked.to_string(), blocked["id"].as_str()],
    )
    .unwrap();
    for action in ["draft.review", "draft.export"] {
        let error = handle(&mut c, action, json!({"brandId":brand["id"],"id":reviewed["id"],"expectedRevision":reviewed["revision"],"format":"markdown"}))
            .unwrap_err();
        assert!(error.contains("尚未审核通过"));
    }
    // Pending facts are valid workspace data and must survive backup validation.
    validate_database(&c).unwrap();
}

#[test]
fn review_freezes_evidence_and_native_save_strips_forged_snapshots() {
    let (mut c, brand, product) = fixture();
    let draft = local_draft(&mut c, &brand, &product);
    let reviewed = review(&mut c, &draft).unwrap();
    let evidence = &reviewed["claims"][0]["evidence"];
    assert_eq!(evidence["snapshotVersion"], 1);
    assert_eq!(evidence["factId"], "capacity");
    assert_eq!(evidence["label"], "容量");
    assert_eq!(evidence["value"], "500mL");
    assert_eq!(evidence["source"], "https://fictional.example/spec");
    assert!(evidence["factReviewedAt"].is_string());
    assert!(evidence["factUpdatedAt"].is_string());
    validate_database(&c).unwrap();

    let mut later_product = product.clone();
    later_product["facts"][0]["value"] = json!("999mL");
    let appendix = claims::markdown_appendix(&reviewed, &later_product);
    assert!(appendix.contains("容量：500mL"));
    assert!(!appendix.contains("999mL"));

    let saved = handle(
        &mut c,
        "draft.save",
        json!({
            "brandId":brand["id"],"id":reviewed["id"],"expectedRevision":reviewed["revision"],
            "title":reviewed["title"],"body":reviewed["body"],"claims":reviewed["claims"]
        }),
    )
    .unwrap();
    assert!(saved["reviewedAt"].is_null());
    assert!(saved["claims"][0].get("evidence").is_none());
    validate_database(&c).unwrap();
}

#[test]
fn outward_actions_reject_tampered_snapshots_and_legacy_reviewed_drafts_remain_readable() {
    let (mut c, brand, product) = fixture();
    let draft = local_draft(&mut c, &brand, &product);
    let reviewed = review(&mut c, &draft).unwrap();
    let mut tampered = reviewed.clone();
    tampered["claims"][0]["evidence"]["factId"] = json!("different");
    c.execute(
        "UPDATE m_drafts SET data=?1 WHERE id=?2",
        params![tampered.to_string(), tampered["id"].as_str()],
    )
    .unwrap();
    let validation_error = claims::validate_stored(&tampered).unwrap_err();
    assert!(
        validation_error.contains("快照与关联事实不一致"),
        "{validation_error}"
    );
    assert!(validate_database(&c).is_err());

    tampered["claims"][0]["evidence"]["factId"] = json!("capacity");
    tampered["claims"][0]["evidence"]["value"] = json!("伪造容量");
    c.execute(
        "UPDATE m_drafts SET data=?1 WHERE id=?2",
        params![tampered.to_string(), tampered["id"].as_str()],
    )
    .unwrap();
    validate_database(&c).unwrap();
    let error = handle(&mut c, "draft.export", json!({"brandId":brand["id"],"id":reviewed["id"],"expectedRevision":reviewed["revision"],"format":"markdown"})).unwrap_err();
    assert!(error.contains("证据快照与当前审核依据不一致"));

    let mut legacy = reviewed.clone();
    legacy["claims"][0]
        .as_object_mut()
        .unwrap()
        .remove("evidence");
    c.execute(
        "UPDATE m_drafts SET data=?1 WHERE id=?2",
        params![legacy.to_string(), legacy["id"].as_str()],
    )
    .unwrap();
    validate_database(&c).unwrap();
    let exported = handle(&mut c, "draft.export", json!({"brandId":brand["id"],"id":legacy["id"],"expectedRevision":legacy["revision"],"format":"markdown"})).unwrap();
    assert!(exported["content"]
        .as_str()
        .unwrap()
        .contains("容量：500mL"));
}
