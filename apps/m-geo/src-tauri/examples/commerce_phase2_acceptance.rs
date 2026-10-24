//! Reproducible disposable acceptance. Never opens a production database.
use m_geo_lib::{backup, files, runtime};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{fs, path::Path};
fn call(db: &mut Connection, action: &str, payload: Value) -> Value {
    runtime::core_dispatch(db, action, payload).unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        3,
        "source fictional db and new mgeo-e2e output directory required"
    );
    let source = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    assert!(source.to_string_lossy().contains("mgeo-e2e-"));
    assert!(output.to_string_lossy().contains("mgeo-e2e-"));
    fs::create_dir_all(output).unwrap();
    let path = output.join("fictional.sqlite3");
    assert!(
        !path.exists(),
        "never overwrite an existing acceptance database"
    );
    fs::copy(source, &path).unwrap();
    let mut db = runtime::open_database(&path).unwrap();
    let brands = call(&mut db, "brand.list", json!({}));
    assert_eq!(brands.as_array().unwrap().len(), 1);
    assert!(brands[0]["name"].as_str().unwrap().contains("虚构"));
    let b = brands[0]["id"].clone();
    let snapshot = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    assert_eq!(snapshot["products"].as_array().unwrap().len(), 3);
    let old_refs: Vec<_> = snapshot["drafts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| json!({"id":d["id"],"expectedRevision":d["revision"]}))
        .collect();
    let make_csv = |s: &Value| {
        let mut rows =
            vec!["SKU,资料版本,价格,币种,库存状态,购买链接,图片链接,依据,有效期".to_owned()];
        for (i, p) in s["products"].as_array().unwrap().iter().enumerate() {
            rows.push(format!(
                "{},{},{},CNY,InStock,,,虚构第二阶段验证资料（非真实报价）,2099-12-31",
                p["sku"].as_str().unwrap(),
                p["revision"],
                101 + i
            ));
        }
        rows.join("\n")
    };
    let input = make_csv(&snapshot);
    let preview = call(
        &mut db,
        "commerce.previewBatch",
        json!({"brandId":b,"content":input}),
    );
    assert_eq!(preview["canApply"], true);
    let invalid = format!("{input}\nUNKNOWN,1,10,CNY,,,,虚构,2099-12-31");
    let invalid_preview = call(
        &mut db,
        "commerce.previewBatch",
        json!({"brandId":b,"content":invalid}),
    );
    assert_eq!(invalid_preview["canApply"], false);
    assert!(runtime::core_dispatch(
        &mut db,
        "commerce.applyBatch",
        json!({"brandId":b,"content":invalid,"token":invalid_preview["token"],"confirm":true})
    )
    .is_err());
    assert_eq!(
        call(&mut db, "workspace.snapshot", json!({"brandId":b}))["products"],
        snapshot["products"]
    );
    let mut concurrent = snapshot["products"][0].clone();
    concurrent["expectedRevision"] = concurrent["revision"].clone();
    concurrent["privateNotes"] = json!("PHASE2_PRIVATE_SENTINEL");
    call(&mut db, "product.save", concurrent);
    assert!(runtime::core_dispatch(
        &mut db,
        "commerce.applyBatch",
        json!({"brandId":b,"content":input,"token":preview["token"],"confirm":true})
    )
    .is_err());
    let current = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    let input = make_csv(&current);
    let preview = call(
        &mut db,
        "commerce.previewBatch",
        json!({"brandId":b,"content":input}),
    );
    let payload = json!({"brandId":b,"content":input,"token":preview["token"],"confirm":true});
    assert_eq!(
        call(&mut db, "commerce.applyBatch", payload.clone())["updated"],
        3
    );
    assert!(runtime::core_dispatch(&mut db, "commerce.applyBatch", payload).is_err());
    assert!(runtime::core_dispatch(
        &mut db,
        "site.prepare",
        json!({"brandId":b,"baseUrl":"","drafts":old_refs})
    )
    .is_err());
    let current = call(&mut db, "workspace.snapshot", json!({"brandId":b}));
    let mut refs = vec![];
    for p in current["products"].as_array().unwrap() {
        assert!(p["confirmedAt"].is_null());
        call(
            &mut db,
            "product.confirm",
            json!({"brandId":b,"id":p["id"],"expectedRevision":p["revision"]}),
        );
        for kind in ["description", "faq", "guide", "comparison"] {
            let d = call(
                &mut db,
                "draft.generateLocal",
                json!({"brandId":b,"productId":p["id"],"kind":kind}),
            );
            let d = call(
                &mut db,
                "draft.review",
                json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
            );
            refs.push(json!({"id":d["id"],"expectedRevision":d["revision"]}));
        }
    }
    let site = call(
        &mut db,
        "site.prepare",
        json!({"brandId":b,"baseUrl":"","drafts":refs}),
    );
    assert_eq!(site["readiness"]["status"], "preview");
    assert_eq!(site["previewOnly"], true);
    assert!(!site.to_string().contains("PHASE2_PRIVATE_SENTINEL"));
    fs::write(
        output.join("fictional-website.zip"),
        files::site_zip(&site).unwrap(),
    )
    .unwrap();
    let configured = call(
        &mut db,
        "site.prepare",
        json!({"brandId":b,"baseUrl":"https://fictional.example/geo/","drafts":refs}),
    );
    assert_eq!(configured["previewOnly"], false);
    assert!(configured["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == "sitemap.xml"));
    fs::write(
        output.join("configured-domain-local-only.zip"),
        files::site_zip(&configured).unwrap(),
    )
    .unwrap();
    let campaign = &snapshot["campaigns"][0];
    let request = json!({"brandId":b,"campaignId":campaign["id"]});
    let report = call(&mut db, "campaign.report", request.clone());
    assert_eq!(report["summary"]["baselineCount"], 120);
    assert_eq!(report["summary"]["followupCount"], 120);
    assert_eq!(report["summary"]["missingCount"], 0);
    fs::write(
        output.join("fictional-experiment-report.zip"),
        files::site_zip(&report).unwrap(),
    )
    .unwrap();
    let encrypted = backup::encrypt_snapshot(&db, "fictional-phase2-only-passphrase").unwrap();
    fs::write(output.join("fictional-backup.mgeo"), &encrypted).unwrap();
    let mut restored = runtime::open_database(&output.join("restored.sqlite3")).unwrap();
    backup::restore_snapshot(
        &mut restored,
        &encrypted,
        "fictional-phase2-only-passphrase",
    )
    .unwrap();
    assert_eq!(
        call(&mut restored, "campaign.report", request.clone())["summary"],
        report["summary"]
    );
    assert_eq!(
        call(&mut restored, "workspace.snapshot", json!({"brandId":b}))["products"],
        call(&mut db, "workspace.snapshot", json!({"brandId":b}))["products"]
    );
    drop(db);
    let mut reopened = runtime::open_database(&path).unwrap();
    assert_eq!(
        call(&mut reopened, "campaign.report", request)["summary"],
        report["summary"]
    );
    let final_snapshot = call(&mut reopened, "workspace.snapshot", json!({"brandId":b}));
    assert_eq!(final_snapshot["publications"].as_array().unwrap().len(), 0);
    let result = json!({"status":"passed","fixture":"fictional-only-not-real-platform-evidence","brandId":b,"campaignId":campaign["id"],"products":3,"newReviewedDrafts":12,"observations":240,"publications":0,"paidModelCalls":0,"checks":["invalid-batch-atomic","stale-preview-rejected","replay-rejected","confirmation-revoked","old-site-rejected","reconfirm-regenerate-review","preview-noindex","configured-domain-local-only","private-sentinel-excluded","three-native-core-zip-files","report-120-plus-120","encrypted-backup-roundtrip","database-reopen"],"summary":report["summary"],"siteReadiness":site["readiness"]});
    fs::write(
        output.join("fixture-result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("Phase 2 isolated core acceptance passed: 3 products, 12 new reviewed drafts, 240 simulated observations; zero publishing/paid calls.");
}
