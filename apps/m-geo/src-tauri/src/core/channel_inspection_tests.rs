use super::*;
const BRAND: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
fn seal(v: &mut Value, field: &str) {
    v.as_object_mut().unwrap().remove(field);
    v[field] = json!(fingerprint(v));
}
fn manifest(start: usize, count: usize) -> Value {
    let items:Vec<Value>=(start..start+count).map(|n| {
        let sku=format!("SECRET-SKU-{n}");
        let mut row=json!({"productId":format!("{n:08x}-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),"draftId":format!("{n:08x}-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),"sku":sku,"productRevision":1,"draftRevision":1,"localStatus":"ready","data":{"id":sku,"title":"Frozen title","description":"SECRET-DESCRIPTION\u{feff}","link":"https://example.com/SECRET-URL","imageLink":"https://example.com/image.png","price":"29.90","currency":"CNY","availability":"InStock","catalog":{"kind":"standalone","condition":"new","titleOrigin":"human","productClass":"physical_non_apparel","brand":"Frozen brand","mpn":"X"}},"validity":[{"field":"commerce:availability","validUntil":"2001-01-01"},{"field":"commerce:price","validUntil":"2001-01-01"}]});
        seal(&mut row,"rowHash"); row
    }).collect();
    let mut m = json!({"format":"m-geo-feed-v1","schemaVersion":1,"profile":"mgeo-catalog-v1","brandId":BRAND,"mode":"local_draft","submissionStatus":"not_submitted","selectionScope":"selected_products_not_full_merchant_catalog","items":items});
    seal(&mut m, "packageId");
    m
}
fn package(files: Vec<(&str, String)>) -> Value {
    let bytes: usize = files.iter().map(|(_, s)| s.len()).sum();
    json!({"source":{"fileName":"frozen.zip","sha256":"b".repeat(64),"compressedBytes":512,"uncompressedBytes":bytes,"fileCount":4},"files":files.into_iter().map(|(path,content)|json!({"path":path,"content":content})).collect::<Vec<_>>()})
}
fn comparison(old: &Value, new: &Value) -> Value {
    let notice="仅比较本次本地选择。移除表示不在当前本地选择中，不代表平台删除，也不是完整商家目录替换授权。所有商品尚未提交平台；摘要用于完整性核对，不是签名或认证。\n";
    let report = json!({"mode":"local_draft","submissionStatus":"not_submitted","selectionScope":"selected_products_not_full_merchant_catalog","summary":{"added":1,"removed":1,"changed":1,"unchanged":1},"rows":[{"sku":"SECRET-SKU-1","change":"removed","fields":[]},{"sku":"SECRET-SKU-2","change":"changed","fields":["price"]},{"sku":"SECRET-SKU-3","change":"unchanged","fields":[]},{"sku":"SECRET-SKU-4","change":"added","fields":[]}],"notice":notice});
    package(vec![
        ("comparison-report.json", report.to_string()),
        ("baseline-manifest.json", old.to_string()),
        ("feed-manifest.json", new.to_string()),
        ("local-operation-notice.txt", notice.into()),
    ])
}
fn comparison_fixture() -> Value {
    let old = manifest(1, 3);
    let mut new = manifest(2, 3);
    new["items"][0]["data"]["price"] = json!("39.90");
    seal(&mut new["items"][0], "rowHash");
    seal(&mut new, "packageId");
    comparison(&old, &new)
}
fn alter_file(p: &mut Value, name: &str, f: impl FnOnce(&mut Value)) {
    let rows = p["files"].as_array_mut().unwrap();
    let row = rows.iter_mut().find(|r| r["path"] == name).unwrap();
    let mut v: Value = serde_json::from_str(row["content"].as_str().unwrap()).unwrap();
    f(&mut v);
    row["content"] = json!(v.to_string());
    sync_size(p);
}
fn sync_size(p: &mut Value) {
    p["source"]["uncompressedBytes"] = json!(p["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["content"].as_str().unwrap().len())
        .sum::<usize>());
}
#[test]
fn channel_inspection_replays_frozen_comparison_without_current_freshness() {
    let p = comparison_fixture();
    let inspected = inspect_channel_report(BRAND, &p).unwrap();
    assert_eq!(inspected.report["status"], "passed");
    assert_eq!(
        inspected.report["summary"],
        json!({"added":1,"removed":1,"changed":1,"unchanged":1})
    );
    assert_eq!(
        inspected.report["counts"],
        json!({"selected":3,"baselineSelected":3,"rows":4,"ready":3,"blocked":0})
    );
    assert_eq!(inspected.rows[1]["fields"], json!(["price"]));
    assert_eq!(inspected.rows[0]["change"], "removed");
}
fn receipt_package(operation: &str, statuses: &[&str]) -> Value {
    let m = manifest(1, 4);
    let receipt_rows:Vec<Value>=statuses.iter().enumerate().map(|(i,s)|{
        let (code,message)=match *s {"accepted"=>("SIMULATED_ACCEPTED","本机模拟接受，不代表平台真实接受"),"rejected"=>("SIMULATED_POLICY_REJECTION","本机模拟策略拒绝，不代表平台真实判定"),_=>("SIMULATED_UNKNOWN","本机模拟结果未知，不能推定成功或失败")};
        json!({"sku":m["items"][i]["sku"],"rowHash":m["items"][i]["rowHash"],"status":s,"code":code,"message":message})
    }).collect();
    let receipt = json!({"format":"m-geo-feed-receipt-v1","mode":"simulated","brandId":BRAND,"profile":m["profile"],"packageId":m["packageId"],"rows":receipt_rows});
    let mut rows = receipt_rows;
    for i in rows.len()..4 {
        rows.push(json!({"sku":m["items"][i]["sku"],"rowHash":m["items"][i]["rowHash"],"status":"missing","code":"SIMULATED_MISSING","message":"模拟回执中缺失此商品，不能推定接受"}));
    }
    let mut summary = json!({"accepted":0,"rejected":0,"unknown":0,"missing":0});
    for r in &rows {
        let k = r["status"].as_str().unwrap();
        summary[k] = json!(summary[k].as_u64().unwrap() + 1);
    }
    let report = json!({"mode":"simulated","submissionStatus":"not_submitted","summary":summary,"rows":rows});
    let (name, notice) = if operation == "simulation" {
        ("simulation-report.json","本机模拟回执，商品尚未提交平台。模拟接受、拒绝、未知均不证明真实平台接受、收录、推荐或拒绝。未写入提交或发布状态。摘要不是平台签名。\n")
    } else {
        ("receipt-check-report.json","仅核对绑定当前本地快照的模拟回执。商品尚未提交平台；缺失行保持缺失，未知保持未知，均不能推定真实成功。摘要不是平台认证或签名。\n")
    };
    package(vec![
        (name, report.to_string()),
        ("simulated-receipt.json", receipt.to_string()),
        ("feed-manifest.json", m.to_string()),
        ("local-operation-notice.txt", notice.into()),
    ])
}
#[test]
fn channel_inspection_recomputes_receipt_counts_missing_and_unknown() {
    for statuses in [vec!["accepted", "rejected", "unknown"], vec![]] {
        let out =
            inspect_channel_report(BRAND, &receipt_package("receipt-check", &statuses)).unwrap();
        assert_eq!(out.report["status"], "passed");
        assert_eq!(out.rows.len(), 4);
        assert_eq!(out.report["baselinePackageId"], Value::Null);
        assert_eq!(out.report["counts"]["baselineSelected"], 0);
        if statuses.is_empty() {
            assert_eq!(
                out.report["summary"],
                json!({"accepted":0,"rejected":0,"unknown":0,"missing":4})
            );
        } else {
            assert_eq!(
                out.report["summary"],
                json!({"accepted":1,"rejected":1,"unknown":1,"missing":1})
            );
            assert_eq!(out.rows[2]["status"], "unknown");
        }
    }
}
#[test]
fn channel_inspection_simulation_requires_full_uniform_fixed_ordered_rows() {
    for status in ["accepted", "rejected", "unknown"] {
        let p = receipt_package("simulation", &[status; 4]);
        assert_eq!(
            inspect_channel_report(BRAND, &p).unwrap().report["status"],
            "passed"
        );
    }
    for statuses in [
        vec![],
        vec!["accepted"],
        vec!["accepted", "accepted", "unknown", "accepted"],
    ] {
        assert!(inspect_channel_report(BRAND, &receipt_package("simulation", &statuses)).is_err());
    }
    for mode in [0, 1, 2] {
        let mut p = receipt_package("simulation", &["accepted"; 4]);
        alter_file(&mut p, "simulated-receipt.json", |v| match mode {
            0 => v["rows"][0]["message"] = json!("SECRET arbitrary message"),
            1 => v["rows"].as_array_mut().unwrap().swap(0, 1),
            _ => v["rows"][0]["code"] = json!("BAD"),
        });
        assert!(inspect_channel_report(BRAND, &p).is_err());
    }
    let mut p = receipt_package("receipt-check", &["accepted"]);
    alter_file(&mut p, "simulated-receipt.json", |v| {
        v["rows"][0]["message"] = json!("SECRET arbitrary message\u{feff}")
    });
    assert_eq!(
        inspect_channel_report(BRAND, &p).unwrap().report["status"],
        "blocked"
    );
}
#[test]
fn channel_inspection_blocks_whole_report_and_notice_tamper() {
    for field in ["summary", "rows", "mode", "extra", "notice"] {
        let mut p = comparison_fixture();
        alter_file(&mut p, "comparison-report.json", |v| {
            v[field] = json!("SECRET tamper")
        });
        let out = inspect_channel_report(BRAND, &p).unwrap();
        assert_eq!(out.report["status"], "blocked");
        assert_eq!(out.report["checks"][1]["status"], "failed");
        assert_eq!(out.rows[1]["fields"], json!(["price"]));
    }
    let mut p = comparison_fixture();
    p["files"][3]["content"] = json!("tamper");
    sync_size(&mut p);
    let out = inspect_channel_report(BRAND, &p).unwrap();
    assert_eq!(out.report["checks"][2]["status"], "failed");
    assert_eq!(out.report["status"], "blocked");
}
#[test]
fn channel_inspection_rejects_invalid_evidence_duplicate_json_and_metadata() {
    for change in [0, 1, 2, 3, 4] {
        let mut p = comparison_fixture();
        alter_file(&mut p, "feed-manifest.json", |v| match change {
            0 => v["items"][0]["data"]["price"] = json!("999"),
            1 => v["brandId"] = json!(Uuid::new_v4().to_string()),
            2 => v["profile"] = json!("google-merchant-basic-v1"),
            3 => v["extra"] = json!(1),
            _ => v["items"] = json!([]),
        });
        assert!(inspect_channel_report(BRAND, &p).is_err());
    }
    for file in [
        "comparison-report.json",
        "feed-manifest.json",
        "baseline-manifest.json",
    ] {
        let mut p = comparison_fixture();
        let r = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["path"] == file)
            .unwrap();
        let s = r["content"].as_str().unwrap().to_owned();
        r["content"] = json!(format!("{{\"dup\":{{\"x\":1,\"x\":2}},{}", &s[1..]));
        sync_size(&mut p);
        assert!(inspect_channel_report(BRAND, &p).is_err());
    }
    for field in ["uncompressedBytes", "fileCount", "sha256", "extra"] {
        let mut p = comparison_fixture();
        p["source"][field] = json!(0);
        assert!(inspect_channel_report(BRAND, &p).is_err());
    }
    let mut p = comparison_fixture();
    p["files"][0]["content"] = json!("{\"x\":\"\\u0000\"}");
    sync_size(&mut p);
    assert!(inspect_channel_report(BRAND, &p).is_err());
}
#[test]
fn channel_inspection_rejects_logical_uuid_aliases_without_rewriting_hashes() {
    for field in ["productId", "draftId"] {
        let mut m = manifest(10, 2);
        let alias = m["items"][0][field].as_str().unwrap().to_uppercase();
        m["items"][1][field] = json!(alias);
        seal(&mut m["items"][1], "rowHash");
        m["items"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|r| r["productId"].as_str().unwrap().to_owned());
        seal(&mut m, "packageId");
        let p = comparison(&m, &m);
        assert!(inspect_channel_report(BRAND, &p).is_err(), "{field}");
    }
    let mut m = manifest(10, 1);
    m["items"][0]["productId"] = json!(m["items"][0]["productId"].as_str().unwrap().to_uppercase());
    seal(&mut m["items"][0], "rowHash");
    seal(&mut m, "packageId");
    let original = m["packageId"].clone();
    let out = inspect_channel_report(BRAND, &comparison(&m, &m)).unwrap();
    assert_eq!(out.report["packageId"], original);
}
#[test]
fn channel_inspection_handles_400_rows_repeated_sku_and_blocked_comparison() {
    let old = manifest(1, 200);
    let mut new = manifest(201, 200);
    for i in 0..200 {
        new["items"][i]["sku"] = old["items"][i]["sku"].clone();
        new["items"][i]["data"]["id"] = old["items"][i]["sku"].clone();
        seal(&mut new["items"][i], "rowHash");
    }
    seal(&mut new, "packageId");
    let mut p = comparison(&old, &new);
    let expected: Vec<Value> = ["removed", "added"]
        .iter()
        .flat_map(|change| {
            (1..=200)
                .map(move |i| json!({"sku":format!("SECRET-SKU-{i}"),"change":change,"fields":[]}))
        })
        .collect();
    alter_file(&mut p, "comparison-report.json", |v| {
        v["summary"] = json!({"added":200,"removed":200,"changed":0,"unchanged":0});
        v["rows"] = json!(expected);
    });
    let out = inspect_channel_report(BRAND, &p).unwrap();
    assert_eq!(out.report["status"], "passed");
    assert_eq!(out.rows.len(), 400);
    assert_eq!(
        out.report["summary"],
        json!({"added":200,"removed":200,"changed":0,"unchanged":0})
    );
    assert_eq!(out.rows[0]["sku"], out.rows[200]["sku"]);
    let mut m = manifest(1, 1);
    m["items"][0]["localStatus"] = json!("blocked");
    m["items"][0]["data"] = json!({});
    seal(&mut m["items"][0], "rowHash");
    seal(&mut m, "packageId");
    let mut p = comparison(&m, &m);
    alter_file(&mut p, "comparison-report.json", |v| {
        v["summary"] = json!({"added":0,"removed":0,"changed":0,"unchanged":1});
        v["rows"] = json!([{"sku":"SECRET-SKU-1","change":"unchanged","fields":[]}]);
    });
    let out = inspect_channel_report(BRAND, &p).unwrap();
    assert_eq!(out.report["status"], "passed");
    assert_eq!(out.report["counts"]["blocked"], 1);
    let mut p = receipt_package("receipt-check", &[]);
    alter_file(&mut p, "feed-manifest.json", |v| {
        v["items"][0]["localStatus"] = json!("blocked");
        v["items"][0]["data"] = json!({});
        seal(&mut v["items"][0], "rowHash");
        seal(v, "packageId");
    });
    assert!(inspect_channel_report(BRAND, &p).is_err());
}
#[test]
fn channel_inspection_export_is_aggregate_fingerprinted_recursive_whitelist() {
    let report = inspect_channel_report(
        BRAND,
        &receipt_package("receipt-check", &["accepted", "rejected", "unknown"]),
    )
    .unwrap()
    .report;
    let export = channel_inspection_export(&report).unwrap();
    assert_eq!(export["files"].as_array().unwrap().len(), 2);
    assert_eq!(export["files"][0]["path"], "inspection.json");
    assert_eq!(export["files"][1]["path"], "report.md");
    for marker in [
        "SECRET-SKU",
        "SECRET-DESCRIPTION",
        "SECRET-URL",
        "本机模拟接受",
        "productId",
        "draftId",
        "rowHash",
    ] {
        assert!(!export.to_string().contains(marker), "{marker}");
    }
    for (parent, key) in [
        ("", "rows"),
        ("source", "raw"),
        ("counts", "sku"),
        ("summary", "message"),
    ] {
        let mut v = report.clone();
        if parent.is_empty() {
            v[key] = json!("SECRET");
        } else {
            v[parent][key] = json!("SECRET");
        }
        seal(&mut v, "reportFingerprint");
        assert!(channel_inspection_export(&v).is_err());
    }
    for edit in [0, 1, 2, 3, 4] {
        let mut v = report.clone();
        match edit {
            0 => v["counts"]["rows"] = json!(3),
            1 => v["summary"]["accepted"] = json!(8),
            2 => v["checks"][0]["status"] = json!("failed"),
            3 => v["checks"][1]["message"] = json!("SECRET"),
            _ => v["caveat"] = json!("SECRET"),
        };
        seal(&mut v, "reportFingerprint");
        assert!(channel_inspection_export(&v).is_err());
    }
    let mut v = report.clone();
    v["source"]["fileName"] = json!("```<img>.zip");
    seal(&mut v, "reportFingerprint");
    let out = channel_inspection_export(&v).unwrap();
    assert!(!out["files"][1]["content"]
        .as_str()
        .unwrap()
        .contains("\n```"));
    v["status"] = json!("blocked");
    assert!(channel_inspection_export(&v).is_err());
}
#[test]
fn channel_inspection_rejects_resealed_profile_conflicts_and_invalid_receipt_shapes() {
    let mut p = comparison_fixture();
    alter_file(&mut p, "baseline-manifest.json", |v| {
        v["profile"] = json!("google-merchant-basic-v1");
        seal(v, "packageId");
    });
    assert!(inspect_channel_report(BRAND, &p).is_err());
    for kind in 0..8 {
        let mut p = receipt_package("receipt-check", &["accepted", "unknown"]);
        alter_file(&mut p, "simulated-receipt.json", |v| match kind {
            0 => {
                v.as_object_mut().unwrap().remove("rows");
            }
            1 => v["extra"] = json!("SECRET"),
            2 => v["rows"][0]["extra"] = json!("SECRET"),
            3 => v["rows"][0]["sku"] = json!("SECRET-UNRECOGNIZED"),
            4 => v["rows"][0]["rowHash"] = json!("0".repeat(64)),
            5 => {
                let r = v["rows"][0].clone();
                v["rows"].as_array_mut().unwrap().push(r);
            }
            6 => v["brandId"] = json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
            _ => v["rows"][0]["status"] = json!("success"),
        });
        let e = inspect_channel_report(BRAND, &p).err().unwrap();
        assert_eq!(e, "历史通道包证据无效、归属不符或超过处理上限");
        assert!(!e.contains("SECRET"));
    }
    let mut p = receipt_package("receipt-check", &["accepted"]);
    let row = p["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["path"] == "simulated-receipt.json")
        .unwrap();
    let s = row["content"].as_str().unwrap().to_owned();
    row["content"] = json!(s.replacen("\"sku\":", "\"sku\":\"duplicate\",\"sku\":", 1));
    sync_size(&mut p);
    assert!(inspect_channel_report(BRAND, &p).is_err());
}
#[test]
fn channel_inspection_export_validates_comparison_counts_and_nested_check_whitelist() {
    let out = inspect_channel_report(BRAND, &comparison_fixture()).unwrap();
    assert!(channel_inspection_export(&out.report).is_ok());
    for field in ["added", "removed", "changed", "unchanged"] {
        let mut r = out.report.clone();
        r["summary"][field] = json!(2);
        seal(&mut r, "reportFingerprint");
        assert!(channel_inspection_export(&r).is_err());
    }
    let mut r = out.report.clone();
    r["checks"][1]["raw"] = json!("SECRET");
    seal(&mut r, "reportFingerprint");
    assert!(channel_inspection_export(&r).is_err());
    let mut r = out.report.clone();
    r["source"]["sha256"] = json!("c".repeat(64));
    assert!(channel_inspection_export(&r).is_err());
    seal(&mut r, "reportFingerprint");
    assert!(channel_inspection_export(&r).is_ok());
    assert_ne!(r["reportFingerprint"], out.report["reportFingerprint"]);
}

#[test]
fn channel_inspection_rejects_resealed_padded_profile() {
    for profile in [
        " mgeo-catalog-v1",
        "mgeo-catalog-v1 ",
        " google-merchant-basic-v1 ",
    ] {
        let mut p = comparison_fixture();
        for file in ["baseline-manifest.json", "feed-manifest.json"] {
            alter_file(&mut p, file, |v| {
                v["profile"] = json!(profile);
                seal(v, "packageId");
            });
        }
        assert!(
            inspect_channel_report(BRAND, &p).is_err(),
            "accepted padded profile {profile:?}"
        );
    }
}
#[test]
fn channel_inspection_rejects_resealed_padded_local_status() {
    for file in ["baseline-manifest.json", "feed-manifest.json"] {
        for status in [" ready", "ready ", " blocked "] {
            let mut p = comparison_fixture();
            alter_file(&mut p, file, |v| {
                v["items"][0]["localStatus"] = json!(status);
                seal(&mut v["items"][0], "rowHash");
                seal(v, "packageId");
            });
            assert!(
                inspect_channel_report(BRAND, &p).is_err(),
                "accepted padded status {status:?} in {file}"
            );
        }
    }
}
