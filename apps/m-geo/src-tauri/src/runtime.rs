//! Native boundary shared by the desktop shell and integration tests.
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

pub const CORE_ACTIONS: &[&str] = &[
    "brand.list",
    "brand.create",
    "brand.profile.save",
    "research.save",
    "research.list",
    "research.archive",
    "research.confirm",
    "research.confirmToQuestion",
    "publication.recordReadback",
    "publication.withdraw",
    "media.import",
    "media.list",
    "media.get",
    "media.review",
    "media.revise",
    "media.setArchived",
    "p0Workflow.create",
    "p0Workflow.resume",
    "p0Workflow.pause",
    "p0Workflow.cancel",
    "p0Workflow.retry",
    "p0Workflow.step",
    "p0Workflow.approvePublish",
    "p0Workflow.reconcile",
    "p0Workflow.rollback",
    "p0Workflow.policyUpdate",
    "p0Workflow.haltAll",
    "p0Workflow.list",
    "p0Workflow.get",
    "p0Workflow.export",
    "p0Workflow.previewMonitor",
    "p0Workflow.auditFixture",
    "p0Workflow.policyGet",
    "offlineWorkflow.create",
    "offlineWorkflow.list",
    "offlineWorkflow.get",
    "offlineWorkflow.resume",
    "offlineWorkflow.pause",
    "offlineWorkflow.cancel",
    "offlineWorkflow.retry",
    "offlineWorkflow.step",
    "offlineWorkflow.export",
    "offlineGeo.audit",
    "automation.create",
    "automation.list",
    "automation.pause",
    "automation.resume",
    "automation.runDue",
    "automation.cancel",
    "workspace.snapshot",
    "product.save",
    "product.confirm",
    "question.save",
    "question.confirm",
    "question.suggest",
    "draft.generateLocal",
    "draft.generateChannels",
    "draft.save",
    "draft.review",
    "draft.export",
    "publication.add",
    "observation.add",
    "observation.assess",
    "campaign.create",
    "attribution.preview",
    "attribution.apply",
    "attribution.list",
    "attribution.report",
    "attribution.template",
    "attribution.csvTemplate",
    "attribution.csvInspect",
    "attribution.csvPrepare",
    "attribution.csvInspectDetailed",
    "attribution.csvDiagnose",
    "optimization.diagnose",
    "optimization.create",
    "optimization.list",
    "optimization.get",
    "optimization.update",
    "optimization.check",
    "optimization.export",
    "localJobs.create",
    "localJobs.list",
    "localJobs.get",
    "localJobs.resume",
    "localJobs.pause",
    "localJobs.step",
    "localJobs.retry",
    "localJobs.cancel",
    "localJobs.export",
    "catalog.inspect",
    "feed.prepare",
    "feed.compare",
    "feed.simulate",
    "feed.checkReceipt",
    "site.prepare",
    "site.audit",
    "pilot.prepare",
    "pilot.createQuestions",
    "collection.prepare",
    "collection.preview",
    "collection.apply",
    "campaign.report",
    "campaign.transitions",
    "campaign.transitionEvidence",
    "campaign.transitionExport",
    "commerce.previewBatch",
    "commerce.applyBatch",
    "import.csv",
    "import.cnvp",
    "knowledge.list",
    "knowledge.recall",
    "knowledge.import",
    "knowledge.revise",
    "knowledge.review",
    "asset.list",
    "asset.create",
    "asset.revise",
    "asset.review",
    "asset.setArchived",
];
pub const AI_ACTIONS: &[&str] = &[
    "ai.settings.get",
    "ai.settings.save",
    "ai.settings.clearKey",
    "ai.test",
    "ai.monitor.prepare",
    "ai.monitor.start",
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
    if !(0..=10).contains(&version) {
        return Err("资料来自更新版本，请升级 M- GEO 后再打开".into());
    }
    if version == 0 {
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "无法检查资料结构")?;
        if count != 0 {
            return Err("资料版本缺失，请使用原版本恢复".into());
        }
    } else {
        crate::backup::validate_schema_version(&db, version)?;
        if version <= 10 {
            crate::core::validate_database(&db)?;
        }
    }
    if [1, 2, 3, 4, 5, 6, 7, 8, 9].contains(&version) && path != Path::new(":memory:") {
        let parent = path.parent().ok_or("数据目录无效")?;
        let mut snapshot =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "无法创建升级前快照")?;
        let mut target = Connection::open(snapshot.path()).map_err(|_| "无法打开升级前快照")?;
        rusqlite::backup::Backup::new(&db, &mut target)
            .map_err(|_| "无法备份升级前资料")?
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .map_err(|_| "升级前备份失败")?;
        drop(target);
        use std::io::Write;
        snapshot.flush().map_err(|_| "无法保存升级前快照")?;
        snapshot
            .as_file()
            .sync_all()
            .map_err(|_| "无法同步升级前快照")?;
        snapshot
            .persist_noclobber(parent.join(format!(
                "{}{}.sqlite3",
                if version == 9 {
                    "before-p0-v10-"
                } else if version == 8 {
                    "before-offline-v9-"
                } else {
                    "before-optimization-v5-"
                },
                uuid::Uuid::new_v4()
            )))
            .map_err(|_| "无法保留升级前快照")?;
    }
    crate::core::initialize(&mut db)?;
    crate::ai::initialize(&mut db)?;
    db.pragma_update(None, "user_version", 10)
        .map_err(|_| "无法保存资料版本".to_string())?;
    crate::core::recover_offline_workflows(&mut db)?;
    crate::core::recover_p0_workflows(&mut db)?;
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
    let p0_running: i64 = db.query_row("SELECT count(*) FROM m_p0_workflows WHERE json_extract(data,'$.status') IN ('running','retry_wait')", [], |r|r.get(0)).map_err(|_| "无法检查接入演练任务".to_string())?;
    if p0_running > 0 {
        return Err("请先暂停或取消全部品牌的接入演练任务，再恢复备份".into());
    }
    let offline_running: i64 = db.query_row("SELECT count(*) FROM m_offline_workflows WHERE json_extract(data,'$.status')='running'", [], |r|r.get(0)).map_err(|_| "无法检查离线任务".to_string())?;
    if offline_running > 0 {
        return Err("请先暂停或取消全部品牌的离线任务，再恢复备份".into());
    }
    if crate::core::has_ready_local_jobs(db)? {
        return Err("请先暂停或取消全部品牌的待执行本地队列，再恢复备份".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn p0_dispatch_audit_backup_and_running_restore_guard() {
        use serde_json::json;
        let mut c = open_database(Path::new(":memory:")).unwrap();
        let b = core_dispatch(&mut c, "brand.create", json!({"name":"虚构入口验收"})).unwrap();
        let p = core_dispatch(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构水杯","sku":"BOUNDARY021","facts":[{"id":"capacity","label":"容量","value":"500mL","source":"虚构规格","visibility":"public"}]})).unwrap();
        let p = core_dispatch(
            &mut c,
            "product.confirm",
            json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
        )
        .unwrap();
        let t = core_dispatch(&mut c,"p0Workflow.create",json!({"brandId":b["id"],"requestId":uuid::Uuid::new_v4().to_string(),"productId":p["id"],"name":"虚构入口","mode":"natural_discovery","question":"通勤如何选择容量？","targetUrl":"https://fixture.invalid/article","scenario":"success","maxUnits":20,"maxCalls":20,"transport":"fixture","evidenceMode":"simulated"})).unwrap();
        let t = core_dispatch(
            &mut c,
            "p0Workflow.resume",
            json!({"brandId":b["id"],"taskId":t["id"],"expectedRevision":t["revision"]}),
        )
        .unwrap();
        assert!(no_active_jobs(&c).is_err());
        let policy =
            core_dispatch(&mut c, "p0Workflow.policyGet", json!({"brandId":b["id"]})).unwrap();
        core_dispatch(
            &mut c,
            "p0Workflow.haltAll",
            json!({"brandId":b["id"],"expectedRevision":policy["revision"]}),
        )
        .unwrap();
        no_active_jobs(&c).unwrap();
        crate::core::validate_database(&c).unwrap();
        let bytes = crate::backup::encrypt_snapshot(&c, "fictional-p0-backup").unwrap();
        let inspection = crate::backup::inspect_snapshot(&bytes, "fictional-p0-backup").unwrap();
        assert_eq!(inspection["sourceVersion"], 10);
        assert_eq!(inspection["targetVersion"], 10);
        assert_eq!(inspection["counts"]["p0Workflows"], 1);
        let mut dest = open_database(Path::new(":memory:")).unwrap();
        crate::backup::restore_snapshot(&mut dest, &bytes, "fictional-p0-backup").unwrap();
        let restored = core_dispatch(
            &mut dest,
            "p0Workflow.get",
            json!({"brandId":b["id"],"taskId":t["id"]}),
        )
        .unwrap();
        assert_eq!(restored["status"], "paused");
        crate::core::validate_database(&dest).unwrap();
    }

    #[test]
    fn schema9_p0_upgrade_preserves_business_and_configuration_with_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v9.sqlite3");
        let mut old = open_database(&path).unwrap();
        let brand = crate::core::handle(
            &mut old,
            "brand.create",
            serde_json::json!({"name":"虚构P0迁移品牌"}),
        )
        .unwrap();
        old.execute("UPDATE ai_settings SET base_url='https://fixture.example.invalid/v1',model='fixture-only',key_ref='fictional-reference-not-a-secret' WHERE id=1", []).unwrap();
        old.execute_batch("DROP TABLE IF EXISTS m_p0_workflows; DROP TABLE IF EXISTS m_p0_policy; UPDATE m_core_meta SET schema_version=9; PRAGMA user_version=9;").unwrap();
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
                .query_row(
                    "SELECT data FROM m_brands WHERE id=?1",
                    [brand["id"].as_str().unwrap()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            brand.to_string()
        );
        let safe: (String, String, bool) = upgraded
            .query_row(
                "SELECT base_url,model,key_ref IS NOT NULL FROM ai_settings WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            safe,
            (
                "https://fixture.example.invalid/v1".into(),
                "fixture-only".into(),
                true
            )
        );
        let snapshots: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| {
                let p = e.ok()?.path();
                p.file_name()?
                    .to_str()?
                    .starts_with("before-p0-v10-")
                    .then_some(p)
            })
            .collect();
        assert_eq!(snapshots.len(), 1);
        let snapshot =
            Connection::open_with_flags(&snapshots[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert_eq!(
            snapshot
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
        crate::backup::validate_schema_version(&snapshot, 9).unwrap();
        assert_eq!(
            snapshot
                .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
    #[test]
    fn phase6_schema3_file_upgrade_preserves_rows_and_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v3.sqlite3");
        let mut old = open_database(&path).unwrap();
        crate::core::handle(
            &mut old,
            "brand.create",
            serde_json::json!({"name":"虚构旧版品牌"}),
        )
        .unwrap();
        let before: String = old
            .query_row("SELECT data FROM m_brands", [], |r| r.get(0))
            .unwrap();
        old.execute_batch("DROP TABLE IF EXISTS m_p0_workflows; DROP TABLE IF EXISTS m_p0_policy; DROP TABLE IF EXISTS m_offline_workflows; DROP TABLE IF EXISTS m_question_research; DROP TABLE IF EXISTS m_media; DROP TABLE IF EXISTS m_automation_tasks; DROP TABLE IF EXISTS m_assets; DROP TABLE IF EXISTS m_knowledge_reviews; DROP TABLE IF EXISTS m_knowledge_documents; DROP TABLE IF EXISTS m_optimization_plans; DROP TABLE IF EXISTS m_local_jobs; UPDATE m_core_meta SET schema_version=3; PRAGMA user_version=3;").unwrap();
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
                .query_row("SELECT count(*) FROM m_local_jobs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let snapshots: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
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
            3
        );
        assert_eq!(
            prior
                .query_row("SELECT data FROM m_brands", [], |r| r.get::<_, String>(0))
                .unwrap(),
            before
        );
    }

    #[test]
    fn schema7_file_upgrade_to_current_preserves_legacy_rows_and_preupgrade_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema7.sqlite3");
        let mut old = open_database(&path).unwrap();
        let brand = crate::core::handle(
            &mut old,
            "brand.create",
            serde_json::json!({"name":"虚构 schema7 品牌"}),
        )
        .unwrap();
        let product = crate::core::handle(
            &mut old,
            "product.save",
            serde_json::json!({"brandId":brand["id"],"name":"虚构旧商品","sku":"LEGACY-7","facts":[]}),
        )
        .unwrap();
        let before_brand: String = old
            .query_row("SELECT data FROM m_brands", [], |row| row.get(0))
            .unwrap();
        let before_product: String = old
            .query_row("SELECT data FROM m_products", [], |row| row.get(0))
            .unwrap();
        old.execute_batch("DROP TABLE m_p0_workflows; DROP TABLE m_p0_policy; DROP TABLE m_offline_workflows; DROP TABLE m_question_research; DROP TABLE m_media; DROP TABLE m_automation_tasks; UPDATE m_core_meta SET schema_version=7; PRAGMA user_version=7;")
            .unwrap();
        drop(old);

        let upgraded = open_database(&path).unwrap();
        assert_eq!(
            upgraded
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(
            upgraded
                .query_row("SELECT schema_version FROM m_core_meta", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(
            upgraded
                .query_row("SELECT data FROM m_brands", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            before_brand
        );
        assert_eq!(
            upgraded
                .query_row("SELECT data FROM m_products", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            before_product.clone()
        );
        for table in ["m_question_research", "m_media", "m_automation_tasks"] {
            assert_eq!(
                upgraded
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        crate::core::validate_database(&upgraded).unwrap();

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
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(
            prior
                .query_row("SELECT data FROM m_products", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            before_product
        );
        assert!(prior
            .query_row(
                "SELECT 1 FROM m_products WHERE id=?1",
                [product["id"].as_str()],
                |row| row.get::<_, i64>(0)
            )
            .is_ok());
        for table in ["m_question_research", "m_media", "m_automation_tasks"] {
            assert_eq!(
                prior
                    .query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        [table],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn attribution_legacy_upgrade_keeps_snapshot_and_rejects_mismatched_versions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.sqlite3");
        let mut old = Connection::open(&path).unwrap();
        crate::core::initialize(&mut old).unwrap();
        crate::ai::initialize(&mut old).unwrap();
        old.execute_batch("DROP TABLE IF EXISTS m_p0_workflows; DROP TABLE IF EXISTS m_p0_policy; DROP TABLE IF EXISTS m_offline_workflows; DROP TABLE IF EXISTS m_question_research; DROP TABLE IF EXISTS m_media; DROP TABLE IF EXISTS m_automation_tasks; DROP TABLE IF EXISTS m_assets; DROP TABLE IF EXISTS m_knowledge_reviews; DROP TABLE IF EXISTS m_knowledge_documents; DROP TABLE m_optimization_plans; DROP TABLE m_local_jobs; DROP TABLE m_attribution_datasets; UPDATE m_core_meta SET schema_version=2; PRAGMA user_version=2;").unwrap();
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
                .query_row("SELECT schema_version FROM m_core_meta", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("before-optimization-v5-"))
                .count(),
            1
        );
        drop(upgraded);
        let broken = Connection::open(&path).unwrap();
        broken
            .execute_batch("UPDATE m_core_meta SET schema_version=2;")
            .unwrap();
        drop(broken);
        assert!(open_database(&path).is_err());
        let still = Connection::open(&path).unwrap();
        assert_eq!(
            still
                .query_row("SELECT schema_version FROM m_core_meta", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn attribution_upgrade_rejects_untrusted_legacy_schema_without_mutation() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut db = Connection::open(file.path()).unwrap();
        crate::core::initialize(&mut db).unwrap();
        crate::ai::initialize(&mut db).unwrap();
        db.execute_batch("DROP TABLE IF EXISTS m_p0_workflows; DROP TABLE IF EXISTS m_p0_policy; DROP TABLE IF EXISTS m_offline_workflows; DROP TABLE IF EXISTS m_question_research; DROP TABLE IF EXISTS m_media; DROP TABLE IF EXISTS m_automation_tasks; DROP TABLE IF EXISTS m_assets; DROP TABLE IF EXISTS m_knowledge_reviews; DROP TABLE IF EXISTS m_knowledge_documents; DROP TABLE m_optimization_plans; DROP TABLE m_local_jobs; DROP TABLE m_attribution_datasets; UPDATE m_core_meta SET schema_version=2; PRAGMA user_version=2; CREATE TABLE unexpected(value TEXT);").unwrap();
        drop(db);
        assert!(open_database(file.path()).is_err());
        let db = Connection::open(file.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='m_attribution_datasets'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn boundary_rejects_internal_commands_and_unknown_actions() {
        let mut db = open_database(Path::new(":memory:")).unwrap();
        for action in [
            "internal.ai.prepare",
            "internal.ai.commit",
            "execute_sql",
            "file.read",
            "file.inspectChannelReport",
            "file.readChannelInspectionPage",
            "file.exportChannelInspection",
            "channel.inspect",
            "channel.inspectionExport",
            "inspect_channel_report",
            "channel_inspection_export",
            "file.inspectAttributionReport",
            "file.readAttributionInspectionPage",
            "file.exportAttributionInspection",
            "attribution.inspect",
            "attribution.inspectionExport",
            "inspect_attribution_report",
            "attribution_inspection_export",
            "file.inspectExperimentReport",
            "file.readExperimentInspectionPage",
            "file.exportExperimentInspection",
            "experiment.inspect",
            "experiment.inspectionExport",
            "inspect_experiment_report",
            "experiment_inspection_export",
        ] {
            assert!(!CORE_ACTIONS.contains(&action));
            assert!(!AI_ACTIONS.contains(&action));
            assert!(core_dispatch(&mut db, action, serde_json::json!({})).is_err());
        }
        assert_eq!(
            core_dispatch(&mut db, "brand.list", serde_json::json!({})).unwrap(),
            serde_json::json!([])
        );
    }
    #[test]
    fn transition_actions_are_registered_and_native_export_is_not_core() {
        let mut db = open_database(Path::new(":memory:")).unwrap();
        let changes = db.total_changes();
        for action in [
            "campaign.transitions",
            "campaign.transitionEvidence",
            "campaign.transitionExport",
        ] {
            assert_eq!(CORE_ACTIONS.iter().filter(|a| **a == action).count(), 1);
            assert!(!AI_ACTIONS.contains(&action));
        }
        for action in ["file.exportTransitions", "file.read", "execute_sql"] {
            assert!(!CORE_ACTIONS.contains(&action));
            assert!(!AI_ACTIONS.contains(&action));
            assert_eq!(
                core_dispatch(
                    &mut db,
                    action,
                    serde_json::json!({"path":"/tmp/unrestricted-transition.zip"}),
                ),
                Err("不支持的操作".into())
            );
        }
        assert_eq!(db.total_changes(), changes);
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            10
        );
    }

    #[test]
    fn batch_update_workspace_remains_backup_valid() {
        use serde_json::json;
        let mut db = open_database(Path::new(":memory:")).unwrap();
        let b = core_dispatch(&mut db, "brand.create", json!({"name":"虚构批量备份回归"})).unwrap()
            ["id"]
            .clone();
        core_dispatch(
            &mut db,
            "product.save",
            json!({"brandId":b,"name":"虚构商品","sku":"TEST-BATCH","facts":[]}),
        )
        .unwrap();
        let content = "SKU,资料版本,价格,币种,库存状态,购买链接,图片链接,依据,有效期\nTEST-BATCH,1,12,CNY,,,,虚构依据,2099-12-31";
        let p = core_dispatch(
            &mut db,
            "commerce.previewBatch",
            json!({"brandId":b,"content":content}),
        )
        .unwrap();
        core_dispatch(
            &mut db,
            "commerce.applyBatch",
            json!({"brandId":b,"content":content,"token":p["token"],"confirm":true}),
        )
        .unwrap();
        crate::core::validate_database(&db).unwrap();
    }
    #[test]
    fn future_database_refuses_initialization() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Connection::open(file.path()).unwrap();
        db.pragma_update(None, "user_version", 11).unwrap();
        drop(db);
        assert!(open_database(file.path()).is_err());
        let db = Connection::open(file.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            11
        );
    }
    #[test]
    fn historical_catalog_facts_restore_without_becoming_current_eligible() {
        use serde_json::json;
        let mut db = open_database(Path::new(":memory:")).unwrap();
        let b = core_dispatch(
            &mut db,
            "brand.create",
            json!({"name":"Historical fictional catalog"}),
        )
        .unwrap()["id"]
            .clone();
        core_dispatch(&mut db,"product.save",json!({"brandId":b,"name":"Fictional legacy item","sku":"HISTORICAL","facts":[{"id":"catalog:kind","label":"Historical kind","value":"legacy_unknown","source":"Historical fixture","visibility":"public","validUntil":"2020-01-01"}]})).unwrap();
        let snapshot = core_dispatch(&mut db, "workspace.snapshot", json!({"brandId":b})).unwrap();
        let bytes = crate::backup::encrypt_snapshot(&db, "fictional-catalog-test-only").unwrap();
        let mut restored = open_database(Path::new(":memory:")).unwrap();
        crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-catalog-test-only")
            .unwrap();
        assert_eq!(
            core_dispatch(&mut restored, "workspace.snapshot", json!({"brandId":b})).unwrap(),
            snapshot
        );
        assert_eq!(
            core_dispatch(&mut restored, "catalog.inspect", json!({"brandId":b})).unwrap()
                ["summary"]["blocked"],
            1
        );
    }
}
