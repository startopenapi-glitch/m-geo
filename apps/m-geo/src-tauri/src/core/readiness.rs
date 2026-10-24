//! Local completeness only; never a reachability or merchant eligibility check.
use super::*;
pub(super) fn evaluate(base: Option<&str>, products: &[(String, Vec<Value>)]) -> Value {
    let mut checks = vec![
        json!({"code":"domain","status":if base.is_some(){"pass"}else{"blocked"},"message":if base.is_some(){"已配置预期地址，仍需人工发布核验"}else{"缺少官网地址；仅可导出本地预览"}}),
    ];
    let today = Utc::now().date_naive();
    for (id, fs) in products {
        let has_brand = catalog::public_brand(fs, today).is_some();
        checks.push(json!({"code":"brand","status":if has_brand{"pass"}else{"warning"},"message":if has_brand{"已提供有效的公开商品品牌依据"}else{"缺少有效的公开商品品牌；请核对唯一品牌、公开来源及有效性"},"productId":id}));
        for (key, message) in [
            ("image", "缺少公开商品图片"),
            ("audience", "缺少适用人群"),
            ("scenario", "缺少使用场景"),
            ("limits", "缺少使用限制"),
            ("afterSales", "缺少售后说明"),
        ] {
            checks.push(json!({"code":key,"status":if commerce::get(fs,key).is_some(){"pass"}else{"warning"},"message":if commerce::get(fs,key).is_some(){"已提供公开资料"}else{message},"productId":id}));
        }
        let complete = ["price", "currency", "purchase", "availability"]
            .iter()
            .all(|k| commerce::get(fs, k).is_some())
            && commerce::get(fs, "availability") != Some("Unknown")
            && fs
                .iter()
                .any(|f| f["id"] == "commerce:price" && f["validUntil"].as_str().is_some());
        checks.push(json!({"code":"offer","status":if complete{"pass"}else{"warning"},"message":if complete{"购买信息完整"}else{"缺少完整价格、币种、库存、购买链接或有效期"},"productId":id}));
    }
    let warnings = checks.iter().filter(|c| c["status"] == "warning").count();
    let blockers = checks.iter().filter(|c| c["status"] == "blocked").count();
    json!({"status":if base.is_none(){"preview"}else if warnings>0||blockers>0{"needs_attention"}else{"ready_for_manual_review"},"checks":checks,"warnings":warnings,"blockers":blockers})
}
pub(super) fn catalog_csv(products: &[Value]) -> String {
    fn cell(v: &Value) -> String {
        let s = v.as_str().map(str::to_owned).unwrap_or_default();
        let s = s.replace(['\r', '\n'], " ");
        let guarded = if s.starts_with(|c: char| c.is_whitespace() || c.is_control())
            || s.starts_with(['=', '+', '-', '@'])
        {
            format!("'{s}")
        } else {
            s
        };
        format!("\"{}\"", guarded.replace('"', "\"\""))
    }
    let mut rows=vec!["SKU,title,url,purchaseUrl,imageUrl,price,currency,availability,priceValidUntil,sourceRevision".to_string()];
    for p in products {
        let offer = &p["offer"];
        let rev = p["sourceRevision"]
            .as_i64()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let values = [
            p["sku"].clone(),
            p["title"].clone(),
            p["url"].clone(),
            p["purchaseUrl"].clone(),
            p["imageUrl"].clone(),
            offer["price"].clone(),
            offer["priceCurrency"].clone(),
            offer["availability"]
                .as_str()
                .map(|s| json!(s.trim_start_matches("https://schema.org/")))
                .unwrap_or(Value::Null),
            offer["priceValidUntil"].clone(),
            json!(rev),
        ];
        rows.push(values.iter().map(cell).collect::<Vec<_>>().join(","));
    }
    rows.join("\r\n") + "\r\n"
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brand_warning_keeps_partial_preview_available_without_leaking_private_data() {
        let facts = vec![
            json!({"id":"catalog:brand","value":"PRIVATE_BRAND_SECRET","visibility":"private","source":"PRIVATE_SOURCE_SECRET"}),
        ];
        let report = evaluate(None, &[("fictional".into(), facts)]);
        assert_eq!(report["status"], "preview");
        assert!(report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "brand" && c["status"] == "warning"));
        assert!(!report.to_string().contains("PRIVATE_BRAND_SECRET"));
        assert!(!report.to_string().contains("PRIVATE_SOURCE_SECRET"));
    }
    #[test]
    fn complete_local_facts_only_allow_manual_review() {
        let mut facts: Vec<Value> = ["image","audience","scenario","limits","afterSales","price","currency","purchase","availability"].iter().map(|key|json!({"id":format!("commerce:{key}"),"value":if *key=="availability"{"InStock"}else{"fixture"},"validUntil":"2099-01-01"})).collect();
        facts.push(json!({"id":"catalog:brand","value":"Fictional Maker","source":"Fictional source","visibility":"public","validUntil":null}));
        let report = evaluate(Some("https://example.com/"), &[("fictional".into(), facts)]);
        assert_eq!(report["status"], "ready_for_manual_review");
        assert_eq!(report["warnings"], 0);
        assert_eq!(report["blockers"], 0);
    }
    #[test]
    fn domain_and_missing_fields_are_local_only() {
        let ps = vec![("fictional".into(), vec![])];
        let r = evaluate(None, &ps);
        assert_eq!(r["status"], "preview");
        assert_eq!(r["blockers"], 1);
        assert_eq!(r["warnings"], 7);
        assert_eq!(
            evaluate(Some("https://example.com/"), &ps)["status"],
            "needs_attention"
        );
    }
    #[test]
    fn csv_escapes_without_inventing_offers() {
        let s = catalog_csv(&[json!({"sku":"=1+1","title":"a\n\"b","sourceRevision":2})]);
        assert!(s.contains("\"'=1+1\""));
        assert!(s.contains("a \"\"b"));
        assert!(!s.contains("Unknown"));
    }
}
