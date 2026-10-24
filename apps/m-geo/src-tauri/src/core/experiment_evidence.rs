//! Portable validation of frozen stored experiment records; never consult current data.
use super::*;
pub(super) fn shape(v: &Value, required: &[&str], optional: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or("记录结构无效")?;
    if required.iter().any(|k| !o.contains_key(*k))
        || o.keys()
            .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err("记录字段无效".into());
    }
    Ok(())
}
fn canonical(v: &Value, key: &str, max: usize, required: bool) -> Result<()> {
    let s = text(v, key, max, required)?;
    if v[key].as_str() != Some(s.as_str()) {
        return Err("文字字段不规范".into());
    }
    Ok(())
}
fn stored_raw(v: &Value, key: &str, max: usize, required: bool) -> Result<()> {
    if !v[key].is_string() {
        return Err("缺少文字字段".into());
    }
    raw(v, key, max, required)?;
    Ok(())
}
fn identity(v: &Value, key: &str) -> Result<()> {
    let id = guid(v, key)?;
    if v[key].as_str() != Some(id.as_str()) {
        return Err("身份字段不规范".into());
    }
    Ok(())
}
fn timestamp(v: &Value, key: &str) -> Result<()> {
    let s = v[key].as_str().ok_or("时间字段无效")?;
    if s.len() > 80 || s.contains('\0') {
        return Err("时间字段无效".into());
    }
    DateTime::parse_from_rfc3339(s).map_err(|_| "时间字段无效")?;
    Ok(())
}
pub(super) fn plan(v: &Value, brand_id: &str) -> Result<usize> {
    shape(
        v,
        &[
            "id",
            "brandId",
            "name",
            "questions",
            "platforms",
            "modelLabel",
            "searchMode",
            "context",
            "repetitions",
            "createdAt",
        ],
        &["evidenceMode"],
    )?;
    if v.to_string().len() > 200_000 || v["brandId"] != brand_id {
        return Err("计划身份或大小无效".into());
    }
    identity(v, "id")?;
    identity(v, "brandId")?;
    for (k, max) in [
        ("name", 100),
        ("modelLabel", 200),
        ("searchMode", 200),
        ("context", 500),
    ] {
        canonical(v, k, max, true)?;
    }
    timestamp(v, "createdAt")?;
    experiments::evidence_mode(v)?;
    let reps = number(v, "repetitions")?;
    if reps > 10 {
        return Err("轮次超出范围".into());
    }
    let qs = v["questions"].as_array().ok_or("问题列表无效")?;
    let ps = v["platforms"].as_array().ok_or("平台列表无效")?;
    if qs.is_empty() || qs.len() > 100 || ps.is_empty() || ps.len() > 4 {
        return Err("计划范围无效".into());
    }
    let mut ids = HashSet::new();
    for q in qs {
        shape(q, &["id", "productId", "text"], &[])?;
        identity(q, "id")?;
        identity(q, "productId")?;
        canonical(q, "text", 1000, true)?;
        if !ids.insert(q["id"].to_string()) {
            return Err("问题重复".into());
        }
    }
    let mut platforms = HashSet::new();
    for p in ps {
        let s = p.as_str().ok_or("平台无效")?;
        if !["doubao", "yuanbao", "qianwen", "baidu"].contains(&s) || !platforms.insert(s) {
            return Err("平台无效或重复".into());
        }
    }
    Ok(qs.len() * ps.len() * reps as usize)
}
fn annotations(v: &Value, plan: &Value) -> Result<()> {
    boolean(v, "mentioned")?;
    canonical(v, "accuracy", 50, true)?;
    choice(v, "accuracy", &["unknown", "correct", "incorrect"])?;
    canonical(v, "citedUrl", 2000, false)?;
    web_url(v["citedUrl"].as_str().ok_or("链接无效")?, false)?;
    stored_raw(v, "evidenceNote", 5000, false)?;
    shape(
        &v["assessment"],
        &[
            "campaignId",
            "repetition",
            "context",
            "recommendation",
            "rank",
            "competitors",
        ],
        &[],
    )?;
    if v["assessment"]["campaignId"] != plan["id"]
        || experiments::assessment_with_plan(v, Some(plan))? != v["assessment"]
    {
        return Err("评估字段不规范".into());
    }
    Ok(())
}
pub(super) fn observations(
    plan: &Value,
    rows: &[Value],
    expected: usize,
) -> Result<(usize, usize)> {
    if rows.len() > 8000 || rows.len() > expected * 2 {
        return Err("样本数超出范围".into());
    }
    let mut bytes = plan.to_string().len();
    let mut ids = HashSet::new();
    let mut slots = HashSet::new();
    let mut reviewed = 0;
    let mut entries = 0;
    for v in rows {
        let size = v.to_string().len();
        bytes = bytes.checked_add(size).ok_or("样本过大")?;
        if size > 2_100_000 || bytes > 4 * 1024 * 1024 {
            return Err("样本证据超过处理预算".into());
        }
        shape(
            v,
            &[
                "id",
                "brandId",
                "productId",
                "questionId",
                "querySnapshot",
                "branded",
                "platform",
                "surface",
                "phase",
                "modelLabel",
                "searchMode",
                "testedAt",
                "answer",
                "mentioned",
                "accuracy",
                "citedUrl",
                "evidenceNote",
                "assessment",
            ],
            &["assessmentRevision", "assessmentHistory"],
        )?;
        for k in ["id", "brandId", "productId", "questionId"] {
            identity(v, k)?;
        }
        if v["brandId"] != plan["brandId"] || !ids.insert(v["id"].to_string()) {
            return Err("样本身份无效或重复".into());
        }
        for (k, max) in [
            ("querySnapshot", 1000),
            ("modelLabel", 200),
            ("searchMode", 200),
        ] {
            canonical(v, k, max, true)?;
        }
        for (k, choices) in [
            ("platform", &["doubao", "yuanbao", "qianwen", "baidu"][..]),
            ("surface", &["consumer"][..]),
            ("phase", &["baseline", "followup"][..]),
        ] {
            canonical(v, k, 50, true)?;
            choice(v, k, choices)?;
        }
        if boolean(v, "branded")? {
            return Err("样本不能包含品牌问题".into());
        }
        timestamp(v, "testedAt")?;
        stored_raw(v, "answer", 30000, true)?;
        annotations(v, plan)?;
        if let Some(history) = v.get("assessmentHistory") {
            let history = history.as_array().ok_or("评估历史无效")?;
            if history.is_empty() || history.len() > 50 {
                return Err("评估历史范围无效".into());
            }
            for h in history {
                shape(
                    h,
                    &[
                        "revision",
                        "supersededAt",
                        "mentioned",
                        "accuracy",
                        "citedUrl",
                        "evidenceNote",
                        "assessment",
                    ],
                    &[],
                )?;
                timestamp(h, "supersededAt")?;
            }
            reviewed += 1;
            entries += history.len();
        }
        observations::validate_history_with(v, |sample| annotations(sample, plan))?;
        let slot = json!([
            v["questionId"],
            v["platform"],
            v["assessment"]["repetition"],
            v["phase"]
        ])
        .to_string();
        if !slots.insert(slot) {
            return Err("样本轮次重复".into());
        }
    }
    Ok((reviewed, entries))
}
