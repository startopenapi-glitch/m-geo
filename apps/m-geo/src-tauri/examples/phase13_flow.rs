//! Disposable real-core fictional fixture. No models, network or publication.
use m_geo_lib::{
    channel_inspection_session::ChannelInspectionSession, core, files, runtime, site_package,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
const PRIVATE: &str = "P13_PRIVATE_8572";
const INVALID: &str = "历史通道包证据无效、归属不符或超过处理上限";
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
fn content<'a>(package: &'a Value, name: &str) -> &'a str {
    package["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == name)
        .unwrap()["content"]
        .as_str()
        .unwrap()
}
fn replace(package: &mut Value, name: &str, text: String) {
    package["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["path"] == name)
        .unwrap()["content"] = json!(text);
}
fn artifact(dir: &Path, name: &str, package: &Value) -> Value {
    write(dir, name, &files::site_zip(package).unwrap());
    site_package::decode_channel_package(name, &fs::read(dir.join(name)).unwrap()).unwrap()
}
fn seal(value: &mut Value, key: &str) {
    value.as_object_mut().unwrap().remove(key);
    value[key] = json!(digest(&serde_json::to_vec(value).unwrap()));
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
fn schema_hash(db: &Connection) -> String {
    let rows: Vec<(String, String, String, Option<String>)> = db
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    digest(&serde_json::to_vec(&rows).unwrap())
}
fn checks(report: &Value, failed: Option<&str>) {
    assert_eq!(
        report["status"],
        if failed.is_some() {
            "blocked"
        } else {
            "passed"
        }
    );
    assert_eq!(report["checks"].as_array().unwrap().len(), 3);
    for (row, code) in
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .zip(["frozen_evidence", "report", "notice"])
    {
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
fn rejected(brand: &str, package: &Value) {
    assert_eq!(
        core::inspect_channel_report(brand, package).err().unwrap(),
        INVALID
    );
}
fn safe_readback(bytes: &[u8], forbidden: &[String]) -> Value {
    for marker in forbidden {
        assert!(
            !bytes
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "aggregate ZIP leaked private bytes"
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
            assert!(!text.contains(marker), "aggregate leaked {marker}");
        }
        for marker in [
            "\"sku\"",
            "\"productId\"",
            "\"draftId\"",
            "\"description\"",
            "feed-manifest.json",
            "simulated-receipt.json",
        ] {
            assert!(
                !text.contains(marker),
                "aggregate leaked raw field {marker}"
            );
        }
        if f.name() == "inspection.json" {
            report = serde_json::from_str(&text).unwrap();
        }
    }
    names.sort();
    assert_eq!(names, ["inspection.json", "report.md"]);
    assert!(!report.is_null());
    assert!(report.get("rows").is_none());
    report
}
fn review(db: &mut Connection, brand: &Value, product: &Value, label: &str) -> Value {
    let p = call(
        db,
        "product.confirm",
        json!({"brandId":brand,"id":product["id"],"expectedRevision":product["revision"]}),
    );
    let d = call(
        db,
        "draft.generateLocal",
        json!({"brandId":brand,"productId":p["id"],"kind":"description"}),
    );
    let d = call(
        db,
        "draft.save",
        json!({"brandId":brand,"id":d["id"],"expectedRevision":d["revision"],"title":"虚构本地审核说明","body":format!("仅本地虚构验收，不出售、不提交平台。{PRIVATE}；{label}；& <示例> \"引号\" \u{feff}保留实验字符。所有规格均为虚构。") }),
    );
    let d = call(
        db,
        "draft.review",
        json!({"brandId":brand,"id":d["id"],"expectedRevision":d["revision"]}),
    );
    json!({"productId":p["id"],"expectedRevision":p["revision"],"draftId":d["id"],"expectedDraftRevision":d["revision"]})
}
fn product(db: &mut Connection, brand: &Value, n: usize, sku: &str) -> Value {
    let facts: Vec<Value> = [("catalog:kind","standalone"),("catalog:condition","new"),("catalog:productClass","physical_non_apparel"),("catalog:titleOrigin","human"),("catalog:brand","虚构Phase13制造品牌"),("catalog:mpn","FICTIONAL-NOT-ASSIGNED"),("commerce:price","29.90"),("commerce:currency","CNY"),("commerce:availability","InStock"),("commerce:purchase","https://example.com/P13_PRIVATE_8572/item"),("commerce:image","https://example.com/P13_PRIVATE_8572/image.png")].iter().map(|(k,v)|json!({"id":k,"label":k,"value":v,"source":"虚构本地验收依据","visibility":"public","validUntil":"2099-12-31"})).collect();
    call(
        db,
        "product.save",
        json!({"brandId":brand,"name":format!("虚构商品通道台灯{n:02}"),"sku":sku,"category":"虚构台灯","officialUrl":"","sourceText":PRIVATE,"privateNotes":PRIVATE,"facts":facts}),
    )
}
fn legacy(root: &Path) -> (Vec<(PathBuf, Vec<u8>)>, Value) {
    let mut originals = vec![];
    let mut evidence = vec![];
    for (name, operation, expected) in [
        (
            "final-native-comparison.zip",
            "comparison",
            json!({"added":0,"removed":0,"changed":0,"unchanged":1}),
        ),
        (
            "native-simulation.zip",
            "simulation",
            json!({"accepted":1,"rejected":0,"unknown":0,"missing":0}),
        ),
        (
            "native-receipt-check.zip",
            "receipt-check",
            json!({"accepted":1,"rejected":0,"unknown":0,"missing":0}),
        ),
    ] {
        let path = root
            .join("mgeo-e2e-phase4-20261023/native-exports")
            .join(name);
        let bytes = fs::read(&path)
            .unwrap_or_else(|e| panic!("mandatory actual legacy fixture {}: {e}", path.display()));
        let decoded = site_package::decode_channel_package(name, &bytes).unwrap();
        let manifest: Value =
            serde_json::from_str(content(&decoded, "feed-manifest.json")).unwrap();
        let inspected =
            core::inspect_channel_report(manifest["brandId"].as_str().unwrap(), &decoded).unwrap();
        checks(&inspected.report, None);
        assert_eq!(inspected.report["operation"], operation);
        assert_eq!(inspected.report["summary"], expected);
        assert_eq!(inspected.report["counts"]["selected"], 1);
        assert_eq!(inspected.rows.len(), 1);
        evidence.push(json!({"path":path,"sha256":digest(&bytes),"operation":operation,"selected":1,"rows":1,"expectedSummary":expected,"summary":inspected.report["summary"],"status":"passed"}));
        originals.push((path, bytes));
    }
    let path = root.join("mgeo-e2e-phase10-20261024/core-flow/normal-basic-xml-feed.zip");
    let bytes = fs::read(&path)
        .unwrap_or_else(|e| panic!("mandatory actual Phase10 fixture {}: {e}", path.display()));
    assert!(site_package::decode_channel_package("normal-basic-xml-feed.zip", &bytes).is_err());
    evidence.push(json!({"path":path,"sha256":digest(&bytes),"expected":"unsupported-original-xml-feed","status":"rejected"}));
    originals.push((path, bytes));
    (originals, json!(evidence))
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!((2..=3).contains(&args.len()),"usage: phase13_flow NEW_ABSOLUTE_OUTPUT [ABSOLUTE_LEGACY_WORK_ROOT]; or MGEO_PHASE13_LEGACY_ROOT");
    let output = PathBuf::from(&args[1]);
    let legacy_root = PathBuf::from(
        args.get(2)
            .cloned()
            .or_else(|| std::env::var("MGEO_PHASE13_LEGACY_ROOT").ok())
            .expect("explicit legacy root required; fixtures must never silently skip"),
    );
    assert!(output.is_absolute() && legacy_root.is_absolute());
    assert!(legacy_root.is_dir());
    assert!(!output.exists(), "refuse to overwrite fixture output");
    let (mut originals, historical) = legacy(&legacy_root);
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    write(&output, "fictional.sqlite3", &[]);
    let path = output.join("fictional.sqlite3");
    let mut db = runtime::open_database(&path).unwrap();
    let brand = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构Phase13历史通道实验室（仅本地模拟）"}),
    )["id"]
        .clone();
    let foreign = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构Phase13隔离品牌（仅本地模拟）"}),
    )["id"]
        .clone();
    assert_eq!(call(&mut db, "brand.list", json!({}))[0]["id"], brand);
    let mut products = vec![];
    let mut selections = vec![];
    for n in 0..21 {
        let p = product(&mut db, &brand, n, &format!("P13-FICTIONAL-{n:02}"));
        selections.push(review(&mut db, &brand, &p, "基线审核说明"));
        products.push(p);
    }
    let profiles = ["mgeo-catalog-v1", "google-merchant-basic-v1"];
    let mut baselines = vec![];
    for profile in profiles {
        let base = call(
            &mut db,
            "feed.prepare",
            json!({"brandId":brand,"profile":profile,"items":selections}),
        );
        assert_eq!(base["summary"]["ready"], 21);
        assert_eq!(base["summary"]["blocked"], 0);
        baselines.push(base["manifest"].clone());
    }
    // Reuse a SKU across old/new snapshots while preserving distinct real product IDs.
    let reused = products[0]["sku"].as_str().unwrap().to_owned();
    let mut retired = products[0].clone();
    retired["expectedRevision"] = retired["revision"].clone();
    retired["sku"] = json!("P13-FICTIONAL-RETIRED");
    let retired = call(&mut db, "product.save", retired);
    review(&mut db, &brand, &retired, "移出选择后SKU已退役");
    products[0] = retired;
    let mut changed = products[1].clone();
    changed["expectedRevision"] = changed["revision"].clone();
    changed["facts"][6]["value"] = json!("39.90");
    let changed = call(&mut db, "product.save", changed);
    selections[1] = review(&mut db, &brand, &changed, "价格改为39.90后的重新审核说明");
    products[1] = changed;
    let added = product(&mut db, &brand, 21, &reused);
    let added_selection = review(&mut db, &brand, &added, "新增商品审核说明");
    products.push(added);
    selections.remove(0);
    selections.push(added_selection);
    let mut packages: Vec<(String, Value, Value)> = vec![];
    let mut receipts = vec![];
    let comparison_summary = json!({"added":1,"removed":1,"changed":1,"unchanged":19});
    for (i, profile) in profiles.iter().enumerate() {
        let prefix = if i == 0 { "" } else { "google-" };
        let request = json!({"brandId":brand,"profile":profile,"items":selections});
        let current = call(&mut db, "feed.prepare", request.clone());
        assert_eq!(current["summary"]["ready"], 21);
        let mut compare = request.clone();
        compare["baseline"] = json!(baselines[i].to_string());
        let comparison = call(&mut db, "feed.compare", compare);
        assert_eq!(comparison["summary"], comparison_summary);
        packages.push((
            format!("{prefix}comparison.zip"),
            comparison,
            comparison_summary.clone(),
        ));
        for scenario in ["accepted", "rejected", "unknown"] {
            let mut simulate = request.clone();
            simulate["scenario"] = json!(scenario);
            let simulated = call(&mut db, "feed.simulate", simulate);
            let mut expected = json!({"accepted":0,"rejected":0,"unknown":0,"missing":0});
            expected[scenario] = json!(21);
            assert_eq!(simulated["summary"], expected);
            if scenario == "accepted" {
                receipts.push(simulated["receipt"].clone());
            }
            packages.push((
                format!("{prefix}{scenario}-simulation.zip"),
                simulated,
                expected,
            ));
        }
        let mut receipt = receipts[i].clone();
        receipt["rows"].as_array_mut().unwrap().pop();
        receipt["rows"][0]["status"] = json!("unknown");
        receipt["rows"][0]["code"] = json!("SIMULATED_UNKNOWN");
        receipt["rows"][0]["message"] = json!(format!("{PRIVATE} 虚构未知必须保持未知"));
        receipt["rows"][1]["status"] = json!("rejected");
        receipt["rows"][1]["code"] = json!("SIMULATED_POLICY_REJECTION");
        receipt["rows"][1]["message"] = json!("虚构本地拒绝");
        let mut check = request.clone();
        check["content"] = json!(receipt.to_string());
        let checked = call(&mut db, "feed.checkReceipt", check);
        let expected = json!({"accepted":18,"rejected":1,"unknown":1,"missing":1});
        assert_eq!(checked["summary"], expected);
        packages.push((format!("{prefix}receipt-check.zip"), checked, expected));
        let mut empty = receipts[i].clone();
        empty["rows"] = json!([]);
        let mut check = request;
        check["content"] = json!(empty.to_string());
        packages.push((
            format!("{prefix}empty-receipt-check.zip"),
            call(&mut db, "feed.checkReceipt", check),
            json!({"accepted":0,"rejected":0,"unknown":0,"missing":21}),
        ));
    }
    // A coherent historical comparison may contain locally blocked selections.
    // Build both frozen snapshots through the live core: one unreviewed local
    // description in the baseline, then a second in the current selection.
    let mut blocked_selections = selections.clone();
    let mut blocked_baseline = Value::Null;
    for index in 0..2 {
        let draft = call(
            &mut db,
            "draft.generateLocal",
            json!({"brandId":brand,"productId":blocked_selections[index]["productId"],"kind":"description"}),
        );
        let draft = call(
            &mut db,
            "draft.save",
            json!({"brandId":brand,"id":draft["id"],"expectedRevision":draft["revision"],"title":"虚构待审核本地说明","body":"仅本地虚构验收；此说明故意保持未审核，历史清单必须保留本地阻止状态。"}),
        );
        assert!(draft["reviewedAt"].is_null());
        blocked_selections[index]["draftId"] = draft["id"].clone();
        blocked_selections[index]["expectedDraftRevision"] = draft["revision"].clone();
        let prepared = call(
            &mut db,
            "feed.prepare",
            json!({"brandId":brand,"profile":"mgeo-catalog-v1","items":blocked_selections}),
        );
        assert_eq!(prepared["summary"]["ready"], 20 - index);
        assert_eq!(prepared["summary"]["blocked"], index + 1);
        if index == 0 {
            blocked_baseline = prepared["manifest"].clone();
        }
    }
    let blocked_comparison = call(
        &mut db,
        "feed.compare",
        json!({"brandId":brand,"profile":"mgeo-catalog-v1","items":blocked_selections,"baseline":blocked_baseline.to_string()}),
    );
    let blocked_summary = json!({"added":0,"removed":0,"changed":1,"unchanged":20});
    assert_eq!(blocked_comparison["summary"], blocked_summary);
    let blocked_counts =
        json!({"selected":21,"baselineSelected":21,"rows":21,"ready":19,"blocked":2});
    packages.push((
        "blocked-items-comparison.zip".into(),
        blocked_comparison,
        blocked_summary.clone(),
    ));
    let mut forbidden = vec![PRIVATE.to_owned(), reused.clone()];
    forbidden.extend(
        products
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned()),
    );
    forbidden.extend(
        selections
            .iter()
            .map(|p| p["draftId"].as_str().unwrap().to_owned()),
    );
    let mut frozen = vec![];
    for (name, package, expected) in &packages {
        let decoded = artifact(&output, name, package);
        let inspected = core::inspect_channel_report(brand.as_str().unwrap(), &decoded).unwrap();
        checks(&inspected.report, None);
        assert_eq!(&inspected.report["summary"], expected);
        if name == "blocked-items-comparison.zip" {
            assert_eq!(inspected.report["counts"], blocked_counts);
            assert_eq!(inspected.report["summary"], blocked_summary);
            assert_eq!(inspected.rows.len(), 21);
            let changed = inspected
                .rows
                .iter()
                .filter(|row| row["change"] == "changed")
                .collect::<Vec<_>>();
            assert_eq!(changed.len(), 1);
            assert!(changed[0]["fields"]
                .as_array()
                .unwrap()
                .contains(&json!("localStatus")));
            assert_eq!(
                inspected
                    .rows
                    .iter()
                    .filter(|row| row["change"] == "unchanged")
                    .count(),
                20
            );
            let baseline: Value =
                serde_json::from_str(content(&decoded, "baseline-manifest.json")).unwrap();
            assert_eq!(
                baseline["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["localStatus"] == "blocked")
                    .count(),
                1
            );
        } else if name.ends_with("comparison.zip") {
            assert_eq!(inspected.rows.len(), 22);
            assert_eq!(
                inspected.rows.iter().filter(|r| r["sku"] == reused).count(),
                2
            );
            let row = inspected
                .rows
                .iter()
                .find(|r| r["change"] == "changed")
                .unwrap();
            assert_eq!(
                row["fields"],
                json!(["description", "draftId", "price", "productRevision"])
            );
            assert_eq!(
                inspected
                    .rows
                    .iter()
                    .filter(|r| r["change"] == "unchanged")
                    .count(),
                19
            );
        } else {
            assert_eq!(inspected.rows.len(), 21);
        }
        frozen.push((name.clone(), decoded, inspected.report, inspected.rows));
    }
    save(
        &output,
        "fixture-expectations.json",
        &json!({"brandId":brand,"foreignBrandId":foreign,"products":22,"selected":21,"comparisonSummary":comparison_summary,"reusedSku":reused,"changedFields":["description","draftId","price","productRevision"],"receiptSummary":{"accepted":18,"rejected":1,"unknown":1,"missing":1}}),
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
    assert_eq!(tables_before.as_object().unwrap().len(), 18);
    let schema_before = schema_hash(&db);
    let schema = db
        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
        .unwrap();
    drop(db);
    let db_before = fs::read(&path).unwrap();
    let hidden = output.join("fictional.sqlite3.unavailable");
    fs::rename(&path, &hidden).unwrap();
    assert!(!path.exists());
    // Below this boundary no database initializer or core dispatch may run.
    for (name, _, _) in &packages {
        originals.push((output.join(name), fs::read(output.join(name)).unwrap()));
    }
    let b = brand.as_str().unwrap();
    let f = foreign.as_str().unwrap();
    let mut summaries = vec![];
    let mut session = ChannelInspectionSession::default();
    for (name, decoded, report, rows) in &frozen {
        let replay =
            site_package::decode_channel_package(name, &fs::read(output.join(name)).unwrap())
                .unwrap();
        assert!(!path.exists());
        let inspected = core::inspect_channel_report(b, &replay).unwrap();
        if name == "blocked-items-comparison.zip" {
            checks(&inspected.report, None);
            assert_eq!(inspected.report["counts"], blocked_counts);
            assert_eq!(inspected.report["summary"], blocked_summary);
            assert_eq!(inspected.rows, *rows);
        }
        assert_eq!(&inspected.report, report);
        assert_eq!(&inspected.rows, rows);
        let generation = session.begin_selection();
        let selected = session.complete_selection(generation, b, replay).unwrap();
        let ticket = selected["inspectionId"].as_str().unwrap();
        let fingerprint = report["reportFingerprint"].as_str().unwrap();
        let first = session.read_page(b, ticket, 0).unwrap();
        let second = session.read_page(b, ticket, 1).unwrap();
        assert_eq!(first["pageSize"], 20);
        assert_eq!(first["rows"].as_array().unwrap().len(), 20);
        assert_eq!(second["rows"].as_array().unwrap().len(), rows.len() - 20);
        let combined: Vec<Value> = first["rows"]
            .as_array()
            .unwrap()
            .iter()
            .chain(second["rows"].as_array().unwrap())
            .cloned()
            .collect();
        assert_eq!(&combined, rows);
        assert!(session.read_page(b, ticket, 2).is_err());
        assert!(session.read_page(b, ticket, usize::MAX).is_err());
        assert!(session.read_page(f, ticket, 0).is_err());
        assert!(session.prepare_export(f, ticket, fingerprint).is_err());
        assert!(session.prepare_export(b, ticket, &"0".repeat(64)).is_err());
        assert!(!path.exists());
        let safe = session.prepare_export(b, ticket, fingerprint).unwrap();
        let export_name = format!("{}-inspection.zip", name.trim_end_matches(".zip"));
        write(&output, &export_name, &files::site_zip(&safe).unwrap());
        assert_eq!(
            safe_readback(&fs::read(output.join(&export_name)).unwrap(), &forbidden),
            *report
        );
        summaries.push(json!({"path":output.join(name),"inspectionPath":output.join(export_name),"report":report,"pages":[20,rows.len()-20],"sourceDatabaseRequired":false}));
        assert_eq!(
            core::inspect_channel_report(b, decoded).unwrap().rows,
            *rows
        );
    }
    let original = packages[0].1.clone();
    let frozen_comparison = &frozen[0].1;
    rejected(f, frozen_comparison);
    let mut tamper_checks = json!({});
    for (name, target, code) in [
        ("tampered-summary.zip", "comparison-report.json", "report"),
        (
            "tampered-notice.zip",
            "local-operation-notice.txt",
            "notice",
        ),
    ] {
        let mut package = original.clone();
        let replacement = if code == "report" {
            let mut report: Value = serde_json::from_str(content(&package, target)).unwrap();
            report["summary"]["added"] = json!(999);
            report.to_string()
        } else {
            format!("{}altered", content(&package, target))
        };
        replace(&mut package, target, replacement);
        let decoded = artifact(&output, name, &package);
        let blocked = core::inspect_channel_report(b, &decoded).unwrap();
        checks(&blocked.report, Some(code));
        assert_eq!(blocked.rows, frozen[0].3);
        assert_eq!(blocked.report["summary"], comparison_summary);
        let safe = core::channel_inspection_export(&blocked.report).unwrap();
        assert_eq!(
            safe_readback(&files::site_zip(&safe).unwrap(), &forbidden),
            blocked.report
        );
        tamper_checks[code] = blocked.report["checks"].clone();
    }
    let mut negative = vec![];
    for kind in ["hash", "foreign", "profile", "duplicate-row"] {
        let mut package = original.clone();
        let mut manifest: Value =
            serde_json::from_str(content(&package, "feed-manifest.json")).unwrap();
        match kind {
            "hash" => manifest["packageId"] = json!("0".repeat(64)),
            "foreign" => {
                manifest["brandId"] = foreign.clone();
                seal(&mut manifest, "packageId");
            }
            "profile" => {
                manifest["profile"] = json!("google-merchant-basic-v1");
                seal(&mut manifest, "packageId");
            }
            _ => {
                let row = manifest["items"][0].clone();
                manifest["items"].as_array_mut().unwrap().push(row);
                seal(&mut manifest, "packageId");
            }
        }
        replace(&mut package, "feed-manifest.json", manifest.to_string());
        let name = format!("invalid-{kind}.zip");
        let decoded = artifact(&output, &name, &package);
        rejected(b, &decoded);
        negative.push(name);
    }
    for target in [
        "comparison-report.json",
        "baseline-manifest.json",
        "feed-manifest.json",
    ] {
        let mut package = original.clone();
        let parsed: Value = serde_json::from_str(content(&package, target)).unwrap();
        let (key, value) = parsed.as_object().unwrap().iter().next().unwrap();
        let duplicate = format!(
            "{{{}:{},{}",
            serde_json::to_string(key).unwrap(),
            value,
            &content(&package, target)[1..]
        );
        replace(&mut package, target, duplicate);
        let name = format!("invalid-duplicate-{target}.zip");
        let decoded = artifact(&output, &name, &package);
        rejected(b, &decoded);
        negative.push(name);
    }
    let simulation = &packages[1].1;
    for kind in [
        "partial-simulation",
        "mixed-simulation",
        "arbitrary-message-simulation",
        "receipt-rowhash",
        "receipt-unknown-sku",
        "receipt-duplicate",
        "receipt-foreign",
    ] {
        let mut package = simulation.clone();
        let mut receipt: Value =
            serde_json::from_str(content(&package, "simulated-receipt.json")).unwrap();
        match kind {
            "partial-simulation" => {
                receipt["rows"].as_array_mut().unwrap().pop();
            }
            "mixed-simulation" => {
                receipt["rows"][0]["status"] = json!("unknown");
                receipt["rows"][0]["code"] = json!("SIMULATED_UNKNOWN");
                receipt["rows"][0]["message"] = json!("本机模拟结果未知，不能推定成功或失败");
            }
            "arbitrary-message-simulation" => {
                receipt["rows"][0]["message"] = json!("arbitrary fictional message")
            }
            "receipt-rowhash" => receipt["rows"][0]["rowHash"] = json!("0".repeat(64)),
            "receipt-unknown-sku" => receipt["rows"][0]["sku"] = json!("UNSELECTED-FICTIONAL-SKU"),
            "receipt-duplicate" => {
                let row = receipt["rows"][0].clone();
                receipt["rows"].as_array_mut().unwrap().push(row);
            }
            _ => receipt["brandId"] = foreign.clone(),
        }
        replace(&mut package, "simulated-receipt.json", receipt.to_string());
        let name = format!("invalid-{kind}.zip");
        let decoded = artifact(&output, &name, &package);
        rejected(b, &decoded);
        negative.push(name);
    }
    let mut mixed = original.clone();
    mixed["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["path"] == "baseline-manifest.json")
        .unwrap()["path"] = json!("simulated-receipt.json");
    write(
        &output,
        "invalid-mixed-set.zip",
        &files::site_zip(&mixed).unwrap(),
    );
    assert!(site_package::decode_channel_package(
        "invalid-mixed-set.zip",
        &fs::read(output.join("invalid-mixed-set.zip")).unwrap()
    )
    .is_err());
    negative.push("invalid-mixed-set.zip".into());
    let mut forged = frozen[0].2.clone();
    forged["summary"]["added"] = json!(999);
    assert!(core::channel_inspection_export(&forged).is_err());
    let generation = session.begin_selection();
    let selection = session
        .complete_selection(generation, b, frozen_comparison.clone())
        .unwrap();
    let ticket = selection["inspectionId"].as_str().unwrap();
    session.begin_selection();
    assert!(session.read_page(b, ticket, 0).is_err());
    assert!(ChannelInspectionSession::default()
        .read_page(b, ticket, 0)
        .is_err());
    let old = session.begin_selection();
    let latest = session.begin_selection();
    assert!(session
        .complete_selection(old, b, frozen_comparison.clone())
        .is_err());
    assert!(session
        .complete_selection(latest, f, frozen_comparison.clone())
        .is_err());
    assert!(session.read_page(b, ticket, 0).is_err());
    assert!(!path.exists());
    for (source, bytes) in &originals {
        assert_eq!(
            fs::read(source).unwrap(),
            *bytes,
            "source ZIP changed: {}",
            source.display()
        );
    }
    fs::rename(&hidden, &path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), db_before);
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tables_after = table_hashes(&db);
    let schema_after = schema_hash(&db);
    assert_eq!(tables_before, tables_after);
    assert_eq!(schema_before, schema_after);
    core::validate_database(&db).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        schema
    );
    drop(db);
    let before_hash = digest(&db_before);
    let after_hash = digest(&fs::read(&path).unwrap());
    assert_eq!(before_hash, after_hash);
    let mut artifacts = json!({});
    let mut entries: Vec<_> = fs::read_dir(&output).unwrap().map(Result::unwrap).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let bytes = fs::read(entry.path()).unwrap();
        artifacts[entry.file_name().to_str().unwrap()] =
            json!({"path":entry.path(),"bytes":bytes.len(),"sha256":digest(&bytes)});
    }
    save(
        &output,
        "acceptance.json",
        &json!({"status":"passed","fixture":"fictional-local-simulated-only","nativeAcceptance":"pending-controller","brandId":brand,"selectedBrandId":brand,"foreignBrandId":foreign,"products":22,"selected":21,"blockedItemsComparison":{"counts":blocked_counts,"summary":blocked_summary,"baselineReady":20,"baselineBlocked":1,"inspectionStatus":"passed"},"schema":schema,"historicalFixtures":historical,"inspections":summaries,"tamperChecks":tamper_checks,"rejectedFixtures":negative,"readonly":{"sourceDatabaseUnavailable":true,"databaseSha256Before":before_hash,"databaseSha256After":after_hash,"schemaSha256Before":schema_before,"schemaSha256After":schema_after,"allTableHashesBefore":tables_before,"allTableHashesAfter":tables_after},"artifacts":artifacts,"modelJobs":0,"networkCalls":0,"platformSubmissions":0,"publications":0,"checks":["mandatory-actual-three-phase4-native-zips-and-phase10-xml-unsupported","22-fresh-products-reviewed-local-descriptions-both-profiles","comparison-four-classes-reused-sku-and-exact-changed-fields","valid-blocked-items-baseline1-current2-preserved-and-exported-db-unavailable","accepted-rejected-unknown-strict-full-simulation","mixed-partial-missing-and-empty-receipt-check","22-row-comparison-pages20plus2-receipt-pages20plus1","closed-source-db-unavailable-portable-replay","18-table-schema-and-final-db-byte-hashes-unchanged","raw-source-zip-bytes-unchanged","summary-notice-blocked-with-recomputed-rows","hash-brand-profile-duplicate-json-mixed-set-invalid-rejected","strict-simulation-partial-mixed-arbitrary-message-rejected","aggregate-export-file-readback-no-private-markers","cancel-failure-late-selection-restart-session-invalidated"]}),
    );
    println!("Phase13 acceptance passed: 22 fresh fictional products; brand={b}; both profiles; comparison1added/1removed/1changed/19unchanged; receipt18accepted/1rejected/1unknown/1missing; pages20+2 and20+1; actual3Phase4ZIPs; actualPhase10XMLrejected; sourceDBunavailable;18tables/schema/DBbytes/sourceZIPsunchanged. Native acceptance pending controller. Output: {}",output.display());
}
