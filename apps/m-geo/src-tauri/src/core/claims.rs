//! Claim-level evidence links for draft review and export.
use super::*;

const MAX_CLAIMS: usize = 50;
const MAX_CLAIM_CHARS: usize = 4000;

pub(super) fn parse(v: &Value) -> Result<Value> {
    parse_impl(v, false)
}

pub(super) fn validate_stored(v: &Value) -> Result<Value> {
    parse_impl(v, true)
}

fn parse_impl(v: &Value, stored: bool) -> Result<Value> {
    let claims = v
        .get("claims")
        .and_then(Value::as_array)
        .ok_or("请提供稿件关键主张列表")?;
    if claims.len() > MAX_CLAIMS {
        return Err("每份稿件最多登记50条关键主张".into());
    }
    let mut ids = HashSet::new();
    let mut out = Vec::with_capacity(claims.len());
    for claim in claims {
        imports::keys(claim, &["id", "text", "factId", "evidence"])?;
        let id = guid(claim, "id")?;
        if !ids.insert(id.clone()) {
            return Err("关键主张标识重复".into());
        }
        let fact_id = text(claim, "factId", 100, false)?;
        let mut parsed = json!({
            "id": id,
            "text": text(claim, "text", MAX_CLAIM_CHARS, true)?,
            "factId": fact_id,
        });
        // Frontend saves may carry an old snapshot, but native code always
        // strips it. Only the review action can create a new frozen snapshot.
        if stored {
            if let Some(evidence) = claim.get("evidence") {
                imports::keys(
                    evidence,
                    &[
                        "snapshotVersion",
                        "factId",
                        "label",
                        "value",
                        "source",
                        "validUntil",
                        "factReviewedAt",
                        "factUpdatedAt",
                    ],
                )?;
                if number(evidence, "snapshotVersion")? != 1 {
                    return Err("不支持的主张证据快照版本".into());
                }
                if text(evidence, "factId", 100, true)? != fact_id {
                    return Err("主张证据快照与关联事实不一致".into());
                }
                let valid_until = nullable_expiry(evidence, "validUntil")?;
                let fact_reviewed_at = nullable_timestamp(evidence, "factReviewedAt")?;
                let fact_updated_at = nullable_timestamp(evidence, "factUpdatedAt")?;
                parsed["evidence"] = json!({
                    "snapshotVersion": 1,
                    "factId": fact_id,
                    "label": text(evidence, "label", 200, true)?,
                    "value": text(evidence, "value", 3000, true)?,
                    "source": text(evidence, "source", 2000, true)?,
                    "validUntil": valid_until,
                    "factReviewedAt": fact_reviewed_at,
                    "factUpdatedAt": fact_updated_at,
                });
            }
        }
        out.push(parsed);
    }
    Ok(json!(out))
}

fn nullable_expiry(v: &Value, key: &str) -> Result<Value> {
    match v.get(key) {
        Some(Value::Null) => Ok(Value::Null),
        Some(Value::String(date)) if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok() => {
            Ok(json!(date))
        }
        _ => Err("证据快照有效期格式错误".into()),
    }
}

fn nullable_timestamp(v: &Value, key: &str) -> Result<Value> {
    match v.get(key) {
        Some(Value::Null) => Ok(Value::Null),
        Some(_) => Ok(json!(date_time(v, key)?)),
        None => Err("证据快照时间字段缺失".into()),
    }
}

fn snapshot(fact: &Value) -> Value {
    json!({
        "snapshotVersion": 1,
        "factId": fact["id"],
        "label": fact["label"],
        "value": fact["value"],
        "source": fact["source"],
        "validUntil": fact["validUntil"],
        "factReviewedAt": fact.get("reviewedAt").unwrap_or(&Value::Null),
        "factUpdatedAt": fact.get("updatedAt").unwrap_or(&Value::Null),
    })
}

pub(super) fn from_snapshot(ctx: &Value) -> Result<Value> {
    let facts = ctx["sourceSnapshot"]["facts"]
        .as_array()
        .ok_or("商品事实格式错误")?;
    let mut claims = Vec::with_capacity(facts.len());
    if facts.len() > MAX_CLAIMS {
        return Err("公开事实超过50条，请精简后再生成稿件".into());
    }
    for fact in facts {
        let label = text(fact, "label", 200, true)?;
        let value = text(fact, "value", 3000, true)?;
        let claim = format!("{}：{}", label, value);
        if claim.chars().count() > MAX_CLAIM_CHARS {
            return Err("单条事实过长，请精简到4000字后再生成稿件".into());
        }
        claims.push(json!({
            "id": id(),
            "text": claim,
            "factId": text(fact, "id", 100, true)?,
        }));
    }
    Ok(json!(claims))
}

pub(super) fn review(draft: &Value, product: &Value) -> Result<Value> {
    let claims = draft
        .get("claims")
        .and_then(Value::as_array)
        .ok_or("稿件缺少关键主张依据，请重新登记并审核")?;
    if claims.is_empty() {
        return Err("请至少登记一条关键主张，并关联有来源的公开事实".into());
    }
    let body = draft["body"].as_str().ok_or("稿件正文损坏")?;
    let facts = product["facts"].as_array().ok_or("商品事实格式错误")?;
    let mut frozen = Vec::with_capacity(claims.len());
    for (index, claim) in claims.iter().enumerate() {
        let number = index + 1;
        let claim_text = claim["text"].as_str().ok_or("关键主张内容损坏")?;
        if !body.contains(claim_text) {
            return Err(format!("第{}条关键主张不在正文中，请粘贴正文原文", number));
        }
        let fact_id = claim["factId"].as_str().unwrap_or("");
        if fact_id.is_empty() {
            return Err(format!(
                "第{}条关键主张缺少事实依据，请选择有来源的公开事实",
                number
            ));
        }
        let fact = facts
            .iter()
            .find(|fact| fact["id"].as_str() == Some(fact_id) && fact["visibility"] == "public");
        let Some(fact) = fact else {
            return Err(format!(
                "第{}条关键主张关联的事实不存在或不是公开事实",
                number
            ));
        };
        if !fact_is_approved(fact) {
            return Err(format!(
                "第{}条关键主张依据的事实尚未审核通过，请先在商品资料中审核",
                number
            ));
        }
        if fact["source"].as_str().unwrap_or("").trim().is_empty() {
            return Err(format!("第{}条关键主张依据缺少来源资料", number));
        }
        commerce::freshness(fact)?;
        frozen.push(json!({
            "id": claim["id"],
            "text": claim["text"],
            "factId": claim["factId"],
            "evidence": snapshot(fact),
        }));
    }
    Ok(json!(frozen))
}

fn linked_facts(draft: &Value, product: &Value) -> Vec<(String, Value)> {
    let claims = draft["claims"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let facts = product["facts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    claims
        .iter()
        .filter_map(|claim| {
            let text = claim["text"].as_str()?.to_owned();
            let fact_id = claim["factId"].as_str()?;
            if let Some(evidence) = claim.get("evidence") {
                return Some((text, evidence.clone()));
            }
            // Pre-0.17 reviewed drafts had no snapshot. Keep their historical
            // export behavior; a new review freezes explicit evidence.
            let fact = facts.iter().find(|fact| {
                fact["id"].as_str() == Some(fact_id) && fact["visibility"] == "public"
            })?;
            Some((text, fact.clone()))
        })
        .collect()
}

pub(super) fn markdown_appendix(draft: &Value, product: &Value) -> String {
    let mut out = String::from("\n## 主张依据（审核记录）\n");
    for (index, (claim, fact)) in linked_facts(draft, product).iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}\n   - 事实：{}：{}\n   - 来源：{}{}\n",
            index + 1,
            claim,
            fact["label"].as_str().unwrap_or(""),
            fact["value"].as_str().unwrap_or(""),
            fact["source"].as_str().unwrap_or(""),
            fact["validUntil"]
                .as_str()
                .map(|date| format!("；有效至 {}", date))
                .unwrap_or_default(),
        ));
    }
    out
}

pub(super) fn html_appendix(draft: &Value, product: &Value) -> String {
    let mut out = String::from("<section><h2>主张依据（审核记录）</h2><ol>");
    for (claim, fact) in linked_facts(draft, product) {
        out.push_str(&format!(
            "<li><p>{}</p><p>事实：{}：{}</p><p>来源：{}{}</p></li>",
            escape(&claim),
            escape(fact["label"].as_str().unwrap_or("")),
            escape(fact["value"].as_str().unwrap_or("")),
            escape(fact["source"].as_str().unwrap_or("")),
            fact["validUntil"]
                .as_str()
                .map(|date| format!("；有效至 {}", escape(date)))
                .unwrap_or_default(),
        ));
    }
    out.push_str("</ol></section>");
    out
}
