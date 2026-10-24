use super::*;
/// Build an unsaved CSV using the same explicit simulated exchange as Phase 5.
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let template = attribution_template::prepare(c, b, p)?;
    let exchange = &template["exchange"];
    let fields = [
        "kind",
        "id",
        "sessionId",
        "sku",
        "occurredAt",
        "source",
        "sourceEvidence",
        "status",
        "amount",
        "orderId",
    ];
    let mut writer = ::csv::Writer::from_writer(Vec::new());
    writer.write_record(fields).map_err(|_| "模拟CSV生成失败")?;
    for kind in ["visits", "orders", "refunds"] {
        for row in exchange[kind].as_array().ok_or("模拟CSV生成失败")? {
            let cells = fields
                .iter()
                .map(|field| match *field {
                    "kind" => kind.to_owned(),
                    "amount" => row["amountMinor"]
                        .as_i64()
                        .map(|amount| format!("{}.{:02}", amount / 100, amount % 100))
                        .unwrap_or_default(),
                    key => row[key].as_str().unwrap_or("").to_owned(),
                })
                .collect::<Vec<_>>();
            writer.write_record(cells).map_err(|_| "模拟CSV生成失败")?;
        }
    }
    let csv = String::from_utf8(writer.into_inner().map_err(|_| "模拟CSV生成失败")?)
        .map_err(|_| "模拟CSV生成失败")?;
    let mut mapping = serde_json::Map::new();
    for field in fields {
        mapping.insert(field.into(), json!(field));
    }
    Ok(
        json!({"name":"虚构商品CSV演练","csv":csv,"delimiter":"comma","mapping":mapping,
      "metadata":{"evidenceMode":exchange["evidenceMode"],"sourceLabel":exchange["sourceLabel"],"coverageStart":exchange["coverageStart"],"coverageEnd":exchange["coverageEnd"],"currency":exchange["currency"],"amountUnit":"major"}}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase6_csv_template_is_unsaved_and_maps_quoted_skus() {
        let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let b =
            handle(&mut c, "brand.create", json!({"name":"虚构CSV演练"})).unwrap()["id"].clone();
        for sku in ["SIM-A", "SIM-B,XL", "SIM-C\n蓝色"] {
            handle(
                &mut c,
                "product.save",
                json!({"brandId":b,"name":"虚构商品","sku":sku,"facts":[]}),
            )
            .unwrap();
        }
        let t = handle(&mut c, "attribution.csvTemplate", json!({"brandId":b})).unwrap();
        assert_eq!(t["metadata"]["evidenceMode"], "simulated");
        let mapped=handle(&mut c,"attribution.csvPrepare",json!({"brandId":b,"name":t["name"],"csv":t["csv"],"delimiter":t["delimiter"],"mapping":t["mapping"],"metadata":t["metadata"]})).unwrap();
        assert_eq!(
            mapped["preview"]["counts"],
            json!({"visits":9,"orders":9,"refunds":2,"duplicates":1})
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM m_attribution_datasets", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM m_local_jobs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
