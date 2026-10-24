//! Fictional, disposable local channel workflow. Never sends network/model requests.
use m_geo_lib::{backup, files, runtime};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{fs, path::Path};
fn call(db: &mut Connection, action: &str, payload: Value) -> Value {
    runtime::core_dispatch(db, action, payload).unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn save_json(dir: &Path, name: &str, value: &Value) {
    fs::write(dir.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn zip(dir: &Path, name: &str, value: &Value) {
    fs::write(dir.join(name), files::site_zip(value).unwrap()).unwrap();
}
fn clone_db(source: &Connection) -> Connection {
    let mut target = Connection::open_in_memory().unwrap();
    rusqlite::backup::Backup::new(source, &mut target)
        .unwrap()
        .run_to_completion(128, std::time::Duration::ZERO, None)
        .unwrap();
    target
}
fn fact(key: &str, value: &str) -> Value {
    json!({"id":format!("catalog:{key}"),"label":key,"value":value,"source":"Fictional phase4 specification; not sold; no real manufacturer identifier","visibility":"public","validUntil":null})
}
fn reviewed(db: &mut Connection, b: &Value, p: &Value, q: &Value) -> (Value, Vec<Value>) {
    let mut description = Value::Null;
    let mut refs = vec![];
    for kind in ["description", "faq", "guide", "comparison"] {
        let d = call(
            db,
            "draft.generateLocal",
            json!({"brandId":b,"productId":p["id"],"questionId":q["id"],"kind":kind}),
        );
        let body = format!("【虚构商品，仅本地流程验收，不出售】\n{}；测试规格信息见已确认商品资料。\n用于阅读格式检查：& <示例> \"双引号\"。不代表真实商品或平台推荐。", p["name"].as_str().unwrap());
        let d = call(
            db,
            "draft.save",
            json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":body}),
        );
        let d = call(
            db,
            "draft.review",
            json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
        );
        if kind == "description" {
            description = d.clone();
        }
        refs.push(json!({"id":d["id"],"expectedRevision":d["revision"]}));
    }
    (
        json!({"productId":p["id"],"expectedRevision":p["revision"],"draftId":description["id"],"expectedDraftRevision":description["revision"]}),
        refs,
    )
}
fn counts(db: &Connection) -> Value {
    let mut result = json!({});
    for name in [
        "m_products",
        "m_drafts",
        "m_observations",
        "m_publications",
        "ai_jobs",
        "m_audit",
    ] {
        result[name] = json!(db
            .query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r
                .get::<_, i64>(0))
            .unwrap());
    }
    result
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "fictional source and new mgeo-e2e directory required"
    );
    let source = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    assert!(source.to_string_lossy().contains("mgeo-e2e-"));
    assert!(output.to_string_lossy().contains("mgeo-e2e-"));
    fs::create_dir_all(output).unwrap();
    let path = output.join("fictional.sqlite3");
    assert!(!path.exists(), "never overwrite an acceptance database");
    fs::copy(source, &path).unwrap();
    let mut db = runtime::open_database(&path).unwrap();
    let brands = call(&mut db, "brand.list", json!({}));
    assert_eq!(brands.as_array().unwrap().len(), 1);
    assert!(brands[0]["name"].as_str().unwrap().contains("虚构"));
    let b = brands[0]["id"].clone();
    let snapshot = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    let mut products = snapshot["products"].as_array().unwrap().clone();
    products.sort_by_key(|p| p["sku"].as_str().unwrap().to_owned());
    assert_eq!(products.len(), 3);
    let mut selections = vec![];
    let mut site_refs = vec![];
    for (i, p) in products.iter_mut().enumerate() {
        let mut updated = p.clone();
        let fs = updated["facts"].as_array_mut().unwrap();
        fs.extend([
            fact("brand", "FICTIONAL M-GEO TEST"),
            fact("kind", if i < 2 { "variant" } else { "standalone" }),
            fact("condition", "new"),
            fact("productClass", "physical_non_apparel"),
            fact("titleOrigin", "ai"),
            fact("mpn", &format!("FICTIONAL-NOT-ASSIGNED-{}", i + 1)),
        ]);
        if i < 2 {
            fs.extend([
                fact("groupId", "FICTIONAL-CUP-FAMILY"),
                fact("groupTitle", "虚构通勤杯系列（非在售）"),
                fact("size", if i == 0 { "350mL" } else { "450mL" }),
            ]);
        }
        updated["expectedRevision"] = p["revision"].clone();
        updated["privateNotes"] = json!("PHASE4_PRIVATE_SENTINEL");
        let saved = call(&mut db, "product.save", updated);
        *p = call(
            &mut db,
            "product.confirm",
            json!({"brandId":b,"id":saved["id"],"expectedRevision":saved["revision"]}),
        );
        let q = snapshot["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["productId"] == p["id"] && q["confirmed"] == true)
            .unwrap();
        let (selection, refs) = reviewed(&mut db, &b, p, q);
        selections.push(selection);
        site_refs.extend(refs);
    }
    let inspect = call(&mut db, "catalog.inspect", json!({"brandId":b}));
    assert_eq!(inspect["summary"]["blocked"], 0, "{inspect}");
    save_json(output, "catalog-inspection.json", &inspect);
    let mut request = json!({"brandId":b,"profile":"mgeo-catalog-v1","items":selections});
    let base = call(&mut db, "feed.prepare", request.clone());
    assert_eq!(base["summary"]["ready"], 3, "{base}");
    assert_eq!(base["summary"]["blocked"], 0);
    assert_eq!(
        call(&mut db, "feed.prepare", request.clone())["packageId"],
        base["packageId"]
    );
    zip(output, "fictional-general-catalog.zip", &base);
    save_json(output, "baseline-manifest.json", &base["manifest"]);
    let google_request =
        json!({"brandId":b,"profile":"google-merchant-basic-v1","items":[selections[2]]});
    let google = call(&mut db, "feed.prepare", google_request.clone());
    assert_eq!(google["summary"]["ready"], 1, "{google}");
    zip(output, "fictional-google-basic-draft.zip", &google);
    let blocked = call(
        &mut db,
        "feed.prepare",
        json!({"brandId":b,"profile":"google-merchant-basic-v1","items":selections}),
    );
    assert_eq!(blocked["summary"]["blocked"], 2, "{blocked}");
    assert!(!blocked["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"].as_str().unwrap().ends_with(".xml")));
    zip(output, "google-unsupported-variants-report.zip", &blocked);
    for package in [&inspect, &base, &google, &blocked] {
        for secret in [
            "PRIVATE_SECRET",
            "RAW_SECRET",
            "NOTE_SECRET",
            "PHASE4_PRIVATE_SENTINEL",
        ] {
            assert!(!package.to_string().contains(secret));
        }
    }
    let before = counts(&db);
    for scenario in ["accepted", "rejected", "unknown"] {
        let mut p = request.clone();
        p["scenario"] = json!(scenario);
        let simulated = call(&mut db, "feed.simulate", p);
        assert_eq!(simulated["receipt"]["mode"], "simulated");
        save_json(
            output,
            &format!("{scenario}-receipt.json"),
            &simulated["receipt"],
        );
        zip(output, &format!("{scenario}-simulation.zip"), &simulated);
        let mut check = request.clone();
        check["content"] = json!(simulated["receipt"].to_string());
        let checked = call(&mut db, "feed.checkReceipt", check);
        assert_eq!(checked["summary"][scenario], 3, "{checked}");
        zip(output, &format!("{scenario}-receipt-check.zip"), &checked);
    }
    let mut p = request.clone();
    p["scenario"] = json!("accepted");
    let receipt = call(&mut db, "feed.simulate", p)["receipt"].clone();
    let mut partial = receipt.clone();
    partial["rows"].as_array_mut().unwrap().pop();
    let mut p = request.clone();
    p["content"] = json!(partial.to_string());
    let checked = call(&mut db, "feed.checkReceipt", p);
    assert_eq!(checked["summary"]["missing"], 1);
    save_json(output, "partial-receipt.json", &partial);
    for kind in ["duplicate", "unknown", "tampered", "real"] {
        let mut invalid = receipt.clone();
        match kind {
            "duplicate" => {
                let row = invalid["rows"][0].clone();
                invalid["rows"].as_array_mut().unwrap().push(row);
            }
            "unknown" => invalid["rows"][0]["sku"] = json!("UNSELECTED-SKU"),
            "tampered" => invalid["rows"][0]["rowHash"] = json!("0".repeat(64)),
            _ => invalid["mode"] = json!("recorded"),
        }
        let mut p = request.clone();
        p["content"] = json!(invalid.to_string());
        assert!(runtime::core_dispatch(&mut db, "feed.checkReceipt", p).is_err());
    }
    let mut p = request.clone();
    p["baseline"] = json!(base["manifest"].to_string());
    assert_eq!(call(&mut db, "feed.compare", p)["summary"]["unchanged"], 3);
    assert_eq!(
        counts(&db),
        before,
        "readonly phase4 operations must not write"
    );
    let mut bad = clone_db(&db);
    let other = call(
        &mut bad,
        "brand.create",
        json!({"name":"Fictional other brand"}),
    );
    let mut cross = request.clone();
    cross["brandId"] = other["id"].clone();
    assert!(runtime::core_dispatch(&mut bad, "feed.prepare", cross).is_err());
    // Exercise source update through the already-shipped atomic price/inventory workflow.
    let csv=format!("SKU,资料版本,价格,币种,库存状态,购买链接,图片链接,依据,有效期\n{},{},121.50,,,,,虚构第四阶段更新依据（非真实报价）,2099-12-31",products[0]["sku"].as_str().unwrap(),products[0]["revision"]);
    let preview = call(
        &mut db,
        "commerce.previewBatch",
        json!({"brandId":b,"content":csv}),
    );
    call(
        &mut db,
        "commerce.applyBatch",
        json!({"brandId":b,"content":csv,"token":preview["token"],"confirm":true}),
    );
    assert!(runtime::core_dispatch(&mut db, "feed.prepare", request.clone()).is_err());
    let updated = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    let p = updated["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == products[0]["id"])
        .unwrap();
    assert!(p["confirmedAt"].is_null());
    let p = call(
        &mut db,
        "product.confirm",
        json!({"brandId":b,"id":p["id"],"expectedRevision":p["revision"]}),
    );
    let q = snapshot["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["productId"] == p["id"] && q["confirmed"] == true)
        .unwrap();
    let (selection, refs) = reviewed(&mut db, &b, &p, q);
    selections[0] = selection;
    site_refs.retain(|r| {
        !base["manifest"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["productId"] == p["id"] && r["id"] == x["draftId"])
    });
    // Rebuild site refs from latest reviewed current source revisions, including all kinds.
    let latest = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    site_refs = latest["drafts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| {
            !d["reviewedAt"].is_null()
                && latest["products"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == d["productId"] && p["revision"] == d["sourceRevision"])
        })
        .map(|d| json!({"id":d["id"],"expectedRevision":d["revision"]}))
        .collect();
    assert_eq!(refs.len(), 4);
    request["items"] = json!(selections);
    let mut p = request.clone();
    p["baseline"] = json!(base["manifest"].to_string());
    let diff = call(&mut db, "feed.compare", p);
    assert_eq!(diff["summary"]["changed"], 1, "{diff}");
    assert_eq!(diff["summary"]["unchanged"], 2);
    zip(output, "updated-catalog-comparison.zip", &diff);
    let mut p = request.clone();
    p["content"] = json!(receipt.to_string());
    assert!(runtime::core_dispatch(&mut db, "feed.checkReceipt", p).is_err());
    let current = call(&mut db, "feed.prepare", request.clone());
    assert_ne!(current["packageId"], base["packageId"]);
    save_json(output, "current-manifest.json", &current["manifest"]);
    save_json(output, "feed-request.json", &request);
    zip(output, "updated-general-catalog.zip", &current);
    let mut p = request.clone();
    p["scenario"] = json!("rejected");
    let simulated = call(&mut db, "feed.simulate", p);
    save_json(
        output,
        "current-rejected-receipt.json",
        &simulated["receipt"],
    );
    let site = call(
        &mut db,
        "site.prepare",
        json!({"brandId":b,"baseUrl":"","drafts":site_refs}),
    );
    zip(output, "fictional-current-website.zip", &site);
    let encrypted = backup::encrypt_snapshot(&db, "fictional-phase4-only-passphrase").unwrap();
    fs::write(output.join("fictional-backup.mgeo"), &encrypted).unwrap();
    let mut restored = runtime::open_database(&output.join("restored.sqlite3")).unwrap();
    backup::restore_snapshot(
        &mut restored,
        &encrypted,
        "fictional-phase4-only-passphrase",
    )
    .unwrap();
    assert_eq!(
        call(&mut restored, "workspace.snapshot", json!({"brandId":b})),
        latest
    );
    assert_eq!(
        call(&mut restored, "feed.prepare", request.clone())["packageId"],
        current["packageId"]
    );
    drop(db);
    let mut reopened = runtime::open_database(&path).unwrap();
    assert_eq!(
        call(&mut reopened, "feed.prepare", request)["packageId"],
        current["packageId"]
    );
    let final_counts = counts(&reopened);
    assert_eq!(final_counts["m_publications"], 0);
    assert_eq!(final_counts["ai_jobs"], 0);
    let result = json!({"status":"passed","fixture":"fictional-only-not-platform-evidence","products":3,"variants":2,"standalone":1,"googleLocalReady":1,"googleUnsupportedVariants":2,"paidModelCalls":0,"platformSubmissions":0,"publications":0,"counts":final_counts,"checks":["identity-family","deterministic-manifest","google-ai-title-escaping","blocked-variants-no-xml","no-private-field-leak","accepted-rejected-unknown-simulation","partial-missing","duplicate-unknown-tampered-real-receipt-rejected","readonly-no-database-writes","cross-brand-rejected","price-update-invalidates-source","reconfirm-and-new-review","comparison-changed-one","old-receipt-invalidated","current-website-regression","encrypted-backup-restore","database-reopen"]});
    save_json(output, "fixture-result.json", &result);
    println!("Phase4 acceptance passed: 3 fictional products, 2 variants, 1 Google basic local draft; diff/receipt/backup/reopen; no model calls, submissions or publishing.");
}
