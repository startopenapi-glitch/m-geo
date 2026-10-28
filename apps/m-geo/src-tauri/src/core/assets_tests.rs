use super::*;

const REQUEST: &str = "90000000-0000-4000-8000-0000000000a1";
fn workspace() -> (Connection, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let brand = handle(&mut c, "brand.create", json!({"name":"虚构资产品牌"})).unwrap();
    (c, brand)
}
fn create(c: &mut Connection, brand: &Value, kind: &str, text: &str, product_id: Value) -> Value {
    handle(
        c,
        "asset.create",
        json!({"brandId":brand["id"],"kind":kind,"text":text,"source":"虚构人工资料","productId":product_id,"requestId":REQUEST}),
    )
    .unwrap()
}
fn review(c: &mut Connection, brand: &Value, asset: &Value, decision: &str, note: &str) -> Value {
    handle(
        c,
        "asset.review",
        json!({"brandId":brand["id"],"id":asset["id"],"expectedRevision":asset["revision"],"decision":decision,"note":note}),
    )
    .unwrap()
}
fn revise(c: &mut Connection, brand: &Value, asset: &Value, text: &str) -> Value {
    handle(
        c,
        "asset.revise",
        json!({"brandId":brand["id"],"id":asset["id"],"expectedRevision":asset["revision"],"text":text,"source":"修订后的人工来源","productId":asset["productId"]}),
    )
    .unwrap()
}

#[test]
fn create_is_pending_idempotent_and_in_workspace_snapshot() {
    let (mut c, brand) = workspace();
    let input = json!({"brandId":brand["id"],"kind":"keyword","text":"  虚构通勤关键词  ","source":"  虚构人工整理表  ","productId":null,"requestId":REQUEST});
    let created = handle(&mut c, "asset.create", input.clone()).unwrap();
    assert_eq!(created["text"], "虚构通勤关键词");
    assert_eq!(created["source"], "虚构人工整理表");
    assert_eq!(created["reviewStatus"], "pending");
    assert_eq!(created["revision"], 1);
    assert!(created["reviewedAt"].is_null());
    assert_eq!(created["createdAt"], created["updatedAt"]);
    assert_eq!(created["history"].as_array().unwrap().len(), 1);
    assert_eq!(created["history"][0]["action"], "created");
    assert_eq!(handle(&mut c, "asset.create", input).unwrap(), created);
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_assets", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM m_audit WHERE action='asset.create'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let snapshot = handle(&mut c, "workspace.snapshot", json!({"brandId":brand["id"]})).unwrap();
    assert_eq!(snapshot["assets"], json!([created]));
    validate_database(&c).unwrap();
}

#[test]
fn create_replay_with_different_payload_and_unknown_fields_never_write() {
    let (mut c, brand) = workspace();
    let created = create(&mut c, &brand, "keyword", "虚构关键词", Value::Null);
    let mut changed = json!({"brandId":brand["id"],"kind":"keyword","text":"不同内容","source":"虚构人工资料","productId":null,"requestId":REQUEST});
    assert!(handle(&mut c, "asset.create", changed.clone()).is_err());
    changed["unexpected"] = json!(true);
    assert!(handle(&mut c, "asset.create", changed).is_err());
    for (action, payload) in [
        ("asset.list", json!({"brandId":brand["id"],"extra":true})),
        (
            "asset.revise",
            json!({"brandId":brand["id"],"id":created["id"],"expectedRevision":1,"text":"修订","source":"来源","productId":null,"extra":true}),
        ),
        (
            "asset.review",
            json!({"brandId":brand["id"],"id":created["id"],"expectedRevision":1,"decision":"approved","note":"","extra":true}),
        ),
    ] {
        assert!(handle(&mut c, action, payload).is_err());
    }
    let listed = handle(&mut c, "asset.list", json!({"brandId":brand["id"]})).unwrap();
    assert_eq!(listed, json!([created]));
    validate_database(&c).unwrap();
}

#[test]
fn list_filters_kind_and_all_writes_enforce_brand_and_product_ownership() {
    let (mut c, brand) = workspace();
    let other = handle(&mut c, "brand.create", json!({"name":"第二虚构资产品牌"})).unwrap();
    let product = handle(
        &mut c,
        "product.save",
        json!({"brandId":brand["id"],"name":"虚构保温杯","sku":"ASSET-CUP","facts":[]}),
    )
    .unwrap();
    let keyword = create(&mut c, &brand, "keyword", "虚构保温", product["id"].clone());
    let title = handle(&mut c,"asset.create",json!({"brandId":brand["id"],"kind":"title","text":"虚构保温杯标题","source":"虚构标题记录","productId":null,"requestId":"90000000-0000-4000-8000-0000000000a2"})).unwrap();
    let keywords = handle(
        &mut c,
        "asset.list",
        json!({"brandId":brand["id"],"kind":"keyword"}),
    )
    .unwrap();
    assert_eq!(keywords, json!([keyword.clone()]));
    assert_eq!(
        handle(&mut c, "asset.list", json!({"brandId":other["id"]})).unwrap(),
        json!([])
    );
    assert!(handle(&mut c,"asset.create",json!({"brandId":other["id"],"kind":"title","text":"越权标题","source":"越权来源","productId":product["id"],"requestId":"90000000-0000-4000-8000-0000000000a3"})).is_err());
    assert!(handle(&mut c,"asset.revise",json!({"brandId":other["id"],"id":keyword["id"],"expectedRevision":1,"text":"越权修订","source":"越权来源","productId":null})).is_err());
    assert!(handle(&mut c,"asset.review",json!({"brandId":other["id"],"id":title["id"],"expectedRevision":1,"decision":"approved","note":""})).is_err());
    assert_eq!(
        handle(&mut c, "asset.list", json!({"brandId":brand["id"]}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    validate_database(&c).unwrap();
}

#[test]
fn rejection_requires_reason_and_every_revision_preserves_review_history() {
    let (mut c, brand) = workspace();
    let created = create(&mut c, &brand, "title", "虚构初版标题", Value::Null);
    assert!(handle(&mut c,"asset.review",json!({"brandId":brand["id"],"id":created["id"],"expectedRevision":1,"decision":"rejected","note":"  "})).is_err());
    let rejected = review(&mut c, &brand, &created, "rejected", "来源不够明确");
    assert_eq!(rejected["revision"], 2);
    assert_eq!(rejected["reviewStatus"], "rejected");
    assert_eq!(rejected["reviewNote"], "来源不够明确");
    let revised = revise(&mut c, &brand, &rejected, "虚构第二版标题");
    assert_eq!(revised["revision"], 3);
    assert_eq!(revised["reviewStatus"], "pending");
    assert_eq!(revised["reviewNote"], "");
    assert!(revised["reviewedAt"].is_null());
    assert_eq!(revised["history"][1]["reviewStatus"], "rejected");
    let approved = review(&mut c, &brand, &revised, "approved", "");
    assert_eq!(approved["revision"], 4);
    assert_eq!(approved["reviewStatus"], "approved");
    assert!(!approved["reviewedAt"].is_null());
    let changed = revise(&mut c, &brand, &approved, "虚构已审核后再修订");
    assert_eq!(changed["revision"], 5);
    assert_eq!(changed["reviewStatus"], "pending");
    assert!(changed["reviewedAt"].is_null());
    assert_eq!(changed["history"].as_array().unwrap().len(), 5);
    assert_eq!(changed["history"][3]["text"], "虚构第二版标题");
    assert_eq!(changed["history"][3]["reviewStatus"], "approved");
    assert_eq!(changed["history"][4]["action"], "revised");
    assert!(handle(&mut c,"asset.review",json!({"brandId":brand["id"],"id":changed["id"],"expectedRevision":5,"decision":"approved","note":"","unknown":null})).is_err());
    validate_database(&c).unwrap();
}

#[test]
fn stale_revision_and_repeated_review_are_rejected_without_mutation() {
    let (mut c, brand) = workspace();
    let created = create(&mut c, &brand, "keyword", "虚构锁版本", Value::Null);
    let approved = review(&mut c, &brand, &created, "approved", "");
    assert!(handle(&mut c,"asset.review",json!({"brandId":brand["id"],"id":created["id"],"expectedRevision":1,"decision":"rejected","note":"旧版本驳回"})).is_err());
    assert!(handle(&mut c,"asset.review",json!({"brandId":brand["id"],"id":approved["id"],"expectedRevision":2,"decision":"approved","note":""})).is_err());
    assert!(handle(&mut c,"asset.revise",json!({"brandId":brand["id"],"id":created["id"],"expectedRevision":1,"text":"旧版本修订","source":"旧来源","productId":null})).is_err());
    assert_eq!(
        handle(&mut c, "asset.list", json!({"brandId":brand["id"]})).unwrap(),
        json!([approved])
    );
    validate_database(&c).unwrap();
}

#[test]
fn encrypted_backup_roundtrip_and_schema6_upgrade_preserve_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("assets.sqlite3");
    let mut source = crate::runtime::open_database(&path).unwrap();
    let brand = handle(
        &mut source,
        "brand.create",
        json!({"name":"虚构资产备份品牌"}),
    )
    .unwrap();
    let created = create(&mut source, &brand, "title", "虚构备份标题", Value::Null);
    let approved = review(&mut source, &brand, &created, "approved", "人工核对");
    let archived = handle(
        &mut source,
        "asset.setArchived",
        json!({
            "brandId":brand["id"],"id":approved["id"],"expectedRevision":approved["revision"],
            "archived":true,"note":"虚构备份归档"
        }),
    )
    .unwrap();
    let bytes = crate::backup::encrypt_snapshot(&source, "fictional-assets-only").unwrap();
    let inspected = crate::backup::inspect_snapshot(&bytes, "fictional-assets-only").unwrap();
    assert_eq!(inspected["targetVersion"], 10);
    assert_eq!(inspected["counts"]["assets"], 1);
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-assets-only").unwrap();
    assert_eq!(
        handle(&mut restored, "asset.list", json!({"brandId":brand["id"]})).unwrap(),
        json!([archived])
    );
    validate_database(&restored).unwrap();

    source.execute_batch("DELETE FROM m_audit WHERE action LIKE 'asset.%'; DROP TABLE m_p0_workflows; DROP TABLE m_p0_policy; DROP TABLE m_offline_workflows; DROP TABLE m_question_research; DROP TABLE m_media; DROP TABLE m_automation_tasks; DROP TABLE m_assets; UPDATE m_core_meta SET schema_version=6; PRAGMA user_version=6;").unwrap();
    drop(source);
    let upgraded = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        upgraded
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        10
    );
    assert_eq!(
        super::brand(&upgraded, brand["id"].as_str().unwrap()).unwrap()["name"],
        "虚构资产备份品牌"
    );
    assert_eq!(
        upgraded
            .query_row("SELECT count(*) FROM m_assets", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("before-optimization-v5-"))
            .count(),
        1
    );
}

#[test]
fn stored_asset_history_and_identity_are_validated_before_restore() {
    let (mut c, brand) = workspace();
    let asset = create(&mut c, &brand, "keyword", "虚构校验", Value::Null);
    let original = c
        .query_row("SELECT data FROM m_assets", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap();
    let mut missing_history = asset.clone();
    missing_history["history"] = json!([]);
    let mut wrong_current = asset.clone();
    wrong_current["updatedAt"] = json!("2030-01-01T00:00:00Z");
    for damaged in [missing_history, wrong_current] {
        c.execute("UPDATE m_assets SET data=?1", [damaged.to_string()])
            .unwrap();
        assert!(validate_database(&c).is_err());
        c.execute("UPDATE m_assets SET data=?1", [original.clone()])
            .unwrap();
    }
    let mut wrong_brand = asset;
    wrong_brand["brandId"] = json!(id());
    c.execute("UPDATE m_assets SET data=?1", [wrong_brand.to_string()])
        .unwrap();
    assert!(validate_database(&c).is_err());
    c.execute("UPDATE m_assets SET data=?1", [original])
        .unwrap();
    validate_database(&c).unwrap();
}

#[test]
fn archive_restore_is_audited_non_destructive_and_blocks_active_writes() {
    let (mut c, brand) = workspace();
    let created = create(&mut c, &brand, "keyword", "虚构归档词", Value::Null);
    let approved = review(&mut c, &brand, &created, "approved", "");
    let archived = handle(
        &mut c,
        "asset.setArchived",
        json!({
            "brandId":brand["id"],"id":approved["id"],"expectedRevision":approved["revision"],
            "archived":true,"note":"虚构活动结束，保留历史引用"
        }),
    )
    .unwrap();
    assert_eq!(archived["revision"], 3);
    assert!(archived["archivedAt"].is_string());
    assert_eq!(archived["archiveNote"], "虚构活动结束，保留历史引用");
    assert_eq!(archived["reviewStatus"], "approved");
    assert_eq!(archived["history"][2]["action"], "archived");
    assert_eq!(
        handle(&mut c, "asset.list", json!({"brandId":brand["id"]})).unwrap(),
        json!([archived.clone()])
    );
    validate_database(&c).unwrap();

    for (action, payload) in [
        (
            "asset.setArchived",
            json!({"brandId":brand["id"],"id":archived["id"],"expectedRevision":3,"archived":true,"note":"重复归档"}),
        ),
        (
            "asset.revise",
            json!({"brandId":brand["id"],"id":archived["id"],"expectedRevision":3,"text":"归档后修订","source":"来源","productId":null}),
        ),
        (
            "asset.review",
            json!({"brandId":brand["id"],"id":archived["id"],"expectedRevision":3,"decision":"approved","note":""}),
        ),
    ] {
        assert!(handle(&mut c, action, payload).is_err());
    }
    assert!(handle(
        &mut c,
        "asset.setArchived",
        json!({
            "brandId":brand["id"],"id":archived["id"],"expectedRevision":3,
            "archived":false,"note":"","unknown":true
        })
    )
    .is_err());

    let restored = handle(
        &mut c,
        "asset.setArchived",
        json!({
            "brandId":brand["id"],"id":archived["id"],"expectedRevision":archived["revision"],
            "archived":false,"note":"恢复时不应保存说明"
        }),
    )
    .unwrap();
    assert_eq!(restored["revision"], 4);
    assert!(restored["archivedAt"].is_null());
    assert_eq!(restored["archiveNote"], "");
    assert_eq!(restored["reviewStatus"], "approved");
    assert_eq!(restored["history"][3]["action"], "restored");
    assert_eq!(restored["history"].as_array().unwrap().len(), 4);
    assert!(handle(&mut c, "asset.setArchived", json!({
        "brandId":brand["id"],"id":restored["id"],"expectedRevision":3,"archived":true,"note":"陈旧版本"
    })).is_err());
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM m_audit WHERE action='asset.setArchived'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    validate_database(&c).unwrap();
}
