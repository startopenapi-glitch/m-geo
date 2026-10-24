use super::*;
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    imports::keys(p, &["brandId"])?;
    brand(c, b)?;
    let mut st = c
        .prepare("SELECT sku FROM m_products WHERE brand_id=?1 ORDER BY sku,id LIMIT 3")
        .map_err(db_error)?;
    let skus = st
        .query_map([b], |r| r.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?;
    if skus.is_empty() {
        return Err("请先添加至少一件虚构商品，再生成模拟模板".into());
    }
    let a = &skus[0];
    let bsku = skus.get(1).unwrap_or(a);
    let csku = skus.get(2).unwrap_or(a);
    let visit = |id: &str, session: &str, sku: &str, at: &str, source: &str| json!({"id":id,"sessionId":session,"sku":sku,"occurredAt":at,"source":source,"sourceEvidence":if source=="unknown"{"unknown"}else{"tagged"}});
    let order = |id: &str,
                 session: Option<&str>,
                 sku: &str,
                 at: &str,
                 status: &str,
                 amount: i64| json!({"id":id,"sessionId":session,"sku":sku,"occurredAt":at,"status":status,"amountMinor":amount});
    let first = visit("SIM-V1", "SIM-S1", a, "2026-10-11T23:50:00Z", "doubao");
    let exchange = json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":"虚构商品与模拟访问订单，仅供本地流程演练","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-10-23T00:00:00Z","currency":"CNY",
      "visits":[first.clone(),first,
        visit("SIM-V2","SIM-S2",bsku,"2026-10-12T01:00:00Z","yuanbao"),
        visit("SIM-V3","SIM-S3",csku,"2026-10-13T01:00:00Z","unknown"),
        visit("SIM-V4","SIM-S4",a,"2026-10-14T01:00:00Z","doubao"),
        visit("SIM-V5","SIM-S4",a,"2026-10-15T01:00:00Z","unknown"),
        visit("SIM-V6","SIM-S5",bsku,"2026-10-16T01:00:00Z","qianwen"),
        visit("SIM-V7","SIM-S6",csku,"2026-10-18T01:00:00Z","doubao"),
        visit("SIM-V8","SIM-S6",csku,"2026-10-18T01:00:00Z","yuanbao"),
        visit("SIM-V9","SIM-S7",csku,"2026-10-21T01:00:00Z","baidu")],
      "orders":[order("SIM-O1",Some("SIM-S1"),a,"2026-10-12T00:10:00Z","paid",10000),
        order("SIM-O2",Some("SIM-S2"),bsku,"2026-10-12T02:00:00Z","paid",20000),
        order("SIM-O3",Some("SIM-S3"),csku,"2026-10-13T02:00:00Z","paid",15000),
        order("SIM-O4",Some("SIM-S4"),a,"2026-10-15T02:00:00Z","paid",5000),
        order("SIM-O5",Some("SIM-S5"),bsku,"2026-10-17T02:00:00Z","paid",12000),
        order("SIM-O6",Some("SIM-S6"),csku,"2026-10-18T02:00:00Z","paid",9000),
        order("SIM-O7",None,a,"2026-10-19T02:00:00Z","paid",7000),
        order("SIM-O8",Some("SIM-NOMATCH"),csku,"2026-10-20T02:00:00Z","paid",3000),
        order("SIM-O9",Some("SIM-S4"),a,"2026-10-21T02:00:00Z","cancelled",6000)],
      "refunds":[{"id":"SIM-R1","orderId":"SIM-O1","occurredAt":"2026-10-15T03:00:00Z","amountMinor":2000},
        {"id":"SIM-R2","orderId":"SIM-O2","occurredAt":"2026-10-22T03:00:00Z","amountMinor":20000}]});
    let requirements="转入真实运行前，请提供品牌官网域名与数据导出权限、商品 SKU 对照、去标识化会话与订单行标识、支付/退款时间和最小货币单位、访问来源标注规则、覆盖日期与时区。请勿导入姓名、电话、地址、IP、密钥或完整 URL。当前模板全部为模拟数据，不会接入账号或公开发布。\n";
    let methodology="本模板只在本地生成，未自动保存。先预览，再显式保存数据集。JSON 小于等于 512KiB、原始行合计不超过1000；覆盖区间左闭右开，UTC整秒。order.id 是订单行标识，多 SKU 订单拆行。金额是整数分，币种统一且只支持 CNY/USD/EUR。模板故意包含一条完全重复访问用于验证去重。来源为人为声明，不证明平台推荐。默认7日末次访问窗口仅为演练参数；未知末次来源保留未知，同秒不同来源视作歧义。取消单不计成交，全额退款保留成交次数。退款回连原单并累计到覆盖结束；这不是现金流或因果收益。跨周转化按访问周、金额按支付周归组；会话数不可跨来源或周相加。\n";
    Ok(json!({"exchange":exchange,"files":[
      {"path":"input.json","content":serde_json::to_string_pretty(&exchange).map_err(|_|"模板生成失败")?},
      {"path":"methodology.txt","content":methodology},
      {"path":"external-requirements.txt","content":requirements}]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn template_is_unsaved_simulated_and_importable() {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b =
            handle(&mut c, "brand.create", json!({"name":"虚构模板验收"})).unwrap()["id"].clone();
        assert!(handle(&mut c, "attribution.template", json!({"brandId":b})).is_err());
        for (sku, name) in [
            ("SIM-A", "虚构水壶"),
            ("SIM-B", "虚构背包"),
            ("SIM-C", "虚构台灯"),
        ] {
            handle(
                &mut c,
                "product.save",
                json!({"brandId":b,"sku":sku,"name":name,"facts":[]}),
            )
            .unwrap();
        }
        let t = handle(&mut c, "attribution.template", json!({"brandId":b})).unwrap();
        assert_eq!(t["exchange"]["evidenceMode"], "simulated");
        assert_eq!(
            handle(&mut c, "attribution.list", json!({"brandId":b})).unwrap()["datasets"],
            json!([])
        );
        let p = handle(
            &mut c,
            "attribution.preview",
            json!({"brandId":b,"name":"模板测试","content":t["exchange"].to_string()}),
        )
        .unwrap();
        assert_eq!(
            p["counts"],
            json!({"visits":9,"orders":9,"refunds":2,"duplicates":1})
        );
        assert_eq!(crate::files::site_zip(&t).unwrap()[..2], *b"PK");
    }
}
