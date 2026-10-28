//! One bounded, brand-scoped, in-memory inspection; never persisted.
use crate::core;
use rusqlite::Connection;
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, String>;

struct StoredInspection {
    id: String,
    brand_id: String,
    package: Value,
    report: Value,
}
#[derive(Default)]
pub struct SiteInspectionSession {
    current: Option<StoredInspection>,
}
fn fingerprint(report: &Value) -> Result<&str> {
    report["reportFingerprint"]
        .as_str()
        .ok_or_else(|| "验收报告无效，请重新选择网站 ZIP".into())
}
impl SiteInspectionSession {
    fn selected(&self, brand: &str, ticket: &str) -> Result<&StoredInspection> {
        self.current
            .as_ref()
            .filter(|s| s.brand_id == brand && s.id == ticket)
            .ok_or_else(|| "验收会话已失效，请重新选择网站 ZIP".into())
    }
    pub fn inspect(&mut self, db: &Connection, brand: &str, package: Value) -> Result<Value> {
        let report = core::inspect_site_package(db, brand, &package)?;
        let id = uuid::Uuid::new_v4().to_string();
        let envelope = json!({"inspectionId":id,"report":report});
        self.current = Some(StoredInspection {
            id,
            brand_id: brand.to_owned(),
            package,
            report,
        });
        Ok(envelope)
    }
    pub fn recheck(&mut self, db: &Connection, brand: &str, ticket: &str) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        let report = core::inspect_site_package(db, brand, &selected.package)?;
        let envelope = json!({"inspectionId":ticket,"report":report});
        if let Some(selected) = self.current.as_mut() {
            selected.report = report;
        }
        Ok(envelope)
    }
    pub fn prepare_export(
        &self,
        db: &Connection,
        brand: &str,
        ticket: &str,
        expected: &str,
    ) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        if fingerprint(&selected.report)? != expected {
            return Err("报告已变化，请重新核对当前品牌后导出".into());
        }
        let current = core::inspect_site_package(db, brand, &selected.package)?;
        if fingerprint(&current)? != expected {
            return Err("当前资料已变化，请重新核对当前品牌后导出".into());
        }
        core::export_site_inspection(&selected.report)
    }
}
/// These endpoints take identifiers only, never a path or caller-authored report.
pub fn validate_payload(action: &str, payload: &Value) -> Result<()> {
    let keys: &[&str] = match action {
        "file.inspectSitePackage" => &["brandId"],
        "file.recheckSitePackage" => &["brandId", "inspectionId"],
        "file.exportSiteInspection" => &["brandId", "inspectionId", "reportFingerprint"],
        _ => return Err("不支持的验收操作".into()),
    };
    let error = || "验收操作参数无效".to_string();
    let object = payload.as_object().ok_or_else(error)?;
    if object.len() != keys.len() || object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(error());
    }
    for key in keys {
        let value = object[*key].as_str().ok_or_else(error)?;
        let valid = if *key == "reportFingerprint" {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        } else {
            value.len() == 36 && uuid::Uuid::parse_str(value).is_ok()
        };
        if !valid {
            return Err(error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{files, runtime, site_package};
    fn setup() -> (Connection, String, Value, Value) {
        let mut db = runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let b = core::handle(&mut db, "brand.create", json!({"name":"虚构交付验收"})).unwrap()
            ["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut refs = vec![];
        let mut first = Value::Null;
        for (i, name) in ["虚构轻量杯", "虚构通勤包", "虚构阅读灯"]
            .iter()
            .enumerate()
        {
            let p=core::handle(&mut db,"product.save",json!({"brandId":b,"name":name,"sku":format!("FICTION-P8-{}",i+1),"privateNotes":"PRIVATE-P8","sourceText":"SOURCE-P8","facts":[{"id":"material","label":"材质","value":"虚构材质","source":"虚构测试资料","visibility":"public","validUntil":null}]})).unwrap();
            core::handle(
                &mut db,
                "product.confirm",
                json!({"brandId":b,"id":p["id"],"expectedRevision":p["revision"]}),
            )
            .unwrap();
            let d = core::handle(
                &mut db,
                "draft.generateLocal",
                json!({"brandId":b,"productId":p["id"],"kind":"description"}),
            )
            .unwrap();
            let d = core::handle(
                &mut db,
                "draft.review",
                json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
            )
            .unwrap();
            refs.push(json!({"id":d["id"],"expectedRevision":d["revision"]}));
            if i == 0 {
                first = p;
            }
        }
        let site = core::handle(
            &mut db,
            "site.prepare",
            json!({"brandId":b,"baseUrl":"","drafts":refs}),
        )
        .unwrap();
        (db, b, site, first)
    }
    fn decoded(site: &Value) -> Value {
        site_package::decode_site_package("fictional.zip", &files::site_zip(site).unwrap()).unwrap()
    }
    #[test]
    fn payload_rejects_fabricated_report_path_extra_keys_and_wrong_ticket() {
        let b = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        for (action, p) in [
            ("file.inspectSitePackage", json!({"brandId":b})),
            (
                "file.recheckSitePackage",
                json!({"brandId":b,"inspectionId":id}),
            ),
            (
                "file.exportSiteInspection",
                json!({"brandId":b,"inspectionId":id,"reportFingerprint":"a".repeat(64)}),
            ),
        ] {
            validate_payload(action, &p).unwrap();
            let mut forged = p.clone();
            forged["path"] = json!("/private/data");
            assert!(validate_payload(action, &forged).is_err());
        }
        for p in [
            json!({"brandId":""}),
            json!({"brandId":b,"report":{}}),
            json!({"brandId":b,"unused":"x".repeat(3000)}),
            json!({"brandId":b,"inspectionId":"wrong"}),
        ] {
            assert!(validate_payload("file.inspectSitePackage", &p).is_err());
        }
        assert!(validate_payload("unknown", &json!({"brandId":b})).is_err());
    }
    #[test]
    fn actual_zip_inspect_export_readback_is_readonly_and_private() {
        let (db, b, site, _) = setup();
        let before = db.total_changes();
        let mut session = SiteInspectionSession::default();
        let envelope = session.inspect(&db, &b, decoded(&site)).unwrap();
        let report = &envelope["report"];
        assert_eq!(report["packageStatus"], "pass");
        assert_eq!(report["currentStatus"], "match");
        assert_eq!(report["products"].as_array().unwrap().len(), 3);
        let output = session
            .prepare_export(
                &db,
                &b,
                envelope["inspectionId"].as_str().unwrap(),
                report["reportFingerprint"].as_str().unwrap(),
            )
            .unwrap();
        let zip = files::site_zip(&output).unwrap();
        let readback = site_package::decode_site_package("report.zip", &zip).unwrap();
        assert_eq!(readback["files"].as_array().unwrap().len(), 4);
        let jsonfile = readback["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "inspection.json")
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(jsonfile["content"].as_str().unwrap()).unwrap(),
            *report
        );
        for secret in ["PRIVATE-P8", "SOURCE-P8"] {
            assert!(!output.to_string().contains(secret));
        }
        assert_eq!(db.total_changes(), before);
        if let Ok(out) = std::env::var("MGEO_PHASE8_OUTPUT") {
            assert!(out.contains("mgeo-e2e-phase8-"));
            let out = std::path::Path::new(&out);
            std::fs::create_dir_all(out).unwrap();
            assert!(!out.join("fictional.sqlite3").exists());
            let mut copy = Connection::open(out.join("fictional.sqlite3")).unwrap();
            rusqlite::backup::Backup::new(&db, &mut copy)
                .unwrap()
                .run_to_completion(128, std::time::Duration::from_millis(1), None)
                .unwrap();
            let mut damaged = site.clone();
            damaged["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"] == "index.html")
                .unwrap()["content"] = json!("tampered local fixture");
            for (name, bytes) in [
                ("fictional-site.zip", files::site_zip(&site).unwrap()),
                ("tampered-site.zip", files::site_zip(&damaged).unwrap()),
                ("inspection-report.zip", zip),
                (
                    "inspection.json",
                    serde_json::to_vec_pretty(report).unwrap(),
                ),
                (
                    "fictional-workspace.mgeobak",
                    crate::backup::encrypt_snapshot(&db, "fictional-phase8-only").unwrap(),
                ),
            ] {
                std::fs::write(out.join(name), bytes).unwrap();
            }
        }
    }
    #[test]
    fn stale_workspace_refuses_export_until_explicit_recheck() {
        let (mut db, b, site, mut product) = setup();
        let mut s = SiteInspectionSession::default();
        let old = s.inspect(&db, &b, decoded(&site)).unwrap();
        let ticket = old["inspectionId"].as_str().unwrap();
        let fp = old["report"]["reportFingerprint"].as_str().unwrap();
        product["expectedRevision"] = product["revision"].clone();
        product["name"] = json!("虚构新名称");
        let updated = core::handle(&mut db, "product.save", product).unwrap();
        assert!(s
            .prepare_export(&db, &b, ticket, fp)
            .unwrap_err()
            .contains("重新核对"));
        let current = s.recheck(&db, &b, ticket).unwrap();
        assert_eq!(current["inspectionId"], old["inspectionId"]);
        assert_eq!(current["report"]["packageStatus"], "pass");
        assert_eq!(current["report"]["currentStatus"], "changed");
        assert!(s.prepare_export(&db, &b, ticket, fp).is_err());
        s.prepare_export(
            &db,
            &b,
            ticket,
            current["report"]["reportFingerprint"].as_str().unwrap(),
        )
        .unwrap();
        core::handle(
            &mut db,
            "product.confirm",
            json!({"brandId":b,"id":updated["id"],"expectedRevision":updated["revision"]}),
        )
        .unwrap();
        let draft = core::handle(
            &mut db,
            "draft.generateLocal",
            json!({"brandId":b,"productId":updated["id"],"kind":"description"}),
        )
        .unwrap();
        let draft = core::handle(
            &mut db,
            "draft.review",
            json!({"brandId":b,"id":draft["id"],"expectedRevision":draft["revision"]}),
        )
        .unwrap();
        let manifest: Value = serde_json::from_str(
            site["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["path"] == "release-manifest.json")
                .unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let refs: Vec<Value> = manifest["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| {
                if r["productId"] == updated["id"] {
                    vec![json!({"id":draft["id"],"expectedRevision":draft["revision"]})]
                } else {
                    r["drafts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|d| json!({"id":d["id"],"expectedRevision":d["revision"]}))
                        .collect()
                }
            })
            .collect();
        let fresh = core::handle(
            &mut db,
            "site.prepare",
            json!({"brandId":b,"baseUrl":"https://example.invalid/shop/","drafts":refs}),
        )
        .unwrap();
        let new = s.inspect(&db, &b, decoded(&fresh)).unwrap();
        assert_eq!(new["report"]["currentStatus"], "match");
        assert_eq!(new["report"]["packageStatus"], "pass");
        assert_eq!(new["report"]["products"].as_array().unwrap().len(), 3);
        assert_eq!(new["report"]["remoteStatus"], "not-verified");
    }
    #[test]
    fn ticket_brand_eviction_and_restart_never_recover_another_session() {
        let (db, b, site, _) = setup();
        let mut s = SiteInspectionSession::default();
        let old = s.inspect(&db, &b, decoded(&site)).unwrap();
        let ticket = old["inspectionId"].as_str().unwrap();
        let fp = old["report"]["reportFingerprint"].as_str().unwrap();
        let other = uuid::Uuid::new_v4().to_string();
        assert!(s.recheck(&db, &other, ticket).is_err());
        assert!(s.prepare_export(&db, &other, ticket, fp).is_err());
        assert!(s.recheck(&db, &b, &other).is_err());
        let fresh = s.inspect(&db, &b, decoded(&site)).unwrap();
        assert_ne!(fresh["inspectionId"], old["inspectionId"]);
        assert!(s.recheck(&db, &b, ticket).is_err());
        assert!(SiteInspectionSession::default()
            .recheck(&db, &b, fresh["inspectionId"].as_str().unwrap())
            .unwrap_err()
            .contains("重新选择"));
    }
    #[test]
    fn reopened_schema5_and_backup_keep_inspection_ephemeral() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.sqlite3");
        let db = runtime::open_database(&path).unwrap();
        let tables: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(db);
        let db = runtime::open_database(&path).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(
            tables,
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
        );
        let bytes = crate::backup::encrypt_snapshot(&db, "fictional-phase8-only").unwrap();
        assert_eq!(
            crate::backup::inspect_snapshot(&bytes, "fictional-phase8-only").unwrap()
                ["targetVersion"],
            10
        );
    }
}

#[cfg(test)]
mod prior_acceptance {
    use super::*;
    #[test]
    fn actual_phase7_copy_opens_with_identical_rows() {
        let Ok(source) = std::env::var("MGEO_PHASE8_LEGACY") else {
            return;
        };
        let prior = Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
        assert_eq!(
            prior
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("phase7-copy.sqlite3");
        let mut copy = Connection::open(&path).unwrap();
        rusqlite::backup::Backup::new(&prior, &mut copy)
            .unwrap()
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .unwrap();
        drop(copy);
        let opened = crate::runtime::open_database(&path).unwrap();
        let tables = prior
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for table in tables {
            let rows = |db: &Connection| {
                let mut st = db
                    .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
                    .unwrap();
                let width = st.column_count();
                let values = st
                    .query_map([], |row| {
                        Ok((0..width)
                            .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                            .collect::<Vec<_>>())
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                values
            };
            assert_eq!(rows(&prior), rows(&opened), "{table}");
        }
        let backup = crate::backup::encrypt_snapshot(&opened, "fictional-phase8-legacy").unwrap();
        assert_eq!(
            crate::backup::inspect_snapshot(&backup, "fictional-phase8-legacy").unwrap()
                ["targetVersion"],
            10
        );
    }
}
