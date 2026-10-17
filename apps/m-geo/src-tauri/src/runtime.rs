//! Native boundary shared by the desktop shell and integration tests.
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

pub const CORE_ACTIONS: &[&str] = &[
    "brand.list",
    "brand.create",
    "workspace.snapshot",
    "product.save",
    "product.confirm",
    "question.save",
    "question.confirm",
    "question.suggest",
    "draft.generateLocal",
    "draft.save",
    "draft.review",
    "draft.export",
    "publication.add",
    "observation.add",
    "import.csv",
    "import.cnvp",
];
pub const AI_ACTIONS: &[&str] = &[
    "ai.settings.get",
    "ai.settings.save",
    "ai.settings.clearKey",
    "ai.test",
    "ai.start",
    "ai.extract",
    "ai.jobs",
    "ai.cancel",
];
pub fn open_database(path: &Path) -> Result<Connection, String> {
    let mut db = Connection::open(path).map_err(|_| "无法打开本地资料库".to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "无法设置资料库".to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| "无法设置资料库".to_string())?;
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| "无法读取资料版本".to_string())?;
    if version > 1 {
        return Err("资料来自更新版本，请升级 M- GEO 后再打开".into());
    }
    crate::core::initialize(&mut db)?;
    crate::ai::initialize(&mut db)?;
    db.pragma_update(None, "user_version", 1)
        .map_err(|_| "无法保存资料版本".to_string())?;
    Ok(db)
}
pub fn core_dispatch(db: &mut Connection, action: &str, payload: Value) -> Result<Value, String> {
    if !CORE_ACTIONS.contains(&action) {
        return Err("不支持的操作".into());
    }
    crate::core::handle(db, action, payload)
}
pub fn no_active_jobs(db: &Connection) -> Result<(), String> {
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM ai_jobs WHERE status IN ('queued','running')",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "无法检查生成任务".to_string())?;
    if count > 0 {
        return Err("请先取消或等待全部品牌的 AI 任务结束，再恢复备份".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_rejects_internal_commands_and_unknown_actions() {
        let mut db = open_database(Path::new(":memory:")).unwrap();
        for action in [
            "internal.ai.prepare",
            "internal.ai.commit",
            "execute_sql",
            "file.read",
        ] {
            assert!(core_dispatch(&mut db, action, serde_json::json!({})).is_err());
        }
        assert_eq!(
            core_dispatch(&mut db, "brand.list", serde_json::json!({})).unwrap(),
            serde_json::json!([])
        );
    }
    #[test]
    fn future_database_refuses_initialization() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Connection::open(file.path()).unwrap();
        db.pragma_update(None, "user_version", 9).unwrap();
        drop(db);
        assert!(open_database(file.path()).is_err());
        let db = Connection::open(file.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
    }
}
