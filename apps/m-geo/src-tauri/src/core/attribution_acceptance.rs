//! End-to-end synthetic acceptance with independently stated expected arithmetic.
use super::*;
#[test]
fn fictional_attribution_import_report_export_restore_reopen() {
    let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let b = handle(
        &mut c,
        "brand.create",
        json!({"name":"第五阶段 · 虚构商品演练"}),
    )
    .unwrap()["id"]
        .clone();
    for (sku, name) in [
        ("SIM-A", "虚构轻量电水壶"),
        ("SIM-B", "虚构通勤背包"),
        ("SIM-C", "虚构阅读台灯"),
    ] {
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"sku":sku,"name":name,"facts":[]}),
        )
        .unwrap();
    }
    let template = handle(&mut c, "attribution.template", json!({"brandId":b})).unwrap();
    let content = template["exchange"].to_string();
    let payload = json!({"brandId":b,"name":"模拟访问与订单验收","content":content});
    let preview = handle(&mut c, "attribution.preview", payload.clone()).unwrap();
    assert_eq!(
        preview["counts"],
        json!({"visits":9,"orders":9,"refunds":2,"duplicates":1})
    );
    let mut apply = payload.clone();
    apply["expectedHash"] = preview["contentHash"].clone();
    let saved = handle(&mut c, "attribution.apply", apply.clone()).unwrap();
    assert_eq!(saved["alreadyImported"], false);
    let replay = handle(&mut c, "attribution.apply", apply).unwrap();
    assert_eq!(replay["datasetId"], saved["datasetId"]);
    assert_eq!(replay["alreadyImported"], true);
    assert_eq!(
        handle(&mut c, "attribution.list", json!({"brandId":b})).unwrap()["datasets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let params = json!({"brandId":b,"datasetId":saved["datasetId"],"from":"2026-10-01T00:00:00Z","to":"2026-10-23T00:00:00Z","lookbackDays":7});
    let before = c.total_changes();
    let report = handle(&mut c, "attribution.report", params.clone()).unwrap();
    assert_eq!(c.total_changes(), before);
    let s = &report["summary"];
    for (key, n) in [
        ("visits", 9),
        ("visitingSessions", 7),
        ("paidOrders", 8),
        ("cancelledOrders", 1),
        ("attributedOrders", 3),
        ("unknownOrders", 5),
        ("convertedSessions", 5),
        ("grossMinor", 81000),
        ("refundMinor", 22000),
        ("netMinor", 59000),
        ("attributedNetMinor", 20000),
        ("unknownNetMinor", 39000),
    ] {
        assert_eq!(s[key], n, "{key}");
    }
    assert_eq!(s["sessionConversionRate"].as_f64().unwrap(), 5.0 / 7.0);
    assert_eq!(s["unknownOrderRate"], 0.625);
    for group in ["bySource", "byProduct", "weekly"] {
        for key in [
            "visits",
            "paidOrders",
            "cancelledOrders",
            "attributedOrders",
            "unknownOrders",
            "grossMinor",
            "refundMinor",
            "netMinor",
            "attributedNetMinor",
            "unknownNetMinor",
        ] {
            assert_eq!(
                report[group]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r[key].as_i64().unwrap())
                    .sum::<i64>(),
                s[key].as_i64().unwrap(),
                "{group}/{key}"
            );
        }
    }
    let first_week = report["weekly"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["weekStart"] == "2026-10-05")
        .unwrap();
    assert_eq!(first_week["convertedSessions"], 1);
    assert_eq!(first_week["paidOrders"], 0);
    for (id, reason) in [
        ("SIM-O3", "unknown_source"),
        ("SIM-O4", "unknown_source"),
        ("SIM-O6", "ambiguous_touch"),
        ("SIM-O7", "missing_session"),
        ("SIM-O8", "no_matching_visit"),
        ("SIM-O9", "cancelled"),
    ] {
        assert_eq!(
            report["orders"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["orderId"] == id)
                .unwrap()["reason"],
            reason
        );
    }
    let mut short = params.clone();
    short["from"] = json!("2026-10-12T00:00:00Z");
    short["to"] = json!("2026-10-13T00:00:00Z");
    let short = handle(&mut c, "attribution.report", short).unwrap();
    assert_eq!(short["summary"]["grossMinor"], 30000);
    assert_eq!(short["summary"]["refundMinor"], 22000);
    assert_eq!(short["summary"]["netMinor"], 8000);
    assert_eq!(short["summary"]["visitingSessions"], 1);
    assert_eq!(short["summary"]["convertedSessions"], 1);
    let zip = crate::files::site_zip(&report).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&zip)).unwrap();
    assert_eq!(archive.len(), 9);
    let mut text = String::new();
    use std::io::Read;
    archive
        .by_name("summary.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let exported: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(exported["summary"], report["summary"]);
    assert!(exported.get("files").is_none());
    text.clear();
    archive
        .by_name("input.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let reimport = handle(
        &mut c,
        "attribution.preview",
        json!({"brandId":b,"name":"导出回读","content":text}),
    )
    .unwrap();
    assert_eq!(reimport["contentHash"], preview["contentHash"]);
    assert_eq!(reimport["alreadyImported"], true);
    validate_database(&c).unwrap();
    let password = "fictional-phase5-test-only";
    let backup = crate::backup::encrypt_snapshot(&c, password).unwrap();
    let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut restored, &backup, password).unwrap();
    assert_eq!(
        handle(&mut restored, "attribution.report", params.clone()).unwrap(),
        report
    );
    let snapshot = handle(&mut c, "workspace.snapshot", json!({"brandId":b})).unwrap();
    assert_eq!(snapshot["publications"], json!([]));
    if let Ok(path) = std::env::var("MGEO_PHASE5_OUTPUT") {
        assert!(path.contains("mgeo-e2e-phase5-"));
        let dir = std::path::Path::new(&path);
        std::fs::create_dir_all(dir).unwrap();
        assert!(
            !dir.join("fictional.sqlite3").exists(),
            "do not overwrite acceptance database"
        );
        let mut dest = Connection::open(dir.join("fictional.sqlite3")).unwrap();
        rusqlite::backup::Backup::new(&c, &mut dest)
            .unwrap()
            .run_to_completion(128, std::time::Duration::from_millis(1), None)
            .unwrap();
        drop(dest);
        let mut reopened = crate::runtime::open_database(&dir.join("fictional.sqlite3")).unwrap();
        assert_eq!(
            handle(&mut reopened, "attribution.report", params).unwrap(),
            report
        );
        std::fs::write(
            dir.join("simulated-input.json"),
            serde_json::to_string_pretty(&template["exchange"]).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("simulated-report.zip"), zip).unwrap();
        std::fs::write(
            dir.join("simulated-template.zip"),
            crate::files::site_zip(&template).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("simulated-workspace.mgeobak"), backup).unwrap();
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_string_pretty(&exported).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("fixture-result.json"),serde_json::to_string_pretty(&json!({"passed":true,"brandId":b,"datasetId":saved["datasetId"],"summary":s,"products":3,"duplicateCollapsed":1,"replayedSameDataset":true,"backupRestored":true,"reopened":true,"publications":0,"modelCalls":0,"externalCalls":0,"evidenceMode":"simulated"})).unwrap()).unwrap();
    }
}
#[test]
fn attribution_schema2_backup_migrates_and_inconsistent_marker_rejects() {
    let mut old = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    let b =
        handle(&mut old, "brand.create", json!({"name":"旧版虚构工作区"})).unwrap()["id"].clone();
    old.execute_batch("DROP TABLE m_optimization_plans; DROP TABLE m_local_jobs; DROP TABLE m_attribution_datasets; UPDATE m_core_meta SET schema_version=2; PRAGMA user_version=2;").unwrap();
    let bytes = crate::backup::encrypt_snapshot(&old, "phase5-migration-test").unwrap();
    let mut dest = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
    crate::backup::restore_snapshot(&mut dest, &bytes, "phase5-migration-test").unwrap();
    assert_eq!(
        dest.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        handle(&mut dest, "attribution.list", json!({"brandId":b})).unwrap()["datasets"],
        json!([])
    );
    old.pragma_update(None, "user_version", 1).unwrap();
    let bytes = crate::backup::encrypt_snapshot(&old, "phase5-migration-test").unwrap();
    assert!(crate::backup::restore_snapshot(&mut dest, &bytes, "phase5-migration-test").is_err());
    assert_eq!(
        handle(&mut dest, "brand.list", json!({})).unwrap()[0]["id"],
        b
    );
}
