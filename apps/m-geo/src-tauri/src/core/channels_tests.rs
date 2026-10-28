use super::*;

fn workspace() -> (Connection, Value, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let brand = handle(&mut c, "brand.create", json!({"name":"虚构渠道品牌"})).unwrap();
    let product = handle(&mut c,"product.save",json!({"brandId":brand["id"],"name":"虚构保温杯","sku":"SIM-CUP-7","category":"测试用品","officialUrl":"https://example.com/cup","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"capacity","label":"容量","value":"500mL","source":"虚构规格表 v1","visibility":"public","validUntil":null},{"id":"cost","label":"成本","value":"PRIVATE_SECRET","source":"内部","visibility":"private","validUntil":null}]})).unwrap();
    let product = handle(
        &mut c,
        "product.confirm",
        json!({"brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]}),
    )
    .unwrap();
    (c, brand, product)
}

#[test]
fn seven_channel_drafts_keep_traceable_claims_and_pass_review_export() {
    let (mut c, brand, product) = workspace();
    let result = handle(
        &mut c,
        "draft.generateChannels",
        json!({"brandId":brand["id"],"productId":product["id"],"channels":channels::IDS}),
    )
    .unwrap();
    let drafts = result["drafts"].as_array().unwrap();
    assert_eq!(drafts.len(), 7);
    for (index, channel) in channels::IDS.iter().enumerate() {
        let draft = &drafts[index];
        assert_eq!(draft["channel"], json!(channel));
        assert_eq!(
            draft["channelTemplateVersion"],
            json!(channels::TEMPLATE_VERSION)
        );
        assert_eq!(draft["generationMode"], "local");
        assert!(draft["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|claim| draft["body"]
                .as_str()
                .unwrap()
                .contains(claim["text"].as_str().unwrap())));
        for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
            assert!(!draft.to_string().contains(secret));
        }
        let reviewed = handle(
            &mut c,
            "draft.review",
            json!({"brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"]}),
        )
        .unwrap();
        assert!(!reviewed["reviewedAt"].is_null());
        let exported = handle(&mut c,"draft.export",json!({"brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"],"format":"markdown"})).unwrap();
        assert!(exported["content"]
            .as_str()
            .unwrap()
            .contains("虚构规格表 v1"));
    }
    validate_database(&c).unwrap();
}

#[test]
fn channel_batch_rejects_bad_selection_and_rolls_back_every_write() {
    for channels in [
        json!([]),
        json!(["unknown"]),
        json!(["jd", "jd"]),
        json!(["jd", null]),
    ] {
        let (mut c, brand, product) = workspace();
        assert!(handle(
            &mut c,
            "draft.generateChannels",
            json!({"brandId":brand["id"],"productId":product["id"],"channels":channels})
        )
        .is_err());
        let snapshot =
            handle(&mut c, "workspace.snapshot", json!({"brandId":brand["id"]})).unwrap();
        assert_eq!(snapshot["drafts"].as_array().unwrap().len(), 0);
        let preparations: i64 = c
            .query_row("SELECT count(*) FROM m_ai_preparations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(preparations, 0);
        validate_database(&c).unwrap();
    }
}

#[test]
fn generic_drafts_keep_the_original_shape_and_remain_restorable() {
    let (mut c, brand, product) = workspace();
    let draft = handle(
        &mut c,
        "draft.generateLocal",
        json!({"brandId":brand["id"],"productId":product["id"],"kind":"description"}),
    )
    .unwrap();
    assert!(draft["channel"].is_null());
    assert_eq!(draft["channelTemplateVersion"], 0);
    assert_eq!(draft["title"], "虚构保温杯 · 商品介绍");
    validate_database(&c).unwrap();
}

#[test]
fn channel_metadata_survives_encrypted_backup_and_restore() {
    let mut source = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let brand = handle(&mut source, "brand.create", json!({"name":"虚构备份品牌"})).unwrap();
    let product = handle(&mut source,"product.save",json!({"brandId":brand["id"],"name":"虚构水壶","sku":"SIM-KETTLE","facts":[{"id":"capacity","label":"容量","value":"1.5L","source":"虚构规格表","visibility":"public","validUntil":null}]})).unwrap();
    let product = handle(
        &mut source,
        "product.confirm",
        json!({"brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]}),
    )
    .unwrap();
    handle(
        &mut source,
        "draft.generateChannels",
        json!({"brandId":brand["id"],"productId":product["id"],"channels":["jd","kuaishou"]}),
    )
    .unwrap();
    let bytes = crate::backup::encrypt_snapshot(&source, "fictional-channel-test-only").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-channel-test-only").unwrap();
    let snapshot = handle(
        &mut restored,
        "workspace.snapshot",
        json!({"brandId":brand["id"]}),
    )
    .unwrap();
    assert_eq!(snapshot["drafts"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["drafts"][0]["channel"], "jd");
    assert_eq!(snapshot["drafts"][1]["channel"], "kuaishou");
    assert_eq!(snapshot["drafts"][0]["channelTemplateVersion"], 1);
}
