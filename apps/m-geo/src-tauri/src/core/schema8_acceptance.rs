//! Schema 8 restore acceptance using only invented in-memory/tempfile fixtures.
use super::*;

fn database() -> (Connection, Value) {
    let mut connection = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let brand = handle(
        &mut connection,
        "brand.create",
        json!({"name":"虚构 schema8 验收品牌"}),
    )
    .unwrap();
    (connection, brand)
}

fn product(c: &mut Connection, brand: &Value) -> Value {
    handle(
        c,
        "product.save",
        json!({"brandId":brand["id"],"name":"虚构验收商品","sku":"SCHEMA8-ACCEPT","facts":[]}),
    )
    .unwrap()
}

fn import_media(c: &mut Connection, brand: &Value) -> Value {
    handle(
        c,
        "media.import",
        json!({
            "brandId":brand["id"],"filename":"fictional.png","mime":"image/png",
            "data":"iVBORw0KGgo=","alt":"虚构替代文本","caption":"虚构素材",
            "source":"虚构自有素材","rightsNote":"虚构测试授权"
        }),
    )
    .unwrap()
}

fn save_research(c: &mut Connection, brand: &Value, product: &Value) -> Value {
    handle(
        c,
        "research.save",
        json!({
            "brandId":brand["id"],"productId":product["id"],"question":"虚构验收商品如何保养？",
            "intent":"usage","sourceKind":"manual","sourceLabel":"虚构验收访谈",
            "sourceUrl":"https://example.com/schema8/research","observedAt":"2026-10-24T01:02:03Z",
            "clusterKey":"验收分组"
        }),
    )
    .unwrap()
}

fn create_task(c: &mut Connection, brand: &Value) -> Value {
    handle(
        c,
        "automation.create",
        json!({
            "brandId":brand["id"],"requestId":"90000000-0000-4000-8000-000000000091",
            "name":"虚构验收只读任务","intervalMinutes":60,
            "nodes":[{"id":"facts","kind":"fact_expiry_scan","dependsOn":[]}]
        }),
    )
    .unwrap()
}

fn stored(c: &Connection, table: &str, id: &str) -> Value {
    let data = c
        .query_row(
            &format!("SELECT data FROM {table} WHERE id=?1"),
            [id],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    parse_data(data).unwrap()
}

fn replace_stored(c: &Connection, table: &str, value: &Value) {
    c.execute(
        &format!("UPDATE {table} SET data=?1 WHERE id=?2"),
        params![value.to_string(), value["id"].as_str()],
    )
    .unwrap();
}

fn assert_invalid_after(c: &Connection, table: &str, original: &Value, invalid: Value) {
    replace_stored(c, table, &invalid);
    assert!(validate_database(c).is_err(), "accepted {invalid}");
    replace_stored(c, table, original);
    validate_database(c).unwrap();
}

#[test]
fn schema8_accepts_bounded_profile_media_research_task_and_legacy_observation() {
    let (mut c, brand) = database();
    let product = product(&mut c, &brand);
    handle(
        &mut c,
        "brand.profile.save",
        json!({
            "brandId":brand["id"],"aliases":["虚构别名","验收别名"],
            "officialUrl":"https://example.com/brand","manufacturer":"虚构厂商",
            "category":"虚构分类","description":"仅用于 schema8 验收"
        }),
    )
    .unwrap();
    import_media(&mut c, &brand);
    save_research(&mut c, &brand, &product);
    create_task(&mut c, &brand);
    let question = handle(
        &mut c,
        "question.save",
        json!({
            "brandId":brand["id"],"productId":product["id"],"text":"日常如何保养保温杯？",
            "intent":"usage","origin":"customer","source":"虚构旧记录","branded":false,"confirmed":true
        }),
    )
    .unwrap();
    let legacy = handle(
        &mut c,
        "observation.add",
        json!({
            "brandId":brand["id"],"productId":product["id"],"questionId":question["id"],
            "platform":"doubao","modelLabel":"虚构人工记录","surface":"consumer","searchMode":"未请求",
            "phase":"baseline","answer":"虚构历史回答，不代表真实平台结果","mentioned":false,
            "accuracy":"unknown","citedUrl":"","evidenceNote":"历史人工记录","testedAt":"2026-10-24T02:00:00Z"
        }),
    )
    .unwrap();
    for key in [
        "evidenceMode",
        "sourceKind",
        "transport",
        "responseModel",
        "providerRequestId",
        "citations",
        "preparationHash",
        "resultIdentity",
    ] {
        assert!(legacy.get(key).is_none());
    }
    validation::validate(&c).unwrap();
    validate_database(&c).unwrap();
    let encrypted = crate::backup::encrypt_snapshot(&c, "schema8-acceptance-only").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &encrypted, "schema8-acceptance-only").unwrap();
    validate_database(&restored).unwrap();
}

#[test]
fn schema8_media_restore_rejects_blob_metadata_dedup_and_identity_tampering() {
    let (mut c, brand) = database();
    let media = import_media(&mut c, &brand);
    let id = media["id"].as_str().unwrap();
    let original = stored(&c, "m_media", id);
    validate_database(&c).unwrap();

    for (key, value) in [
        ("size", json!(7)),
        ("sha256", json!("0".repeat(64))),
        ("mime", json!("image/gif")),
        ("revision", json!(0)),
        ("brandId", json!(super::id())),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        assert_invalid_after(&c, "m_media", &original, invalid);
    }
    let mut extra = original.clone();
    extra["data"] = json!("iVBORw0KGgo=");
    assert_invalid_after(&c, "m_media", &original, extra);

    c.execute(
        "UPDATE m_media SET blob=?1 WHERE id=?2",
        params![b"GIF89a".as_slice(), id],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
    c.execute(
        "UPDATE m_media SET blob=?1 WHERE id=?2",
        params![b"\x89PNG\r\n\x1a\n".as_slice(), id],
    )
    .unwrap();
    validate_database(&c).unwrap();
    c.execute(
        "UPDATE m_media SET dedup=?1 WHERE id=?2",
        params!["0".repeat(64), id],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
}

#[test]
fn schema8_research_restore_rejects_unknown_fields_ownership_dates_revision_and_dedup() {
    let (mut c, brand) = database();
    let product = product(&mut c, &brand);
    let research = save_research(&mut c, &brand, &product);
    let id = research["id"].as_str().unwrap();
    let original = stored(&c, "m_question_research", id);

    for (key, value) in [
        ("revision", json!(0)),
        ("observedAt", json!("2026-99-99")),
        ("sourceKind", json!("real_api")),
        ("productId", json!(super::id())),
        ("brandId", json!(super::id())),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        assert_invalid_after(&c, "m_question_research", &original, invalid);
    }
    let mut unknown = original.clone();
    unknown["unexpected"] = json!(true);
    assert_invalid_after(&c, "m_question_research", &original, unknown);
    c.execute(
        "UPDATE m_question_research SET dedup=?1 WHERE id=?2",
        params!["0".repeat(64), id],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
}

fn publication_fixture(c: &mut Connection, brand: &Value) -> Value {
    let mut product = handle(
        c,
        "product.save",
        json!({
            "brandId":brand["id"],"name":"虚构回读商品","sku":"SCHEMA8-READBACK","facts":[{
                "id":"fact","label":"虚构容量","value":"500mL","source":"虚构规格表",
                "visibility":"public","validUntil":null
            }]
        }),
    )
    .unwrap();
    product = handle(
        c,
        "product.confirm",
        json!({"brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]}),
    )
    .unwrap();
    assert!(product["confirmedAt"].is_string());
    let draft = handle(
        c,
        "draft.generateLocal",
        json!({"brandId":brand["id"],"productId":product["id"],"kind":"description"}),
    )
    .unwrap();
    let draft = handle(
        c,
        "draft.review",
        json!({"brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"]}),
    )
    .unwrap();
    handle(
        c,
        "publication.add",
        json!({
            "brandId":brand["id"],"draftId":draft["id"],"expectedRevision":draft["revision"],
            "channel":"虚构发布登记","url":"https://example.com/schema8-publication",
            "publishedAt":"2026-10-24T01:00:00Z","note":"未执行真实发布"
        }),
    )
    .unwrap()
}

fn readback(c: &Connection, brand: &Value, publication: &Value) -> Value {
    publication_readback::dispatch(
        c,
        "publication.recordReadback",
        &json!({
            "id":publication["id"],"result":{
                "fetchedAt":"2026-10-24T02:00:00Z","httpStatus":200,
                "finalUrl":"https://example.com/schema8-publication","title":"虚构页面",
                "bodySha256":"a".repeat(64),"draftRevision":publication["draftRevision"],
                "matchedContent":true,"errorCategory":null
            }
        }),
        brand["id"].as_str().unwrap(),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn schema8_publication_and_profile_optional_history_is_strictly_bounded() {
    let (mut c, brand) = database();
    let publication = publication_fixture(&mut c, &brand);
    let readback = readback(&c, &brand, &publication);
    validate_database(&c).unwrap();
    let original = stored(&c, "m_publications", publication["id"].as_str().unwrap());

    let mut bad_hash = original.clone();
    bad_hash["readbacks"][0]["bodySha256"] = json!("not-a-hash");
    assert_invalid_after(&c, "m_publications", &original, bad_hash);
    let mut bad_transport = original.clone();
    bad_transport["readbacks"][0]["transport"] = json!("real_api");
    assert_invalid_after(&c, "m_publications", &original, bad_transport);
    let mut too_many = original.clone();
    too_many["readbacks"] = json!(vec![readback["readbacks"][0].clone(); 13]);
    assert_invalid_after(&c, "m_publications", &original, too_many);

    let withdrawn = publication_readback::dispatch(
        &c,
        "publication.withdraw",
        &json!({
            "id":publication["id"],"confirm":true,"reason":"虚构人工撤回登记","status":"withdrawing"
        }),
        brand["id"].as_str().unwrap(),
    )
    .unwrap()
    .unwrap();
    validate_database(&c).unwrap();
    let mut excessive_history = withdrawn.clone();
    excessive_history["withdrawal"]["history"] = json!(vec![
        json!({
            "status":"withdrawn","reason":"虚构历史","recordedAt":"2026-10-24T03:00:00Z"
        });
        13
    ]);
    assert_invalid_after(&c, "m_publications", &withdrawn, excessive_history);

    handle(
        &mut c,
        "brand.profile.save",
        json!({"brandId":brand["id"],"aliases":["虚构别名"],"officialUrl":"https://example.com/brand"}),
    )
    .unwrap();
    let original_brand = super::brand(&c, brand["id"].as_str().unwrap()).unwrap();
    let mut invalid_profile = original_brand.clone();
    invalid_profile["profile"]["aliases"] =
        json!((0..21).map(|n| format!("别名{n}")).collect::<Vec<_>>());
    assert_invalid_after(&c, "m_brands", &original_brand, invalid_profile);
}

fn real_observation_fixture(c: &mut Connection, brand: &Value) -> Value {
    let product = handle(
        c,
        "product.save",
        json!({"brandId":brand["id"],"name":"虚构监测商品","sku":"SCHEMA8-MONITOR","facts":[]}),
    )
    .unwrap();
    let question = handle(
        c,
        "question.save",
        json!({
            "brandId":brand["id"],"productId":product["id"],"text":"保温杯如何清洗？",
            "intent":"usage","origin":"customer","source":"虚构监测固定问题","branded":false,"confirmed":true
        }),
    )
    .unwrap();
    let campaign = handle(
        c,
        "campaign.create",
        json!({
            "brandId":brand["id"],"name":"虚构真实证据格式验收","evidenceMode":"recorded",
            "questionIds":[question["id"]],"platforms":["doubao"],"modelLabel":"fictional-model",
            "searchMode":"虚构 API 条件","context":"固定验收上下文","repetitions":1
        }),
    )
    .unwrap();
    let assessment = json!({
        "campaignId":campaign["id"],"repetition":1,"context":"固定验收上下文",
        "recommendation":"unknown","rank":null,"competitors":""
    });
    let preparation = json!({
        "kind":"monitor","brandId":brand["id"],"productId":product["id"],"questionId":question["id"],
        "modelLabel":"fictional-model","searchMode":"虚构 API 条件","platform":"doubao",
        "phase":"baseline","assessment":assessment
    });
    let preparation_hash = fingerprint(&preparation);
    c.execute(
        "INSERT INTO m_ai_preparations(hash,context,draft_id) VALUES(?1,?2,NULL)",
        params![preparation_hash, preparation.to_string()],
    )
    .unwrap();
    let citations = json!(["https://example.com/schema8-evidence"]);
    let tested_at = "2026-10-24T04:05:06Z";
    let answer = "虚构监测回答，不发起网络请求";
    let provider_request_id = json!("fictional-provider-request");
    let result_identity = fingerprint(&json!({
        "preparationHash":preparation_hash,"testedAt":tested_at,"model":"fictional-model",
        "providerRequestId":provider_request_id,"answer":answer,"citations":citations
    }));
    let observation = add_monitor_observation(
        c,
        brand["id"].as_str().unwrap(),
        &json!({
            "brandId":brand["id"],"productId":product["id"],"questionId":question["id"],
            "platform":"doubao","surface":"api","phase":"baseline","modelLabel":"fictional-model",
            "searchMode":"虚构 API 条件","testedAt":tested_at,"answer":answer,"mentioned":false,
            "accuracy":"unknown","citedUrl":citations[0],"evidenceNote":"虚构真实格式夹具",
            "assessment":assessment,"evidenceMode":"recorded","sourceKind":"real_api",
            "transport":"real_api","responseModel":"fictional-model","providerRequestId":provider_request_id,
            "citations":citations,"preparationHash":preparation_hash,"resultIdentity":result_identity
        }),
    )
    .unwrap();
    // Recovery snapshots intentionally exclude transient preparations. The durable
    // observation must remain independently verifiable after they are gone.
    c.execute("DELETE FROM m_ai_preparations", []).unwrap();
    validate_database(c).unwrap();
    observation
}

#[test]
fn schema8_real_observation_identity_and_historical_annotations_survive_restore() {
    let (mut c, brand) = database();
    let observation = real_observation_fixture(&mut c, &brand);
    let reviewed = handle(
        &mut c,
        "observation.assess",
        json!({
            "brandId":brand["id"],"id":observation["id"],"expectedRevision":1,
            "mentioned":true,"accuracy":"correct","citedUrl":"https://example.com/human-reviewed-citation",
            "evidenceNote":"虚构人工复核","recommendation":"yes","rank":1,"competitors":""
        }),
    )
    .unwrap();
    assert_eq!(reviewed["assessmentRevision"], 2);
    assert_eq!(reviewed["assessmentHistory"].as_array().unwrap().len(), 1);
    validate_database(&c).unwrap();

    let original = stored(&c, "m_observations", observation["id"].as_str().unwrap());
    for (key, value) in [
        ("evidenceMode", json!("simulated")),
        ("sourceKind", json!("manual")),
        ("transport", json!("consumer")),
        ("responseModel", json!("different-model")),
        ("preparationHash", json!("not-a-hash")),
        ("resultIdentity", json!("0".repeat(64))),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        assert_invalid_after(&c, "m_observations", &original, invalid);
    }
    let mut bad_citation = original.clone();
    bad_citation["citations"] = json!(["http://localhost/private"]);
    assert_invalid_after(&c, "m_observations", &original, bad_citation);
    let mut bad_original_annotation = original.clone();
    bad_original_annotation["assessmentHistory"][0]["citedUrl"] = json!("");
    assert_invalid_after(&c, "m_observations", &original, bad_original_annotation);

    let mut duplicate = original.clone();
    duplicate["id"] = json!(super::id());
    duplicate["phase"] = json!("followup");
    let duplicate_id = duplicate["id"].as_str().unwrap().to_owned();
    let duplicate_dedup = fingerprint(&observations::submission(&duplicate));
    c.execute(
        "INSERT INTO m_observations(id,brand_id,product_id,question_id,dedup,data) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            duplicate_id,
            duplicate["brandId"].as_str(),
            duplicate["productId"].as_str(),
            duplicate["questionId"].as_str(),
            duplicate_dedup,
            duplicate.to_string()
        ],
    )
    .unwrap();
    assert!(validate_database(&c).is_err());
    c.execute("DELETE FROM m_observations WHERE id=?1", [duplicate_id])
        .unwrap();
    validate_database(&c).unwrap();

    let encrypted = crate::backup::encrypt_snapshot(&c, "schema8-observation-only").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &encrypted, "schema8-observation-only").unwrap();
    let after = stored(
        &restored,
        "m_observations",
        observation["id"].as_str().unwrap(),
    );
    assert_eq!(after, reviewed);
    validate_database(&restored).unwrap();
}

#[test]
fn schema8_core_restore_invokes_automation_validation() {
    let (mut c, brand) = database();
    let task = create_task(&mut c, &brand);
    validate_database(&c).unwrap();
    let mut invalid = stored(&c, "m_automation_tasks", task["id"].as_str().unwrap());
    invalid["createFingerprint"] = json!("0".repeat(64));
    replace_stored(&c, "m_automation_tasks", &invalid);
    assert!(validate_database(&c).is_err());
}
