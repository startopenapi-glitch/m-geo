use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use rusqlite::Connection;
use std::{io::Write, time::Duration};
const MAGIC: &[u8; 8] = b"MGEOBK01";
const MAX: usize = 100 * 1024 * 1024;
fn error<T>(_: T) -> String {
    "备份或恢复未完成，当前资料未被替换".into()
}
fn key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    if password.chars().count() < 10 || password.len() > 256 {
        return Err("备份口令请使用至少10个字符，最多256字节".into());
    }
    let mut result = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut result)
        .map_err(error)?;
    Ok(result)
}
fn schema(db: &Connection) -> Result<Vec<(String, String, String)>, String> {
    let mut statement=db.prepare("SELECT type,name,COALESCE(sql,'') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").map_err(error)?;
    let result = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(error)?;
    Ok(result)
}
// Compare against code-owned shapes, never a possibly damaged destination database.
pub(crate) fn validate_schema_version(db: &Connection, version: i64) -> Result<(), String> {
    let marker: i64 = db
        .query_row(
            "SELECT schema_version FROM m_core_meta WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(error)?;
    let user: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(error)?;
    if ![1, 2, 3, 4, 5].contains(&version) || marker != version || user != version {
        return Err("资料版本标记不一致，请使用原版本恢复".into());
    }
    let mut reference = Connection::open_in_memory().map_err(error)?;
    crate::core::initialize(&mut reference)?;
    crate::ai::initialize(&mut reference)?;
    let expected: Vec<_> = schema(&reference)?
        .into_iter()
        .filter(|(_, name, _)| {
            !(version < 5 && name == "m_optimization_plans"
                || version < 4 && name == "m_local_jobs"
                || version < 3 && name == "m_attribution_datasets"
                || version < 2 && name == "m_campaigns")
        })
        .collect();
    if schema(db)? != expected {
        return Err("备份结构与此版本不一致，请使用原版本恢复".into());
    }
    Ok(())
}
fn validate_storage(db: &Connection) -> Result<(), String> {
    db.pragma_update(None, "trusted_schema", false)
        .map_err(error)?;
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(error)?;
    if ![1, 2, 3, 4, 5].contains(&version) {
        return Err("数据格式版本不兼容，请使用对应版本的 M- GEO".into());
    }
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(error)?;
    if integrity != "ok" {
        return Err("备份中的数据库校验失败".into());
    }
    Ok(())
}
fn validate(db: &Connection) -> Result<(), String> {
    validate_storage(db)?;
    let mut stmt = db.prepare("PRAGMA foreign_key_check").map_err(error)?;
    if stmt
        .query([])
        .map_err(error)?
        .next()
        .map_err(error)?
        .is_some()
    {
        return Err("备份中的数据关联校验失败".into());
    }
    Ok(())
}
pub fn encrypt_snapshot(db: &Connection, password: &str) -> Result<Vec<u8>, String> {
    validate(db)?;
    crate::core::validate_database(db)?;
    encrypt_recovery_snapshot(db, password)
}
// A recovery snapshot must preserve logically damaged current records, too.
// It still excludes credentials and requires a readable, internally consistent SQLite database.
fn encrypt_recovery_snapshot(db: &Connection, password: &str) -> Result<Vec<u8>, String> {
    validate_storage(db)?;
    let temp = tempfile::NamedTempFile::new().map_err(error)?;
    let mut copy = Connection::open(temp.path()).map_err(error)?;
    {
        let backup = rusqlite::backup::Backup::new(db, &mut copy).map_err(error)?;
        backup
            .run_to_completion(128, Duration::from_millis(1), None)
            .map_err(error)?;
    }
    for table in ["ai_settings", "ai_jobs", "m_ai_preparations"] {
        let exists: i64 = copy
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .map_err(error)?;
        if exists > 0 {
            copy.execute(&format!("DELETE FROM {table}"), [])
                .map_err(error)?;
        }
    }
    // Erase deleted credential references and transient prompts from free pages, too.
    copy.execute_batch("VACUUM;").map_err(error)?;
    drop(copy);
    let bytes = std::fs::read(temp.path()).map_err(error)?;
    if bytes.len() > MAX {
        return Err("当前备份超过100MB，请联系支持处理".into());
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut secret = key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&secret).map_err(error)?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &bytes,
                aad: MAGIC,
            },
        )
        .map_err(error)?;
    secret.fill(0);
    let mut out = Vec::with_capacity(36 + encrypted.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend(encrypted);
    Ok(out)
}
struct PreparedRestore {
    source: Connection,
    source_version: i64,
    // Drop the open SQLite connection before unlinking its private file (Windows included).
    _file: tempfile::NamedTempFile,
}
fn prepare_restore(data: &[u8], password: &str) -> Result<PreparedRestore, String> {
    if data.len() < 52 || data.len() > MAX + 52 || &data[..8] != MAGIC {
        return Err("不是有效的 M- GEO 加密备份，或文件过大".into());
    }
    let mut secret = key(password, &data[8..24])?;
    let cipher = Aes256Gcm::new_from_slice(&secret).map_err(error)?;
    let decoded = cipher
        .decrypt(
            Nonce::from_slice(&data[24..36]),
            Payload {
                msg: &data[36..],
                aad: MAGIC,
            },
        )
        .map_err(|_| "口令不正确，或备份文件已损坏".to_string())?;
    secret.fill(0);
    let mut file = tempfile::NamedTempFile::new().map_err(error)?;
    file.write_all(&decoded).map_err(error)?;
    file.flush().map_err(error)?;
    let mut source = Connection::open(file.path()).map_err(error)?;
    validate(&source)?;
    let version: i64 = source
        .query_row(
            "SELECT schema_version FROM m_core_meta WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(error)?;
    validate_schema_version(&source, version)?;
    crate::core::validate_database(&source)?;
    crate::core::initialize(&mut source)?;
    source
        .pragma_update(None, "user_version", 5)
        .map_err(error)?;
    crate::core::validate_database(&source)?;
    for table in ["ai_settings", "ai_jobs", "m_ai_preparations"] {
        let count: i64 = source
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .map_err(error)?;
        if count != 0 {
            return Err("备份含模型配置或临时任务，已拒绝恢复以保护本机凭据".into());
        }
    }
    Ok(PreparedRestore {
        source,
        source_version: version,
        _file: file,
    })
}
fn apply_restore(db: &mut Connection, prepared: &PreparedRestore) -> Result<(), String> {
    // SQLite backup commits the replacement atomically after all candidate checks.
    let backup = rusqlite::backup::Backup::new(&prepared.source, db).map_err(error)?;
    backup
        .run_to_completion(128, Duration::from_millis(1), None)
        .map_err(error)
}
pub fn inspect_snapshot(data: &[u8], password: &str) -> Result<serde_json::Value, String> {
    let prepared = prepare_restore(data, password)?;
    let mut counts = serde_json::Map::new();
    for (key, table) in [
        ("brands", "m_brands"),
        ("products", "m_products"),
        ("datasets", "m_attribution_datasets"),
        ("localJobs", "m_local_jobs"),
        ("optimizationPlans", "m_optimization_plans"),
    ] {
        let count: i64 = prepared
            .source
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .map_err(error)?;
        counts.insert(key.into(), serde_json::json!(count));
    }
    Ok(
        serde_json::json!({"valid":true,"sourceVersion":prepared.source_version,"targetVersion":5,"counts":counts,"workspaceUnchanged":true}),
    )
}
pub fn restore_snapshot(db: &mut Connection, data: &[u8], password: &str) -> Result<(), String> {
    let prepared = prepare_restore(data, password)?;
    apply_restore(db, &prepared)
}
pub fn restore_workspace(
    db: &mut Connection,
    data: &[u8],
    password: &str,
    dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    crate::runtime::no_active_jobs(db)?;
    // Reject a wrong password or invalid candidate before writing any recovery file.
    let mut prepared = prepare_restore(data, password)?;
    // No fallible database mutation after the destination replacement has committed.
    crate::ai::initialize(&mut prepared.source)?;
    let safety = encrypt_recovery_snapshot(db, password)?;
    std::fs::create_dir_all(dir).map_err(error)?;
    let path = dir.join(format!("before-restore-{}.mgeobak", uuid::Uuid::new_v4()));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(error)?;
    tmp.write_all(&safety).map_err(error)?;
    tmp.as_file().sync_all().map_err(error)?;
    tmp.persist_noclobber(&path).map_err(error)?;
    #[cfg(unix)]
    std::fs::File::open(dir)
        .and_then(|directory| directory.sync_all())
        .map_err(error)?;
    // If replacement fails, the old workspace and its encrypted safety snapshot remain.
    apply_restore(db, &prepared)?;
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn phase6_backup_inspection_validates_without_live_mutation() {
        let current = sample();
        let bytes = encrypt_snapshot(&current, "my-safe-passphrase").unwrap();
        let version: i64 = current
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let before = sample_value(&current);
        let result = inspect_snapshot(&bytes, "my-safe-passphrase").unwrap();
        assert_eq!(result["valid"], true);
        assert_eq!(result["sourceVersion"], version);
        assert_eq!(result["targetVersion"], 5);
        assert_eq!(result["counts"]["brands"], 1);
        assert_eq!(result["counts"]["localJobs"], 0);
        assert_eq!(result["workspaceUnchanged"], true);
        assert!(inspect_snapshot(&bytes, "wrong-password").is_err());
        let mut corrupt = bytes;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(inspect_snapshot(&corrupt, "my-safe-passphrase").is_err());
        assert_eq!(sample_value(&current), before);
        assert_eq!(
            current
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
    }

    fn sample() -> Connection {
        let db = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        set_sample_value(&db, "原始资料");
        db
    }
    fn raw_backup(db: &Connection) -> Vec<u8> {
        let f = tempfile::NamedTempFile::new().unwrap();
        let mut copy = Connection::open(f.path()).unwrap();
        rusqlite::backup::Backup::new(db, &mut copy)
            .unwrap()
            .run_to_completion(128, Duration::from_millis(1), None)
            .unwrap();
        drop(copy);
        let bytes = std::fs::read(f.path()).unwrap();
        let salt = [7u8; 16];
        let nonce = [8u8; 12];
        let k = key("my-safe-passphrase", &salt).unwrap();
        let encrypted = Aes256Gcm::new_from_slice(&k)
            .unwrap()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &bytes,
                    aad: MAGIC,
                },
            )
            .unwrap();
        [
            MAGIC.as_slice(),
            salt.as_slice(),
            nonce.as_slice(),
            encrypted.as_slice(),
        ]
        .concat()
    }
    const SAMPLE_ID: &str = "90000000-0000-4000-8000-000000000001";
    fn set_sample_value(db: &Connection, value: &str) {
        let data = json!({"id":SAMPLE_ID,"name":value,"createdAt":"2026-10-01T00:00:00Z"});
        db.execute("INSERT INTO m_brands(id,name,data) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET name=excluded.name,data=excluded.data",rusqlite::params![SAMPLE_ID,value,data.to_string()]).unwrap();
    }
    fn sample_value(db: &Connection) -> String {
        db.query_row("SELECT name FROM m_brands WHERE id=?1", [SAMPLE_ID], |r| {
            r.get(0)
        })
        .unwrap()
    }
    #[test]
    fn encrypted_backup_round_trip_and_wrong_password_preserves_existing() {
        let original = sample();
        let data = encrypt_snapshot(&original, "my-safe-passphrase").unwrap();
        assert!(!String::from_utf8_lossy(&data).contains("原始资料"));
        let mut target = sample();
        set_sample_value(&target, "当前资料");
        assert!(restore_snapshot(&mut target, &data, "wrong-password").is_err());
        assert_eq!(sample_value(&target), "当前资料");
        restore_snapshot(&mut target, &data, "my-safe-passphrase").unwrap();
        assert_eq!(sample_value(&target), "原始资料");
    }
    #[test]
    fn tampered_and_future_backup_do_not_replace_data() {
        let original = sample();
        let mut data = encrypt_snapshot(&original, "my-safe-passphrase").unwrap();
        let last = data.len() - 1;
        data[last] ^= 1;
        let mut target = sample();
        assert!(restore_snapshot(&mut target, &data, "my-safe-passphrase").is_err());
        original.pragma_update(None, "user_version", 900).unwrap();
        assert!(encrypt_snapshot(&original, "my-safe-passphrase").is_err());
    }
    #[test]
    fn backup_excludes_model_configuration_and_jobs() {
        let original = sample();
        original
            .execute("UPDATE ai_settings SET key_ref='provider-secret-ref'", [])
            .unwrap();
        original.execute_batch("INSERT INTO ai_jobs(id,brand_id,kind,status,created_at,context_json) VALUES('id','brand','extract','failed','2026-10-17T00:00:00Z','sensitive-prompt');").unwrap();
        let data = encrypt_snapshot(&original, "my-safe-passphrase").unwrap();
        let mut target = sample();
        restore_snapshot(&mut target, &data, "my-safe-passphrase").unwrap();
        for table in ["ai_settings", "ai_jobs", "m_ai_preparations"] {
            let count: i64 = target
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0);
        }
    }
    #[test]
    fn unexpected_database_schema_is_rejected() {
        let original = sample();
        original
            .execute_batch(
                "CREATE TRIGGER surprise AFTER UPDATE ON m_brands BEGIN DELETE FROM m_brands;END;",
            )
            .unwrap();
        let data = encrypt_snapshot(&original, "my-safe-passphrase").unwrap();
        let mut target = sample();
        assert!(restore_snapshot(&mut target, &data, "my-safe-passphrase").is_err());
        assert_eq!(sample_value(&target), "原始资料");
    }
    #[test]
    fn restore_rejects_business_corruption_and_preserves_target() {
        for cross_brand in [false, true] {
            let mut source = sample();
            let b =
                crate::core::handle(&mut source, "brand.create", json!({"name":"来源"})).unwrap();
            let mut product = crate::core::handle(
                &mut source,
                "product.save",
                json!({"brandId":b["id"],"name":"杯","sku":"1","facts":[]}),
            )
            .unwrap();
            product["brandId"] = json!("wrong-brand");
            source
                .execute(
                    "UPDATE m_products SET data=?1",
                    [if cross_brand {
                        product.to_string()
                    } else {
                        "not JSON".to_string()
                    }],
                )
                .unwrap();
            source.execute("DELETE FROM ai_settings", []).unwrap();
            let bytes = raw_backup(&source);
            let mut target = sample();
            set_sample_value(&target, "当前资料");
            assert!(restore_snapshot(&mut target, &bytes, "my-safe-passphrase").is_err());
            assert_eq!(sample_value(&target), "当前资料");
        }
    }
    #[test]
    fn restore_rejects_carried_model_configuration() {
        let source = sample();
        source
            .execute("UPDATE ai_settings SET key_ref='existing-vault-ref'", [])
            .unwrap();
        let bytes = raw_backup(&source);
        let mut target = sample();
        assert!(restore_snapshot(&mut target, &bytes, "my-safe-passphrase").is_err());
        assert_eq!(
            target
                .query_row("SELECT key_ref FROM ai_settings", [], |r| r
                    .get::<_, Option<String>>(0))
                .unwrap(),
            None
        );
    }
    #[test]
    fn workspace_wrong_password_creates_no_recovery_files() {
        let source = sample();
        let data = encrypt_snapshot(&source, "my-safe-passphrase").unwrap();
        let mut target = sample();
        set_sample_value(&target, "当前资料");
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        assert!(restore_workspace(&mut target, &data, "wrong-passphrase", &recovery).is_err());
        assert_eq!(sample_value(&target), "当前资料");
        assert!(
            !recovery.exists(),
            "invalid backup must not create a misleading recovery file"
        );
    }
    #[test]
    fn workspace_restore_keeps_usable_safety_snapshot_and_reinitializes_model() {
        let source = sample();
        let data = encrypt_snapshot(&source, "my-safe-passphrase").unwrap();
        let mut target = sample();
        set_sample_value(&target, "恢复前资料");
        target
            .execute("UPDATE ai_settings SET key_ref='local-only-ref'", [])
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = restore_workspace(&mut target, &data, "my-safe-passphrase", dir.path()).unwrap();
        assert_eq!(sample_value(&target), "原始资料");
        assert_eq!(
            target
                .query_row(
                    "SELECT count(*) FROM ai_settings WHERE key_ref IS NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        let safety = std::fs::read(&path).unwrap();
        restore_workspace(&mut target, &safety, "my-safe-passphrase", dir.path()).unwrap();
        assert_eq!(sample_value(&target), "恢复前资料");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    #[test]
    fn workspace_failed_safety_write_or_active_job_preserves_target() {
        let source = sample();
        let data = encrypt_snapshot(&source, "my-safe-passphrase").unwrap();
        let mut target = sample();
        set_sample_value(&target, "当前资料");
        let blocker = tempfile::NamedTempFile::new().unwrap();
        assert!(
            restore_workspace(&mut target, &data, "my-safe-passphrase", blocker.path()).is_err()
        );
        assert_eq!(sample_value(&target), "当前资料");
        target.execute_batch("INSERT INTO ai_jobs(id,brand_id,kind,status,created_at,context_json) VALUES('id','brand','extract','running','2026-10-17T00:00:00Z','{}');").unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(restore_workspace(&mut target, &data, "my-safe-passphrase", dir.path()).is_err());
        assert_eq!(sample_value(&target), "当前资料");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn valid_backup_can_recover_logically_corrupted_live_workspace() {
        let mut target = sample();
        crate::core::handle(&mut target, "brand.create", json!({"name":"完整品牌"})).unwrap();
        let data = encrypt_snapshot(&target, "my-safe-passphrase").unwrap();
        target
            .execute("UPDATE m_brands SET data='broken-json'", [])
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = restore_workspace(&mut target, &data, "my-safe-passphrase", dir.path()).unwrap();
        assert!(path.exists());
        crate::core::validate_database(&target).unwrap();
    }
    #[test]
    fn valid_backup_can_recover_live_orphan_relationships() {
        let mut target = sample();
        let brand =
            crate::core::handle(&mut target, "brand.create", json!({"name":"完整品牌"})).unwrap();
        crate::core::handle(
            &mut target,
            "product.save",
            json!({"brandId":brand["id"],"name":"商品","sku":"RESTORE-1","facts":[]}),
        )
        .unwrap();
        let data = encrypt_snapshot(&target, "my-safe-passphrase").unwrap();
        target.pragma_update(None, "foreign_keys", false).unwrap();
        target.execute("DELETE FROM m_brands", []).unwrap();
        target.pragma_update(None, "foreign_keys", true).unwrap();
        assert!(validate(&target).is_err());
        let dir = tempfile::tempdir().unwrap();
        restore_workspace(&mut target, &data, "my-safe-passphrase", dir.path()).unwrap();
        validate(&target).unwrap();
        crate::core::validate_database(&target).unwrap();
    }
    #[test]
    fn full_workspace_roundtrip_preserves_all_records_after_reopen() {
        use crate::core::handle;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.sqlite3");
        let mut db = crate::runtime::open_database(&path).unwrap();
        let brand = handle(&mut db, "brand.create", json!({"name":"往返验收品牌"})).unwrap();
        let other = handle(&mut db, "brand.create", json!({"name":"第二品牌"})).unwrap();
        let product=handle(&mut db,"product.save",json!({"brandId":brand["id"],"name":"保温杯","sku":"ROUND-001","sourceText":"原始资料留本地","privateNotes":"内部备注","facts":[{"id":"f1","label":"容量","value":"500mL","source":"测试厂家参数表","visibility":"public","validUntil":null}]})).unwrap();
        handle(&mut db,"product.confirm",json!({"brandId":brand["id"],"id":product["id"],"expectedRevision":product["revision"]})).unwrap();
        let question=handle(&mut db,"question.save",json!({"brandId":brand["id"],"productId":product["id"],"text":"这款保温杯容量多少？","intent":"usage","origin":"customer","source":"测试问答","confirmed":true,"branded":false})).unwrap();
        let draft = handle(
            &mut db,
            "draft.generateLocal",
            json!({"brandId":brand["id"],"productId":product["id"],"kind":"description"}),
        )
        .unwrap();
        let draft = handle(
            &mut db,
            "draft.review",
            json!({"brandId":brand["id"],"id":draft["id"],"expectedRevision":draft["revision"]}),
        )
        .unwrap();
        handle(&mut db,"publication.add",json!({"brandId":brand["id"],"draftId":draft["id"],"expectedRevision":draft["revision"],"channel":"测试记录未发布","url":"https://example.com/test","publishedAt":"2026-10-17T00:00:00Z","note":"隔离夹具"})).unwrap();
        handle(&mut db,"observation.add",json!({"brandId":brand["id"],"productId":product["id"],"questionId":question["id"],"platform":"doubao","modelLabel":"模拟","surface":"consumer","searchMode":"模拟无请求","phase":"baseline","answer":"模拟回答，无真实收录","mentioned":false,"accuracy":"unknown","citedUrl":"","evidenceNote":"隔离测试无真实请求","testedAt":"2026-10-17T00:00:00Z"})).unwrap();
        let before = handle(
            &mut db,
            "workspace.snapshot",
            json!({"brandId":brand["id"]}),
        )
        .unwrap();
        let data = encrypt_snapshot(&db, "my-safe-passphrase").unwrap();
        handle(&mut db, "brand.create", json!({"name":"恢复应移除"})).unwrap();
        let safety = restore_workspace(
            &mut db,
            &data,
            "my-safe-passphrase",
            &dir.path().join("recovery"),
        )
        .unwrap();
        assert!(safety.exists());
        drop(db);
        let mut reopened = crate::runtime::open_database(&path).unwrap();
        assert_eq!(
            handle(
                &mut reopened,
                "workspace.snapshot",
                json!({"brandId":brand["id"]})
            )
            .unwrap(),
            before
        );
        let brands = handle(&mut reopened, "brand.list", json!({})).unwrap();
        assert_eq!(brands.as_array().unwrap().len(), 2);
        let isolated = handle(
            &mut reopened,
            "workspace.snapshot",
            json!({"brandId":other["id"]}),
        )
        .unwrap();
        assert_eq!(isolated["products"], json!([]));
        assert_eq!(
            reopened
                .query_row("SELECT count(*) FROM ai_jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn insufficient_destination_capacity_rolls_back_and_survives_reopen() {
        let mut source = sample();
        for i in 0..20 {
            crate::core::handle(&mut source,"product.save",json!({"brandId":SAMPLE_ID,"name":format!("Large fixture {i}"),"sku":format!("LARGE-{i}"),"sourceText":"x".repeat(80000),"facts":[]})).unwrap();
        }
        let data = encrypt_snapshot(&source, "my-safe-passphrase").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("limited.sqlite3");
        let mut target = crate::runtime::open_database(&path).unwrap();
        set_sample_value(&target, "保留当前资料");
        let pages: i64 = target
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        target.pragma_update(None, "max_page_count", pages).unwrap();
        assert!(restore_workspace(
            &mut target,
            &data,
            "my-safe-passphrase",
            &dir.path().join("recovery")
        )
        .is_err());
        assert_eq!(sample_value(&target), "保留当前资料");
        drop(target);
        let reopened = crate::runtime::open_database(&path).unwrap();
        assert_eq!(sample_value(&reopened), "保留当前资料");
    }
}
