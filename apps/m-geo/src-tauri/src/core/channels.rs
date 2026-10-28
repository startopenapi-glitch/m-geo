//! Versioned local channel templates. They format approved facts only and never publish.
use super::*;

pub(super) const IDS: &[&str] = &[
    "jd",
    "tmall",
    "douyin",
    "wechat_channels",
    "xiaohongshu",
    "kuaishou",
    "vipshop",
];
pub(super) const TEMPLATE_VERSION: i64 = 1;

pub(super) fn parse(v: &Value, key: &str) -> Result<Option<String>> {
    if v.get(key).is_none_or(Value::is_null) {
        return Ok(None);
    }
    choice(v, key, IDS).map(Some)
}
pub(super) fn default_kind(channel: &str) -> &'static str {
    match channel {
        "wechat_channels" | "xiaohongshu" => "guide",
        _ => "description",
    }
}
pub(super) fn label(channel: &str) -> &'static str {
    match channel {
        "jd" => "京东商品参数与详情",
        "tmall" => "天猫类目资料与规格选择",
        "douyin" => "抖音短视频与商品图文",
        "wechat_channels" => "视频号口播与公众号说明",
        "xiaohongshu" => "小红书场景选购与商品笔记",
        "kuaishou" => "快手短视频与直播问答",
        "vipshop" => "唯品会供应商商品资料",
        _ => "商品内容",
    }
}
fn facts(ctx: &Value) -> Result<String> {
    let mut out = String::new();
    for fact in ctx["sourceSnapshot"]["facts"]
        .as_array()
        .ok_or("商品事实格式错误")?
    {
        out.push_str(&format!(
            "- {}：{}\n",
            fact["label"].as_str().unwrap_or(""),
            fact["value"].as_str().unwrap_or("")
        ));
    }
    Ok(out)
}
pub(super) fn body(ctx: &Value) -> Result<Option<String>> {
    let Some(channel) = ctx["channel"].as_str() else {
        return Ok(None);
    };
    let name = ctx["sourceSnapshot"]["productName"]
        .as_str()
        .ok_or("商品名称损坏")?;
    let sku = ctx["sourceSnapshot"]["sku"].as_str().unwrap_or("");
    let facts = facts(ctx)?;
    let body = match channel {
        "jd" => format!("## 商品信息\n商品：{name}\nSKU：{sku}\n\n## 参数与规格\n{facts}\n## 选购与使用说明\n以上内容仅依据已确认商品资料。价格、库存、促销和配送以实际销售页面为准；未列出的功效或承诺不作推定。\n"),
        "tmall" => format!("## 商品资料\n商品：{name}\nSKU：{sku}\n\n## 属性与规格\n{facts}\n## 规格选择提示\n请根据上述已确认属性选择对应 SKU。标题、主图、属性和实际销售规格应保持一致；价格与库存以当前页面为准。\n"),
        "douyin" => format!("【开场】\n了解{name}，先核对这几项商品信息。\n\n【商品展示】\n{facts}\n【展示建议】\n逐项展示对应商品和资料，不使用无法证明的效果画面。\n\n【结尾口播】\n规格、价格和库存请以当前商品页面为准。\n"),
        "wechat_channels" => format!("## 视频号口播\n{name}有哪些已经确认的商品信息？\n{facts}\n选择时请核对自己的使用需求和对应规格。\n\n## 公众号延伸阅读｜选购与使用说明\n以上信息来自当前已审核商品资料。未提供的功效、评价和承诺不作补充；价格、库存和售后以当前微信小店页面为准。\n"),
        "xiaohongshu" => format!("## 选购{name}时，可以先看这些信息\n{facts}\n## 怎么选\n先对照自己的使用场景，再核对对应规格。本文仅整理商品资料，不代表个人使用体验、效果承诺或虚构评价。\n"),
        "kuaishou" => format!("## 短视频口播\n今天核对{name}的商品信息：\n{facts}\n选择前看清规格，价格、库存和售后以当前商品页面为准。\n\n## 直播问答提示\n只根据上述资料回答；未确认的信息当场说明需要核实，不临时编造。\n"),
        "vipshop" => format!("## 供应商商品资料\n商品：{name}\nSKU：{sku}\n\n## SPU／SKU 属性与规格\n{facts}\n## 资料核对说明\n尺码、规格和销售信息以当前供应商后台及类目要求为准；未提供的字段不作推定。\n"),
        _ => return Err("不支持的渠道模板".into()),
    };
    Ok(Some(body))
}
