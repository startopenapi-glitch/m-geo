use super::*;

const INVALID: &str = "归因包证据无效、归属不符或超过处理上限";
fn fixture() -> (Connection, String, Value, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let b = handle(&mut c, "brand.create", json!({"name":"虚构历史品牌"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let p=handle(&mut c,"product.save",json!({"brandId":b,"name":"=PRIVATE_NAME<em>\n\t\u{feff}","sku":"=PRIVATE_SKU\nX\t\u{feff}","category":"","officialUrl":"","sourceText":"PRIVATE_SOURCE","privateNotes":"PRIVATE_NOTES","facts":[]})).unwrap();
    let sku = p["sku"].clone();
    let input = json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":"PRIVATE_LABEL","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY",
    "visits":[{"id":"PRIVATE_VISIT","sessionId":"PRIVATE_SESSION","sku":sku,"occurredAt":"2026-10-12T00:00:00Z","source":"doubao","sourceEvidence":"tagged"}],
    "orders":[{"id":"PRIVATE_ORDER_A","sessionId":"PRIVATE_SESSION","sku":sku,"occurredAt":"2026-10-19T00:00:00Z","status":"paid","amountMinor":1001},
    {"id":"PRIVATE_ORDER_B","sessionId":null,"sku":sku,"occurredAt":"2026-10-20T00:00:00Z","status":"paid","amountMinor":999},
    {"id":"PRIVATE_ORDER_C","sessionId":"PRIVATE_SESSION","sku":sku,"occurredAt":"2026-10-20T00:00:00Z","status":"cancelled","amountMinor":800}],
    "refunds":[{"id":"PRIVATE_REFUND","orderId":"PRIVATE_ORDER_A","occurredAt":"2026-10-30T00:00:00Z","amountMinor":501}]});
    let mut import = json!({"brandId":b,"name":"PRIVATE_DATASET","content":input.to_string()});
    import["expectedHash"] = attribution::preview(&c, &b, &import).unwrap()["contentHash"].clone();
    let saved = attribution::apply(&c, &b, &import).unwrap();
    let params = json!({"brandId":b,"datasetId":saved["datasetId"],"from":"2026-10-12T00:00:00Z","to":"2026-10-26T00:00:00Z","lookbackDays":7});
    let report = attribution_report::prepare(&c, &b, &params).unwrap();
    (c, b, package(report["files"].clone()), p)
}
fn package(files: Value) -> Value {
    let bytes: usize = files
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["content"].as_str().unwrap().len())
        .sum();
    json!({"source":{"fileName":"fixture.zip","sha256":"a".repeat(64),"compressedBytes":100,"uncompressedBytes":bytes,"fileCount":9},"files":files})
}
fn content<'a>(p: &'a Value, path: &str) -> &'a str {
    p["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap()["content"]
        .as_str()
        .unwrap()
}
fn set(p: &mut Value, path: &str, text: String) {
    p["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["path"] == path)
        .unwrap()["content"] = json!(text);
    let bytes: usize = p["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["content"].as_str().unwrap().len())
        .sum();
    p["source"]["uncompressedBytes"] = json!(bytes);
}
fn dataset(p: &Value) -> Value {
    serde_json::from_str(content(p, "dataset.json")).unwrap()
}
fn replace_dataset(p: &mut Value, mut d: Value) {
    d["contentHash"] = json!(fingerprint(
        &json!({"input":d["input"],"products":d["products"]})
    ));
    set(p, "input.json", d["input"].to_string());
    let mut summary: Value = serde_json::from_str(content(p, "summary.json")).unwrap();
    summary["contentHash"] = d["contentHash"].clone();
    summary["evidenceMode"] = d["input"]["evidenceMode"].clone();
    set(p, "summary.json", summary.to_string());
    set(p, "dataset.json", d.to_string());
}
fn rejected(b: &str, p: &Value) {
    assert_eq!(
        inspect_attribution_report(b, p).err().as_deref(),
        Some(INVALID)
    );
}
fn sign(r: &mut Value) {
    r.as_object_mut().unwrap().remove("reportFingerprint");
    r["reportFingerprint"] = json!(fingerprint(r));
}

#[test]
fn attribution_inspection_current_v1_roundtrip() {
    let (_, b, p, _) = fixture();
    let result = inspect_attribution_report(&b, &p).unwrap();
    let r = result.report;
    assert_eq!(r["status"], "passed");
    assert_eq!(r["format"], "m-geo-attribution-inspection-v1");
    assert_eq!(r["profile"], "attribution-v1");
    assert_eq!(r["brandId"], b);
    assert_eq!(r["contentHash"], dataset(&p)["contentHash"]);
    assert_eq!(
        r["counts"],
        json!({"visits":1,"orders":3,"refunds":1,"products":1})
    );
    assert_eq!(r["period"]["refundAsOf"], "2026-11-01T00:00:00Z");
    let codes: Vec<_> = r["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            assert_eq!(v["status"], "passed");
            v["code"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            "frozen_dataset",
            "input",
            "summary",
            "orders_csv",
            "sources_csv",
            "products_csv",
            "weeks_csv",
            "methodology",
            "external_requirements"
        ]
    );
    for (k, n) in [
        ("visits", 1),
        ("visitingSessions", 1),
        ("paidOrders", 2),
        ("cancelledOrders", 1),
        ("attributedOrders", 1),
        ("unknownOrders", 1),
        ("grossMinor", 2000),
        ("refundMinor", 501),
        ("netMinor", 1499),
        ("convertedSessions", 1),
    ] {
        assert_eq!(r["summary"][k], n, "{k}");
    }
    assert_eq!(r["summary"]["sessionConversionRate"], 1.0);
    assert_eq!(r["summary"]["unknownOrderRate"], 0.5);
    assert_eq!(result.orders.len(), 3);
    assert_eq!(result.products.len(), 1);
    assert_eq!(result.orders[0]["reason"], "matched");
    assert_eq!(result.orders[0]["netMinor"], 500);
    assert_eq!(r["weekly"][0]["weekStart"], "2026-10-12");
    assert_eq!(r["weekly"][0]["convertedSessions"], 1);
    assert_eq!(r["weekly"][0]["grossMinor"], 0);
    assert!(r["weekly"][1]["sessionConversionRate"].is_null());
    attribution_inspection_export(&r).unwrap();
}
#[test]
fn attribution_inspection_historical_catalog_independent_and_live_ownership_preserved() {
    let (c, b, p, product) = fixture();
    let d = dataset(&p);
    c.execute(
        "UPDATE m_products SET brand_id=?1 WHERE id=?2",
        params![Uuid::new_v4().to_string(), product["id"].as_str().unwrap()],
    )
    .unwrap_err();
    c.execute("PRAGMA foreign_keys=OFF", []).unwrap();
    c.execute(
        "UPDATE m_products SET brand_id=?1 WHERE id=?2",
        params![Uuid::new_v4().to_string(), product["id"].as_str().unwrap()],
    )
    .unwrap();
    assert!(attribution::validate_frozen_dataset(&c, &b, &d).is_err());
    drop(c);
    assert_eq!(
        inspect_attribution_report(&b, &p).unwrap().report["status"],
        "passed"
    );
    rejected(&Uuid::new_v4().to_string(), &p);
}
#[test]
fn attribution_inspection_malformed_frozen_rejected() {
    let (_, b, p, _) = fixture();
    for file in ["dataset.json", "input.json", "summary.json"] {
        let mut bad = p.clone();
        let original = content(&bad, file);
        let duplicate = format!("{{\"duplicate\":0,\"duplicate\":1,{}", &original[1..]);
        set(&mut bad, file, duplicate);
        rejected(&b, &bad);
    }
    for case in 0..13 {
        let mut bad = p.clone();
        let mut d = dataset(&p);
        match case {
            0 => d["input"]["orders"][0]["amountMinor"] = json!(0),
            1 => d["input"]["orders"][0]["status"] = json!("cancelled"),
            2 => d["input"]["refunds"][0]["amountMinor"] = json!(1002),
            3 => d["input"]["orders"].as_array_mut().unwrap().reverse(),
            4 => {
                let row = d["input"]["orders"][0].clone();
                d["input"]["orders"].as_array_mut().unwrap().push(row);
            }
            5 => d["input"]["coverageStart"] = json!("2026-10-01T08:00:00+08:00"),
            6 => d["products"][0]["privateNotes"] = json!("PRIVATE_EXTRA"),
            7 => d["products"] = json!([]),
            8 => {
                let row = d["products"][0].clone();
                d["products"].as_array_mut().unwrap().push(row);
            }
            9 => d["products"][0]["revision"] = json!(0),
            10 => d["createdAt"] = json!("2026-10-01T08:00:00+08:00"),
            11 => d["id"] = json!("invalid-uuid"),
            _ => d["input"]["visits"][0]["sourceEvidence"] = json!("unknown"),
        };
        replace_dataset(&mut bad, d);
        rejected(&b, &bad);
    }
    let mut bad = p.clone();
    let mut d = dataset(&p);
    d["contentHash"] = json!("0".repeat(64));
    set(&mut bad, "dataset.json", d.to_string());
    rejected(&b, &bad);
    let mut bad = p.clone();
    let mut i: Value = serde_json::from_str(content(&p, "input.json")).unwrap();
    i["sourceLabel"] = json!("OTHER");
    set(&mut bad, "input.json", i.to_string());
    rejected(&b, &bad);
    for key in [
        "datasetId",
        "contentHash",
        "evidenceMode",
        "currency",
        "brandId",
    ] {
        let mut bad = p.clone();
        let mut s: Value = serde_json::from_str(content(&p, "summary.json")).unwrap();
        s[key] = json!("foreign");
        set(&mut bad, "summary.json", s.to_string());
        rejected(&b, &bad);
    }
    for (key, value) in [
        ("from", json!("2026-10-12T08:00:00+08:00")),
        ("to", json!("2026-12-01T00:00:00Z")),
        ("lookbackDays", json!(31)),
        ("refundAsOf", json!("2026-10-26T00:00:00Z")),
    ] {
        let mut bad = p.clone();
        let mut s: Value = serde_json::from_str(content(&p, "summary.json")).unwrap();
        s["period"][key] = value;
        set(&mut bad, "summary.json", s.to_string());
        rejected(&b, &bad);
    }
}
#[test]
fn attribution_inspection_companion_tampering_blocked() {
    let (_, b, p, _) = fixture();
    for (file, code) in [
        ("summary.json", "summary"),
        ("orders.csv", "orders_csv"),
        ("sources.csv", "sources_csv"),
        ("products.csv", "products_csv"),
        ("weeks.csv", "weeks_csv"),
        ("methodology.txt", "methodology"),
        ("external-requirements.txt", "external_requirements"),
    ] {
        let mut bad = p.clone();
        let changed = if file == "summary.json" {
            let mut s: Value = serde_json::from_str(content(&p, file)).unwrap();
            s["summary"]["grossMinor"] = json!(123456);
            s.to_string()
        } else {
            format!("{}TAMPER", content(&p, file))
        };
        set(&mut bad, file, changed);
        let r = inspect_attribution_report(&b, &bad).unwrap().report;
        assert_eq!(r["status"], "blocked");
        assert_eq!(r["summary"]["grossMinor"], 2000);
        let failed: Vec<_> = r["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["status"] == "failed")
            .map(|v| v["code"].as_str().unwrap())
            .collect();
        assert_eq!(failed, vec![code]);
        attribution_inspection_export(&r).unwrap();
    }
    let mut bad = p.clone();
    let mut s: Value = serde_json::from_str(content(&p, "summary.json")).unwrap();
    s["privateExtra"] = json!("PRIVATE_EXTRA");
    set(&mut bad, "summary.json", s.to_string());
    let r = inspect_attribution_report(&b, &bad).unwrap().report;
    assert_eq!(r["status"], "blocked");
    assert!(!r.to_string().contains("PRIVATE_EXTRA"));
}
#[test]
fn attribution_inspection_accepted_1000_rejected_1001_and_bytes() {
    let (_, b, p, _) = fixture();
    let mut d = dataset(&p);
    let order = d["input"]["orders"][1].clone();
    d["input"]["visits"] = json!([]);
    d["input"]["refunds"] = json!([]);
    d["input"]["orders"] = json!((0..1000)
        .map(|i| {
            let mut o = order.clone();
            o["id"] = json!(format!("o{i:04}"));
            o["amountMinor"] = json!(1_000_000_000);
            o
        })
        .collect::<Vec<_>>());
    let mut bounded = p.clone();
    replace_dataset(&mut bounded, d.clone());
    let r = inspect_attribution_report(&b, &bounded).unwrap().report;
    assert_eq!(r["summary"]["grossMinor"], 1_000_000_000_000_i64);
    assert!(r["summary"]["sessionConversionRate"].is_null());
    let mut extra = order;
    extra["id"] = json!("o1000");
    d["input"]["orders"].as_array_mut().unwrap().push(extra);
    replace_dataset(&mut bounded, d);
    rejected(&b, &bounded);
    let mut bad = p.clone();
    let original = content(&p, "input.json");
    set(
        &mut bad,
        "input.json",
        format!("{}{}", original, " ".repeat(512 * 1024 - original.len())),
    );
    assert!(inspect_attribution_report(&b, &bad).is_ok());
    let text = format!("{} ", content(&bad, "input.json"));
    set(&mut bad, "input.json", text);
    rejected(&b, &bad);
    for (key, n) in [
        ("compressedBytes", 20 * 1024 * 1024 + 1),
        ("uncompressedBytes", 16 * 1024 * 1024 + 1),
    ] {
        let mut bad = p.clone();
        bad["source"][key] = json!(n);
        rejected(&b, &bad);
    }
    let mut bad = p.clone();
    let d = dataset(&p);
    let mut oversized = d.clone();
    oversized["name"] = json!("x".repeat(2 * 1024 * 1024));
    set(&mut bad, "dataset.json", oversized.to_string());
    rejected(&b, &bad);
}
#[test]
fn attribution_inspection_exporter_whitelist_and_inert_text() {
    let (_, b, mut p, _) = fixture();
    p["source"]["fileName"] = json!("```<img onerror=alert(1)>[link](https:evil).zip");
    let r = inspect_attribution_report(&b, &p).unwrap().report;
    let output = attribution_inspection_export(&r).unwrap();
    assert_eq!(output["files"].as_array().unwrap().len(), 2);
    let md = output["files"][1]["content"].as_str().unwrap();
    assert!(!md.contains("\n```<img"));
    for secret in [
        "PRIVATE_DATASET",
        "PRIVATE_NAME",
        "PRIVATE_SKU",
        "PRIVATE_LABEL",
        "PRIVATE_VISIT",
        "PRIVATE_ORDER",
        "PRIVATE_REFUND",
        "PRIVATE_SESSION",
        "PRIVATE_SOURCE",
        "PRIVATE_NOTES",
    ] {
        assert!(!output.to_string().contains(secret), "{secret}");
    }
    let mut changed = r.clone();
    changed["summary"]["grossMinor"] = json!(12);
    assert!(attribution_inspection_export(&changed).is_err());
    for target in [
        "root", "summary", "source", "counts", "period", "coverage", "checks", "bySource", "weekly",
    ] {
        let mut changed = r.clone();
        match target {
            "root" => changed["private"] = json!("PRIVATE"),
            "checks" => changed["checks"][0]["message"] = json!("PRIVATE"),
            "bySource" => changed["bySource"][0]["source"] = json!("PRIVATE"),
            "weekly" => changed["weekly"][0]["weekStart"] = json!("PRIVATE"),
            other => changed[other]["private"] = json!("PRIVATE"),
        };
        sign(&mut changed);
        assert!(attribution_inspection_export(&changed).is_err(), "{target}");
    }
    let mut d = dataset(&p);
    d["input"]["evidenceMode"] = json!("recorded");
    replace_dataset(&mut p, d);
    let r = inspect_attribution_report(&b, &p).unwrap().report;
    assert_eq!(r["evidenceMode"], "recorded");
    assert!(r["evidenceLabel"].as_str().unwrap().contains("调用方"));
    assert!(r["caveat"].as_str().unwrap().contains("因果"));
}

#[test]
fn attribution_inspection_snapshot_uuid_uniqueness_and_canonical_coverage() {
    let (_, b, p, _) = fixture();
    let mut d = dataset(&p);
    d["products"][0]["id"] = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let mut product = d["products"][0].clone();
    product["id"] = json!("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA");
    product["sku"] = json!("ZZZ");
    d["products"].as_array_mut().unwrap().push(product);
    let mut order = d["input"]["orders"][1].clone();
    order["id"] = json!("ZZZ");
    order["sku"] = json!("ZZZ");
    d["input"]["orders"].as_array_mut().unwrap().push(order);
    let mut bad = p.clone();
    replace_dataset(&mut bad, d.clone());
    rejected(&b, &bad);
    d["products"][1]["id"] = json!(Uuid::new_v4().to_string());
    replace_dataset(&mut bad, d.clone());
    assert!(inspect_attribution_report(&b, &bad).is_ok());
    d["products"].as_array_mut().unwrap().reverse();
    replace_dataset(&mut bad, d.clone());
    rejected(&b, &bad);
    d["products"].as_array_mut().unwrap().reverse();
    d["products"][1]["sku"] = json!("ZZZ-missing");
    replace_dataset(&mut bad, d);
    rejected(&b, &bad);
}

#[test]
fn attribution_inspection_normalized_budget_and_dataset_compact_budget() {
    let (_, b, p, _) = fixture();
    let mut padded = p.clone();
    let original = content(&p, "dataset.json");
    set(
        &mut padded,
        "dataset.json",
        format!("{original}{}", " ".repeat(2 * 1024 * 1024)),
    );
    assert_eq!(
        inspect_attribution_report(&b, &padded).unwrap().report["status"],
        "passed"
    );
    let mut d = dataset(&p);
    let sku = format!("A{}Z", "\u{0001}".repeat(98));
    d["products"][0]["sku"] = json!(sku);
    d["input"]["visits"] = json!([]);
    d["input"]["refunds"] = json!([]);
    let order = d["input"]["orders"][1].clone();
    for count in [600, 1000] {
        d["input"]["orders"] = json!((0..count)
            .map(|i| {
                let mut o = order.clone();
                o["id"] = json!(format!("o{i:04}"));
                o["sku"] = json!(sku);
                o
            })
            .collect::<Vec<_>>());
        d["contentHash"] = json!(fingerprint(
            &json!({"input":d["input"],"products":d["products"]})
        ));
        assert_eq!(d["input"].to_string().len() > 512 * 1024, count == 1000);
        assert_eq!(
            attribution::validate_portable_dataset(&b, &d).is_ok(),
            count == 600
        );
    }
    let mut nul = p.clone();
    let mut s: Value = serde_json::from_str(content(&p, "summary.json")).unwrap();
    s["extra"] = json!("\0");
    set(&mut nul, "summary.json", s.to_string());
    rejected(&b, &nul);
}

#[test]
fn attribution_inspection_fractional_rate_parser_roundtrip_is_exact() {
    let original = json!({"unknownOrderRate": 0.9130434782608695_f64});
    let serialized = serde_json::to_string(&original).unwrap();
    assert_eq!(serialized, "{\"unknownOrderRate\":0.9130434782608695}");
    let parsed = strict_json::parse(&serialized).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn attribution_inspection_fractional_rate_real_core_export_roundtrip() {
    let (c, b, p, _) = fixture();
    let mut input = dataset(&p)["input"].clone();
    let order = input["orders"][1].clone();
    input["orders"] = json!((0..23)
        .map(|i| {
            let mut o = order.clone();
            o["id"] = json!(format!("o{i:02}"));
            o["occurredAt"] = json!("2026-10-19T00:00:00Z");
            o["sessionId"] = if i < 2 {
                json!("PRIVATE_SESSION")
            } else {
                Value::Null
            };
            o
        })
        .collect::<Vec<_>>());
    input["refunds"] = json!([]);
    let mut import = json!({"brandId":b,"name":"fractional roundtrip","content":input.to_string()});
    import["expectedHash"] = attribution::preview(&c, &b, &import).unwrap()["contentHash"].clone();
    let saved = attribution::apply(&c, &b, &import).unwrap();
    let params = json!({"brandId":b,"datasetId":saved["datasetId"],"from":"2026-10-12T00:00:00Z","to":"2026-10-26T00:00:00Z","lookbackDays":7});
    let original = attribution_report::prepare(&c, &b, &params).unwrap();
    assert_eq!(original["summary"]["paidOrders"], 23);
    assert_eq!(original["summary"]["unknownOrders"], 21);
    assert_eq!(
        original["summary"]["unknownOrderRate"],
        0.9130434782608695_f64
    );
    let inspected = inspect_attribution_report(&b, &package(original["files"].clone())).unwrap();
    assert_eq!(inspected.report["status"], "passed");
    assert_eq!(
        inspected.report["summary"]["unknownOrderRate"],
        0.9130434782608695_f64
    );
    let exported = attribution_inspection_export(&inspected.report).unwrap();
    let safe_report =
        strict_json::parse(exported["files"][0]["content"].as_str().unwrap()).unwrap();
    attribution_inspection_export(&safe_report).unwrap();
}
