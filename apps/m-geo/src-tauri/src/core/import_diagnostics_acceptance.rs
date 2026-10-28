//! Phase9 acceptance through the same command boundary used by the native shell.
use super::*;
use std::io::Read;

fn rows(db: &Connection) -> Vec<(String, Vec<Vec<String>>)> {
    let tables = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut q = db
                .prepare(&format!(
                    "SELECT * FROM \"{}\" ORDER BY rowid",
                    table.replace('"', "\"\"")
                ))
                .unwrap();
            let width = q.column_count();
            let data = q
                .query_map([], |r| {
                    Ok((0..width)
                        .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                        .collect::<Vec<_>>())
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (table, data)
        })
        .collect()
}
fn call(db: &mut Connection, a: &str, p: Value) -> Value {
    crate::runtime::core_dispatch(db, a, p).unwrap()
}
fn job_request(b: &Value, j: &Value) -> Value {
    json!({"brandId":b,"jobId":j["id"],"expectedRevision":j["revision"]})
}
fn copy(db: &Connection, path: &std::path::Path) {
    let mut dest = Connection::open(path).unwrap();
    rusqlite::backup::Backup::new(db, &mut dest)
        .unwrap()
        .run_to_completion(128, std::time::Duration::from_millis(1), None)
        .unwrap();
}

#[test]
fn phase9_fictional_repair_queue_report_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("fictional.sqlite3");
    let mut db = crate::runtime::open_database(&db_path).unwrap();
    let b = call(
        &mut db,
        "brand.create",
        json!({"name":"第九阶段 · 虚构导入修正"}),
    )["id"]
        .clone();
    for (sku, name) in [
        ("P9-A", "虚构水壶"),
        ("P9-B,XL", "虚构背包"),
        ("P9-C\n蓝色", "虚构台灯"),
    ] {
        call(
            &mut db,
            "product.save",
            json!({"brandId":b,"sku":sku,"name":name,"facts":[],"privateNotes":"PRIVATE-P9"}),
        );
    }
    let template = call(&mut db, "attribution.csvTemplate", json!({"brandId":b}));
    let valid = json!({"brandId":b,"name":"虚构修正验收","csv":template["csv"],"delimiter":template["delimiter"],"mapping":template["mapping"],"metadata":template["metadata"]});
    let mut invalid = valid.clone();
    invalid["csv"] = json!(valid["csv"]
        .as_str()
        .unwrap()
        .replacen("100.00", "100.001", 1));
    assert_ne!(invalid["csv"], valid["csv"]);
    let before = rows(&db);
    let structure = call(
        &mut db,
        "attribution.csvInspectDetailed",
        json!({"brandId":b,"csv":invalid["csv"],"delimiter":"comma"}),
    );
    assert_eq!(structure["status"], "ready");
    assert_eq!(structure["rowCount"], 21);
    assert!(structure["prepared"].is_null());
    let bad = call(&mut db, "attribution.csvDiagnose", invalid.clone());
    assert_eq!(bad["status"], "blocked");
    assert!(bad["prepared"].is_null());
    assert!(bad["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["record"] == 11 && i["field"] == "amount"));
    assert!(!bad["issues"].to_string().contains("100.001"));
    let good = call(&mut db, "attribution.csvDiagnose", valid.clone());
    assert_eq!(good["status"], "ready");
    assert_eq!(good["totalIssues"], 0);
    let legacy = call(&mut db, "attribution.csvPrepare", valid.clone());
    assert_eq!(good["prepared"], legacy);
    assert_eq!(
        legacy["preview"]["counts"],
        json!({"visits":9,"orders":9,"refunds":2,"duplicates":1})
    );
    assert_eq!(
        rows(&db),
        before,
        "diagnosis must be read-only including audit"
    );
    let output = std::env::var("MGEO_PHASE9_OUTPUT")
        .ok()
        .map(std::path::PathBuf::from);
    if let Some(out) = &output {
        std::fs::create_dir_all(out).unwrap();
        copy(&db, &out.join("native-seed.sqlite3"));
        for (name, value) in [
            ("valid-request.json", &valid),
            ("invalid-request.json", &invalid),
            ("blocked-diagnostic.json", &bad),
            ("ready-diagnostic.json", &good),
        ] {
            std::fs::write(out.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
        }
        std::fs::write(out.join("invalid.csv"), invalid["csv"].as_str().unwrap()).unwrap();
        std::fs::write(out.join("repaired.csv"), valid["csv"].as_str().unwrap()).unwrap();
    }
    let mut job = call(
        &mut db,
        "localJobs.create",
        json!({"brandId":b,"name":"虚构修正后暂停队列","entries":[{"name":valid["name"],"content":legacy["content"],"expectedHash":legacy["preview"]["contentHash"]}]}),
    );
    assert_eq!(job["status"], "paused");
    assert_eq!(
        job.as_object_mut().unwrap().remove("alreadyExists"),
        Some(json!(false))
    );
    let get = json!({"brandId":b,"jobId":job["id"]});
    drop(db);
    let mut db = crate::runtime::open_database(&db_path).unwrap();
    assert_eq!(call(&mut db, "localJobs.get", get.clone()), job);
    job = call(&mut db, "localJobs.resume", job_request(&b, &job));
    job = call(&mut db, "localJobs.step", job_request(&b, &job));
    assert_eq!(job["status"], "completed");
    assert_eq!(job["entries"][0]["status"], "succeeded");
    let report_request = json!({"brandId":b,"datasetId":job["entries"][0]["datasetId"],"from":"2026-10-01T00:00:00Z","to":"2026-10-23T00:00:00Z","lookbackDays":7});
    let report = call(&mut db, "attribution.report", report_request.clone());
    assert_eq!(report["summary"]["grossMinor"], 81000);
    assert_eq!(report["summary"]["refundMinor"], 22000);
    assert_eq!(report["summary"]["netMinor"], 59000);
    let queue_export = call(&mut db, "localJobs.export", get.clone());
    let queue_zip = crate::files::site_zip(&queue_export).unwrap();
    let report_zip = crate::files::site_zip(&report).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&queue_zip)).unwrap();
    let mut json = String::new();
    archive
        .by_name("job.json")
        .unwrap()
        .read_to_string(&mut json)
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), job);
    assert!(!json.contains("PRIVATE-P9"));
    assert_eq!(
        db.query_row("SELECT count(*) FROM ai_jobs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let bytes = crate::backup::encrypt_snapshot(&db, "fictional-phase9-only").unwrap();
    assert_eq!(
        crate::backup::inspect_snapshot(&bytes, "fictional-phase9-only").unwrap()["targetVersion"],
        10
    );
    let restored_path = temp.path().join("restored.sqlite3");
    let mut restored = crate::runtime::open_database(&restored_path).unwrap();
    crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-phase9-only").unwrap();
    // Backups deliberately remove model configuration, including initialized defaults.
    assert_eq!(
        rows(&restored)
            .into_iter()
            .filter(|(t, _)| t != "ai_settings")
            .collect::<Vec<_>>(),
        rows(&db)
            .into_iter()
            .filter(|(t, _)| t != "ai_settings")
            .collect::<Vec<_>>()
    );
    assert_eq!(
        restored
            .query_row("SELECT count(*) FROM ai_settings", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    if let Some(out) = &output {
        copy(&db, &out.join("fictional.sqlite3"));
        std::fs::write(out.join("queue-evidence.zip"), queue_zip).unwrap();
        std::fs::write(out.join("attribution-report.zip"), report_zip).unwrap();
        std::fs::write(
            out.join("report-request.json"),
            serde_json::to_vec_pretty(&report_request).unwrap(),
        )
        .unwrap();
        std::fs::write(
            out.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        std::fs::write(out.join("fictional-workspace.mgeobak"), bytes).unwrap();
    }
}

#[test]
fn phase9_bad_brand_and_extra_payload_never_write() {
    let mut db = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let b = call(&mut db, "brand.create", json!({"name":"虚构请求边界"}))["id"].clone();
    let before = rows(&db);
    for payload in [
        json!({"brandId":b,"csv":"a\nx","delimiter":"comma","extra":true}),
        json!({"brandId":Uuid::new_v4().to_string(),"csv":"a\nx","delimiter":"comma"}),
    ] {
        assert!(
            crate::runtime::core_dispatch(&mut db, "attribution.csvInspectDetailed", payload)
                .is_err()
        );
    }
    assert_eq!(rows(&db), before);
}

#[test]
fn phase9_actual_phase8_schema5_copy_is_unchanged() {
    let Ok(source) = std::env::var("MGEO_PHASE9_LEGACY") else {
        return;
    };
    let prior =
        Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("legacy.sqlite3");
    copy(&prior, &path);
    let opened = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        opened
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(rows(&prior), rows(&opened));
    let bytes = crate::backup::encrypt_snapshot(&opened, "phase9-legacy-only").unwrap();
    assert_eq!(
        crate::backup::inspect_snapshot(&bytes, "phase9-legacy-only").unwrap()["targetVersion"],
        10
    );
}
