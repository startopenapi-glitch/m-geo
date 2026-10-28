use super::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xd9];
const WEBP: &[u8] = b"RIFF\x00\x00\x00\x00WEBP";
const GIF: &[u8] = b"GIF89a";
const MP4: &[u8] = b"\x00\x00\x00\x18ftypisom";
const WEBM: &[u8] = b"\x1a\x45\xdf\xa3webm";

fn workspace() -> (Connection, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let brand = handle(&mut c, "brand.create", json!({"name":"虚构媒体品牌"})).unwrap();
    (c, brand)
}
fn encoded(bytes: &[u8]) -> String {
    // Test fixtures deliberately use tiny invented bytes, never real product media.
    media::encode_base64(bytes)
}
fn import_media(c: &mut Connection, brand: &Value, bytes: &[u8], mime: &str) -> Value {
    handle(
        c,
        "media.import",
        json!({
            "brandId":brand["id"],"filename":"虚构素材.bin","mime":mime,"data":encoded(bytes),
            "alt":"虚构替代文本","caption":"虚构说明","source":"虚构自有素材记录",
            "rightsNote":"虚构测试：已获内部使用授权"
        }),
    )
    .unwrap()
}
fn review(c: &mut Connection, brand: &Value, item: &Value, decision: &str) -> Result<Value> {
    handle(
        c,
        "media.review",
        json!({
            "brandId":brand["id"],"id":item["id"],"expectedRevision":item["revision"],"decision":decision
        }),
    )
}

#[test]
fn imports_all_supported_formats_and_rejects_mime_or_base64_spoofing() {
    let (mut c, brand) = workspace();
    for (bytes, mime) in [
        (PNG, "image/png"),
        (JPEG, "image/jpeg"),
        (WEBP, "image/webp"),
        (GIF, "image/gif"),
        (MP4, "video/mp4"),
        (WEBM, "video/webm"),
    ] {
        let item = import_media(&mut c, &brand, bytes, mime);
        assert_eq!(item["mime"], mime);
        assert_eq!(item["size"], bytes.len());
        assert_eq!(item["reviewStatus"], "pending");
        assert_eq!(item["archived"], false);
        assert!(item.get("data").is_none());
    }
    for payload in [
        json!({"brandId":brand["id"],"filename":"fake.png","mime":"image/png","data":"eHl6"}),
        json!({"brandId":brand["id"],"filename":"fake.png","mime":"image/jpeg","data":encoded(PNG)}),
        json!({"brandId":brand["id"],"filename":"fake.png","mime":"image/png","data":"AA=A"}),
        json!({"brandId":brand["id"],"filename":"fake.webm","mime":"video/webm","data":encoded(b"\x1a\x45\xdf\xa3matroska")}),
    ] {
        assert!(handle(&mut c, "media.import", payload).is_err());
    }
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_media", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
}

#[test]
fn duplicate_bytes_are_idempotent_per_brand_and_get_returns_verified_blob_only_on_demand() {
    let (mut c, brand) = workspace();
    let other = handle(&mut c, "brand.create", json!({"name":"第二虚构媒体品牌"})).unwrap();
    let first = handle(&mut c, "media.import", json!({
        "brandId":brand["id"],"filename":"/Users/fictional/private/original.png","mime":"image/png",
        "data":encoded(PNG),"source":"虚构来源","rightsNote":"虚构权利"
    })).unwrap();
    assert_eq!(first["filename"], "original.png");
    assert!(!first.to_string().contains("/Users/"));
    let replay = handle(
        &mut c,
        "media.import",
        json!({
            "brandId":brand["id"],"filename":"different.gif","mime":"image/png","data":encoded(PNG),
            "alt":"重复导入不会覆盖元数据"
        }),
    )
    .unwrap();
    assert_eq!(replay, first);
    assert_eq!(
        c.query_row("SELECT count(*) FROM m_media", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let isolated = import_media(&mut c, &other, PNG, "image/png");
    assert_ne!(isolated["id"], first["id"]);
    assert!(handle(
        &mut c,
        "media.get",
        json!({"brandId":other["id"],"id":first["id"]})
    )
    .is_err());
    let fetched = handle(
        &mut c,
        "media.get",
        json!({"brandId":brand["id"],"id":first["id"]}),
    )
    .unwrap();
    assert_eq!(fetched["data"], encoded(PNG));
    assert_eq!(
        media::decode_base64(fetched["data"].as_str().unwrap()).unwrap(),
        PNG
    );
    assert!(handle(&mut c, "media.import", json!({
        "brandId":brand["id"],"filename":"path.png","mime":"image/png","data":encoded(b"GIF87a"),
        "source":"file:///Users/fictional/source.png"
    })).is_err());
}

#[test]
fn approval_requires_source_and_rights_and_any_revision_invalidates_old_review() {
    let (mut c, brand) = workspace();
    let pending = handle(
        &mut c,
        "media.import",
        json!({
            "brandId":brand["id"],"filename":"pending.png","mime":"image/png","data":encoded(PNG)
        }),
    )
    .unwrap();
    assert!(review(&mut c, &brand, &pending, "approved")
        .unwrap_err()
        .contains("来源或权利说明"));
    let with_rights = handle(
        &mut c,
        "media.revise",
        json!({
            "brandId":brand["id"],"id":pending["id"],"expectedRevision":1,"filename":"pending.png",
            "source":"虚构来源登记","rightsNote":"虚构内部授权","alt":"","caption":""
        }),
    )
    .unwrap();
    assert_eq!(with_rights["revision"], 2);
    let approved = review(&mut c, &brand, &with_rights, "approved").unwrap();
    assert_eq!(approved["reviewStatus"], "approved");
    assert!(approved["reviewedAt"].is_string());
    assert!(review(&mut c, &brand, &with_rights, "rejected").is_err());
    let revised = handle(
        &mut c,
        "media.revise",
        json!({
            "brandId":brand["id"],"id":approved["id"],"expectedRevision":approved["revision"],
            "filename":"renamed.png","source":"虚构修订来源","rightsNote":"虚构修订授权",
            "alt":"新替代文本","caption":"新说明","mime":"image/gif","data":encoded(GIF)
        }),
    )
    .unwrap();
    assert_eq!(revised["reviewStatus"], "pending");
    assert!(revised["reviewedAt"].is_null());
    assert_eq!(revised["mime"], "image/gif");
    assert_ne!(revised["sha256"], approved["sha256"]);
    assert_eq!(
        revised["revision"],
        approved["revision"].as_i64().unwrap() + 1
    );
    let fetched = handle(
        &mut c,
        "media.get",
        json!({"brandId":brand["id"],"id":revised["id"]}),
    )
    .unwrap();
    assert_eq!(fetched["data"], encoded(GIF));
}

#[test]
fn optimistic_lock_archive_and_cross_brand_writes_do_not_mutate() {
    let (mut c, brand) = workspace();
    let other = handle(&mut c, "brand.create", json!({"name":"媒体隔离品牌"})).unwrap();
    let item = import_media(&mut c, &brand, PNG, "image/png");
    assert!(handle(
        &mut c,
        "media.revise",
        json!({
            "brandId":brand["id"],"id":item["id"],"expectedRevision":0,"filename":"bad.png"
        })
    )
    .is_err());
    assert!(handle(
        &mut c,
        "media.review",
        json!({
            "brandId":other["id"],"id":item["id"],"expectedRevision":1,"decision":"approved"
        })
    )
    .is_err());
    let approved = review(&mut c, &brand, &item, "approved").unwrap();
    let archived = handle(&mut c, "media.setArchived", json!({
        "brandId":brand["id"],"id":item["id"],"expectedRevision":approved["revision"],"archived":true
    })).unwrap();
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["reviewStatus"], "approved");
    assert!(handle(&mut c, "media.revise", json!({
        "brandId":brand["id"],"id":item["id"],"expectedRevision":archived["revision"],"filename":"archived.png"
    })).is_err());
    assert!(review(&mut c, &brand, &archived, "rejected").is_err());
    let restored = handle(&mut c, "media.setArchived", json!({
        "brandId":brand["id"],"id":item["id"],"expectedRevision":archived["revision"],"archived":false
    })).unwrap();
    assert_eq!(restored["archived"], false);
    assert_eq!(restored["reviewStatus"], "approved");
    assert!(handle(
        &mut c,
        "media.setArchived",
        json!({
            "brandId":brand["id"],"id":item["id"],"expectedRevision":1,"archived":true
        })
    )
    .is_err());
    let current = handle(
        &mut c,
        "media.get",
        json!({"brandId":brand["id"],"id":item["id"]}),
    )
    .unwrap();
    assert_eq!(current["revision"], restored["revision"]);
}

#[test]
fn list_filters_metadata_without_serializing_blob() {
    let (mut c, brand) = workspace();
    let pending = import_media(&mut c, &brand, PNG, "image/png");
    let rejected = import_media(&mut c, &brand, GIF, "image/gif");
    let rejected = review(&mut c, &brand, &rejected, "rejected").unwrap();
    let all = handle(&mut c, "media.list", json!({"brandId":brand["id"]})).unwrap();
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert!(!all.to_string().contains("\"data\""));
    assert_eq!(
        handle(
            &mut c,
            "media.list",
            json!({"brandId":brand["id"],"reviewStatus":"pending"})
        )
        .unwrap(),
        json!([pending])
    );
    assert_eq!(
        handle(
            &mut c,
            "media.list",
            json!({"brandId":brand["id"],"reviewStatus":"rejected"})
        )
        .unwrap(),
        json!([rejected])
    );
    assert!(handle(
        &mut c,
        "media.list",
        json!({"brandId":brand["id"],"unexpected":true})
    )
    .is_err());
}

#[test]
fn encrypted_backup_roundtrip_preserves_blob_and_metadata_without_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("media.sqlite3");
    let mut source = crate::runtime::open_database(&path).unwrap();
    let brand = handle(
        &mut source,
        "brand.create",
        json!({"name":"虚构媒体备份品牌"}),
    )
    .unwrap();
    let item = import_media(&mut source, &brand, WEBM, "video/webm");
    let bytes = crate::backup::encrypt_snapshot(&source, "fictional-media-only").unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-media-only").unwrap();
    let fetched = handle(
        &mut restored,
        "media.get",
        json!({"brandId":brand["id"],"id":item["id"]}),
    )
    .unwrap();
    assert_eq!(fetched["data"], encoded(WEBM));
    assert_eq!(fetched["sha256"], item["sha256"]);
    let blob = restored
        .query_row(
            "SELECT blob FROM m_media WHERE id=?1",
            [item["id"].as_str().unwrap()],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .unwrap();
    assert_eq!(blob, WEBM);
    assert!(!fetched.to_string().contains("file://"));
}
