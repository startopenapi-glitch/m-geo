//! Structured buying facts reuse the existing provenance, privacy and revision boundary.
use super::*;
pub(super) const FIELDS: &[(&str, &str)] = &[
    ("price", "价格"),
    ("currency", "币种"),
    ("availability", "库存状态"),
    ("audience", "适用人群"),
    ("scenario", "使用场景"),
    ("limits", "使用限制"),
    ("purchase", "购买链接"),
    ("afterSales", "售后说明"),
    ("image", "商品图片"),
];
pub(super) fn value(f: &Value) -> Result<()> {
    let Some(key) = f["id"].as_str().and_then(|s| s.strip_prefix("commerce:")) else {
        return Ok(());
    };
    if !FIELDS.iter().any(|(k, _)| *k == key) {
        return Err("不支持的购买资料字段".into());
    }
    let s = text(f, "value", 3000, true)?;
    match key {
        "price" => {
            let parts: Vec<_> = s.split('.').collect();
            if parts.len() > 2
                || parts[0].is_empty()
                || parts[0].len() > 9
                || !parts[0].bytes().all(|b| b.is_ascii_digit())
                || (parts.len() == 2
                    && (parts[1].is_empty()
                        || parts[1].len() > 2
                        || !parts[1].bytes().all(|b| b.is_ascii_digit())))
            {
                return Err("价格请填写非负数字，最多两位小数".into());
            }
        }
        "currency" if !["CNY", "USD", "EUR", "GBP", "JPY", "HKD"].contains(&s.as_str()) => {
            return Err("请选择受支持的币种代码".into())
        }
        "availability"
            if !["InStock", "OutOfStock", "PreOrder", "Unknown"].contains(&s.as_str()) =>
        {
            return Err("请选择有效库存状态".into())
        }
        "purchase" | "image" => web_url(&s, true)?,
        _ => {}
    }
    Ok(())
}
pub(super) fn freshness(f: &Value) -> Result<()> {
    if ["commerce:price", "commerce:availability"].contains(&f["id"].as_str().unwrap_or(""))
        && f["validUntil"].is_null()
    {
        return Err("价格和库存必须填写有效期，避免发布过期购买信息".into());
    }
    Ok(())
}
pub(super) fn get<'a>(facts: &'a [Value], key: &str) -> Option<&'a str> {
    facts
        .iter()
        .find(|f| f["id"] == format!("commerce:{key}"))
        .and_then(|f| f["value"].as_str())
}
