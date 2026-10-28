//! Reviewed content-asset references and immutable review snapshots for drafts.
use super::*;

const MAX_DRAFT_ASSETS: usize = 50;

fn ids(draft: &Value) -> Result<Vec<String>> {
    let Some(value) = draft.get("assetIds") else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or("稿件内容资产列表格式错误")?;
    if values.len() > MAX_DRAFT_ASSETS {
        return Err("每份稿件最多关联50条内容资产".into());
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::with_capacity(values.len());
    for value in values {
        let id = value.as_str().ok_or("稿件内容资产标识格式错误")?;
        let id = guid(&json!({"id":id}), "id")?;
        if !seen.insert(id.clone()) {
            return Err("稿件内容资产不能重复关联".into());
        }
        ids.push(id);
    }
    Ok(ids)
}

pub(super) fn parse_ids(payload: &Value) -> Result<Value> {
    Ok(json!(ids(payload)?))
}

fn check_scope(asset: &Value, draft: &Value) -> Result<()> {
    if asset["productId"].is_null() || asset["productId"] == draft["productId"] {
        Ok(())
    } else {
        Err("内容资产属于其他商品，不能关联到当前稿件".into())
    }
}

fn uses_text(draft: &Value, text: &str) -> bool {
    draft["title"]
        .as_str()
        .is_some_and(|title| title.contains(text))
        || draft["body"]
            .as_str()
            .is_some_and(|body| body.contains(text))
}

fn snapshot(asset: &Value) -> Result<Value> {
    let history = asset["history"].as_array().ok_or("内容资产历史缺失")?;
    // Archive/restore revisions do not constitute a new content approval.
    let approval = history
        .iter()
        .rev()
        .find(|entry| {
            entry["action"] == "approved"
                && entry["reviewStatus"] == "approved"
                && entry["text"] == asset["text"]
                && entry["source"] == asset["source"]
                && entry["productId"] == asset["productId"]
                && entry["reviewedAt"] == asset["reviewedAt"]
        })
        .ok_or("找不到内容资产当前文本的实际审核记录")?;
    Ok(json!({
        "snapshotVersion":1,
        "assetId":asset["id"],
        "kind":asset["kind"],
        "text":approval["text"],
        "source":approval["source"],
        "productId":approval["productId"],
        "assetRevision":approval["revision"],
        "assetReviewedAt":approval["reviewedAt"],
        "assetUpdatedAt":approval["changedAt"],
    }))
}

pub(super) fn review(c: &Connection, brand_id: &str, draft: &Value) -> Result<Value> {
    let mut snapshots = Vec::new();
    for id in ids(draft)? {
        let asset = load(c, "m_assets", brand_id, &id)?;
        check_scope(&asset, draft)?;
        if !asset["archivedAt"].is_null() {
            return Err("内容资产已归档，不能用于新稿件审核；历史稿件快照不受影响".into());
        }
        if asset["reviewStatus"] != "approved" || asset["reviewedAt"].is_null() {
            return Err("仅已审核通过的内容资产可以关联稿件，请先完成资产审核".into());
        }
        let text = asset["text"].as_str().ok_or("内容资产正文损坏")?;
        if !uses_text(draft, text) {
            return Err(format!(
                "内容资产“{text}”未出现在稿件标题或正文中，请实际使用后再审核"
            ));
        }
        snapshots.push(snapshot(&asset)?);
    }
    Ok(json!(snapshots))
}

fn snapshots(draft: &Value) -> Result<&[Value]> {
    match draft.get("assetSnapshots") {
        None => Ok(&[]),
        Some(value) => value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| "稿件内容资产快照格式错误".into()),
    }
}

pub(super) fn verify_frozen(c: &Connection, brand_id: &str, draft: &Value) -> Result<()> {
    let ids = ids(draft)?;
    let snapshots = snapshots(draft)?;
    if ids.len() != snapshots.len() {
        return Err("稿件内容资产与审核快照数量不一致，请重新审核".into());
    }
    for (id, snapshot) in ids.iter().zip(snapshots) {
        imports::keys(
            snapshot,
            &[
                "snapshotVersion",
                "assetId",
                "kind",
                "text",
                "source",
                "productId",
                "assetRevision",
                "assetReviewedAt",
                "assetUpdatedAt",
            ],
        )?;
        if number(snapshot, "snapshotVersion")? != 1
            || guid(snapshot, "assetId")? != *id
            || snapshot["kind"] != "keyword" && snapshot["kind"] != "title"
        {
            return Err("稿件内容资产快照标识或版本不受支持".into());
        }
        let asset_text = text(snapshot, "text", 500, true)?;
        text(snapshot, "source", 2000, true)?;
        let revision = number(snapshot, "assetRevision")?;
        let reviewed_at = date_time(snapshot, "assetReviewedAt")?;
        let updated_at = date_time(snapshot, "assetUpdatedAt")?;
        if snapshot["productId"] != draft["productId"] && !snapshot["productId"].is_null() {
            return Err("稿件内容资产快照的商品归属不一致".into());
        }
        if let Some(product_id) = snapshot["productId"].as_str() {
            guid(snapshot, "productId")?;
            if product_id != draft["productId"].as_str().unwrap_or("") {
                return Err("稿件内容资产快照的商品归属不一致".into());
            }
        }
        if !uses_text(draft, &asset_text) {
            return Err("稿件正文已不再包含已审核内容资产，请重新审核".into());
        }
        let asset = load(c, "m_assets", brand_id, id)?;
        check_scope(&asset, draft)?;
        if asset["kind"] != snapshot["kind"] || asset["productId"] != snapshot["productId"] {
            return Err("稿件内容资产快照与资产档案不一致".into());
        }
        let history = asset["history"].as_array().ok_or("内容资产历史缺失")?;
        let Some(entry) = history.iter().find(|entry| entry["revision"] == revision) else {
            return Err("找不到稿件使用的内容资产历史版本".into());
        };
        if entry["action"] != "approved"
            || entry["reviewStatus"] != "approved"
            || entry["text"] != snapshot["text"]
            || entry["source"] != snapshot["source"]
            || entry["productId"] != snapshot["productId"]
            || entry["reviewedAt"] != json!(reviewed_at)
            || entry["changedAt"] != json!(updated_at)
        {
            return Err("稿件内容资产快照与历史审核记录不一致".into());
        }
    }
    Ok(())
}

pub(super) fn validate_stored(c: &Connection, brand_id: &str, draft: &Value) -> Result<()> {
    for id in ids(draft)? {
        let asset = load(c, "m_assets", brand_id, &id)?;
        check_scope(&asset, draft)?;
    }
    let has_snapshots = draft.get("assetSnapshots").is_some();
    if has_snapshots {
        verify_frozen(c, brand_id, draft)?;
    } else if !ids(draft)?.is_empty() && !draft["reviewedAt"].is_null() {
        return Err("已审核稿件缺少内容资产快照".into());
    }
    if draft["reviewedAt"].is_null() && !snapshots(draft)?.is_empty() {
        return Err("未审核稿件不能保留内容资产审核快照".into());
    }
    Ok(())
}

fn kind_label(value: &Value) -> &str {
    if value["kind"] == "title" {
        "标题"
    } else {
        "关键词"
    }
}

pub(super) fn markdown_appendix(draft: &Value) -> Result<String> {
    let snapshots = snapshots(draft)?;
    if snapshots.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::from("\n## 内容资产（审核快照）\n");
    for snapshot in snapshots {
        out.push_str(&format!(
            "\n- {}：{}\n  - 来源：{}\n  - 资产：{} · 审核版本 v{}\n",
            kind_label(snapshot),
            snapshot["text"].as_str().unwrap_or(""),
            snapshot["source"].as_str().unwrap_or(""),
            snapshot["assetId"].as_str().unwrap_or(""),
            snapshot["assetRevision"].as_i64().unwrap_or(0),
        ));
    }
    Ok(out)
}

pub(super) fn html_appendix(draft: &Value) -> Result<String> {
    let snapshots = snapshots(draft)?;
    if snapshots.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::from("<section><h2>内容资产（审核快照）</h2><ul>");
    for snapshot in snapshots {
        out.push_str(&format!(
            "<li><p>{}：{}</p><p>来源：{}</p><p>资产：{} · 审核版本 v{}</p></li>",
            kind_label(snapshot),
            escape(snapshot["text"].as_str().unwrap_or("")),
            escape(snapshot["source"].as_str().unwrap_or("")),
            escape(snapshot["assetId"].as_str().unwrap_or("")),
            snapshot["assetRevision"].as_i64().unwrap_or(0),
        ));
    }
    out.push_str("</ul></section>");
    Ok(out)
}
