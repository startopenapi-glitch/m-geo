//! Disposable fictional identity acceptance; no network, model, or publishing actions.
use m_geo_lib::{core, files, runtime, site_package};
use quick_xml::{events::Event, Reader};
use rusqlite::{types::ValueRef, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const STORE: &str = "虚构多品牌小店（仅本地验收）";
const SENTINEL: &str = "PHASE10_PRIVATE_SENTINEL";
const MAKERS: [&str; 3] = [
    "FICTIONAL Harbor Works",
    "FICTIONAL Harbor Works",
    "FICTIONAL Meadow Lab",
];
const NAMES: [&str; 3] = [
    "虚构晨光杯（非在售）",
    "虚构午后盒（非在售）",
    "虚构晚风托盘（非在售）",
];
fn call(db: &mut Connection, action: &str, payload: Value) -> Value {
    runtime::core_dispatch(db, action, payload).unwrap_or_else(|e| panic!("{action}: {e}"))
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
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn artifact(dir: &Path, name: &str, package: &Value) -> Value {
    write(dir, name, &files::site_zip(package).unwrap());
    site_package::decode_site_package(name, &fs::read(dir.join(name)).unwrap()).unwrap()
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
fn rehash(package: &mut Value) {
    let mut manifest: Value =
        serde_json::from_str(content(package, "release-manifest.json")).unwrap();
    for row in manifest["files"].as_array_mut().unwrap() {
        row["sha256"] = json!(digest(
            content(package, row["path"].as_str().unwrap()).as_bytes()
        ));
    }
    replace(
        package,
        "release-manifest.json",
        serde_json::to_string_pretty(&manifest).unwrap(),
    );
}
fn product_data(html: &str) -> Option<Value> {
    html.split_once("<script type=\"application/ld+json\">")
        .map(|(_, s)| serde_json::from_str(s.split_once("</script>").unwrap().0).unwrap())
}
fn parsed_products(package: &Value) -> BTreeMap<String, Value> {
    package["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["path"].as_str().unwrap().ends_with(".html"))
        .filter_map(|f| product_data(f["content"].as_str().unwrap()))
        .filter(|v| v["@type"] == "Product")
        .map(|v| (v["sku"].as_str().unwrap().to_owned(), v))
        .collect()
}
fn assert_identity(package: &Value, brands: &[&str; 3]) {
    let products = parsed_products(package);
    assert_eq!(products.len(), 3);
    for i in 0..3 {
        let p = &products[&format!("FICTIONAL-P10-{}", i + 1)];
        assert_eq!(p["name"], NAMES[i]);
        assert_eq!(
            p["brand"],
            json!({"@type":"Brand","name":brands[i]}),
            "website must use manufacturer identity, never workspace name"
        );
        assert_ne!(p["brand"]["name"], STORE);
    }
}
fn parsed_feed(xml: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut rows = BTreeMap::new();
    let mut item = None::<BTreeMap<String, String>>;
    let mut tag = String::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                tag = String::from_utf8(e.name().as_ref().to_vec()).unwrap();
                if tag == "item" {
                    item = Some(BTreeMap::new());
                }
            }
            Event::Text(e) => {
                if let Some(row) = item.as_mut() {
                    row.entry(tag.clone())
                        .or_default()
                        .push_str(&e.unescape().unwrap());
                }
            }
            Event::End(e) if e.name().as_ref() == b"item" => {
                let row = item.take().unwrap();
                rows.insert(row["g:id"].clone(), row);
            }
            Event::Eof => break,
            _ => (),
        }
    }
    rows
}
fn assert_feed(package: &Value, brands: &[&str; 3]) {
    assert_eq!(package["summary"]["ready"], 3, "{package}");
    assert_eq!(package["summary"]["blocked"], 0);
    let xml = package["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"].as_str().unwrap().ends_with(".xml"))
        .unwrap()["content"]
        .as_str()
        .unwrap();
    let rows = parsed_feed(xml);
    assert_eq!(rows.len(), 3);
    for i in 0..3 {
        let row = &rows[&format!("FICTIONAL-P10-{}", i + 1)];
        assert_eq!(row["g:brand"], brands[i]);
        assert_eq!(row["g:title"], NAMES[i]);
        assert_ne!(row["g:brand"], STORE);
    }
}
fn fact(id: &str, value: &str) -> Value {
    json!({"id":id,"label":id,"value":value,"source":"Fictional Phase10 specification; not a real manufacturer claim or offer","visibility":"public","validUntil":"2099-12-31"})
}
fn reviewed(db: &mut Connection, b: &Value, p: &Value, q: &Value) -> (Vec<Value>, Value) {
    let mut refs = vec![];
    let mut selection = Value::Null;
    for kind in ["description", "faq", "guide", "comparison"] {
        let d = call(
            db,
            "draft.generateLocal",
            json!({"brandId":b,"productId":p["id"],"questionId":q["id"],"kind":kind}),
        );
        assert_eq!(d["generationMode"], "local");
        let d = call(
            db,
            "draft.save",
            json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":format!("【虚构资料，仅本地软件流程验收，非在售】\n{}。所有品牌、规格与购买链接均为虚构占位，不代表真实厂商或平台认可。",p["name"].as_str().unwrap())}),
        );
        let d = call(
            db,
            "draft.review",
            json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
        );
        assert!(!d["reviewedAt"].is_null());
        refs.push(json!({"id":d["id"],"expectedRevision":d["revision"]}));
        if kind == "description" {
            selection = json!({"productId":p["id"],"expectedRevision":p["revision"],"draftId":d["id"],"expectedDraftRevision":d["revision"]});
        }
    }
    (refs, selection)
}
fn confirmed(db: &mut Connection, b: &Value, mut p: Value) -> Value {
    p["brandId"] = b.clone();
    if !p["id"].is_null() {
        p["expectedRevision"] = p["revision"].clone();
    }
    let p = call(db, "product.save", p);
    assert!(p["confirmedAt"].is_null());
    call(
        db,
        "product.confirm",
        json!({"brandId":b,"id":p["id"],"expectedRevision":p["revision"]}),
    )
}
fn logical_snapshot(db: &Connection) -> Value {
    let tables: Vec<String> = db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    let mut snapshot = json!({});
    for name in tables {
        assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
        let mut statement = db.prepare(&format!("SELECT * FROM {name}")).unwrap();
        let n = statement.column_count();
        let mut rows: Vec<String> = statement
            .query_map([], |r| {
                let values: Vec<Value> = (0..n)
                    .map(|i| match r.get_ref(i).unwrap() {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(v) => json!(v),
                        ValueRef::Real(v) => json!(v),
                        ValueRef::Text(v) => json!({"text":String::from_utf8(v.to_vec()).unwrap()}),
                        ValueRef::Blob(v) => json!({"blob":hex::encode(v)}),
                    })
                    .collect();
                Ok(serde_json::to_string(&values).unwrap())
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows.sort();
        snapshot[name] = json!(rows);
    }
    snapshot
}
fn has(report: &Value, code: &str, status: &str) -> bool {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["code"] == code && c["status"] == status)
}
fn inspect(db: &Connection, b: &Value, output: &Path, name: &str, package: &Value) -> Value {
    let before = logical_snapshot(db);
    let db_path = Path::new(db.path().unwrap());
    let before_digest = digest(&fs::read(db_path).unwrap());
    let report = core::inspect_site_package(db, b.as_str().unwrap(), package).unwrap();
    assert!(!report.to_string().contains(SENTINEL));
    save(output, &format!("{name}-inspection.json"), &report);
    artifact(
        output,
        &format!("{name}-inspection-report.zip"),
        &core::export_site_inspection(&report).unwrap(),
    );
    assert_eq!(
        logical_snapshot(db),
        before,
        "inspection/export changed logical tables"
    );
    assert_eq!(
        digest(&fs::read(db_path).unwrap()),
        before_digest,
        "inspection/export changed DB bytes"
    );
    report
}
fn no_side_effects(db: &Connection) {
    for table in ["m_publications", "ai_jobs"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        2,
        "one NEW output directory containing mgeo-e2e- required"
    );
    let output = Path::new(&args[1]);
    assert!(output.to_string_lossy().contains("mgeo-e2e-"));
    assert!(
        !output.exists(),
        "refuse to overwrite an existing output directory or database"
    );
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(output).unwrap();
    let path = output.join("fictional.sqlite3");
    assert!(!path.exists());
    let mut db = runtime::open_database(&path).unwrap();
    let b = call(&mut db, "brand.create", json!({"name":STORE}))["id"].clone();
    let mut products = vec![];
    let mut questions = vec![];
    let mut groups = vec![];
    let mut selections = vec![];
    for i in 0..3 {
        let mut facts: Vec<Value> = [
            ("catalog:brand", MAKERS[i]),
            ("catalog:kind", "standalone"),
            ("catalog:condition", "new"),
            ("catalog:productClass", "physical_non_apparel"),
            ("catalog:titleOrigin", "human"),
            ("commerce:price", "29.90"),
            ("commerce:currency", "CNY"),
            ("commerce:availability", "InStock"),
            ("commerce:audience", "虚构成年人测试场景"),
            ("commerce:scenario", "虚构桌面收纳"),
            ("commerce:limits", "仅限本地流程验收，不出售"),
            ("commerce:afterSales", "虚构测试资料，无交易与售后承诺"),
        ]
        .iter()
        .map(|(k, v)| fact(k, v))
        .collect();
        facts.push(fact(
            "catalog:mpn",
            &format!("FICTIONAL-NOT-ASSIGNED-P10-{}", i + 1),
        ));
        facts.push(fact(
            "commerce:purchase",
            &format!("https://fictional.example/item/{}", i + 1),
        ));
        facts.push(fact(
            "commerce:image",
            &format!("https://fictional.example/images/{}.png", i + 1),
        ));
        facts.push(json!({"id":"private:internal","label":"私密备注","value":SENTINEL,"source":SENTINEL,"visibility":"private","validUntil":null}));
        let p = confirmed(
            &mut db,
            &b,
            json!({"name":NAMES[i],"sku":format!("FICTIONAL-P10-{}",i+1),"category":"虚构日用品","officialUrl":"","sourceText":SENTINEL,"privateNotes":SENTINEL,"facts":facts}),
        );
        let q = call(
            &mut db,
            "question.save",
            json!({"brandId":b,"productId":p["id"],"text":format!("虚构验收：{}适合什么使用场景？",NAMES[i]),"intent":"usage","origin":"customer","source":"虚构问题，仅本地模拟","branded":false,"confirmed":false}),
        );
        let q = call(
            &mut db,
            "question.confirm",
            json!({"brandId":b,"id":q["id"],"confirmed":true}),
        );
        let (refs, selection) = reviewed(&mut db, &b, &p, &q);
        products.push(p);
        questions.push(q);
        groups.push(refs);
        selections.push(selection);
    }
    let site_request = |groups: &Vec<Vec<Value>>| json!({"brandId":b,"baseUrl":"","drafts":groups.iter().flatten().cloned().collect::<Vec<_>>()});
    let site = call(&mut db, "site.prepare", site_request(&groups));
    let normal = artifact(output, "normal-website.zip", &site);
    assert_identity(&normal, &MAKERS);
    assert!(!site.to_string().contains(SENTINEL));
    let normal_bytes = fs::read(output.join("normal-website.zip")).unwrap();
    let normal_digest = digest(&normal_bytes);
    let feed = call(
        &mut db,
        "feed.prepare",
        json!({"brandId":b,"profile":"google-merchant-basic-v1","items":selections}),
    );
    assert_feed(&feed, &MAKERS);
    assert!(!feed.to_string().contains(SENTINEL));
    artifact(output, "normal-basic-xml-feed.zip", &feed);
    let before = logical_snapshot(&db);
    let before_digest = digest(&fs::read(&path).unwrap());
    let report = inspect(&db, &b, output, "normal", &normal);
    assert_eq!(report["packageStatus"], "pass", "{report}");
    assert!(has(&report, "product-brand", "pass"));
    // Rehash after changing only the HTML declaration: integrity is valid, semantics are not.
    let mut wrong = normal.clone();
    let row = wrong["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
        .unwrap();
    let html = row["content"].as_str().unwrap();
    let mut data = product_data(html).unwrap();
    let old = data.to_string();
    data["brand"] = json!({"@type":"Brand","name":STORE});
    row["content"] = json!(html.replace(&old, &data.to_string()));
    rehash(&mut wrong);
    let wrong = artifact(output, "wrong-brand-rehashed.zip", &wrong);
    assert!(parsed_products(&wrong)
        .values()
        .any(|p| p["brand"]["name"] == STORE));
    let wrong_report = inspect(&db, &b, output, "wrong-brand", &wrong);
    assert!(has(&wrong_report, "hashes", "pass"));
    assert!(
        has(&wrong_report, "product-brand", "blocked"),
        "{wrong_report}"
    );
    assert_eq!(wrong_report["packageStatus"], "blocked");
    // Historical expiry is evaluated against the manifest date, never today's date.
    let mut historical = normal.clone();
    let mut catalog: Value = serde_json::from_str(content(&historical, "catalog.json")).unwrap();
    catalog["generatedAt"] = json!("1999-12-31T12:00:00Z");
    for p in catalog["products"].as_array_mut().unwrap() {
        for f in p["publicFacts"].as_array_mut().unwrap() {
            if f["id"] == "catalog:brand" {
                f["validUntil"] = json!("2000-01-01");
            }
        }
    }
    replace(
        &mut historical,
        "catalog.json",
        serde_json::to_string_pretty(&catalog).unwrap(),
    );
    let mut manifest: Value =
        serde_json::from_str(content(&historical, "release-manifest.json")).unwrap();
    manifest["generatedAt"] = json!("1999-12-31T12:00:00Z");
    replace(
        &mut historical,
        "release-manifest.json",
        serde_json::to_string_pretty(&manifest).unwrap(),
    );
    rehash(&mut historical);
    let historical = artifact(output, "historical-date-website.zip", &historical);
    let history_report = inspect(&db, &b, output, "historical-date", &historical);
    assert!(has(&history_report, "hashes", "pass"));
    assert!(has(&history_report, "product-brand", "pass"));
    assert!(!has(&history_report, "product-brand", "blocked"));
    assert_eq!(history_report["packageStatus"], "pass", "{history_report}");
    assert_eq!(
        logical_snapshot(&db),
        before,
        "ZIP inspection/report export changed logical DB contents"
    );
    assert_eq!(
        digest(&fs::read(&path).unwrap()),
        before_digest,
        "ZIP inspection/report export changed database bytes"
    );
    save(
        output,
        "readonly-evidence.json",
        &json!({"databaseSha256Before":before_digest,"databaseSha256After":digest(&fs::read(&path).unwrap()),"logicalSha256Before":digest(before.to_string().as_bytes()),"logicalSha256After":digest(logical_snapshot(&db).to_string().as_bytes()),"tables":before.as_object().unwrap().keys().collect::<Vec<_>>()}),
    );
    // Missing brand remains a useful partial preview after normal confirmation/review.
    let mut missing = products[2].clone();
    missing["facts"]
        .as_array_mut()
        .unwrap()
        .retain(|f| f["id"] != "catalog:brand");
    products[2] = confirmed(&mut db, &b, missing);
    let (refs, selection) = reviewed(&mut db, &b, &products[2], &questions[2]);
    groups[2] = refs;
    selections[2] = selection;
    let partial = call(&mut db, "site.prepare", site_request(&groups));
    assert!(has(&partial["readiness"], "brand", "warning"));
    let partial = artifact(output, "missing-brand-preview.zip", &partial);
    assert!(parsed_products(&partial)["FICTIONAL-P10-3"]
        .get("brand")
        .is_none());
    let partial_report = inspect(&db, &b, output, "missing-brand", &partial);
    assert_eq!(partial_report["packageStatus"], "pass", "{partial_report}");
    assert!(has(&partial_report, "product-brand", "warning"));
    save(
        output,
        "missing-brand-site-request.json",
        &site_request(&groups),
    );
    let missing_path = output.join("missing-brand-native-seed.sqlite3");
    assert!(!missing_path.exists());
    let mut missing_seed = Connection::open(&missing_path).unwrap();
    rusqlite::backup::Backup::new(&db, &mut missing_seed)
        .unwrap()
        .run_to_completion(128, std::time::Duration::ZERO, None)
        .unwrap();
    no_side_effects(&missing_seed);
    drop(missing_seed);
    // Restore normal third product, then exercise manufacturer change and stale gates.
    let mut restored = products[2].clone();
    restored["facts"]
        .as_array_mut()
        .unwrap()
        .push(fact("catalog:brand", MAKERS[2]));
    products[2] = confirmed(&mut db, &b, restored);
    let (refs, selection) = reviewed(&mut db, &b, &products[2], &questions[2]);
    groups[2] = refs;
    selections[2] = selection;
    let old_site_request = site_request(&groups);
    let old_feed_request =
        json!({"brandId":b,"profile":"google-merchant-basic-v1","items":selections});
    let mut changed = products[0].clone();
    changed["expectedRevision"] = changed["revision"].clone();
    changed["facts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["id"] == "catalog:brand")
        .unwrap()["value"] = json!(MAKERS[2]);
    let changed = call(&mut db, "product.save", changed);
    assert!(changed["confirmedAt"].is_null());
    let stale_site_error =
        runtime::core_dispatch(&mut db, "site.prepare", old_site_request.clone()).unwrap_err();
    let stale_feed_error =
        runtime::core_dispatch(&mut db, "feed.prepare", old_feed_request).unwrap_err();
    products[0] = call(
        &mut db,
        "product.confirm",
        json!({"brandId":b,"id":changed["id"],"expectedRevision":changed["revision"]}),
    );
    assert!(
        runtime::core_dispatch(&mut db, "site.prepare", old_site_request).is_err(),
        "reconfirmation must not revive old-source drafts"
    );
    let (refs, selection) = reviewed(&mut db, &b, &products[0], &questions[0]);
    groups[0] = refs;
    selections[0] = selection;
    let final_site_request = site_request(&groups);
    let final_feed_request =
        json!({"brandId":b,"profile":"google-merchant-basic-v1","items":selections});
    let current = call(&mut db, "site.prepare", final_site_request.clone());
    let current = artifact(output, "current-website.zip", &current);
    assert_identity(&current, &[MAKERS[2], MAKERS[1], MAKERS[2]]);
    let current_feed = call(&mut db, "feed.prepare", final_feed_request.clone());
    assert_feed(&current_feed, &[MAKERS[2], MAKERS[1], MAKERS[2]]);
    artifact(output, "current-basic-xml-feed.zip", &current_feed);
    let current_report = inspect(&db, &b, output, "current", &current);
    assert_eq!(current_report["packageStatus"], "pass");
    assert_eq!(current_report["currentStatus"], "match");
    let original_report = inspect(&db, &b, output, "original-after-update", &normal);
    assert_eq!(original_report["packageStatus"], "pass");
    assert_eq!(original_report["currentStatus"], "changed");
    assert_eq!(
        fs::read(output.join("normal-website.zip")).unwrap(),
        normal_bytes
    );
    save(output, "site-request.json", &final_site_request);
    save(output, "feed-request.json", &final_feed_request);
    save(
        output,
        "stale-export-evidence.json",
        &json!({"siteError":stale_site_error,"feedError":stale_feed_error,"reconfirmationAloneRevivesOldDrafts":false,"normalZipSha256Before":normal_digest,"normalZipSha256After":digest(&fs::read(output.join("normal-website.zip")).unwrap())}),
    );
    no_side_effects(&db);
    drop(db);
    let mut db = runtime::open_database(&path).unwrap();
    no_side_effects(&db);
    let reopened = call(&mut db, "site.prepare", final_site_request);
    assert_identity(&reopened, &[MAKERS[2], MAKERS[1], MAKERS[2]]);
    let snapshot = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    assert_eq!(snapshot["products"].as_array().unwrap().len(), 3);
    let summary = json!({"status":"passed","fixture":"fictional-local-simulated-only","nativeAcceptance":"pending-controller","brandId":b,"workspaceName":STORE,"schema":5,"products":3,"manufacturerBrands":2,"currentProducts":products.iter().map(|p| json!({"id":p["id"],"sku":p["sku"],"name":p["name"],"revision":p["revision"]})).collect::<Vec<_>>(),"reviewedCurrentDrafts":12,"totalDrafts":snapshot["drafts"].as_array().unwrap().len(),"modelJobs":0,"networkCalls":0,"platformSubmissions":0,"publications":0,"normalZipSha256":normal_digest,"databaseSha256":digest(&fs::read(&path).unwrap()),"checks":["fresh-schema5-db","real-core-product-question-confirmations","simulated-generate-save-review","parsed-jsonld-identity","parsed-basic-xml-identity","workspace-brand-never-product-brand","missing-brand-omitted-warning","rehash-valid-wrong-brand-blocked","manifest-date-historical-brand-validity","readonly-database-bytes-and-all-tables-unchanged","private-sentinel-excluded","catalog-brand-update-invalidates-confirmation-and-drafts","reconfirmation-alone-cannot-export-stale-drafts","new-current-reviews-export-new-brand","old-zip-bytes-unchanged","final-fixture-normal-and-reopen"]});
    save(output, "fixture-result.json", &summary);
    println!("Phase10 acceptance passed: three fictional products, two manufacturer brands; parsed website/XML, actual ZIP inspections, readonly/stale/reopen gates; zero network/model/publishing. Native acceptance pending controller.");
}
