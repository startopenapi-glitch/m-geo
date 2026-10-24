//! Historical channel evidence inspection. Every computation uses the frozen files only.
use super::*;
use std::collections::BTreeMap;
const INVALID: &str = "历史通道包证据无效、归属不符或超过处理上限";
const LIMIT: usize = 16 * 1024 * 1024;
const CAVEAT: &str = "仅复核历史文件的内部一致性，不证明当前有效期、真实人工审核或来源真实性；不代表平台提交、接受、收录、推荐或排名。哈希是完整性校验，不是数字签名。本次选择不是完整商家目录；模拟接受、拒绝或未知不是真实平台结果。";
const CHECKS: [(&str, &str); 3] = [
    ("frozen_evidence", "冻结清单、身份与模拟回执完整性核对"),
    ("report", "完整操作报告与冻结证据复算结果核对"),
    ("notice", "本地操作固定说明逐字节核对"),
];
pub struct InspectedChannel {
    pub report: Value,
    pub rows: Vec<Value>,
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let object = v.as_object().ok_or(INVALID)?;
    if object.len() != keys.len() || keys.iter().any(|k| !object.contains_key(*k)) {
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
fn source(v: &Value) -> Result<()> {
    exact(
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
        || name.contains(['/', '\\'])
        || name.chars().any(|c| c.is_control())
        || matches!(name, "." | "..")
        || !hash(&v["sha256"])
        || v["fileCount"] != 4
        || !v["compressedBytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 20 * 1024 * 1024)
        || !v["uncompressedBytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= LIMIT as u64)
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn files(p: &Value) -> Result<BTreeMap<&str, &str>> {
    exact(p, &["source", "files"])?;
    source(&p["source"])?;
    let entries = p["files"]
        .as_array()
        .filter(|v| v.len() == 4)
        .ok_or(INVALID)?;
    let mut out = BTreeMap::new();
    let mut bytes = 0usize;
    for entry in entries {
        exact(entry, &["path", "content"])?;
        let name = entry["path"].as_str().ok_or(INVALID)?;
        let content = entry["content"].as_str().ok_or(INVALID)?;
        bytes = bytes.checked_add(content.len()).ok_or(INVALID)?;
        if bytes > LIMIT || content.contains('\0') || out.insert(name, content).is_some() {
            return Err(INVALID.into());
        }
    }
    if p["source"]["uncompressedBytes"].as_u64() != Some(bytes as u64) {
        return Err(INVALID.into());
    }
    Ok(out)
}
fn has_nul(v: &Value) -> bool {
    match v {
        Value::String(s) => s.contains('\0'),
        Value::Array(a) => a.iter().any(has_nul),
        Value::Object(o) => o.iter().any(|(k, v)| k.contains('\0') || has_nul(v)),
        _ => false,
    }
}
fn manifest(v: &Value, brand: &str) -> Result<()> {
    // Frozen enums must be canonical before shared live validation interprets them.
    // Never trim or rewrite the evidence that its hashes bind.
    if !["mgeo-catalog-v1", "google-merchant-basic-v1"]
        .contains(&v["profile"].as_str().ok_or(INVALID)?)
        || v["items"]
            .as_array()
            .ok_or(INVALID)?
            .iter()
            .any(|row| !matches!(row["localStatus"].as_str(), Some("ready" | "blocked")))
    {
        return Err(INVALID.into());
    }
    feed::validate_manifest(v, brand)?;
    feed::validate_export_roundtrip(v)?;
    // Compare logical UUIDs only for uniqueness; never normalize frozen rows before hashing.
    for field in ["productId", "draftId"] {
        let mut ids = HashSet::new();
        for row in v["items"].as_array().ok_or(INVALID)? {
            let id = Uuid::parse_str(row[field].as_str().ok_or(INVALID)?).map_err(|_| INVALID)?;
            if !ids.insert(id) {
                return Err(INVALID.into());
            }
        }
    }
    Ok(())
}
fn inspect(brand: &str, p: &Value) -> Result<InspectedChannel> {
    if Uuid::parse_str(brand).map_err(|_| INVALID)?.to_string() != brand {
        return Err(INVALID.into());
    }
    let files = files(p)?;
    let operation = if files.contains_key("comparison-report.json") {
        "comparison"
    } else if files.contains_key("simulation-report.json") {
        "simulation"
    } else if files.contains_key("receipt-check-report.json") {
        "receipt-check"
    } else {
        return Err(INVALID.into());
    };
    let report_name = match operation {
        "comparison" => "comparison-report.json",
        "simulation" => "simulation-report.json",
        _ => "receipt-check-report.json",
    };
    let second = if operation == "comparison" {
        "baseline-manifest.json"
    } else {
        "simulated-receipt.json"
    };
    if [
        report_name,
        second,
        "feed-manifest.json",
        "local-operation-notice.txt",
    ]
    .iter()
    .any(|k| !files.contains_key(k))
    {
        return Err(INVALID.into());
    }
    let mut jsons = BTreeMap::new();
    for name in [report_name, second, "feed-manifest.json"] {
        let value = strict_json::parse(files[name])?;
        if has_nul(&value) {
            return Err(INVALID.into());
        }
        jsons.insert(name, value);
    }
    let current = &jsons["feed-manifest.json"];
    manifest(current, brand)?;
    let items = current["items"].as_array().ok_or(INVALID)?;
    let ready = items.iter().filter(|r| r["localStatus"] == "ready").count();
    let (recomputed, baseline_id, baseline_selected) = if operation == "comparison" {
        let baseline = &jsons[second];
        manifest(baseline, brand)?;
        if baseline["profile"] != current["profile"] {
            return Err(INVALID.into());
        }
        (
            feed_ops::comparison_report(baseline, current)?,
            baseline["packageId"].clone(),
            baseline["items"].as_array().ok_or(INVALID)?.len(),
        )
    } else {
        if ready != items.len() {
            return Err(INVALID.into());
        }
        let receipt = &jsons[second];
        let checked = feed_ops::receipt_report(receipt, current, brand)?;
        let report = if operation == "simulation" {
            let status = receipt["rows"]
                .as_array()
                .and_then(|r| r.first())
                .and_then(|r| r["status"].as_str())
                .ok_or(INVALID)?;
            if *receipt != feed_ops::simulation_receipt(current, brand, status)? {
                return Err(INVALID.into());
            }
            feed_ops::simulation_report(receipt)?
        } else {
            checked
        };
        (report, Value::Null, 0)
    };
    let rows = recomputed["rows"].as_array().ok_or(INVALID)?.clone();
    let passed = [
        true,
        jsons[report_name] == recomputed,
        files["local-operation-notice.txt"] == feed_ops::operation_notice(operation)?,
    ];
    let checks = CHECKS.iter().zip(passed).map(|((code,message),pass)| json!({"code":code,"status":if pass {"passed"} else {"failed"},"message":message})).collect::<Vec<_>>();
    let mut report = json!({"format":"m-geo-channel-inspection-v1","operation":operation,"profile":current["profile"],"source":p["source"],"brandId":brand,"packageId":current["packageId"],"baselinePackageId":baseline_id,"status":if passed.iter().all(|p|*p){"passed"}else{"blocked"},"counts":{"selected":items.len(),"baselineSelected":baseline_selected,"rows":rows.len(),"ready":ready,"blocked":items.len()-ready},"summary":recomputed["summary"],"checks":checks,"caveat":CAVEAT});
    report["reportFingerprint"] = json!(fingerprint(&report));
    Ok(InspectedChannel { report, rows })
}
/// Recompute a decoded, bounded operation package without consulting current database state.
pub fn inspect_channel_report(brand_id: &str, package: &Value) -> Result<InspectedChannel> {
    inspect(brand_id, package).map_err(|_| INVALID.into())
}
fn count(v: &Value, key: &str, maximum: u64) -> Result<u64> {
    v[key]
        .as_u64()
        .filter(|n| *n <= maximum)
        .ok_or_else(|| INVALID.into())
}
fn validate_report(v: &Value) -> Result<()> {
    exact(
        v,
        &[
            "format",
            "operation",
            "profile",
            "source",
            "brandId",
            "packageId",
            "baselinePackageId",
            "status",
            "counts",
            "summary",
            "checks",
            "caveat",
            "reportFingerprint",
        ],
    )?;
    if v.to_string().len() > 100_000
        || v["format"] != "m-geo-channel-inspection-v1"
        || v["caveat"] != CAVEAT
        || !hash(&v["packageId"])
        || !hash(&v["reportFingerprint"])
    {
        return Err(INVALID.into());
    }
    source(&v["source"])?;
    let brand = v["brandId"].as_str().ok_or(INVALID)?;
    if Uuid::parse_str(brand).map_err(|_| INVALID)?.to_string() != brand {
        return Err(INVALID.into());
    }
    if !["mgeo-catalog-v1", "google-merchant-basic-v1"]
        .contains(&v["profile"].as_str().ok_or(INVALID)?)
    {
        return Err(INVALID.into());
    }
    let operation = v["operation"].as_str().ok_or(INVALID)?;
    if !["comparison", "simulation", "receipt-check"].contains(&operation) {
        return Err(INVALID.into());
    }
    let c = &v["counts"];
    exact(
        c,
        &["selected", "baselineSelected", "rows", "ready", "blocked"],
    )?;
    let selected = count(c, "selected", 200)?;
    let baseline = count(c, "baselineSelected", 200)?;
    let rows = count(c, "rows", 400)?;
    let ready = count(c, "ready", 200)?;
    let blocked = count(c, "blocked", 200)?;
    if selected == 0 || ready + blocked != selected {
        return Err(INVALID.into());
    }
    let summary = &v["summary"];
    if operation == "comparison" {
        if baseline == 0 || !hash(&v["baselinePackageId"]) {
            return Err(INVALID.into());
        }
        exact(summary, &["added", "removed", "changed", "unchanged"])?;
        let added = count(summary, "added", 200)?;
        let removed = count(summary, "removed", 200)?;
        let changed = count(summary, "changed", 200)?;
        let unchanged = count(summary, "unchanged", 200)?;
        if added + removed + changed + unchanged != rows
            || added + changed + unchanged != selected
            || removed + changed + unchanged != baseline
        {
            return Err(INVALID.into());
        }
    } else {
        if baseline != 0 || !v["baselinePackageId"].is_null() || rows != selected || blocked != 0 {
            return Err(INVALID.into());
        }
        exact(summary, &["accepted", "rejected", "unknown", "missing"])?;
        let counts = ["accepted", "rejected", "unknown", "missing"]
            .iter()
            .map(|k| count(summary, k, 200))
            .collect::<Result<Vec<_>>>()?;
        if counts.iter().sum::<u64>() != rows
            || (operation == "simulation"
                && (counts[3] != 0 || counts[..3].iter().filter(|n| **n > 0).count() != 1))
        {
            return Err(INVALID.into());
        }
    }
    let checks = v["checks"]
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or(INVALID)?;
    let mut all_passed = true;
    for (i, (code, message)) in CHECKS.iter().enumerate() {
        let check = &checks[i];
        exact(check, &["code", "status", "message"])?;
        if check["code"] != *code
            || check["message"] != *message
            || !["passed", "failed"].contains(&check["status"].as_str().ok_or(INVALID)?)
            || (i == 0 && check["status"] != "passed")
        {
            return Err(INVALID.into());
        }
        all_passed &= check["status"] == "passed";
    }
    if v["status"] != if all_passed { "passed" } else { "blocked" } {
        return Err(INVALID.into());
    }
    let mut unsigned = v.clone();
    unsigned
        .as_object_mut()
        .ok_or(INVALID)?
        .remove("reportFingerprint");
    if v["reportFingerprint"] != fingerprint(&unsigned) {
        return Err(INVALID.into());
    }
    Ok(())
}
/// Export only aggregate operational information; the picker filename remains user supplied text.
pub fn channel_inspection_export(report: &Value) -> Result<Value> {
    validate_report(report).map_err(|_| INVALID.to_owned())?;
    let content = serde_json::to_string_pretty(report).map_err(|_| INVALID)?;
    let indented = content
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let markdown = format!("# 历史商品通道操作复核结论\n\n{CAVEAT}\n\n本文件是汇总操作报告，保留所选文件名，不是匿名或公开安全报告。仅含标识、计数与一致性检查，不包含原始商品或回执行。\n\n{indented}");
    if content.len() + markdown.len() > LIMIT {
        return Err(INVALID.into());
    }
    Ok(
        json!({"fileName":"channel-inspection.zip","files":[{"path":"inspection.json","content":content},{"path":"report.md","content":markdown}]}),
    )
}
