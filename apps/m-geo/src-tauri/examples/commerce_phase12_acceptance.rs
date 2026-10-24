//! Real-core fictional/simulated acceptance. No network, models, or publishing.
use m_geo_lib::{
    attribution_inspection_session::AttributionInspectionSession, core, files, runtime,
    site_package,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
const PRIVATE: &str = "P12_PRIVATE_6941";
const INVALID: &str = "归因包证据无效、归属不符或超过处理上限";
fn call(db: &mut Connection, action: &str, payload: Value) -> Value {
    runtime::core_dispatch(db, action, payload).unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn write(dir: &Path, name: &str, bytes: &[u8]) {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))
        .unwrap();
    f.write_all(bytes).unwrap();
    f.sync_all().unwrap();
}
fn save(dir: &Path, name: &str, value: &Value) {
    write(dir, name, &serde_json::to_vec_pretty(value).unwrap());
}
fn content<'a>(package: &'a Value, path: &str) -> &'a str {
    package["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap()["content"]
        .as_str()
        .unwrap()
}
fn replace(package: &mut Value, path: &str, text: String) {
    package["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["path"] == path)
        .unwrap()["content"] = json!(text);
}
fn decode(name: &str, package: &Value) -> Value {
    site_package::decode_attribution_package(name, &files::site_zip(package).unwrap()).unwrap()
}
fn artifact(dir: &Path, name: &str, package: &Value) -> Value {
    write(dir, name, &files::site_zip(package).unwrap());
    site_package::decode_attribution_package(name, &fs::read(dir.join(name)).unwrap()).unwrap()
}
fn table_hashes(db: &Connection) -> Value {
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut result = json!({});
    for name in names {
        assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
        let mut st = db.prepare(&format!("SELECT * FROM {name}")).unwrap();
        let columns = st.column_count();
        let mut rows: Vec<String> = st
            .query_map([], |r| {
                let values: Vec<Value> = (0..columns)
                    .map(|i| match r.get_ref(i).unwrap() {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!({"integer":n}),
                        ValueRef::Real(n) => json!({"real":n}),
                        ValueRef::Text(s) => json!({"text":hex::encode(s)}),
                        ValueRef::Blob(s) => json!({"blob":hex::encode(s)}),
                    })
                    .collect();
                Ok(serde_json::to_string(&values).unwrap())
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows.sort();
        result[name] = json!({"rows":rows.len(),"sha256":digest(serde_json::to_string(&rows).unwrap().as_bytes())});
    }
    result
}
fn required_fixture(key: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(key).unwrap_or_else(|| {
        panic!("required historical fixture environment variable {key} is missing")
    }));
    assert!(
        path.is_file(),
        "required historical fixture {key} is missing"
    );
    path
}

fn checks(report: &Value, failed: Option<&str>) {
    assert_eq!(
        report["status"],
        if failed.is_some() {
            "blocked"
        } else {
            "passed"
        },
        "checks: {}",
        report["checks"]
    );
    let codes = [
        "frozen_dataset",
        "input",
        "summary",
        "orders_csv",
        "sources_csv",
        "products_csv",
        "weeks_csv",
        "methodology",
        "external_requirements",
    ];
    assert_eq!(report["checks"].as_array().unwrap().len(), codes.len());
    for (row, code) in report["checks"].as_array().unwrap().iter().zip(codes) {
        assert_eq!(row["code"], code);
        assert_eq!(
            row["status"],
            if failed == Some(code) {
                "failed"
            } else {
                "passed"
            }
        );
    }
}
fn metrics(summary: &Value, values: &[(&str, i64)]) {
    for (key, n) in values {
        assert_eq!(summary[*key], *n, "metric {key}");
    }
}
fn order<'a>(rows: &'a [Value], suffix: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["orderId"] == format!("{PRIVATE}-O-{suffix}"))
        .unwrap()
}
fn reason(rows: &[Value], suffix: &str, expected: &str, visit: Option<&str>) {
    let row = order(rows, suffix);
    assert_eq!(row["reason"], expected, "reason {suffix}");
    assert_eq!(
        row["visitId"],
        visit
            .map(|v| json!(format!("{PRIVATE}-V-{v}")))
            .unwrap_or(Value::Null)
    );
}
fn safe_readback(bytes: &[u8], forbidden: &[String]) -> Value {
    for marker in forbidden {
        assert!(
            !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
            "private bytes {marker}"
        );
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), 2);
    let mut names = vec![];
    let mut report = Value::Null;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        names.push(f.name().to_owned());
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();
        for marker in forbidden {
            assert!(!text.contains(marker), "private text {marker}");
        }
        for marker in [
            "\"sessionId\"",
            "\"orderId\"",
            "\"visitId\"",
            "\"sku\"",
            "\"name\"",
            "dataset.json",
            "input.json",
            "orders.csv",
            "sources.csv",
            "products.csv",
            "weeks.csv",
        ] {
            assert!(!text.contains(marker), "raw field/file {marker}");
        }
        if f.name() == "inspection.json" {
            report = serde_json::from_str(&text).unwrap();
        }
    }
    names.sort();
    assert_eq!(names, ["inspection.json", "report.md"]);
    assert!(!report.is_null());
    report
}
fn legacy_fixture(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    let package =
        site_package::decode_attribution_package("native-final-report.zip", &bytes).unwrap();
    let dataset: Value = serde_json::from_str(content(&package, "dataset.json")).unwrap();
    let brand = dataset["brandId"].as_str().unwrap();
    let inspected = core::inspect_attribution_report(brand, &package).unwrap();
    checks(&inspected.report, None);
    assert_eq!(inspected.report["evidenceMode"], "simulated");
    metrics(
        &inspected.report["summary"],
        &[
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
        ],
    );
    assert_eq!(
        inspected.report["summary"]["sessionConversionRate"]
            .as_f64()
            .unwrap(),
        5.0 / 7.0
    );
    assert_eq!(inspected.report["summary"]["unknownOrderRate"], 0.625);
    for (id, expected) in [
        ("SIM-O1", "matched"),
        ("SIM-O2", "matched"),
        ("SIM-O3", "unknown_source"),
        ("SIM-O4", "unknown_source"),
        ("SIM-O5", "matched"),
        ("SIM-O6", "ambiguous_touch"),
        ("SIM-O7", "missing_session"),
        ("SIM-O8", "no_matching_visit"),
        ("SIM-O9", "cancelled"),
    ] {
        assert_eq!(
            inspected
                .orders
                .iter()
                .find(|o| o["orderId"] == id)
                .unwrap()["reason"],
            expected
        );
    }
    assert_eq!(
        fs::read(path).unwrap(),
        bytes,
        "actual historical ZIP modified"
    );
    json!({"path":path,"sha256":digest(&bytes),"brandId":brand,"status":"passed","summary":inspected.report["summary"],"reasonRows":9,"sourceDatabaseRequired":false})
}
struct Fixture {
    products: Vec<Value>,
    report: Value,
    empty: Value,
    template: Value,
}
fn seed(db: &mut Connection, brand: &Value) -> Fixture {
    let mut products = vec![];
    for i in 0..21 {
        products.push(call(db,"product.save",json!({"brandId":brand,"name":format!("=虚构商品<{PRIVATE}>\n\t\u{feff}-{i:02}"),"sku":format!("={PRIVATE}-SKU\n\t\u{feff}-{i:02}"),"facts":[]})));
    }
    let sku = products[0]["sku"].as_str().unwrap();
    let visit = |id: &str, session: &str, at: &str, source: &str| json!({"id":format!("{PRIVATE}-V-{id}"),"sessionId":format!("{PRIVATE}-S-{session}"),"sku":sku,"occurredAt":at,"source":source,"sourceEvidence":if source=="unknown"{"unknown"}else{"tagged"}});
    let mut visits = vec![
        visit("main", "main", "2026-10-12T00:00:00Z", "doubao"),
        visit("main-later", "main", "2026-10-20T00:00:00Z", "qianwen"),
        visit("known", "unknown", "2026-10-12T00:00:00Z", "doubao"),
        visit("unknown", "unknown", "2026-10-13T00:00:00Z", "unknown"),
        visit("conflict-a", "conflict", "2026-10-14T00:00:00Z", "doubao"),
        visit("conflict-b", "conflict", "2026-10-14T00:00:00Z", "yuanbao"),
        visit("same-b", "same", "2026-10-15T00:00:00Z", "qianwen"),
        visit("same-a", "same", "2026-10-15T00:00:00Z", "qianwen"),
        visit("cross", "cross", "2026-10-18T00:00:00Z", "baidu"),
        visit("before", "before", "2026-10-11T00:00:00Z", "other"),
        visit("end", "end", "2026-10-26T00:00:00Z", "doubao"),
    ];
    // Reverse the input to prove canonical event ordering is the real import's job.
    visits.reverse();
    let event = |id: &str, session: Option<&str>, at: &str, status: &str, amount: i64| json!({"id":format!("{PRIVATE}-O-{id}"),"sessionId":session.map(|s|format!("{PRIVATE}-S-{s}")),"sku":sku,"occurredAt":at,"status":status,"amountMinor":amount});
    let mut orders = vec![
        event("A", Some("main"), "2026-10-12T00:00:00Z", "paid", 1000),
        event("B", Some("main"), "2026-10-19T00:00:00Z", "paid", 2000),
        event("C", Some("main"), "2026-10-19T00:00:01Z", "paid", 3000),
        event("D", Some("unknown"), "2026-10-14T00:00:00Z", "paid", 4000),
        event("E", Some("conflict"), "2026-10-15T00:00:00Z", "paid", 5000),
        event("F", Some("same"), "2026-10-16T00:00:00Z", "paid", 6000),
        event("G", None, "2026-10-17T00:00:00Z", "paid", 7000),
        event("H", Some("main"), "2026-10-18T00:00:00Z", "cancelled", 8000),
        event("I", Some("cross"), "2026-10-19T00:00:00Z", "paid", 9000),
        event("J", Some("before"), "2026-10-12T00:00:00Z", "paid", 10000),
        event(
            "excluded-before",
            None,
            "2026-10-11T23:59:59Z",
            "paid",
            123456,
        ),
        event(
            "excluded-end",
            Some("end"),
            "2026-10-26T00:00:00Z",
            "paid",
            654321,
        ),
    ];
    for (i, p) in products.iter().enumerate().skip(1) {
        orders.push(json!({"id":format!("{PRIVATE}-O-fill-{i:02}"),"sessionId":null,"sku":p["sku"],"occurredAt":"2026-10-20T00:00:00Z","status":"paid","amountMinor":100}));
    }
    let refund = |id: &str, oid: &str, at: &str, amount: i64| json!({"id":format!("{PRIVATE}-R-{id}"),"orderId":format!("{PRIVATE}-O-{oid}"),"occurredAt":at,"amountMinor":amount});
    let input = json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":format!("虚构本地模拟来源-{PRIVATE}"),"coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","visits":visits,"orders":orders,"refunds":[refund("full","A","2026-10-30T00:00:00Z",1000),refund("partial","I","2026-10-20T00:00:00Z",500),refund("late","I","2026-10-31T23:59:59Z",400)]});
    let mut request = json!({"brandId":brand,"name":format!("虚构模拟数据集-{PRIVATE}"),"content":input.to_string()});
    let preview = call(db, "attribution.preview", request.clone());
    assert_eq!(
        preview["counts"],
        json!({"visits":11,"orders":32,"refunds":3,"duplicates":0})
    );
    request["expectedHash"] = preview["contentHash"].clone();
    let saved = call(db, "attribution.apply", request);
    let params = json!({"brandId":brand,"datasetId":saved["datasetId"],"from":"2026-10-12T00:00:00Z","to":"2026-10-26T00:00:00Z","lookbackDays":7});
    let report = call(db, "attribution.report", params.clone());
    let mut empty = params;
    empty["from"] = json!("2026-10-27T00:00:00Z");
    empty["to"] = json!("2026-10-28T00:00:00Z");
    let empty = call(db, "attribution.report", empty);
    let template = call(db, "attribution.template", json!({"brandId":brand}));
    Fixture {
        products,
        report,
        empty,
        template,
    }
}
fn hand_assertions(inspected: &core::InspectedAttribution) {
    let r = &inspected.report;
    metrics(
        &r["summary"],
        &[
            ("visits", 9),
            ("visitingSessions", 5),
            ("paidOrders", 29),
            ("cancelledOrders", 1),
            ("attributedOrders", 5),
            ("unknownOrders", 24),
            ("convertedSessions", 4),
            ("grossMinor", 49000),
            ("refundMinor", 1900),
            ("netMinor", 47100),
            ("attributedNetMinor", 26100),
            ("unknownNetMinor", 21000),
        ],
    );
    assert_eq!(r["summary"]["sessionConversionRate"], 0.8);
    assert_eq!(
        r["summary"]["unknownOrderRate"].as_f64().unwrap(),
        24.0 / 29.0
    );
    assert_eq!(r["summary"]["lookbackCoverageIncomplete"], false);
    assert_eq!(
        r["counts"],
        json!({"visits":11,"orders":32,"refunds":3,"products":21})
    );
    assert_eq!(inspected.orders.len(), 30);
    assert_eq!(inspected.products.len(), 21);
    for (id, why, vid) in [
        ("A", "matched", Some("main")),
        ("B", "matched", Some("main")),
        ("C", "no_matching_visit", None),
        ("D", "unknown_source", Some("unknown")),
        ("E", "ambiguous_touch", None),
        ("F", "matched", Some("same-a")),
        ("G", "missing_session", None),
        ("H", "cancelled", None),
        ("I", "matched", Some("cross")),
        ("J", "matched", Some("before")),
    ] {
        reason(&inspected.orders, id, why, vid);
    }
    assert_eq!(order(&inspected.orders, "A")["netMinor"], 0);
    assert_eq!(order(&inspected.orders, "A")["visitInPeriod"], true);
    assert_eq!(order(&inspected.orders, "I")["refundMinor"], 900);
    assert_eq!(order(&inspected.orders, "J")["visitInPeriod"], false);
    assert_eq!(order(&inspected.orders, "H")["netMinor"], 0);
    assert!(!inspected
        .orders
        .iter()
        .any(|o| o["orderId"].as_str().unwrap().contains("excluded")));
    // Source amounts and product amounts are hand expectations, independent of the exporter.
    for (source, visits, sessions, converted, paid, gross, refunds, net) in [
        ("doubao", 3, 3, 1, 2, 3000, 1000, 2000),
        ("qianwen", 3, 2, 1, 1, 6000, 0, 6000),
        ("yuanbao", 1, 1, 0, 0, 0, 0, 0),
        ("unknown", 1, 1, 1, 24, 21000, 0, 21000),
        ("baidu", 1, 1, 1, 1, 9000, 900, 8100),
        ("other", 0, 0, 0, 1, 10000, 0, 10000),
    ] {
        let row = r["bySource"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["source"] == source)
            .unwrap();
        metrics(
            row,
            &[
                ("visits", visits),
                ("visitingSessions", sessions),
                ("convertedSessions", converted),
                ("paidOrders", paid),
                ("grossMinor", gross),
                ("refundMinor", refunds),
                ("netMinor", net),
            ],
        );
    }
    for product in &inspected.products {
        if product["sku"].as_str().unwrap().ends_with("-00") {
            metrics(
                product,
                &[
                    ("paidOrders", 9),
                    ("cancelledOrders", 1),
                    ("grossMinor", 47000),
                    ("refundMinor", 1900),
                    ("netMinor", 45100),
                    ("visitingSessions", 5),
                    ("convertedSessions", 4),
                ],
            );
        } else {
            metrics(
                product,
                &[
                    ("paidOrders", 1),
                    ("grossMinor", 100),
                    ("refundMinor", 0),
                    ("netMinor", 100),
                    ("visitingSessions", 0),
                    ("convertedSessions", 0),
                ],
            );
            assert!(product["sessionConversionRate"].is_null());
            assert_eq!(product["unknownOrderRate"], 1.0);
        }
    }
    let weeks = r["weekly"].as_array().unwrap();
    let first = weeks
        .iter()
        .find(|w| w["weekStart"] == "2026-10-12")
        .unwrap();
    let second = weeks
        .iter()
        .find(|w| w["weekStart"] == "2026-10-19")
        .unwrap();
    metrics(
        first,
        &[
            ("convertedSessions", 4),
            ("paidOrders", 6),
            ("grossMinor", 33000),
            ("refundMinor", 1000),
            ("netMinor", 32000),
        ],
    );
    metrics(
        second,
        &[
            ("convertedSessions", 0),
            ("paidOrders", 23),
            ("grossMinor", 16000),
            ("refundMinor", 900),
            ("netMinor", 15100),
        ],
    );
    assert_eq!(
        weeks
            .iter()
            .map(|w| w["visitingSessions"].as_i64().unwrap())
            .sum::<i64>(),
        6
    );
    assert_eq!(
        r["bySource"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["visitingSessions"].as_i64().unwrap())
            .sum::<i64>(),
        8
    );
    let other = r["bySource"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] == "other")
        .unwrap();
    assert!(other["sessionConversionRate"].is_null());
    assert_eq!(other["paidOrders"], 1);
    for group in ["bySource", "weekly"] {
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
                r[group]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x[key].as_i64().unwrap())
                    .sum::<i64>(),
                r["summary"][key].as_i64().unwrap(),
                "additive {group}/{key}"
            );
        }
    }
}
fn rejected(brand: &str, package: &Value) {
    let error = core::inspect_attribution_report(brand, package)
        .err()
        .unwrap();
    assert_eq!(error, INVALID);
    assert!(!error.contains(PRIVATE));
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 2, "one NEW output directory required");
    // Mandatory actual historical archive is resolved and inspected before creating outputs.
    let legacy = required_fixture("MGEO_PHASE12_LEGACY");
    assert_eq!(
        legacy.file_name().and_then(|name| name.to_str()),
        Some("native-final-report.zip"),
        "MGEO_PHASE12_LEGACY must reference the actual Phase5 native-final-report.zip fixture"
    );
    let legacy_bytes = fs::read(&legacy).unwrap();
    let historical = legacy_fixture(&legacy);
    let output = Path::new(&args[1]);
    assert!(
        !output.exists(),
        "refuse to overwrite output directory/database"
    );
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(output).unwrap();
    let path = output.join("fictional.sqlite3");
    write(output, "fictional.sqlite3", &[]);
    let mut db = runtime::open_database(&path).unwrap();
    let brand = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构历史归因实验室（仅模拟验收）"}),
    )["id"]
        .clone();
    let foreign = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构隔离归因实验室（仅模拟验收）"}),
    )["id"]
        .clone();
    let fixture = seed(&mut db, &brand);
    let wrong_fixture = seed(&mut db, &foreign);
    let wrong = artifact(output, "wrong-brand.zip", &wrong_fixture.report);
    checks(
        &core::inspect_attribution_report(foreign.as_str().unwrap(), &wrong)
            .unwrap()
            .report,
        None,
    );
    let original = fixture.report.clone();
    let frozen = artifact(output, "current-attribution.zip", &original);
    let frozen_bytes = fs::read(output.join("current-attribution.zip")).unwrap();
    let inspected = core::inspect_attribution_report(brand.as_str().unwrap(), &frozen).unwrap();
    checks(&inspected.report, None);
    hand_assertions(&inspected);
    let csv = content(&frozen, "products.csv");
    assert!(csv.contains("\r\n") && csv.contains('\u{feff}'));
    assert!(csv.contains("\"'=") && csv.contains("<P12_PRIVATE_6941>\n\t"));
    // Change both live name and SKU after export; frozen IDs and labels remain portable.
    let mut changed = fixture.products[0].clone();
    changed["expectedRevision"] = changed["revision"].clone();
    changed["name"] = json!(format!("虚构修改后-{PRIVATE}"));
    changed["sku"] = json!(format!("LIVE-{PRIVATE}"));
    let changed = call(&mut db, "product.save", changed);
    assert_eq!(changed["revision"], 2);
    let mut forbidden = vec![PRIVATE.to_string()];
    forbidden.extend(
        fixture
            .products
            .iter()
            .chain(wrong_fixture.products.iter())
            .map(|p| p["id"].as_str().unwrap().to_owned()),
    );
    core::validate_database(&db).unwrap();
    for table in ["ai_jobs", "m_publications"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    let tables_before = table_hashes(&db);
    let schema = db
        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
        .unwrap();
    assert_eq!(schema, 5);
    drop(db);
    let bytes_before = fs::read(&path).unwrap();
    let hidden = output.join("fictional.sqlite3.unavailable");
    fs::rename(&path, &hidden).unwrap();
    assert!(!path.exists());
    // No core dispatch or database initializer below this line: only portable ZIP/session APIs.
    let reread = site_package::decode_attribution_package(
        "current-attribution.zip",
        &fs::read(output.join("current-attribution.zip")).unwrap(),
    )
    .unwrap();
    let independent = core::inspect_attribution_report(brand.as_str().unwrap(), &reread).unwrap();
    assert_eq!(independent.report, inspected.report);
    assert_eq!(independent.orders, inspected.orders);
    assert_eq!(independent.products, inspected.products);
    hand_assertions(&independent);
    let mut session = AttributionInspectionSession::default();
    let generation = session.begin_selection();
    let selected = session
        .complete_selection(generation, brand.as_str().unwrap(), reread)
        .unwrap();
    let ticket = selected["inspectionId"].as_str().unwrap();
    let fingerprint = selected["report"]["reportFingerprint"].as_str().unwrap();
    let mut page_counts = json!({});
    for (kind, expected) in [
        ("orders", &independent.orders),
        ("products", &independent.products),
    ] {
        let a = session
            .read_page(brand.as_str().unwrap(), ticket, kind, 0)
            .unwrap();
        let b = session
            .read_page(brand.as_str().unwrap(), ticket, kind, 1)
            .unwrap();
        assert_eq!(a["pageSize"], 20);
        assert_eq!(a["total"], expected.len());
        assert_eq!(a["rows"].as_array().unwrap().len(), 20);
        assert_eq!(b["rows"].as_array().unwrap().len(), expected.len() - 20);
        let combined: Vec<Value> = a["rows"]
            .as_array()
            .unwrap()
            .iter()
            .chain(b["rows"].as_array().unwrap())
            .cloned()
            .collect();
        assert_eq!(&combined, expected);
        page_counts[kind] = json!([20, expected.len() - 20]);
        assert!(session
            .read_page(brand.as_str().unwrap(), ticket, kind, 2)
            .is_err());
        assert!(session
            .read_page(foreign.as_str().unwrap(), ticket, kind, 0)
            .is_err());
    }
    assert!(session
        .read_page(brand.as_str().unwrap(), ticket, "visits", 0)
        .is_err());
    assert!(session
        .prepare_export(foreign.as_str().unwrap(), ticket, fingerprint)
        .is_err());
    assert!(session
        .prepare_export(brand.as_str().unwrap(), ticket, &"0".repeat(64))
        .is_err());
    let safe = session
        .prepare_export(brand.as_str().unwrap(), ticket, fingerprint)
        .unwrap();
    let mut forged_report = inspected.report.clone();
    forged_report["summary"]["grossMinor"] = json!(999);
    assert!(core::attribution_inspection_export(&forged_report).is_err());

    let safe_bytes = files::site_zip(&safe).unwrap();
    write(output, "inspection.zip", &safe_bytes);
    assert_eq!(
        safe_readback(
            &fs::read(output.join("inspection.zip")).unwrap(),
            &forbidden
        ),
        inspected.report
    );
    session.begin_selection();
    assert!(session
        .read_page(brand.as_str().unwrap(), ticket, "orders", 0)
        .is_err());
    assert!(session
        .prepare_export(brand.as_str().unwrap(), ticket, fingerprint)
        .is_err());
    assert!(AttributionInspectionSession::default()
        .read_page(brand.as_str().unwrap(), ticket, "orders", 0)
        .is_err());
    let older = session.begin_selection();
    let latest = session.begin_selection();
    assert!(session
        .complete_selection(older, brand.as_str().unwrap(), frozen.clone())
        .is_err());
    assert!(session
        .complete_selection(latest, brand.as_str().unwrap(), wrong.clone())
        .is_err());
    let empty = decode("empty.zip", &fixture.empty);
    let empty_inspected =
        core::inspect_attribution_report(brand.as_str().unwrap(), &empty).unwrap();
    checks(&empty_inspected.report, None);
    for key in ["sessionConversionRate", "unknownOrderRate"] {
        assert!(empty_inspected.report["summary"][key].is_null());
    }
    assert_eq!(empty_inspected.report["summary"]["grossMinor"], 0);
    assert!(empty_inspected.orders.is_empty() && empty_inspected.products.is_empty());
    let generation = session.begin_selection();
    let empty_selected = session
        .complete_selection(generation, brand.as_str().unwrap(), empty)
        .unwrap();
    let empty_ticket = empty_selected["inspectionId"].as_str().unwrap();
    for kind in ["orders", "products"] {
        let page = session
            .read_page(brand.as_str().unwrap(), empty_ticket, kind, 0)
            .unwrap();
        assert_eq!(page["rows"], json!([]));
        assert_eq!(page["total"], 0);
        assert!(session
            .read_page(brand.as_str().unwrap(), empty_ticket, kind, 1)
            .is_err());
    }
    let mut tamper_checks = json!({});
    for (name, code) in [
        ("summary.json", "summary"),
        ("orders.csv", "orders_csv"),
        ("sources.csv", "sources_csv"),
        ("products.csv", "products_csv"),
        ("weeks.csv", "weeks_csv"),
        ("methodology.txt", "methodology"),
        ("external-requirements.txt", "external_requirements"),
    ] {
        let mut tampered = original.clone();
        let replacement = if name == "summary.json" {
            let mut s: Value = serde_json::from_str(content(&tampered, name)).unwrap();
            s["summary"]["grossMinor"] = json!(999);
            s.to_string()
        } else {
            format!("{}tampered", content(&tampered, name))
        };
        replace(&mut tampered, name, replacement);
        let decoded = if name == "orders.csv" {
            artifact(output, "blocked-companion.zip", &tampered)
        } else {
            decode("tampered.zip", &tampered)
        };
        let blocked = core::inspect_attribution_report(brand.as_str().unwrap(), &decoded).unwrap();
        checks(&blocked.report, Some(code));
        hand_assertions(&blocked);
        assert_eq!(blocked.orders, independent.orders);
        let generation = session.begin_selection();
        let selected = session
            .complete_selection(generation, brand.as_str().unwrap(), decoded)
            .unwrap();
        let blocked_ticket = selected["inspectionId"].as_str().unwrap();
        let blocked_page = session
            .read_page(brand.as_str().unwrap(), blocked_ticket, "orders", 0)
            .unwrap();
        assert_eq!(
            blocked_page["rows"].as_array().unwrap(),
            &blocked.orders[..20]
        );
        let export = session
            .prepare_export(
                brand.as_str().unwrap(),
                blocked_ticket,
                selected["report"]["reportFingerprint"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(
            safe_readback(&files::site_zip(&export).unwrap(), &forbidden),
            blocked.report
        );
        tamper_checks[code] = blocked.report["checks"].clone();
    }
    let mut raw_conflict = original.clone();
    let mut input: Value = serde_json::from_str(content(&raw_conflict, "input.json")).unwrap();
    input["orders"][0]["amountMinor"] = json!(777);
    replace(&mut raw_conflict, "input.json", input.to_string());
    rejected(
        brand.as_str().unwrap(),
        &decode("raw-conflict.zip", &raw_conflict),
    );
    let mut bad_hash = original.clone();
    let mut dataset: Value = serde_json::from_str(content(&bad_hash, "dataset.json")).unwrap();
    dataset["contentHash"] = json!("0".repeat(64));
    replace(&mut bad_hash, "dataset.json", dataset.to_string());
    rejected(brand.as_str().unwrap(), &decode("hash.zip", &bad_hash));
    rejected(brand.as_str().unwrap(), &wrong);
    let mut extra = original.clone();
    extra["files"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":"extra.txt","content":"extra"}));
    assert!(site_package::decode_attribution_package(
        "extra.zip",
        &files::site_zip(&extra).unwrap()
    )
    .is_err());
    assert!(site_package::decode_attribution_package(
        "template.zip",
        &files::site_zip(&fixture.template).unwrap()
    )
    .is_err());
    assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);
    assert_eq!(
        fs::read(output.join("current-attribution.zip")).unwrap(),
        frozen_bytes
    );
    assert!(!path.exists());
    fs::rename(&hidden, &path).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes_before,
        "inspection/export changed DB bytes"
    );
    // Reopen strictly read-only: runtime::open_database writes a startup header.
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tables_after = table_hashes(&db);
    assert_eq!(tables_before, tables_after);
    core::validate_database(&db).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        schema
    );
    drop(db);
    let before_hash = digest(&bytes_before);
    let after_hash = digest(&fs::read(&path).unwrap());
    assert_eq!(
        before_hash, after_hash,
        "FINAL read-only validation changed database bytes"
    );
    let mut artifacts = json!({});
    for name in [
        "fictional.sqlite3",
        "current-attribution.zip",
        "blocked-companion.zip",
        "wrong-brand.zip",
        "inspection.zip",
    ] {
        let bytes = fs::read(output.join(name)).unwrap();
        artifacts[name] = json!({"bytes":bytes.len(),"sha256":digest(&bytes)});
    }
    save(
        output,
        "acceptance.json",
        &json!({"status":"passed","fixture":"fictional-local-simulated-only","nativeAcceptance":"pending-controller","brandId":brand,"foreignBrandId":foreign,"datasetId":inspected.report["datasetId"],"schema":schema,"inspection":inspected.report,"historicalFixture":historical,"pages":page_counts,"emptySummary":empty_inspected.report["summary"],"tamperChecks":tamper_checks,"handExpected":{"grossMinor":49000,"refundMinor":1900,"netMinor":47100,"paidOrders":29,"cancelledOrders":1,"attributedOrders":5,"unknownOrders":24,"visitingSessions":5,"convertedSessions":4},"readonly":{"sourceDatabaseUnavailable":true,"databaseSha256Before":before_hash,"databaseSha256After":after_hash,"allTableHashesBefore":tables_before,"allTableHashesAfter":tables_after},"artifacts":artifacts,"modelJobs":0,"networkCalls":0,"platformSubmissions":0,"publications":0,"checks":["actual-phase5-native-archive-required-and-unchanged","hand-money-and-cohort-expectations","inclusive-lookback-and-payment-boundaries-exclusive-period-end","latest-unknown-no-fallback-ambiguous-conflict-lexical-same-evidence-tie","cancelled-full-partial-late-refund-missing-session","cross-week-and-source-nonadditive-null-denominator","orders20-plus10-products20-plus1-paging","live-product-name-and-sku-changed-after-export","source-database-unavailable-during-inspection-export","all-table-and-FINAL-database-byte-hashes-unchanged","each-seven-companions-independently-blocked-and-safe-exported","raw-input-conflict-hash-foreign-template-extra-rejected","safe-ZIP-file-readback-private-byte-and-text-scan","empty-range-pages-zero-and-null-rates","cancel-failure-late-selection-and-restart-session-invalidated"]}),
    );
    println!("Phase12 acceptance passed: actual Phase5 ZIP, 30 orders/21 products, hand gross49000/refund1900/net47100, order pages20+10/product pages20+1, DB-unavailable historical inspection/export, final DB/table hashes unchanged. Native acceptance pending controller.");
}
