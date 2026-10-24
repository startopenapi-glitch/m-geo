//! Bounded, read-only local comparisons and simulated receipt checks.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const MAX_INPUT: usize = 2 * 1024 * 1024;
const MAX_EXPORT: usize = 16 * 1024 * 1024;
const RECEIPT_ERROR: &str = "模拟回执格式、范围或快照绑定无效，请重新生成或核对文件";
// Typed decoding rejects duplicate as well as unknown envelope/row fields.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReceiptImport {
    format: String,
    mode: String,
    brand_id: String,
    profile: String,
    package_id: String,
    rows: Vec<ReceiptRowImport>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReceiptRowImport {
    sku: String,
    row_hash: String,
    status: String,
    code: String,
    message: String,
}
// Parse every object ourselves: Value's default decoder discards duplicate members
// before envelope and digest validation can inspect them, including nested data.
struct UniqueJson(Value);
impl<'de> serde::Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> serde::de::Visitor<'de> for JsonVisitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("JSON with unique object members")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                value: bool,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Bool(value)))
            }
            fn visit_i64<E: serde::de::Error>(
                self,
                value: i64,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Number(value.into())))
            }
            fn visit_u64<E: serde::de::Error>(
                self,
                value: u64,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Number(value.into())))
            }
            fn visit_f64<E: serde::de::Error>(
                self,
                value: f64,
            ) -> std::result::Result<UniqueJson, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| UniqueJson(Value::Number(number)))
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::String(value.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                value: String,
            ) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::String(value)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<UniqueJson, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueJson>()? {
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut members: A,
            ) -> std::result::Result<UniqueJson, A::Error> {
                let mut object = serde_json::Map::new();
                while let Some(key) = members.next_key::<String>()? {
                    if object.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate JSON member"));
                    }
                    object.insert(key, members.next_value::<UniqueJson>()?.0);
                }
                Ok(UniqueJson(Value::Object(object)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}
fn imported(p: &Value, key: &str) -> Result<Value> {
    let input = p[key].as_str().ok_or("请提供本地 JSON 文本")?;
    if input.is_empty() || input.len() > MAX_INPUT {
        return Err("本地 JSON 文本为空或超过2MiB".into());
    }
    if key == "content" {
        let receipt: ReceiptImport = serde_json::from_str(input).map_err(|_| RECEIPT_ERROR)?;
        serde_json::to_value(receipt).map_err(|_| RECEIPT_ERROR.into())
    } else {
        serde_json::from_str::<UniqueJson>(input)
            .map(|value| value.0)
            .map_err(|_| "本地 JSON 文本格式无效".into())
    }
}
fn request(p: &Value) -> Result<Value> {
    if p.to_string().len() > MAX_INPUT {
        return Err("本地操作请求超过2MiB".into());
    }
    Ok(json!({"brandId":p["brandId"],"profile":p["profile"],"items":p["items"]}))
}
fn current(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    feed::prepare(c, b, &request(p)?)
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let object = v.as_object().ok_or(RECEIPT_ERROR)?;
    if object.len() != keys.len() || keys.iter().any(|k| !object.contains_key(*k)) {
        return Err(RECEIPT_ERROR.into());
    }
    Ok(())
}
fn receipt_text<'a>(v: &'a Value, key: &str, limit: usize) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| {
            !s.trim().is_empty()
                && s.chars().count() <= limit
                && !s
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        })
        .ok_or_else(|| RECEIPT_ERROR.into())
}
fn files(entries: Vec<(&str, Value)>, notice: &str) -> Result<Value> {
    let mut rows = vec![];
    let mut bytes = notice.len();
    for (path, value) in entries {
        if matches!(path, "feed-manifest.json" | "baseline-manifest.json") {
            feed::validate_export_roundtrip(&value)?;
        }
        let content = serde_json::to_string_pretty(&value).map_err(|_| "无法生成本地报告")?;
        bytes = bytes.checked_add(content.len()).ok_or("本地报告过大")?;
        rows.push(json!({"path":path,"content":content}));
    }
    if bytes > MAX_EXPORT {
        return Err("本地报告超过16MiB".into());
    }
    rows.push(json!({"path":"local-operation-notice.txt","content":notice}));
    Ok(json!(rows))
}
fn changed_fields(previous: &Value, next: &Value) -> Vec<String> {
    let mut fields = BTreeSet::new();
    for key in [
        "sku",
        "productRevision",
        "draftId",
        "draftRevision",
        "localStatus",
        "validity",
    ] {
        if previous[key] != next[key] {
            fields.insert(key.to_owned());
        }
    }
    for key in [
        "id",
        "title",
        "description",
        "link",
        "imageLink",
        "price",
        "currency",
        "availability",
    ] {
        if previous["data"][key] != next["data"][key] {
            fields.insert(key.to_owned());
        }
    }
    let empty = serde_json::Map::new();
    let old = previous["data"]["catalog"].as_object().unwrap_or(&empty);
    let new = next["data"]["catalog"].as_object().unwrap_or(&empty);
    for key in old.keys().chain(new.keys()) {
        if old.get(key) != new.get(key) {
            fields.insert(format!("catalog.{key}"));
        }
    }
    fields.into_iter().collect()
}
pub(super) fn comparison_report(baseline: &Value, manifest: &Value) -> Result<Value> {
    let index = |m: &Value| -> Result<BTreeMap<String, Value>> {
        m["items"]
            .as_array()
            .ok_or("本地商品清单无效")?
            .iter()
            .map(|row| {
                Ok((
                    row["productId"]
                        .as_str()
                        .ok_or("本地商品标识无效")?
                        .to_owned(),
                    row.clone(),
                ))
            })
            .collect()
    };
    let old = index(baseline)?;
    let new = index(manifest)?;
    let ids: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let mut summary = json!({"added":0,"removed":0,"changed":0,"unchanged":0});
    let mut rows = vec![];
    for id in ids {
        let (row, change, fields) = match (old.get(id), new.get(id)) {
            (None, Some(next)) => (next, "added", vec![]),
            (Some(previous), None) => (previous, "removed", vec![]),
            (Some(previous), Some(next)) => {
                let fields = changed_fields(previous, next);
                (
                    next,
                    if fields.is_empty() {
                        "unchanged"
                    } else {
                        "changed"
                    },
                    fields,
                )
            }
            _ => unreachable!(),
        };
        summary[change] = json!(summary[change].as_u64().unwrap_or(0) + 1);
        rows.push(json!({"sku":row["sku"],"change":change,"fields":fields}));
    }
    let notice = operation_notice("comparison")?;
    let report = json!({"mode":"local_draft","submissionStatus":"not_submitted","selectionScope":"selected_products_not_full_merchant_catalog","summary":summary,"rows":rows,"notice":notice});
    Ok(report)
}
pub(super) fn compare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(p)?;
    let baseline = imported(p, "baseline")?;
    feed::validate_manifest(&baseline, b)?;
    let package = current(c, b, p)?;
    let manifest = &package["manifest"];
    if baseline["profile"] != manifest["profile"] {
        return Err("请选择相同品牌与格式的本地基线清单".into());
    }
    let report = comparison_report(&baseline, manifest)?;
    let summary = report["summary"].clone();
    let rows = report["rows"].clone();
    let notice = operation_notice("comparison")?;
    let export = files(
        vec![
            ("comparison-report.json", report),
            ("baseline-manifest.json", baseline),
            ("feed-manifest.json", manifest.clone()),
        ],
        notice,
    )?;
    Ok(json!({"summary":summary,"rows":rows,"files":export}))
}
pub(super) fn operation_notice(operation: &str) -> Result<&'static str> {
    match operation {
        "comparison" => Ok("仅比较本次本地选择。移除表示不在当前本地选择中，不代表平台删除，也不是完整商家目录替换授权。所有商品尚未提交平台；摘要用于完整性核对，不是签名或认证。\n"),
        "simulation" => Ok("本机模拟回执，商品尚未提交平台。模拟接受、拒绝、未知均不证明真实平台接受、收录、推荐或拒绝。未写入提交或发布状态。摘要不是平台签名。\n"),
        "receipt-check" => Ok("仅核对绑定当前本地快照的模拟回执。商品尚未提交平台；缺失行保持缺失，未知保持未知，均不能推定真实成功。摘要不是平台认证或签名。\n"),
        _ => Err(RECEIPT_ERROR.into()),
    }
}
fn simulation(status: &str) -> Result<(&'static str, &'static str)> {
    match status {
        "accepted" => Ok(("SIMULATED_ACCEPTED", "本机模拟接受，不代表平台真实接受")),
        "rejected" => Ok((
            "SIMULATED_POLICY_REJECTION",
            "本机模拟策略拒绝，不代表平台真实判定",
        )),
        "unknown" => Ok(("SIMULATED_UNKNOWN", "本机模拟结果未知，不能推定成功或失败")),
        _ => Err("请选择接受、拒绝或未知的本地模拟场景".into()),
    }
}
fn summary(rows: &[Value]) -> Value {
    let mut out = json!({"accepted":0,"rejected":0,"unknown":0,"missing":0});
    for row in rows {
        if let Some(status) = row["status"].as_str() {
            if out.get(status).is_some() {
                out[status] = json!(out[status].as_u64().unwrap_or(0) + 1);
            }
        }
    }
    out
}
fn ready(package: &Value) -> Result<()> {
    if package["summary"]["blocked"].as_u64() != Some(0) {
        return Err("所有选中商品均通过当前本地检查后才能生成或核对模拟回执".into());
    }
    Ok(())
}
pub(super) fn simulation_receipt(manifest: &Value, b: &str, status: &str) -> Result<Value> {
    let (code, message) = simulation(status)?;
    let rows: Vec<Value> = manifest["items"].as_array().ok_or("本地清单无效")?.iter().map(|row| json!({"sku":row["sku"],"rowHash":row["rowHash"],"status":status,"code":code,"message":message})).collect();
    Ok(
        json!({"format":"m-geo-feed-receipt-v1","mode":"simulated","brandId":b,"profile":manifest["profile"],"packageId":manifest["packageId"],"rows":rows}),
    )
}
pub(super) fn simulation_report(receipt: &Value) -> Result<Value> {
    let rows = receipt["rows"].as_array().ok_or(RECEIPT_ERROR)?;
    Ok(
        json!({"mode":"simulated","submissionStatus":"not_submitted","summary":summary(rows),"rows":rows}),
    )
}
pub(super) fn simulate(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let status = p["scenario"].as_str().ok_or("请选择本地模拟场景")?;
    simulation(status)?;
    let package = current(c, b, p)?;
    ready(&package)?;
    let manifest = &package["manifest"];
    let receipt = simulation_receipt(manifest, b, status)?;
    let report = simulation_report(&receipt)?;
    let summary = report["summary"].clone();
    let notice = operation_notice("simulation")?;
    let export = files(
        vec![
            ("simulated-receipt.json", receipt.clone()),
            ("simulation-report.json", report),
            ("feed-manifest.json", manifest.clone()),
        ],
        notice,
    )?;
    Ok(json!({"receipt":receipt,"summary":summary,"files":export}))
}
fn validate_receipt(receipt: &Value, manifest: &Value, b: &str) -> Result<BTreeMap<String, Value>> {
    exact(
        receipt,
        &["format", "mode", "brandId", "profile", "packageId", "rows"],
    )?;
    if receipt["format"] != "m-geo-feed-receipt-v1"
        || receipt["mode"] != "simulated"
        || receipt["brandId"] != b
        || receipt["profile"] != manifest["profile"]
        || receipt["packageId"] != manifest["packageId"]
    {
        return Err(RECEIPT_ERROR.into());
    }
    let rows = receipt["rows"]
        .as_array()
        .filter(|r| r.len() <= 200)
        .ok_or(RECEIPT_ERROR)?;
    let expected: BTreeMap<&str, &Value> = manifest["items"]
        .as_array()
        .ok_or(RECEIPT_ERROR)?
        .iter()
        .map(|row| (row["sku"].as_str().unwrap_or(""), row))
        .collect();
    let mut received = BTreeMap::new();
    for row in rows {
        exact(row, &["sku", "rowHash", "status", "code", "message"])?;
        let sku = receipt_text(row, "sku", 100)?;
        let hash = receipt_text(row, "rowHash", 64)?;
        let status = receipt_text(row, "status", 20)?;
        let code = receipt_text(row, "code", 80)?;
        receipt_text(row, "message", 500)?;
        let (expected_code, _) = simulation(status).map_err(|_| RECEIPT_ERROR)?;
        let expected_row = expected.get(sku).ok_or(RECEIPT_ERROR)?;
        if expected_row["rowHash"] != hash
            || code != expected_code
            || received.insert(sku.to_owned(), row.clone()).is_some()
        {
            return Err(RECEIPT_ERROR.into());
        }
    }
    Ok(received)
}
pub(super) fn receipt_report(receipt: &Value, manifest: &Value, b: &str) -> Result<Value> {
    let received = validate_receipt(receipt, manifest, b)?;
    let mut rows = vec![];
    for row in manifest["items"].as_array().ok_or("本地清单无效")? {
        let sku = row["sku"].as_str().ok_or("本地商品标识无效")?;
        rows.push(received.get(sku).cloned().unwrap_or_else(|| json!({"sku":sku,"rowHash":row["rowHash"],"status":"missing","code":"SIMULATED_MISSING","message":"模拟回执中缺失此商品，不能推定接受"})));
    }
    let summary = summary(&rows);

    let report = json!({"mode":"simulated","submissionStatus":"not_submitted","summary":summary,"rows":rows});
    Ok(report)
}
pub(super) fn check_receipt(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(p)?;
    let receipt = imported(p, "content")?;
    let package = current(c, b, p)?;
    ready(&package)?;
    let manifest = &package["manifest"];
    let report = receipt_report(&receipt, manifest, b)?;
    let summary = report["summary"].clone();
    let rows = report["rows"].clone();
    let notice = operation_notice("receipt-check")?;
    let export = files(
        vec![
            ("receipt-check-report.json", report),
            ("simulated-receipt.json", receipt),
            ("feed-manifest.json", manifest.clone()),
        ],
        notice,
    )?;
    Ok(json!({"summary":summary,"rows":rows,"files":export}))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, String, Value) {
        fixture_with(3, "经过本地审核的虚构商品说明")
    }
    fn fixture_with(count: usize, body: &str) -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构回执品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut items = vec![];
        for n in 1..=count {
            let facts:Vec<Value>=[("catalog:kind","standalone"),("catalog:condition","new"),("catalog:productClass","physical_non_apparel"),("catalog:titleOrigin","human"),("catalog:brand","虚构制造品牌"),("catalog:mpn","FICTIONAL-MPN"),("commerce:price","29.90"),("commerce:currency","CNY"),("commerce:availability","InStock"),("commerce:purchase","https://example.com/item"),("commerce:image","https://example.com/image.png")].iter().map(|(k,v)|json!({"id":k,"label":k,"value":v,"source":"SOURCE_SECRET","visibility":"public","validUntil":"2099-01-01"})).collect();
            let prod=handle(&mut c,"product.save",json!({"brandId":b,"name":format!("虚构台灯{n}"),"sku":format!("FICTIONAL-{n}"),"category":"灯","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":facts})).unwrap();
            handle(
                &mut c,
                "product.confirm",
                json!({"brandId":b,"id":prod["id"],"expectedRevision":1}),
            )
            .unwrap();
            let d = handle(
                &mut c,
                "draft.generateLocal",
                json!({"brandId":b,"productId":prod["id"],"kind":"description"}),
            )
            .unwrap();
            let d=handle(&mut c,"draft.save",json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"],"title":"虚构商品说明","body":body})).unwrap();
            let d = handle(
                &mut c,
                "draft.review",
                json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
            )
            .unwrap();
            items.push(json!({"productId":prod["id"],"expectedRevision":1,"draftId":d["id"],"expectedDraftRevision":d["revision"]}));
        }
        (
            c,
            b.clone(),
            json!({"brandId":b,"profile":"mgeo-catalog-v1","items":items}),
        )
    }
    fn input(p: &Value, key: &str, value: Value) -> Value {
        let mut out = p.clone();
        out[key] = value;
        out
    }
    fn edit_price(c: &mut Connection, b: &str, item: &mut Value) {
        let mut prod = load(c, "m_products", b, item["productId"].as_str().unwrap()).unwrap();
        prod["expectedRevision"] = prod["revision"].clone();
        prod["facts"][6]["value"] = json!("39.90");
        prod["facts"][6]["validUntil"] = json!("2099-02-01");
        let prod = handle(c, "product.save", prod).unwrap();
        handle(
            c,
            "product.confirm",
            json!({"brandId":b,"id":prod["id"],"expectedRevision":prod["revision"]}),
        )
        .unwrap();
        let d = handle(
            c,
            "draft.generateLocal",
            json!({"brandId":b,"productId":prod["id"],"kind":"description"}),
        )
        .unwrap();
        let d=handle(c,"draft.save",json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"],"title":"虚构新商品说明","body":"价格更新后经过本地审核的虚构商品说明"})).unwrap();
        let d = handle(
            c,
            "draft.review",
            json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"]}),
        )
        .unwrap();
        *item = json!({"productId":prod["id"],"expectedRevision":prod["revision"],"draftId":d["id"],"expectedDraftRevision":d["revision"]});
    }
    #[test]
    fn ops_exported_manifests_roundtrip_through_dispatch_at_escaped_boundary() {
        let body = "quoted: \"fictional\"\\\n".repeat(1200);
        let (mut c, b, all) = fixture_with(65, &body);
        let mut last = None;
        let mut rejected = false;
        for count in 40..=65 {
            let mut request = all.clone();
            request["items"] = json!(&all["items"].as_array().unwrap()[..count]);
            match handle(&mut c, "feed.prepare", request.clone()) {
                Ok(package) => last = Some((request, package, count)),
                Err(message) => {
                    assert!(message.contains("减少商品数量"), "{message}");
                    rejected = true;
                    break;
                }
            }
        }
        assert!(
            rejected,
            "oversize escaped re-import must be rejected before export"
        );
        let (request, package, count) = last.unwrap();
        let exact_file = |value: &Value, path: &str| {
            use std::io::Read;
            let zip = crate::files::site_zip(value).unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
            let mut content = String::new();
            archive
                .by_name(path)
                .unwrap()
                .read_to_string(&mut content)
                .unwrap();
            content
        };
        let compact = exact_file(&package, "feed-manifest.json");
        let mut import = input(&request, "baseline", json!(compact));
        import["operation"] = json!("compare");
        assert!(
            import.to_string().len() > 1_900_000,
            "fixture must reach the boundary"
        );
        eprintln!("boundary: accepted_items={count}, compact_file_bytes={}, compact_request_bytes={}, next_selection_rejected={}", compact.len(), import.to_string().len(), count + 1);
        let compared = handle(&mut c, "feed.compare", import).unwrap();
        assert_eq!(compared["summary"]["unchanged"], count);
        for path in ["baseline-manifest.json", "feed-manifest.json"] {
            let pretty = exact_file(&compared, path);
            assert!(pretty.contains('\n'));
            assert!(pretty.len() > compact.len());
            let mut import = input(&request, "baseline", json!(pretty));
            import["operation"] = json!("compare");
            assert!(import.to_string().len() <= 2 * 1024 * 1024);
            eprintln!(
                "{path}: pretty_file_bytes={}, request_bytes={}",
                pretty.len(),
                import.to_string().len()
            );
            let roundtrip = handle(&mut c, "feed.compare", import).unwrap();
            assert_eq!(roundtrip["summary"]["unchanged"], count);
        }
        // A previously created baseline may fit the compact import but fail the
        // conservative pretty export budget. Check that export path separately
        // with a small current selection, so current preparation cannot mask it.
        let mut single = all.clone();
        single["items"] = json!([all["items"][count]]);
        let extra = handle(&mut c, "feed.prepare", single.clone()).unwrap();
        let mut historical = package["manifest"].clone();
        let rows = historical["items"].as_array_mut().unwrap();
        rows.push(extra["manifest"]["items"][0].clone());
        rows.sort_by_key(|row| row["productId"].as_str().unwrap().to_owned());
        historical.as_object_mut().unwrap().remove("packageId");
        historical["packageId"] = json!(fingerprint(&historical));
        feed::validate_manifest(&historical, &b).unwrap();
        let mut imported = input(&single, "baseline", json!(historical.to_string()));
        imported["operation"] = json!("compare");
        assert!(imported.to_string().len() <= 2 * 1024 * 1024);
        let message = handle(&mut c, "feed.compare", imported).unwrap_err();
        assert!(message.contains("减少商品数量"), "{message}");
        let simulated = handle(
            &mut c,
            "feed.simulate",
            input(&request, "scenario", json!("accepted")),
        )
        .unwrap();
        let receipt = exact_file(&simulated, "simulated-receipt.json");
        let checked = handle(
            &mut c,
            "feed.checkReceipt",
            input(&request, "content", json!(receipt)),
        )
        .unwrap();
        for package in [simulated, checked] {
            let mut import = input(
                &request,
                "baseline",
                json!(exact_file(&package, "feed-manifest.json")),
            );
            import["operation"] = json!("compare");
            assert_eq!(
                handle(&mut c, "feed.compare", import).unwrap()["summary"]["unchanged"],
                count
            );
        }
    }
    #[test]
    fn ops_legacy_catalog_removal_recovers_normal_saved_product_readiness() {
        let (mut c, b, mut request) = fixture();
        request["items"] = json!([request["items"][0]]);
        let mut product = load(
            &c,
            "m_products",
            &b,
            request["items"][0]["productId"].as_str().unwrap(),
        )
        .unwrap();
        let legacy = json!({"id":"catalog:legacyCode","label":"Legacy code","value":"FICTIONAL-LEGACY","source":"Fictional legacy record","visibility":"private","validUntil":null});
        product["facts"]
            .as_array_mut()
            .unwrap()
            .push(legacy.clone());
        product["expectedRevision"] = product["revision"].clone();
        let mut saved = handle(&mut c, "product.save", product).unwrap();
        assert!(saved["facts"].as_array().unwrap().contains(&legacy));
        assert!(saved["confirmedAt"].is_null());
        let report = handle(&mut c, "catalog.inspect", json!({"brandId":b})).unwrap();
        assert_eq!(report["summary"]["blocked"], 1);
        assert!(!report.to_string().contains("FICTIONAL-LEGACY"));
        handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":saved["id"],"expectedRevision":2}),
        )
        .unwrap();
        let draft = handle(
            &mut c,
            "draft.generateLocal",
            json!({"brandId":b,"productId":saved["id"],"kind":"description"}),
        )
        .unwrap();
        let draft = handle(
            &mut c,
            "draft.review",
            json!({"brandId":b,"id":draft["id"],"expectedRevision":draft["revision"]}),
        )
        .unwrap();
        request["items"][0] = json!({"productId":saved["id"],"expectedRevision":2,"draftId":draft["id"],"expectedDraftRevision":draft["revision"]});
        let blocked = handle(&mut c, "feed.prepare", request.clone()).unwrap();
        assert_eq!(blocked["summary"]["blocked"], 1);
        assert_eq!(blocked["manifest"]["items"][0]["data"], json!({}));
        assert!(!blocked.to_string().contains("FICTIONAL-LEGACY"));
        saved["expectedRevision"] = saved["revision"].clone();
        saved["facts"]
            .as_array_mut()
            .unwrap()
            .retain(|f| f["id"] != "catalog:legacyCode");
        let saved = handle(&mut c, "product.save", saved).unwrap();
        assert_eq!(saved["revision"], 3);
        assert!(saved["confirmedAt"].is_null());
        assert_eq!(
            handle(&mut c, "catalog.inspect", json!({"brandId":b})).unwrap()["summary"]["blocked"],
            0
        );
        handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":saved["id"],"expectedRevision":3}),
        )
        .unwrap();
        let draft = handle(
            &mut c,
            "draft.generateLocal",
            json!({"brandId":b,"productId":saved["id"],"kind":"description"}),
        )
        .unwrap();
        let draft = handle(
            &mut c,
            "draft.review",
            json!({"brandId":b,"id":draft["id"],"expectedRevision":draft["revision"]}),
        )
        .unwrap();
        request["items"][0] = json!({"productId":saved["id"],"expectedRevision":3,"draftId":draft["id"],"expectedDraftRevision":draft["revision"]});
        let ready = handle(&mut c, "feed.prepare", request).unwrap();
        assert_eq!(ready["summary"]["ready"], 1);
        assert!(!ready.to_string().contains("FICTIONAL-LEGACY"));
    }
    #[test]
    fn ops_added_removed_revision_diff() {
        let (mut c, b, all) = fixture();
        let mut previous = all.clone();
        previous["items"] = json!([all["items"][0], all["items"][1]]);
        let baseline = feed::prepare(&c, &b, &previous).unwrap()["manifest"].clone();
        let same = compare(
            &c,
            &b,
            &input(&previous, "baseline", json!(baseline.to_string())),
        )
        .unwrap();
        assert_eq!(
            same["summary"],
            json!({"added":0,"removed":0,"changed":0,"unchanged":2})
        );
        let mut current = all.clone();
        current["items"] = json!([all["items"][1], all["items"][2]]);
        edit_price(&mut c, &b, &mut current["items"][0]);
        let diff = compare(
            &c,
            &b,
            &input(&current, "baseline", json!(baseline.to_string())),
        )
        .unwrap();
        assert_eq!(
            diff["summary"],
            json!({"added":1,"removed":1,"changed":1,"unchanged":0})
        );
        let changed = diff["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["change"] == "changed")
            .unwrap();
        for field in ["price", "productRevision", "draftId", "validity"] {
            assert!(
                changed["fields"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field)),
                "missing field {field}: {changed}"
            )
        }
        assert!(diff["files"].to_string().contains("本地选择"));
        let mut other = current.clone();
        other["profile"] = json!("google-merchant-basic-v1");
        assert!(compare(
            &c,
            &b,
            &input(&other, "baseline", json!(baseline.to_string()))
        )
        .is_err());
        let mut bad = baseline.clone();
        bad["items"][0]["sku"] = json!("TAMPERED");
        assert!(compare(&c, &b, &input(&current, "baseline", json!(bad.to_string()))).is_err());
    }
    #[test]
    fn ops_receipt_partial_duplicate_unknown_tampered_stale() {
        let (mut c, b, mut p) = fixture();
        let receipt =
            simulate(&c, &b, &input(&p, "scenario", json!("accepted"))).unwrap()["receipt"].clone();
        let mut partial = receipt.clone();
        partial["rows"].as_array_mut().unwrap().pop();
        let checked =
            check_receipt(&c, &b, &input(&p, "content", json!(partial.to_string()))).unwrap();
        assert_eq!(
            checked["summary"],
            json!({"accepted":2,"rejected":0,"unknown":0,"missing":1})
        );
        assert_eq!(
            checked["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["status"] == "missing")
                .count(),
            1
        );
        let mut duplicate = receipt.clone();
        let row = duplicate["rows"][0].clone();
        duplicate["rows"].as_array_mut().unwrap().push(row);
        assert!(
            check_receipt(&c, &b, &input(&p, "content", json!(duplicate.to_string()))).is_err()
        );
        for (path, value) in [
            ("sku", json!("UNKNOWN")),
            ("rowHash", json!("0".repeat(64))),
            ("status", json!("real_accepted")),
            ("code", json!("REAL_ACCEPTED")),
            ("message", json!("x".repeat(501))),
        ] {
            let mut bad = receipt.clone();
            bad["rows"][0][path] = value;
            assert!(
                check_receipt(&c, &b, &input(&p, "content", json!(bad.to_string()))).is_err(),
                "accepted bad {path}"
            )
        }
        for (path, value) in [
            ("brandId", json!(id())),
            ("profile", json!("google-merchant-basic-v1")),
            ("packageId", json!("0".repeat(64))),
            ("mode", json!("real")),
            ("submissionStatus", json!("submitted")),
        ] {
            let mut bad = receipt.clone();
            bad[path] = value;
            assert!(
                check_receipt(&c, &b, &input(&p, "content", json!(bad.to_string()))).is_err(),
                "accepted bad envelope {path}"
            )
        }
        edit_price(&mut c, &b, &mut p["items"][0]);
        assert!(check_receipt(&c, &b, &input(&p, "content", json!(receipt.to_string()))).is_err());
    }
    #[test]
    fn ops_simulated_scenarios_and_empty_receipt() {
        let (c, b, p) = fixture();
        for status in ["accepted", "rejected", "unknown"] {
            let out = simulate(&c, &b, &input(&p, "scenario", json!(status))).unwrap();
            assert_eq!(out["receipt"]["mode"], "simulated");
            assert_eq!(out["summary"][status], 3);
            let verified = check_receipt(
                &c,
                &b,
                &input(&p, "content", json!(out["receipt"].to_string())),
            )
            .unwrap();
            assert_eq!(verified["summary"][status], 3)
        }
        let mut receipt =
            simulate(&c, &b, &input(&p, "scenario", json!("unknown"))).unwrap()["receipt"].clone();
        receipt["rows"] = json!([]);
        assert_eq!(
            check_receipt(&c, &b, &input(&p, "content", json!(receipt.to_string()))).unwrap()
                ["summary"],
            json!({"accepted":0,"rejected":0,"unknown":0,"missing":3})
        );
        assert!(simulate(&c, &b, &input(&p, "scenario", json!("real"))).is_err());
        let mut prod = load(
            &c,
            "m_products",
            &b,
            p["items"][0]["productId"].as_str().unwrap(),
        )
        .unwrap();
        prod["confirmedAt"] = Value::Null;
        update(&c, "m_products", &prod).unwrap();
        assert!(simulate(&c, &b, &input(&p, "scenario", json!("accepted"))).is_err());
    }
    #[test]
    fn ops_no_db_writes() {
        let (c, b, p) = fixture();
        let before = c.total_changes();
        c.execute_batch("PRAGMA query_only=ON").unwrap();
        let manifest = feed::prepare(&c, &b, &p).unwrap()["manifest"].clone();
        let compare = compare(&c, &b, &input(&p, "baseline", json!(manifest.to_string()))).unwrap();
        let sim = simulate(&c, &b, &input(&p, "scenario", json!("accepted"))).unwrap();
        let check = check_receipt(
            &c,
            &b,
            &input(&p, "content", json!(sim["receipt"].to_string())),
        )
        .unwrap();
        assert_eq!(before, c.total_changes());
        for out in [compare, sim, check] {
            for secret in ["SOURCE_SECRET", "RAW_SECRET", "NOTE_SECRET"] {
                assert!(!out.to_string().contains(secret))
            }
            for f in out["files"].as_array().unwrap() {
                let path = f["path"].as_str().unwrap();
                assert!(!path.contains(".."));
                assert!(!path.starts_with('/'));
            }
        }
    }
    #[test]
    fn ops_import_limits_and_errors_do_not_echo_input() {
        let (c, b, p) = fixture();
        for (key, value) in [
            ("baseline", json!("SECRET_PARSE_INPUT")),
            ("content", json!("SECRET_PARSE_INPUT")),
            ("content", json!("x".repeat(2 * 1024 * 1024 + 1))),
        ] {
            let q = input(&p, key, value);
            let error = if key == "baseline" {
                compare(&c, &b, &q)
            } else {
                check_receipt(&c, &b, &q)
            }
            .unwrap_err();
            assert!(!error.contains("SECRET_PARSE_INPUT"));
        }
        let mut receipt =
            simulate(&c, &b, &input(&p, "scenario", json!("accepted"))).unwrap()["receipt"].clone();
        receipt["rows"] = json!(vec![receipt["rows"][0].clone(); 201]);
        assert!(check_receipt(&c, &b, &input(&p, "content", json!(receipt.to_string()))).is_err());
    }
    #[test]
    fn ops_receipt_rejects_ambiguous_duplicate_json_fields() {
        let (c, b, p) = fixture();
        let receipt =
            simulate(&c, &b, &input(&p, "scenario", json!("accepted"))).unwrap()["receipt"].clone();
        let raw = receipt.to_string().replacen(
            "\"mode\":\"simulated\"",
            "\"mode\":\"real\",\"mode\":\"simulated\"",
            1,
        );
        assert!(check_receipt(&c, &b, &input(&p, "content", json!(raw))).is_err());
        let raw = receipt.to_string().replacen(
            "\"status\":\"accepted\"",
            "\"status\":\"real_accepted\",\"status\":\"accepted\"",
            1,
        );
        assert!(check_receipt(&c, &b, &input(&p, "content", json!(raw))).is_err());
    }
    fn duplicate_member(raw: &str, field: &str, value: &Value) -> String {
        let member = format!("{}:{}", json!(field), value);
        assert!(raw.contains(&member), "fixture member must exist");
        raw.replacen(
            &member,
            &format!("{}:\"IMPORTED_SECRET_AMBIGUITY\",{member}", json!(field)),
            1,
        )
    }
    #[test]
    fn ops_baseline_rejects_duplicate_envelope_keys_before_digest_validation() {
        let (c, b, p) = fixture();
        let manifest = feed::prepare(&c, &b, &p).unwrap()["manifest"].clone();
        let raw = manifest.to_string();
        assert_eq!(
            compare(&c, &b, &input(&p, "baseline", json!(raw))).unwrap()["summary"]["unchanged"],
            3
        );
        for (field, conflicting) in [
            ("mode", "real"),
            ("submissionStatus", "submitted"),
            ("packageId", "IMPORTED_SECRET_AMBIGUITY"),
        ] {
            let ambiguous = duplicate_member(&raw, field, &manifest[field])
                .replace("IMPORTED_SECRET_AMBIGUITY", conflicting);
            let result = compare(&c, &b, &input(&p, "baseline", json!(ambiguous)));
            assert!(
                result.is_err(),
                "duplicate envelope member {field} accepted"
            );
            assert!(!result.unwrap_err().contains("IMPORTED_SECRET_AMBIGUITY"));
        }
    }
    #[test]
    fn ops_baseline_rejects_duplicate_keys_recursively() {
        let (c, b, p) = fixture();
        let manifest = feed::prepare(&c, &b, &p).unwrap()["manifest"].clone();
        let raw = manifest.to_string();
        let row = &manifest["items"][0];
        for (field, value) in [
            ("sku", &row["sku"]),
            ("data", &row["data"]),
            ("price", &row["data"]["price"]),
            ("brand", &row["data"]["catalog"]["brand"]),
            ("field", &row["validity"][0]["field"]),
        ] {
            let ambiguous = duplicate_member(&raw, field, value);
            let result = compare(&c, &b, &input(&p, "baseline", json!(ambiguous)));
            assert!(result.is_err(), "duplicate nested member {field} accepted");
            assert!(!result.unwrap_err().contains("IMPORTED_SECRET_AMBIGUITY"));
        }
    }
}
