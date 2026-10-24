//! Read-only historical attribution inspection from frozen, bounded evidence.
use super::*;
use std::collections::BTreeMap;

const INVALID: &str = "归因包证据无效、归属不符或超过处理上限";
const CAVEAT: &str = "仅核对本地文件与调用方声明来源的一致性；simulated 为虚构演练，recorded 为调用方提供的记录，未经独立来源验证。不证明平台推荐、AI 带来销量或销售因果关系；哈希不是数字签名。来源和周会话数不可相加（non-additive）。包含汇总财务指标，不代表匿名或可公开发布。";
const PATHS: [&str; 9] = [
    "dataset.json",
    "input.json",
    "summary.json",
    "orders.csv",
    "sources.csv",
    "products.csv",
    "weeks.csv",
    "methodology.txt",
    "external-requirements.txt",
];
const CHECKS: [(&str, &str); 9] = [
    ("frozen_dataset", "冻结数据集结构、身份与资源范围核对"),
    ("input", "原始交换输入与冻结记录核对"),
    ("summary", "摘要与复算结果核对"),
    ("orders_csv", "订单 CSV 与复算结果核对"),
    ("sources_csv", "来源 CSV 与复算结果核对"),
    ("products_csv", "商品 CSV 与复算结果核对"),
    ("weeks_csv", "周 CSV 与复算结果核对"),
    ("methodology", "固定方法说明与元数据核对"),
    ("external_requirements", "外部接入要求说明核对"),
];

pub struct InspectedAttribution {
    pub report: Value,
    pub orders: Vec<Value>,
    pub products: Vec<Value>,
}
fn shape(v: &Value, keys: &[&str]) -> Result<()> {
    if !v
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn hash(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn canonical_time(v: &Value) -> Result<i64> {
    let s = v.as_str().ok_or(INVALID)?;
    let dt = DateTime::parse_from_rfc3339(s).map_err(|_| INVALID)?;
    if s != dt
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
    {
        return Err(INVALID.into());
    }
    Ok(dt.timestamp())
}
fn source(v: &Value) -> Result<()> {
    shape(
        v,
        &[
            "fileName",
            "sha256",
            "compressedBytes",
            "uncompressedBytes",
            "fileCount",
        ],
    )?;
    let name = v["fileName"].as_str().ok_or(INVALID)?;
    if name.is_empty()
        || name.chars().count() > 255
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\'])
        || !hash(&v["sha256"])
        || v["fileCount"] != 9
        || !v["compressedBytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 20 * 1024 * 1024)
        || !v["uncompressedBytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 16 * 1024 * 1024)
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn files(p: &Value) -> Result<BTreeMap<&str, &str>> {
    shape(p, &["source", "files"])?;
    source(&p["source"])?;
    let rows = p["files"].as_array().ok_or(INVALID)?;
    if rows.len() != 9 {
        return Err(INVALID.into());
    }
    let mut result = BTreeMap::new();
    let mut bytes = 0usize;
    for row in rows {
        shape(row, &["path", "content"])?;
        let path = row["path"].as_str().ok_or(INVALID)?;
        let content = row["content"].as_str().ok_or(INVALID)?;
        bytes = bytes.checked_add(content.len()).ok_or(INVALID)?;
        if !PATHS.contains(&path)
            || content.contains('\0')
            || result.insert(path, content).is_some()
            || bytes > 16 * 1024 * 1024
        {
            return Err(INVALID.into());
        }
    }
    if p["source"]["uncompressedBytes"].as_u64() != Some(bytes as u64) {
        return Err(INVALID.into());
    }
    Ok(result)
}
fn no_nul(v: &Value) -> bool {
    match v {
        Value::String(s) => !s.contains('\0'),
        Value::Array(a) => a.iter().all(no_nul),
        Value::Object(o) => o.iter().all(|(k, v)| !k.contains('\0') && no_nul(v)),
        _ => true,
    }
}
fn mode_label(mode: &Value) -> Result<&'static str> {
    match mode.as_str() {
        Some("simulated") => Ok("虚构演练"),
        Some("recorded") => Ok("调用方提供的记录"),
        _ => Err(INVALID.into()),
    }
}
fn period(period: &Value, coverage: &Value) -> Result<()> {
    shape(period, &["from", "to", "refundAsOf", "lookbackDays"])?;
    shape(coverage, &["from", "to"])?;
    let start = canonical_time(&coverage["from"])?;
    let end = canonical_time(&coverage["to"])?;
    let from = canonical_time(&period["from"])?;
    let to = canonical_time(&period["to"])?;
    if start >= end
        || end - start > 366 * 86400
        || from < start
        || to > end
        || from >= to
        || period["refundAsOf"] != coverage["to"]
        || !period["lookbackDays"]
            .as_u64()
            .is_some_and(|n| (1..=30).contains(&n))
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn inspect(brand_id: &str, p: &Value) -> Result<InspectedAttribution> {
    Uuid::parse_str(brand_id).map_err(|_| INVALID)?;
    let files = files(p)?;
    if files["input.json"].len() > 512 * 1024 {
        return Err(INVALID.into());
    }
    let dataset = strict_json::parse(files["dataset.json"])?;
    let input = strict_json::parse(files["input.json"])?;
    let supplied = strict_json::parse(files["summary.json"])?;
    if !no_nul(&dataset) || !no_nul(&input) || !no_nul(&supplied) || input != dataset["input"] {
        return Err(INVALID.into());
    }
    attribution::validate_portable_dataset(brand_id, &dataset)?;
    for (key, value) in [
        ("datasetId", &dataset["id"]),
        ("contentHash", &dataset["contentHash"]),
        ("evidenceMode", &input["evidenceMode"]),
        ("currency", &input["currency"]),
    ] {
        if &supplied[key] != value {
            return Err(INVALID.into());
        }
    }
    if supplied.get("brandId").is_some_and(|v| v != brand_id) {
        return Err(INVALID.into());
    }
    let coverage = json!({"from":input["coverageStart"],"to":input["coverageEnd"]});
    period(&supplied["period"], &coverage)?;
    let parameters = json!({"brandId":brand_id,"datasetId":dataset["id"],"from":supplied["period"]["from"],"to":supplied["period"]["to"],"lookbackDays":supplied["period"]["lookbackDays"]});
    let mut calculated = attribution_report::calculate_frozen(brand_id, &dataset, &parameters)?;
    let generated = calculated
        .as_object_mut()
        .ok_or(INVALID)?
        .remove("files")
        .ok_or(INVALID)?;
    let generated: BTreeMap<_, _> = generated
        .as_array()
        .ok_or(INVALID)?
        .iter()
        .map(|f| (f["path"].as_str().unwrap(), f["content"].as_str().unwrap()))
        .collect();
    let mut passed = [true; 9];
    passed[2] = supplied == calculated;
    for i in 3..9 {
        passed[i] = files[PATHS[i]] == generated[PATHS[i]];
    }
    let checks: Vec<_> = CHECKS.iter().zip(passed).map(|((code, message), pass)| {
        json!({"code":code, "status":if pass {"passed"} else {"failed"}, "message":message})
    }).collect();
    let mut report = json!({
        "format":"m-geo-attribution-inspection-v1", "profile":"attribution-v1",
        "source":p["source"], "brandId":brand_id, "datasetId":dataset["id"],
        "contentHash":dataset["contentHash"], "evidenceMode":input["evidenceMode"],
        "evidenceLabel":mode_label(&input["evidenceMode"])?,
        "status":if passed.iter().all(|p|*p) {"passed"} else {"blocked"},
        "currency":input["currency"], "coverage":coverage, "period":calculated["period"],
        "counts":{
            "visits":input["visits"].as_array().unwrap().len(),
            "orders":input["orders"].as_array().unwrap().len(),
            "refunds":input["refunds"].as_array().unwrap().len(),
            "products":dataset["products"].as_array().unwrap().len()
        },
        "summary":calculated["summary"], "bySource":calculated["bySource"],
        "weekly":calculated["weekly"], "checks":checks, "caveat":CAVEAT
    });
    report["reportFingerprint"] = json!(fingerprint(&report));
    Ok(InspectedAttribution {
        report,
        orders: calculated["orders"].as_array().unwrap().clone(),
        products: calculated["byProduct"].as_array().unwrap().clone(),
    })
}
/// Pure inspector: the selected UUID and frozen package are the only inputs.
pub fn inspect_attribution_report(brand_id: &str, package: &Value) -> Result<InspectedAttribution> {
    inspect(brand_id, package).map_err(|_| INVALID.into())
}
fn metrics(v: &Value, group: Option<&str>) -> Result<()> {
    let mut keys = attribution_report::METRIC_KEYS.to_vec();
    if let Some(k) = group {
        keys.push(k);
    }
    shape(v, &keys)?;
    for key in attribution_report::METRIC_KEYS {
        let valid = match *key {
            "sessionConversionRate" | "unknownOrderRate" => {
                v[key].is_null() || v[key].as_f64().is_some_and(|n| (0.0..=1.0).contains(&n))
            }
            "lookbackCoverageIncomplete" => v[key].is_boolean(),
            key => v[key].as_u64().is_some_and(|n| {
                n <= if key.ends_with("Minor") {
                    1_000_000_000_000
                } else {
                    1000
                }
            }),
        };
        if !valid {
            return Err(INVALID.into());
        }
    }
    Ok(())
}
fn validate_report(r: &Value) -> Result<()> {
    shape(
        r,
        &[
            "format",
            "profile",
            "source",
            "brandId",
            "datasetId",
            "contentHash",
            "evidenceMode",
            "evidenceLabel",
            "status",
            "currency",
            "coverage",
            "period",
            "counts",
            "summary",
            "bySource",
            "weekly",
            "checks",
            "caveat",
            "reportFingerprint",
        ],
    )?;
    if r.to_string().len() > 200_000
        || r["format"] != "m-geo-attribution-inspection-v1"
        || r["profile"] != "attribution-v1"
        || r["caveat"] != CAVEAT
        || !hash(&r["reportFingerprint"])
        || !hash(&r["contentHash"])
    {
        return Err(INVALID.into());
    }
    source(&r["source"])?;
    for key in ["brandId", "datasetId"] {
        Uuid::parse_str(r[key].as_str().ok_or(INVALID)?).map_err(|_| INVALID)?;
    }
    if r["evidenceLabel"] != mode_label(&r["evidenceMode"])?
        || !["CNY", "USD", "EUR"].contains(&r["currency"].as_str().ok_or(INVALID)?)
    {
        return Err(INVALID.into());
    }
    period(&r["period"], &r["coverage"])?;
    shape(&r["counts"], &["visits", "orders", "refunds", "products"])?;
    let mut events = 0;
    for key in ["visits", "orders", "refunds", "products"] {
        let n = r["counts"][key]
            .as_u64()
            .filter(|n| *n <= 1000)
            .ok_or(INVALID)?;
        if key != "products" {
            events += n;
        }
    }
    if events > 1000 || r["counts"]["products"].as_u64() == Some(0) {
        return Err(INVALID.into());
    }
    metrics(&r["summary"], None)?;
    for (group, key, max) in [("bySource", "source", 6), ("weekly", "weekStart", 54)] {
        let rows = r[group].as_array().ok_or(INVALID)?;
        if rows.len() > max {
            return Err(INVALID.into());
        }
        let mut previous = None;
        for row in rows {
            metrics(row, Some(key))?;
            let label = row[key].as_str().ok_or(INVALID)?;
            if previous.is_some_and(|p| p >= label) {
                return Err(INVALID.into());
            }
            previous = Some(label);
            if key == "source" {
                if !["doubao", "yuanbao", "qianwen", "baidu", "other", "unknown"].contains(&label) {
                    return Err(INVALID.into());
                }
            } else {
                use chrono::Datelike;
                let date = NaiveDate::parse_from_str(label, "%Y-%m-%d").map_err(|_| INVALID)?;
                if date.to_string() != label || date.weekday() != chrono::Weekday::Mon {
                    return Err(INVALID.into());
                }
            }
        }
    }
    let checks = r["checks"].as_array().ok_or(INVALID)?;
    if checks.len() != 9 {
        return Err(INVALID.into());
    }
    let mut all_pass = true;
    for (i, (code, message)) in CHECKS.iter().enumerate() {
        let check = &checks[i];
        shape(check, &["code", "status", "message"])?;
        if check["code"] != *code
            || check["message"] != *message
            || !["passed", "failed"].contains(&check["status"].as_str().ok_or(INVALID)?)
            || (i < 2 && check["status"] != "passed")
        {
            return Err(INVALID.into());
        }
        all_pass &= check["status"] == "passed";
    }
    if r["status"] != if all_pass { "passed" } else { "blocked" } {
        return Err(INVALID.into());
    }
    let mut unsigned = r.clone();
    unsigned
        .as_object_mut()
        .unwrap()
        .remove("reportFingerprint");
    if r["reportFingerprint"] != fingerprint(&unsigned) {
        return Err(INVALID.into());
    }
    Ok(())
}
/// Export only the bounded whitelist; raw evidence and the local ledgers are excluded.
pub fn attribution_inspection_export(report: &Value) -> Result<Value> {
    validate_report(report).map_err(|_| INVALID.to_string())?;
    let content = serde_json::to_string_pretty(report).map_err(|_| INVALID)?;
    // Indented code cannot be closed by a caller-supplied Markdown fence or HTML filename.
    let inert = content
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let markdown=format!("# 历史归因包本地验收报告\n\n{CAVEAT}\n\n含汇总收入、退款、净额和转化指标；省略商品、订单、访问、退款、会话标识及原始记录。\n\n{inert}");
    Ok(
        json!({"fileName":"attribution-inspection.zip","files":[{"path":"inspection.json","content":content},{"path":"report.md","content":markdown}]}),
    )
}
