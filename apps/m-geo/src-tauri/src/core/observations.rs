//! Raw measurement evidence and submission identity stay immutable; review annotations are versioned.
use super::*;
const ANNOTATIONS: &[&str] = &[
    "mentioned",
    "accuracy",
    "citedUrl",
    "evidenceNote",
    "assessment",
];
fn revision_number(v: &Value) -> Result<i64> {
    if v.get("assessmentRevision").is_none() {
        Ok(1)
    } else {
        number(v, "assessmentRevision")
    }
}
fn annotations(v: &Value) -> Value {
    let mut a = json!({});
    for key in ANNOTATIONS {
        a[*key] = v[*key].clone();
    }
    a
}
fn apply_annotations(v: &mut Value, a: &Value) {
    for key in ANNOTATIONS {
        if *key == "assessment" && a[*key].is_null() {
            v.as_object_mut().unwrap().remove(*key);
        } else {
            v[*key] = a[*key].clone();
        }
    }
}
pub(super) fn submission(v: &Value) -> Value {
    let mut original = v.clone();
    let map = original.as_object_mut().unwrap();
    map.remove("id");
    map.remove("assessmentRevision");
    map.remove("assessmentHistory");
    if let Some(first) = v["assessmentHistory"].as_array().and_then(|h| h.first()) {
        apply_annotations(&mut original, first);
    }
    original
}
fn validate_annotations(c: &Connection, b: &str, v: &Value) -> Result<()> {
    boolean(v, "mentioned")?;
    choice(v, "accuracy", &["unknown", "correct", "incorrect"])?;
    web_url(&text(v, "citedUrl", 2000, false)?, false)?;
    raw(v, "evidenceNote", 5000, false)?;
    if v.get("assessment").is_some() && experiments::assessment(c, b, v, true)? != v["assessment"] {
        return Err("评估字段不规范".into());
    }
    Ok(())
}
pub(super) fn assess(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    imports::keys(
        p,
        &[
            "brandId",
            "id",
            "expectedRevision",
            "mentioned",
            "accuracy",
            "citedUrl",
            "evidenceNote",
            "recommendation",
            "rank",
            "competitors",
        ],
    )?;
    let oid = guid(p, "id")?;
    let mut v = load(c, "m_observations", b, &oid)?;
    let revision = revision_number(&v)?;
    if number(p, "expectedRevision")? != revision {
        return Err("这条评估已被更新，请刷新后再核对".into());
    }
    let mut history = v
        .get("assessmentHistory")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if history.len() >= 50 {
        return Err("这条样本已更正50次，请保留证据并联系支持处理".into());
    }
    let mut previous = annotations(&v);
    previous["revision"] = json!(revision);
    previous["supersededAt"] = json!(now());
    history.push(previous);
    v["mentioned"] = json!(boolean(p, "mentioned")?);
    v["accuracy"] = json!(choice(p, "accuracy", &["unknown", "correct", "incorrect"])?);
    v["citedUrl"] = json!(text(p, "citedUrl", 2000, false)?);
    v["evidenceNote"] = json!(raw(p, "evidenceNote", 5000, false)?);
    let mut a=v.get("assessment").cloned().unwrap_or_else(||json!({"campaignId":null,"repetition":1,"context":"历史记录：测试条件未记载","recommendation":"unknown","rank":null,"competitors":""}));
    for key in ["recommendation", "rank", "competitors"] {
        a[key] = p[key].clone();
    }
    v["assessment"] = a;
    v["assessment"] = experiments::assessment(c, b, &v, true)?;
    validate_annotations(c, b, &v)?;
    v["assessmentHistory"] = json!(history);
    v["assessmentRevision"] = json!(revision + 1);
    if v.to_string().len() > 2_100_000 {
        return Err("评估历史已超过记录大小限制".into());
    }
    update(c, "m_observations", &v)?;
    Ok(v)
}
pub(super) fn validate_history(c: &Connection, b: &str, v: &Value) -> Result<()> {
    validate_history_with(v, |sample| validate_annotations(c, b, sample))
}
pub(super) fn validate_history_with(
    v: &Value,
    mut validate: impl FnMut(&Value) -> Result<()>,
) -> Result<()> {
    if v.get("assessmentHistory").is_none() && v.get("assessmentRevision").is_none() {
        return Ok(());
    }
    let history = v["assessmentHistory"].as_array().ok_or("评估历史缺失")?;
    if history.is_empty() || history.len() > 50 || revision_number(v)? != history.len() as i64 + 1 {
        return Err("评估历史版本不一致".into());
    }
    for (i, h) in history.iter().enumerate() {
        imports::keys(
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
        )?;
        if number(h, "revision")? != i as i64 + 1 {
            return Err("历史评估版本不连续".into());
        }
        date_time(h, "supersededAt")?;
        let mut sample = v.clone();
        apply_annotations(&mut sample, h);
        validate(&sample)?;
        if let Some(a) = sample.get("assessment") {
            for key in ["campaignId", "context", "repetition"] {
                if a[key] != v["assessment"][key] {
                    return Err("评估历史不能更改测试条件".into());
                }
            }
        } else if i > 0 || !v["assessment"]["campaignId"].is_null() {
            return Err("评估历史不完整".into());
        }
    }
    Ok(())
}
