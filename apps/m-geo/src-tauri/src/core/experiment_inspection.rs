//! Read-only historical experiment inspection, recomputed exclusively from frozen evidence.
use super::*;
use experiment_evidence::shape;
use std::collections::BTreeMap;
const INVALID: &str = "实验包证据无效、归属不符或超过处理上限";
const BRAND_MISMATCH: &str = "实验包与当前品牌不匹配，请切换到导出时的品牌后重试";
const EVIDENCE_INVALID: &str = "实验包原始证据未通过校验，可能已被修改；请使用未修改的原始导出包";
const LIMIT_EXCEEDED: &str = "实验包超过处理上限，请减少固定问题、平台、轮次或样本后重新导出";
fn public_error(error: String) -> String {
    match error.as_str() {
        BRAND_MISMATCH => BRAND_MISMATCH.into(),
        "轮次超出范围" | "样本数超出范围" | "样本过大" | "样本证据超过处理预算" => {
            LIMIT_EXCEEDED.into()
        }
        INVALID => EVIDENCE_INVALID.into(),
        _ => format!("{EVIDENCE_INVALID}：{error}"),
    }
}
const CAVEAT:&str="本地文件一致性不证明来源真实、平台认证、因果改善或 AI 首页排名。人工实测记录由用户录入，未经独立平台来源验证；哈希不等于数字签名。";
const CHECKS: [(&str, &str); 5] = [
    ("evidence_schema", "冻结证据结构、身份与资源范围核对"),
    ("assessment_history", "全部评估版本与固定条件核对"),
    ("summary", "摘要与独立复算结果核对"),
    ("csv", "CSV 与原始证据及顺序核对"),
    ("markdown", "Markdown 固定说明与摘要核对"),
];
pub struct InspectedExperiment {
    pub report: Value,
    pub observations: Vec<Value>,
}
fn hash(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
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
        &[],
    )?;
    let name = v["fileName"].as_str().ok_or(INVALID)?;
    if name.is_empty()
        || name.chars().count() > 255
        || name.chars().any(|c| c.is_control())
        || name.contains(['/', '\\'])
        || !hash(&v["sha256"])
        || v["fileCount"] != 3
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
    shape(p, &["source", "files"], &[])?;
    source(&p["source"])?;
    let rows = p["files"].as_array().ok_or(INVALID)?;
    if rows.len() != 3 {
        return Err(INVALID.into());
    }
    let mut result = BTreeMap::new();
    let mut bytes = 0usize;
    for row in rows {
        shape(row, &["path", "content"], &[])?;
        let path = row["path"].as_str().ok_or(INVALID)?;
        let content = row["content"].as_str().ok_or(INVALID)?;
        bytes = bytes.checked_add(content.len()).ok_or(INVALID)?;
        if !["report.md", "evidence.json", "observations.csv"].contains(&path)
            || content.contains('\0')
            || result.insert(path, content).is_some()
            || bytes > 16 * 1024 * 1024
        {
            return Err(INVALID.into());
        }
    }
    Ok(result)
}
fn cohort_shape(v: &Value) -> Result<()> {
    shape(
        v,
        &[
            "platform",
            "expectedPerPhase",
            "baseline",
            "followup",
            "matchedPairs",
            "matchedAssessedPairs",
            "matchedBaselineRecommended",
            "matchedFollowupRecommended",
            "unmatchedSamples",
            "deltaPercentagePoints",
        ],
        &[],
    )?;
    for phase in ["baseline", "followup"] {
        shape(
            &v[phase],
            &[
                "samples",
                "assessed",
                "recommended",
                "unknown",
                "recommendationRate",
                "ranked",
                "top3",
            ],
            &[],
        )?;
    }
    Ok(())
}
fn profile(summary: &Value) -> Result<bool> {
    shape(
        summary,
        &[
            "campaign",
            "expectedPerPhase",
            "baselineCount",
            "followupCount",
            "missingCount",
            "missing",
            "platforms",
            "citationHosts",
        ],
        &["evidenceMode", "evidenceLabel"],
    )?;
    let current = match (summary.get("evidenceMode"), summary.get("evidenceLabel")) {
        (None, None) => {
            if summary["campaign"].get("evidenceMode").is_some() {
                return Err(INVALID.into());
            }
            false
        }
        (Some(mode), Some(label)) => {
            let effective = experiments::evidence_mode(&summary["campaign"])?;
            if mode != effective || label != reports::mode_label(effective) {
                return Err(INVALID.into());
            }
            true
        }
        _ => return Err(INVALID.into()),
    };
    let cohorts = summary["platforms"].as_array().ok_or(INVALID)?;
    if cohorts.len() > 4 {
        return Err(INVALID.into());
    }
    for row in cohorts {
        cohort_shape(row)?;
    }
    let missing = summary["missing"].as_array().ok_or(INVALID)?;
    if missing.len() > 8000 {
        return Err(INVALID.into());
    }
    for row in missing {
        shape(
            row,
            &[
                "questionId",
                "questionText",
                "platform",
                "repetition",
                "phase",
            ],
            &[],
        )?;
    }
    if !summary["citationHosts"].is_object() {
        return Err(INVALID.into());
    }
    Ok(current)
}
fn markdown_matches(markdown: &str, summary: &Value, current: bool) -> bool {
    markdown
        .strip_prefix(&reports::markdown_prefix(summary, current))
        .and_then(|s| s.strip_suffix("\n```\n"))
        .and_then(|s| strict_json::parse(s).ok())
        .is_some_and(|v| v == *summary)
}
// Companion summary text is untrusted too, even when only its numeric comparison fails.
fn contains_nul(v: &Value) -> bool {
    match v {
        Value::String(s) => s.contains('\0'),
        Value::Array(a) => a.iter().any(contains_nul),
        Value::Object(o) => o.iter().any(|(k, v)| k.contains('\0') || contains_nul(v)),
        _ => false,
    }
}
fn inspect(brand_id: &str, p: &Value) -> Result<InspectedExperiment> {
    Uuid::parse_str(brand_id).map_err(|_| INVALID)?;
    let files = files(p)?;
    let evidence = strict_json::parse(files["evidence.json"])?;
    if contains_nul(&evidence) {
        return Err(INVALID.into());
    }
    shape(&evidence, &["format", "summary", "observations"], &[])?;
    if evidence["format"] != "m-geo-experiment-evidence-v1" {
        return Err(INVALID.into());
    }
    let supplied = &evidence["summary"];
    let plan = &supplied["campaign"];
    if plan["brandId"].as_str().is_some_and(|id| id != brand_id) {
        return Err(BRAND_MISMATCH.into());
    }
    let expected = experiment_evidence::plan(plan, brand_id)?;
    let current = profile(supplied)?;
    let observations = evidence["observations"].as_array().ok_or(INVALID)?;
    let (reviewed, history) = experiment_evidence::observations(plan, observations, expected)?;
    let summary = reports::summarize(plan, observations, current)?;
    let passed = [
        true,
        true,
        *supplied == summary,
        files["observations.csv"] == reports::csv(plan, observations, current)?,
        markdown_matches(files["report.md"], supplied, current),
    ];
    let checks=CHECKS.iter().zip(passed).map(|((code,message),pass)|json!({"code":code,"status":if pass{"passed"}else{"failed"},"message":message})).collect::<Vec<_>>();
    let mode = experiments::evidence_mode(plan)?;
    let mut report = json!({"format":"m-geo-experiment-inspection-v1","source":p["source"],"profile":if current{"current-v1"}else{"legacy-v1"},"brandId":plan["brandId"],"campaignId":plan["id"],"evidenceMode":mode,"evidenceLabel":reports::mode_label(mode),"status":if passed.iter().all(|p|*p){"passed"}else{"blocked"},"counts":{"observations":observations.len(),"expectedPerPhase":expected,"baseline":summary["baselineCount"],"followup":summary["followupCount"],"missing":summary["missingCount"],"reviewedObservations":reviewed,"historyEntries":history},"platforms":summary["platforms"],"checks":checks,"caveat":CAVEAT});
    report["reportFingerprint"] = json!(fingerprint(&report));
    Ok(InspectedExperiment {
        report,
        observations: observations.clone(),
    })
}
/// Inspect a bounded decoded package using only its frozen records and active brand identity.
pub fn inspect_experiment_report(brand_id: &str, package: &Value) -> Result<InspectedExperiment> {
    inspect(brand_id, package).map_err(public_error)
}
fn validate_report(report: &Value) -> Result<()> {
    shape(
        report,
        &[
            "format",
            "source",
            "profile",
            "brandId",
            "campaignId",
            "evidenceMode",
            "evidenceLabel",
            "status",
            "counts",
            "platforms",
            "checks",
            "caveat",
            "reportFingerprint",
        ],
        &[],
    )?;
    if report.to_string().len() > 100_000
        || report["format"] != "m-geo-experiment-inspection-v1"
        || report["caveat"] != CAVEAT
        || !hash(&report["reportFingerprint"])
    {
        return Err(INVALID.into());
    }
    source(&report["source"])?;
    for key in ["brandId", "campaignId"] {
        let s = guid(report, key)?;
        if report[key] != s {
            return Err(INVALID.into());
        }
    }
    if !["legacy-v1", "current-v1"].contains(&report["profile"].as_str().ok_or(INVALID)?) {
        return Err(INVALID.into());
    }
    let mode = experiments::evidence_mode(report)?;
    if report["evidenceLabel"] != reports::mode_label(mode)
        || (report["profile"] == "legacy-v1" && mode != "unspecified")
    {
        return Err(INVALID.into());
    }
    shape(
        &report["counts"],
        &[
            "observations",
            "expectedPerPhase",
            "baseline",
            "followup",
            "missing",
            "reviewedObservations",
            "historyEntries",
        ],
        &[],
    )?;
    for (k, max) in [
        ("observations", 8000),
        ("expectedPerPhase", 4000),
        ("baseline", 4000),
        ("followup", 4000),
        ("missing", 8000),
        ("reviewedObservations", 8000),
        ("historyEntries", 400000),
    ] {
        if !report["counts"][k].as_u64().is_some_and(|n| n <= max) {
            return Err(INVALID.into());
        }
    }
    let cohorts = report["platforms"].as_array().ok_or(INVALID)?;
    if cohorts.is_empty() || cohorts.len() > 4 {
        return Err(INVALID.into());
    }
    for row in cohorts {
        cohort_shape(row)?;
        if !["doubao", "yuanbao", "qianwen", "baidu"]
            .contains(&row["platform"].as_str().ok_or(INVALID)?)
        {
            return Err(INVALID.into());
        }
        // Only the calculator's bounded numeric cells are exportable, never supplied text.
        for key in [
            "expectedPerPhase",
            "matchedPairs",
            "matchedAssessedPairs",
            "matchedBaselineRecommended",
            "matchedFollowupRecommended",
            "unmatchedSamples",
        ] {
            if !row[key].as_u64().is_some_and(|n| n <= 8000) {
                return Err(INVALID.into());
            }
        }
        if !row["deltaPercentagePoints"].is_null()
            && !row["deltaPercentagePoints"]
                .as_f64()
                .is_some_and(|n| (-100.0..=100.0).contains(&n))
        {
            return Err(INVALID.into());
        }
        for phase in ["baseline", "followup"] {
            for key in [
                "samples",
                "assessed",
                "recommended",
                "unknown",
                "ranked",
                "top3",
            ] {
                if !row[phase][key].as_u64().is_some_and(|n| n <= 4000) {
                    return Err(INVALID.into());
                }
            }
            if !row[phase]["recommendationRate"].is_null()
                && !row[phase]["recommendationRate"]
                    .as_f64()
                    .is_some_and(|n| (0.0..=1.0).contains(&n))
            {
                return Err(INVALID.into());
            }
        }
    }
    let checks = report["checks"].as_array().ok_or(INVALID)?;
    if checks.len() != 5 {
        return Err(INVALID.into());
    }
    let mut all_pass = true;
    for (i, (code, message)) in CHECKS.iter().enumerate() {
        let check = &checks[i];
        shape(check, &["code", "status", "message"], &[])?;
        if check["code"] != *code
            || check["message"] != *message
            || !["passed", "failed"].contains(&check["status"].as_str().ok_or(INVALID)?)
        {
            return Err(INVALID.into());
        }
        if i < 2 && check["status"] != "passed" {
            return Err(INVALID.into());
        }
        all_pass &= check["status"] == "passed";
    }
    if report["status"] != if all_pass { "passed" } else { "blocked" } {
        return Err(INVALID.into());
    }
    let mut unsigned = report.clone();
    unsigned
        .as_object_mut()
        .ok_or(INVALID)?
        .remove("reportFingerprint");
    if report["reportFingerprint"] != fingerprint(&unsigned) {
        return Err(INVALID.into());
    }
    Ok(())
}
/// Native sessions pass only their retained report; the allowlist prevents evidence leakage.
pub fn experiment_inspection_export(report: &Value) -> Result<Value> {
    validate_report(report).map_err(|_| INVALID.to_string())?;
    let content = serde_json::to_string_pretty(report).map_err(|_| INVALID)?;
    let markdown=format!("# 历史实验包本地验收报告\n\n{CAVEAT}\n\n只含标识、覆盖、分平台统计与一致性检查。\n\n```json\n{content}\n```\n");
    if content.len() + markdown.len() > 16 * 1024 * 1024 {
        return Err(INVALID.into());
    }
    Ok(
        json!({"fileName":"experiment-inspection.zip","files":[{"path":"inspection.json","content":content},{"path":"report.md","content":markdown}]}),
    )
}
