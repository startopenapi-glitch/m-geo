use super::*;

fn fixture() -> (Connection, Value, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let brand = handle(&mut c, "brand.create", json!({"name":"虚构资产品牌"})).unwrap();
    let product = handle(&mut c, "product.save", json!({
        "brandId":brand["id"],"name":"虚构水杯","sku":"SIM-ASSET-CUP","category":"测试",
        "officialUrl":"https://fictional.example/cup","sourceText":"PRIVATE_RAW",
        "privateNotes":"PRIVATE_NOTE","facts":[
            {"id":"capacity","label":"容量","value":"500mL","source":"https://fictional.example/spec","visibility":"public","validUntil":null}
        ]
    })).unwrap();
    let product = handle(
        &mut c,
        "product.confirm",
        json!({
            "brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]
        }),
    )
    .unwrap();
    (c, brand, product)
}

fn draft(c: &mut Connection, brand: &Value, product: &Value) -> Value {
    handle(
        c,
        "draft.generateLocal",
        json!({
            "brandId":brand["id"],"productId":product["id"],"kind":"description"
        }),
    )
    .unwrap()
}

fn asset(c: &mut Connection, brand: &Value, product: &Value, text: &str) -> Value {
    handle(
        c,
        "asset.create",
        json!({
            "brandId":brand["id"],"kind":"keyword","text":text,
            "source":"虚构内容 brief 第 1 版","productId":product["id"],"requestId":id()
        }),
    )
    .unwrap()
}

fn approve(c: &mut Connection, brand: &Value, asset: &Value) -> Value {
    handle(
        c,
        "asset.review",
        json!({
            "brandId":brand["id"],"id":asset["id"],
            "expectedRevision":asset["revision"],"decision":"approved","note":"虚构审核通过"
        }),
    )
    .unwrap()
}

fn approved_asset(c: &mut Connection, brand: &Value, product: &Value, text: &str) -> Value {
    let asset = asset(c, brand, product, text);
    approve(c, brand, &asset)
}

fn save(c: &mut Connection, draft: &Value, body: String, asset_ids: Value) -> Value {
    handle(
        c,
        "draft.save",
        json!({
            "brandId":draft["brandId"],"id":draft["id"],"expectedRevision":draft["revision"],
            "title":draft["title"],"body":body,"claims":draft["claims"],"assetIds":asset_ids
        }),
    )
    .unwrap()
}

fn review(c: &mut Connection, draft: &Value) -> Result<Value> {
    handle(
        c,
        "draft.review",
        json!({
            "brandId":draft["brandId"],"id":draft["id"],"expectedRevision":draft["revision"]
        }),
    )
}

#[test]
fn approved_asset_is_frozen_and_later_revision_preserves_historical_export() {
    let (mut c, brand, product) = fixture();
    let asset = approved_asset(&mut c, &brand, &product, "便携水杯");
    let draft = draft(&mut c, &brand, &product);
    let body = format!("{}\n\n便携水杯", draft["body"].as_str().unwrap());
    let saved = save(&mut c, &draft, body, json!([asset["id"]]));
    let reviewed = review(&mut c, &saved).unwrap();
    let snapshot = &reviewed["assetSnapshots"][0];
    assert_eq!(snapshot["assetId"], asset["id"]);
    assert_eq!(snapshot["assetRevision"], asset["revision"]);
    assert_eq!(snapshot["text"], "便携水杯");
    assert!(snapshot["assetReviewedAt"].is_string());
    validate_database(&c).unwrap();

    let revised = handle(
        &mut c,
        "asset.revise",
        json!({
            "brandId":brand["id"],"id":asset["id"],"expectedRevision":asset["revision"],
            "text":"新款水杯","source":"虚构内容 brief 第 2 版","productId":product["id"]
        }),
    )
    .unwrap();
    assert_eq!(revised["reviewStatus"], "pending");
    for format in ["markdown", "html"] {
        let exported = handle(
            &mut c,
            "draft.export",
            json!({
                "brandId":brand["id"],"id":reviewed["id"],
                "expectedRevision":reviewed["revision"],"format":format
            }),
        )
        .unwrap();
        let content = exported["content"].as_str().unwrap();
        assert!(content.contains("内容资产（审核快照）"));
        assert!(content.contains("便携水杯"));
        assert!(content.contains("虚构内容 brief 第 1 版"));
        assert!(!content.contains("新款水杯"));
        assert!(!content.contains("PRIVATE_RAW"));
        assert!(!content.contains("PRIVATE_NOTE"));
        assert!(!content.contains("/Users/"));
    }
    validate_database(&c).unwrap();

    let reapproved = approve(&mut c, &brand, &revised);
    let saved = save(
        &mut c,
        &reviewed,
        reviewed["body"].as_str().unwrap().to_owned(),
        json!([asset["id"]]),
    );
    assert!(saved.get("assetSnapshots").is_none());
    assert!(review(&mut c, &saved).unwrap_err().contains("新款水杯"));
    let body = format!("{}\n\n新款水杯", saved["body"].as_str().unwrap());
    let saved = save(&mut c, &saved, body, json!([asset["id"]]));
    let rereviewed = review(&mut c, &saved).unwrap();
    assert_eq!(
        rereviewed["assetSnapshots"][0]["assetRevision"],
        reapproved["revision"]
    );
    assert_eq!(rereviewed["assetSnapshots"][0]["text"], "新款水杯");
}

#[test]
fn pending_unused_and_cross_product_assets_block_review_without_mutation() {
    let (mut c, brand, product) = fixture();
    let pending = asset(&mut c, &brand, &product, "轻量出行");
    let draft = draft(&mut c, &brand, &product);
    let body = format!("{}\n\n轻量出行", draft["body"].as_str().unwrap());
    let saved = save(&mut c, &draft, body, json!([pending["id"]]));
    let before = load(
        &c,
        "m_drafts",
        brand["id"].as_str().unwrap(),
        saved["id"].as_str().unwrap(),
    )
    .unwrap();
    assert!(review(&mut c, &saved).unwrap_err().contains("仅已审核通过"));
    assert_eq!(
        load(
            &c,
            "m_drafts",
            brand["id"].as_str().unwrap(),
            saved["id"].as_str().unwrap()
        )
        .unwrap(),
        before
    );

    let approved = approve(&mut c, &brand, &pending);
    assert!(review(&mut c, &saved).is_ok());
    let saved = save(
        &mut c,
        &saved,
        draft["body"].as_str().unwrap().to_owned(),
        json!([approved["id"]]),
    );
    assert!(review(&mut c, &saved)
        .unwrap_err()
        .contains("未出现在稿件标题或正文"));

    let other = handle(
        &mut c,
        "product.save",
        json!({
            "brandId":brand["id"],"name":"虚构另一商品","sku":"SIM-OTHER","category":"测试",
            "officialUrl":"","sourceText":"","privateNotes":"","facts":[]
        }),
    )
    .unwrap();
    let foreign = handle(
        &mut c,
        "asset.create",
        json!({
            "brandId":brand["id"],"kind":"keyword","text":"另一商品词","source":"虚构资料",
            "productId":other["id"],"requestId":id()
        }),
    )
    .unwrap();
    let foreign = approve(&mut c, &brand, &foreign);
    let body = format!("{}\n\n另一商品词", saved["body"].as_str().unwrap());
    let saved = save(&mut c, &saved, body, json!([foreign["id"]]));
    assert!(review(&mut c, &saved).unwrap_err().contains("属于其他商品"));
}

#[test]
fn save_strips_forged_snapshots_and_rejects_duplicate_or_unknown_ids() {
    let (mut c, brand, product) = fixture();
    let approved = approved_asset(&mut c, &brand, &product, "安全材质");
    let draft = draft(&mut c, &brand, &product);
    let body = format!("{}\n\n安全材质", draft["body"].as_str().unwrap());
    let forged = json!([{"snapshotVersion":1,"assetId":approved["id"],"text":"伪造"}]);
    let saved = handle(
        &mut c,
        "draft.save",
        json!({
            "brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"],
            "title":draft["title"],"body":body,"claims":draft["claims"],
            "assetIds":[approved["id"]],"assetSnapshots":forged
        }),
    )
    .unwrap();
    assert!(saved.get("assetSnapshots").is_none());
    assert!(saved["reviewedAt"].is_null());
    validate_database(&c).unwrap();

    for ids in [
        json!([approved["id"], approved["id"]]),
        json!(["not-a-guid"]),
    ] {
        let error = handle(
            &mut c,
            "draft.save",
            json!({
                "brandId":brand["id"],"id":saved["id"],"expectedRevision":saved["revision"],
                "title":saved["title"],"body":saved["body"],"claims":saved["claims"],"assetIds":ids
            }),
        )
        .unwrap_err();
        assert!(error.contains("不能重复") || error.contains("标识"));
    }
    assert_eq!(
        load(
            &c,
            "m_drafts",
            brand["id"].as_str().unwrap(),
            saved["id"].as_str().unwrap()
        )
        .unwrap(),
        saved
    );
}

#[test]
fn tampered_asset_snapshot_is_rejected_by_export_and_backup_validation() {
    let (mut c, brand, product) = fixture();
    let approved = approved_asset(&mut c, &brand, &product, "通勤水杯");
    let draft = draft(&mut c, &brand, &product);
    let body = format!("{}\n\n通勤水杯", draft["body"].as_str().unwrap());
    let saved = save(&mut c, &draft, body, json!([approved["id"]]));
    let reviewed = review(&mut c, &saved).unwrap();
    let mut tampered = reviewed.clone();
    tampered["assetSnapshots"][0]["source"] = json!("伪造来源");
    c.execute(
        "UPDATE m_drafts SET data=?1 WHERE id=?2",
        params![tampered.to_string(), tampered["id"].as_str()],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
    let error = handle(
        &mut c,
        "draft.export",
        json!({
            "brandId":brand["id"],"id":tampered["id"],
            "expectedRevision":tampered["revision"],"format":"markdown"
        }),
    )
    .unwrap_err();
    assert!(error.contains("历史审核记录不一致"));
}

#[test]
fn archived_asset_preserves_historical_export_but_blocks_new_review_until_restored() {
    let (mut c, brand, product) = fixture();
    let approved = approved_asset(&mut c, &brand, &product, "归档水杯");
    let draft = draft(&mut c, &brand, &product);
    let body = format!("{}\n\n归档水杯", draft["body"].as_str().unwrap());
    let saved = save(&mut c, &draft, body, json!([approved["id"]]));
    let reviewed = review(&mut c, &saved).unwrap();
    let archived = handle(
        &mut c,
        "asset.setArchived",
        json!({
            "brandId":brand["id"],"id":approved["id"],"expectedRevision":approved["revision"],
            "archived":true,"note":"虚构归档，仍保留历史引用"
        }),
    )
    .unwrap();
    let exported = handle(
        &mut c,
        "draft.export",
        json!({
            "brandId":brand["id"],"id":reviewed["id"],
            "expectedRevision":reviewed["revision"],"format":"markdown"
        }),
    )
    .unwrap();
    assert!(exported["content"].as_str().unwrap().contains("归档水杯"));
    validate_database(&c).unwrap();

    let saved = save(
        &mut c,
        &reviewed,
        reviewed["body"].as_str().unwrap().to_owned(),
        json!([approved["id"]]),
    );
    assert!(review(&mut c, &saved).unwrap_err().contains("已归档"));
    let restored = handle(
        &mut c,
        "asset.setArchived",
        json!({
            "brandId":brand["id"],"id":archived["id"],"expectedRevision":archived["revision"],
            "archived":false,"note":""
        }),
    )
    .unwrap();
    assert_eq!(restored["history"][3]["action"], "restored");
    let rereviewed = review(&mut c, &saved).unwrap();
    assert_eq!(
        rereviewed["assetSnapshots"][0]["assetRevision"],
        approved["revision"]
    );
    validate_database(&c).unwrap();
}
