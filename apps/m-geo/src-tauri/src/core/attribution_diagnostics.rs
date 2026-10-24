//! Advisory explanations for a failed authoritative import; never an import permission.
use super::*;

const KINDS: [&str; 3] = ["visits", "orders", "refunds"];

fn issue(
    scope: &str,
    location: Option<(&str, usize)>,
    field: &str,
    code: &str,
    message: &str,
    related: Vec<(&str, usize)>,
) -> Value {
    let related: BTreeSet<_> = related.into_iter().collect();
    json!({"scope":scope,"kind":location.map(|v|v.0),"index":location.map(|v|v.1),
        "field":field,"code":code,"message":message,
        "related":related.into_iter().take(20).map(|(kind,index)|json!({"kind":kind,"index":index})).collect::<Vec<_>>()})
}

pub(super) fn diagnose(c: &Connection, b: &str, input: &Value, name: &Value) -> Result<Vec<Value>> {
    exact(
        input,
        &[
            "format",
            "evidenceMode",
            "sourceLabel",
            "coverageStart",
            "coverageEnd",
            "currency",
            "visits",
            "orders",
            "refunds",
        ],
    )?;
    let mut count = 0usize;
    for kind in KINDS {
        count = count
            .checked_add(input[kind].as_array().ok_or(INVALID)?.len())
            .ok_or(INVALID)?;
    }
    if count > 1000 {
        return Err(INVALID.into());
    }
    let mut out = Vec::new();
    let mut meta = |valid: bool, field, code, message| {
        if !valid {
            out.push(issue("metadata", None, field, code, message, vec![]));
        }
    };
    meta(
        plain(&json!({"name":name}), "name", 100).is_ok(),
        "name",
        "metadata.name",
        "请输入有效的资料名称，最多一百个字符且不含控制字符",
    );
    meta(
        selection(input, "format", &["m-geo-attribution-v1"]).is_ok(),
        "format",
        "metadata.format",
        "请使用支持的归因资料格式",
    );
    meta(
        selection(input, "evidenceMode", &["simulated", "recorded"]).is_ok(),
        "evidenceMode",
        "metadata.evidenceMode",
        "请选择有效的证据模式",
    );
    meta(
        plain(input, "sourceLabel", 100).is_ok(),
        "sourceLabel",
        "metadata.sourceLabel",
        "请输入有效的来源说明，最多一百个字符且不含控制字符",
    );
    meta(
        selection(input, "currency", &["CNY", "USD", "EUR"]).is_ok(),
        "currency",
        "metadata.currency",
        "请选择支持的币种",
    );
    let start = time(input, "coverageStart");
    let end = time(input, "coverageEnd");
    meta(
        start.is_ok(),
        "coverageStart",
        "metadata.time",
        "覆盖开始时间须包含明确时区并精确到整秒",
    );
    meta(
        end.is_ok(),
        "coverageEnd",
        "metadata.time",
        "覆盖结束时间须包含明确时区并精确到整秒",
    );
    if let (Ok((_, st)), Ok((_, en))) = (&start, &end) {
        meta(
            en > st && en - st <= 366 * 86400,
            "coverageEnd",
            "metadata.coverage",
            "覆盖结束时间须晚于开始时间，且区间不超过三百六十六天",
        );
    }
    if !out.is_empty() {
        return Ok(out);
    }
    let st = start?.1;
    let en = end?.1;

    // Validate every independent field before examining references or the catalog.
    let mut canonical = BTreeMap::<&str, Vec<Value>>::new();
    for kind in KINDS {
        let mut rows = Vec::new();
        for (index, row) in input[kind].as_array().unwrap().iter().enumerate() {
            let loc = Some((kind, index));
            let keys: &[&str] = match kind {
                "visits" => &[
                    "id",
                    "sessionId",
                    "sku",
                    "occurredAt",
                    "source",
                    "sourceEvidence",
                ],
                "orders" => &[
                    "id",
                    "sessionId",
                    "sku",
                    "occurredAt",
                    "status",
                    "amountMinor",
                ],
                _ => &["id", "orderId", "occurredAt", "amountMinor"],
            };
            if exact(row, keys).is_err() {
                out.push(issue(
                    "row",
                    loc,
                    "row",
                    "row.fields",
                    "请检查此记录的字段是否完整且符合记录类型",
                    vec![],
                ));
                continue;
            }
            let mut check = |valid: bool, field, code, message| {
                if !valid {
                    out.push(issue("row", loc, field, code, message, vec![]));
                }
            };
            check(
                token(row, "id").is_ok(),
                "id",
                "row.token",
                "记录标识须为一至八十位字母、数字或允许的标点",
            );
            let at = time(row, "occurredAt");
            match &at {
                Err(_) => check(
                    false,
                    "occurredAt",
                    "row.time",
                    "发生时间须包含明确时区并精确到整秒",
                ),
                Ok((_, ts)) => check(
                    *ts >= st && *ts < en,
                    "occurredAt",
                    "row.coverage",
                    "发生时间须在覆盖区间内，包含开始且不包含结束",
                ),
            }
            if kind == "refunds" {
                check(
                    token(row, "orderId").is_ok(),
                    "orderId",
                    "row.token",
                    "订单标识须为一至八十位字母、数字或允许的标点",
                );
                check(
                    money(row).is_ok(),
                    "amountMinor",
                    "row.amount",
                    "金额须为一至十亿之间的整数最小货币单位",
                );
            } else {
                check(
                    text(row, "sku", 100, true).is_ok(),
                    "sku",
                    "row.sku",
                    "请填写有效的商品编码，最多一百个字符且不含空字符",
                );
                check(
                    (kind == "orders" && row["sessionId"].is_null())
                        || token(row, "sessionId").is_ok(),
                    "sessionId",
                    "row.token",
                    "会话标识须为一至八十位字母、数字或允许的标点",
                );
                if kind == "visits" {
                    let source = selection(
                        row,
                        "source",
                        &["doubao", "yuanbao", "qianwen", "baidu", "other", "unknown"],
                    );
                    let evidence =
                        selection(row, "sourceEvidence", &["tagged", "referrer", "unknown"]);
                    check(
                        source.is_ok(),
                        "source",
                        "row.source",
                        "请选择有效的访问来源",
                    );
                    check(
                        evidence.is_ok(),
                        "sourceEvidence",
                        "row.sourceEvidence",
                        "请选择有效的来源证据类型",
                    );
                    if let (Ok(source), Ok(evidence)) = (source, evidence) {
                        check(
                            (source == "unknown") == (evidence == "unknown"),
                            "sourceEvidence",
                            "row.sourcePair",
                            "访问来源与来源证据须同时为未知或同时提供",
                        );
                    }
                } else {
                    check(
                        selection(row, "status", &["paid", "cancelled"]).is_ok(),
                        "status",
                        "row.status",
                        "订单状态须为已付款或已取消",
                    );
                    check(
                        money(row).is_ok(),
                        "amountMinor",
                        "row.amount",
                        "金额须为一至十亿之间的整数最小货币单位",
                    );
                }
            }
            // These are the only row values changed by normalize: UTC time and trimmed SKU.
            let mut normalized = row.clone();
            if let Ok((at, _)) = at {
                normalized["occurredAt"] = json!(at);
            }
            if kind != "refunds" {
                if let Ok(sku) = text(row, "sku", 100, true) {
                    normalized["sku"] = json!(sku);
                }
            }
            rows.push(normalized);
        }
        canonical.insert(kind, rows);
    }
    if !out.is_empty() {
        return Ok(out);
    }

    // Identity is scoped to the record kind. Equal canonical repeats are not evidence twice.
    let mut first = BTreeMap::<&str, BTreeMap<String, usize>>::new();
    let mut conflicts = BTreeSet::new();
    for kind in KINDS {
        let mut by_id = BTreeMap::new();
        for (index, row) in canonical[kind].iter().enumerate() {
            let id = row["id"].as_str().unwrap();
            if let Some(&original) = by_id.get(id) {
                if row != &canonical[kind][original] {
                    conflicts.insert((kind, id.to_owned()));
                    out.push(issue(
                        "reference",
                        Some((kind, index)),
                        "id",
                        "reference.duplicate",
                        "相同类型的记录标识存在内容冲突，请核对原记录",
                        vec![(kind, original)],
                    ));
                }
            } else {
                by_id.insert(id.to_owned(), index);
            }
        }
        first.insert(kind, by_id);
    }
    // Skip ambiguous evidence, but retain independent errors elsewhere in this stage.
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    let mut invalid_groups = BTreeSet::new();
    for (index, refund) in canonical["refunds"].iter().enumerate() {
        let order_id = refund["orderId"].as_str().unwrap();
        if conflicts.contains(&("refunds", refund["id"].as_str().unwrap().to_owned())) {
            if let Some(&order_index) = first["orders"].get(order_id) {
                invalid_groups.insert(order_index);
            }
            continue;
        }
        if conflicts.contains(&("orders", order_id.to_owned())) {
            continue;
        }
        if first["refunds"][refund["id"].as_str().unwrap()] != index {
            continue;
        }
        let Some(&order_index) = first["orders"].get(refund["orderId"].as_str().unwrap()) else {
            out.push(issue(
                "reference",
                Some(("refunds", index)),
                "orderId",
                "reference.orderMissing",
                "退款引用的订单不在本次资料中，请补齐或更正引用",
                vec![],
            ));
            continue;
        };
        let order = &canonical["orders"][order_index];
        let related = vec![("orders", order_index)];
        if order["status"] != "paid" {
            out.push(issue(
                "reference",
                Some(("refunds", index)),
                "orderId",
                "reference.orderNotPaid",
                "退款须引用已付款订单",
                related.clone(),
            ));
            invalid_groups.insert(order_index);
        }
        if time(refund, "occurredAt")?.1 < time(order, "occurredAt")?.1 {
            out.push(issue(
                "reference",
                Some(("refunds", index)),
                "occurredAt",
                "reference.refundBeforeOrder",
                "退款时间不得早于其订单时间",
                related,
            ));
            invalid_groups.insert(order_index);
        }
        groups.entry(order_index).or_default().push(index);
    }
    for (order_index, refunds) in groups {
        if invalid_groups.contains(&order_index) {
            continue;
        }
        let sum = refunds.iter().try_fold(0i64, |sum, &i| {
            sum.checked_add(money(&canonical["refunds"][i])?)
                .ok_or_else(|| INVALID.to_owned())
        })?;
        if sum > money(&canonical["orders"][order_index])? {
            let index = *refunds.last().unwrap();
            let mut related = vec![("orders", order_index)];
            related.extend(
                refunds
                    .into_iter()
                    .filter(|&i| i != index)
                    .map(|i| ("refunds", i)),
            );
            out.push(issue(
                "reference",
                Some(("refunds", index)),
                "amountMinor",
                "reference.refundExcess",
                "同一订单的不同退款合计超过订单金额，请核对相关记录",
                related,
            ));
        }
    }
    out.sort_by_key(|v| {
        (
            KINDS
                .iter()
                .position(|kind| v["kind"] == *kind)
                .unwrap_or(3),
            v["index"].as_u64().unwrap_or(0),
        )
    });
    if !out.is_empty() {
        return Ok(out);
    }

    // Reuse authoritative normalization for final dataset constraints and catalog identities.
    // No explanation is an acceptance decision: unexplained legacy errors fall back upstream.
    let Ok((normalized, _)) = normalize(input) else {
        return Ok(vec![]);
    };
    let mut availability = BTreeMap::new();
    let mut query = c
        .prepare("SELECT id,data FROM m_products WHERE brand_id=?1 AND sku=?2")
        .map_err(db_error)?;
    for sku in skus(&normalized) {
        let rows = query
            .query_map(params![b, sku], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_error)?;
        let available = rows.len() == 1;
        if available {
            let (id, data) = &rows[0];
            let product = parse(data)?;
            if product["id"] != *id || product["brandId"] != b || product["sku"] != sku {
                return Err(INVALID.into());
            }
            Uuid::parse_str(id).map_err(|_| INVALID)?;
            text(&product, "name", 200, true).map_err(|_| INVALID)?;
            product["revision"]
                .as_i64()
                .filter(|v| *v > 0)
                .ok_or(INVALID)?;
        }
        availability.insert(sku, available);
    }
    for kind in ["visits", "orders"] {
        for (index, row) in canonical[kind].iter().enumerate() {
            if !availability[row["sku"].as_str().unwrap()] {
                out.push(issue(
                    "catalog",
                    Some((kind, index)),
                    "sku",
                    "catalog.skuUnavailable",
                    "商品编码在当前品牌目录中不存在或不唯一，请核对目录与编码",
                    vec![],
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构诊断品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构茶杯","sku":"SIM-1","facts":[]}),
        )
        .unwrap();
        let x = json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":"虚构演练","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","visits":[{"id":"v1","sessionId":"s1","sku":"SIM-1","occurredAt":"2026-10-02T00:00:00Z","source":"doubao","sourceEvidence":"tagged"}],"orders":[{"id":"o1","sessionId":null,"sku":"SIM-1","occurredAt":"2026-10-03T00:00:00Z","status":"paid","amountMinor":1000}],"refunds":[{"id":"r1","orderId":"o1","occurredAt":"2026-10-04T00:00:00Z","amountMinor":100}]});
        (c, b, x)
    }
    pub(super) fn issues(c: &Connection, b: &str, x: &Value) -> Vec<Value> {
        diagnose(c, b, x, &json!("虚构导入")).unwrap()
    }
    fn locations(v: &[Value]) -> Vec<(String, Value, Value)> {
        v.iter()
            .map(|i| {
                (
                    i["scope"].as_str().unwrap().to_owned(),
                    i["kind"].clone(),
                    i["field"].clone(),
                )
            })
            .collect()
    }
    #[test]
    fn metadata_collects_independent_failures_before_rows() {
        let (c, b, mut x) = fixture();
        x["evidenceMode"] = json!("PRIVATE-MARKER");
        x["sourceLabel"] = json!("");
        x["currency"] = json!("bad");
        x["coverageEnd"] = json!("2026-10-01T00:00:00Z");
        x["visits"][0]["id"] = json!("bad id");
        let got = diagnose(&c, &b, &x, &json!("\n")).unwrap();
        assert_eq!(
            got.iter()
                .map(|i| i["field"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "name",
                "evidenceMode",
                "sourceLabel",
                "currency",
                "coverageEnd"
            ]
        );
        assert!(got
            .iter()
            .all(|i| i["scope"] == "metadata" && i["index"].is_null() && i["kind"].is_null()));
        assert!(!serde_json::to_string(&got)
            .unwrap()
            .contains("PRIVATE-MARKER"));
    }
    #[test]
    fn row_stage_collects_times_tokens_skus_sources_status_and_amounts() {
        let (c, b, mut x) = fixture();
        x["visits"][0]["id"] = json!("bad id");
        x["visits"][0]["occurredAt"] = json!("invalid-secret");
        x["visits"][0]["sessionId"] = json!("");
        x["visits"][0]["sku"] = json!("\0");
        x["visits"][0]["sourceEvidence"] = json!("unknown");
        x["orders"][0]["status"] = json!("invalid");
        x["orders"][0]["amountMinor"] = json!(0);
        x["refunds"][0]["orderId"] = json!("bad id");
        let got = issues(&c, &b, &x);
        assert_eq!(
            locations(&got),
            vec![
                ("row".into(), json!("visits"), json!("id")),
                ("row".into(), json!("visits"), json!("occurredAt")),
                ("row".into(), json!("visits"), json!("sku")),
                ("row".into(), json!("visits"), json!("sessionId")),
                ("row".into(), json!("visits"), json!("sourceEvidence")),
                ("row".into(), json!("orders"), json!("status")),
                ("row".into(), json!("orders"), json!("amountMinor")),
                ("row".into(), json!("refunds"), json!("orderId"))
            ]
        );
        assert!(!serde_json::to_string(&got)
            .unwrap()
            .contains("invalid-secret"));
    }
    #[test]
    fn row_coverage_is_half_open_and_invalid_parent_blocks_reference_stage() {
        let (c, b, mut x) = fixture();
        x["orders"][0]["occurredAt"] = x["coverageEnd"].clone();
        x["refunds"][0]["orderId"] = json!("missing");
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["field"], "occurredAt");
        assert_eq!(got[0]["code"], "row.coverage");
    }
    #[test]
    fn canonical_duplicates_and_per_kind_identity_do_not_conflict() {
        let (c, b, mut x) = fixture();
        let mut visit = x["visits"][0].clone();
        visit["occurredAt"] = json!("2026-10-02T08:00:00+08:00");
        visit["sku"] = json!(" SIM-1 ");
        x["visits"].as_array_mut().unwrap().push(visit);
        x["orders"][0]["id"] = json!("v1");
        x["refunds"][0]["orderId"] = json!("v1");
        assert!(normalize(&x).is_ok());
        assert!(issues(&c, &b, &x).is_empty());
        x["visits"][1]["sessionId"] = json!("different");
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["code"], "reference.duplicate");
        assert_eq!(got[0]["index"], 1);
        assert_eq!(got[0]["related"], json!([{"kind":"visits","index":0}]));
    }
    #[test]
    fn reference_stage_explains_orphan_nonpaid_and_early_refunds() {
        let (c, b, mut x) = fixture();
        let mut cancelled = x["orders"][0].clone();
        cancelled["id"] = json!("cancelled");
        cancelled["status"] = json!("cancelled");
        x["orders"].as_array_mut().unwrap().push(cancelled);
        x["refunds"] = json!([{"id":"r1","orderId":"absent","occurredAt":"2026-10-04T00:00:00Z","amountMinor":10},{"id":"r2","orderId":"cancelled","occurredAt":"2026-10-04T00:00:00Z","amountMinor":10},{"id":"r3","orderId":"o1","occurredAt":"2026-10-02T00:00:00Z","amountMinor":10}]);
        let got = issues(&c, &b, &x);
        assert_eq!(
            got.iter()
                .map(|i| i["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "reference.orderMissing",
                "reference.orderNotPaid",
                "reference.refundBeforeOrder"
            ]
        );
        assert_eq!(got[1]["related"], json!([{"kind":"orders","index":1}]));
    }
    #[test]
    fn distinct_refund_sum_links_parent_and_contributors_without_counting_duplicates() {
        let (c, b, mut x) = fixture();
        x["refunds"][0]["amountMinor"] = json!(600);
        let same = x["refunds"][0].clone();
        x["refunds"].as_array_mut().unwrap().push(same);
        assert!(normalize(&x).is_ok());
        assert!(issues(&c, &b, &x).is_empty());
        let mut second = x["refunds"][0].clone();
        second["id"] = json!("r2");
        second["amountMinor"] = json!(500);
        x["refunds"].as_array_mut().unwrap().push(second);
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["code"], "reference.refundExcess");
        assert_eq!(got[0]["index"], 2);
        assert_eq!(
            got[0]["related"],
            json!([{"kind":"orders","index":0},{"kind":"refunds","index":0}])
        );
    }
    #[test]
    fn catalog_respects_brand_and_valid_sku_whitespace_and_is_readonly() {
        let (mut c, b, mut x) = fixture();
        let other = handle(&mut c, "brand.create", json!({"name":"虚构别的品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        handle(
            &mut c,
            "product.save",
            json!({"brandId":other,"name":"虚构别牌商品","sku":"PRIVATE-SKU","facts":[]}),
        )
        .unwrap();
        let sku = "SIM\nBLUE\t1";
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构带空白商品","sku":sku,"facts":[]}),
        )
        .unwrap();
        x["visits"][0]["sku"] = json!(format!(" {sku} "));
        assert!(issues(&c, &b, &x).is_empty());
        x["orders"][0]["sku"] = json!("PRIVATE-SKU");
        let before = c.total_changes();
        let got = issues(&c, &b, &x);
        assert_eq!(c.total_changes(), before);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["scope"], "catalog");
        assert_eq!(got[0]["field"], "sku");
        assert!(!serde_json::to_string(&got).unwrap().contains("PRIVATE-SKU"));
    }
    #[test]
    fn corrupt_referenced_catalog_data_propagates_safe_failure() {
        let (c, b, x) = fixture();
        c.execute(
            "UPDATE m_products SET data='PRIVATE-CORRUPT' WHERE brand_id=?1",
            [&b],
        )
        .unwrap();
        let got = diagnose(&c, &b, &x, &json!("虚构导入"));
        assert!(got.is_err());
        assert!(!got.unwrap_err().contains("PRIVATE-CORRUPT"));
    }
}

#[cfg(test)]
mod edge_tests {
    use super::tests::{fixture, issues};
    use super::*;

    #[test]
    fn metadata_rejects_invalid_times_and_long_coverage_without_row_cascades() {
        let (c, b, mut x) = fixture();
        x["coverageStart"] = json!("2026-10-01T00:00:00.1Z");
        x["coverageEnd"] = json!("2026-11-01T00:00:00");
        let got = issues(&c, &b, &x);
        assert_eq!(
            got.iter()
                .map(|i| i["field"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["coverageStart", "coverageEnd"]
        );
        x["coverageStart"] = json!("2026-10-01T00:00:00Z");
        x["coverageEnd"] = json!("2027-10-03T00:00:00Z");
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["code"], "metadata.coverage");
    }

    #[test]
    fn invalid_enums_and_refund_fields_are_independent_and_safe() {
        let (c, b, mut x) = fixture();
        x["visits"][0]["source"] = json!("PRIVATE-SOURCE");
        x["visits"][0]["sourceEvidence"] = json!("PRIVATE-EVIDENCE");
        x["orders"][0]["sessionId"] = json!("bad token");
        x["refunds"][0]["id"] = json!("");
        x["refunds"][0]["amountMinor"] = json!(1000000001i64);
        let got = issues(&c, &b, &x);
        assert_eq!(
            got.iter()
                .map(|i| i["field"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["source", "sourceEvidence", "sessionId", "id", "amountMinor"]
        );
        for value in &got {
            assert_eq!(value.as_object().unwrap().len(), 7);
            assert!(value["related"].as_array().unwrap().is_empty());
        }
        assert!(!serde_json::to_string(&got).unwrap().contains("PRIVATE"));
    }

    #[test]
    fn conflicting_parent_is_explained_before_dependent_refund_checks() {
        let (c, b, mut x) = fixture();
        let mut duplicate = x["orders"][0].clone();
        duplicate["status"] = json!("cancelled");
        x["orders"].as_array_mut().unwrap().push(duplicate);
        x["refunds"][0]["amountMinor"] = json!(2000);
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["code"], "reference.duplicate");
        assert_eq!(got[0]["related"], json!([{"kind":"orders","index":0}]));
    }

    #[test]
    fn conflicting_parent_does_not_hide_independent_reference_errors() {
        let (c, b, mut x) = fixture();
        let mut duplicate = x["orders"][0].clone();
        duplicate["status"] = json!("cancelled");
        x["orders"].as_array_mut().unwrap().push(duplicate);
        x["refunds"][0]["amountMinor"] = json!(2000);
        let mut orphan = x["refunds"][0].clone();
        orphan["id"] = json!("r2");
        orphan["orderId"] = json!("missing");
        x["refunds"].as_array_mut().unwrap().push(orphan);
        let got = issues(&c, &b, &x);
        assert_eq!(
            got.iter()
                .map(|i| i["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["reference.duplicate", "reference.orderMissing"]
        );
    }

    #[test]
    fn refund_duplicates_canonicalize_offsets_and_bounded_related_references() {
        let (c, b, mut x) = fixture();
        let mut duplicate = x["refunds"][0].clone();
        duplicate["occurredAt"] = json!("2026-10-04T08:00:00+08:00");
        x["refunds"].as_array_mut().unwrap().push(duplicate);
        assert!(issues(&c, &b, &x).is_empty());
        let row = x["refunds"][0].clone();
        x["refunds"] = json!((0..25)
            .map(|i| {
                let mut r = row.clone();
                r["id"] = json!(format!("r{i}"));
                r["amountMinor"] = json!(50);
                r
            })
            .collect::<Vec<_>>());
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["index"], 24);
        let refs = got[0]["related"].as_array().unwrap();
        assert_eq!(refs.len(), 20);
        assert_eq!(refs[0], json!({"kind":"orders","index":0}));
        assert_eq!(refs[19], json!({"kind":"refunds","index":18}));
    }

    #[test]
    fn catalog_ambiguity_is_actionable_but_database_unavailability_is_error() {
        let (c, b, x) = fixture();
        let d = Connection::open_in_memory().unwrap();
        d.execute_batch("CREATE TABLE m_products(id TEXT,data TEXT,brand_id TEXT,sku TEXT);")
            .unwrap();
        let p: String = c
            .query_row("SELECT data FROM m_products WHERE brand_id=?1", [&b], |r| {
                r.get(0)
            })
            .unwrap();
        for _ in 0..2 {
            d.execute(
                "INSERT INTO m_products VALUES(?1,?2,?3,'SIM-1')",
                params!["unused", p, b],
            )
            .unwrap();
        }
        let got = issues(&d, &b, &x);
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|i| i["code"] == "catalog.skuUnavailable"));
        d.execute("DROP TABLE m_products", []).unwrap();
        assert!(diagnose(&d, &b, &x, &json!("虚构导入")).is_err());
    }

    #[test]
    fn helper_bounds_source_rows_and_returns_all_examined_stage_issues() {
        let (c, b, mut x) = fixture();
        x["orders"] = json!([]);
        x["refunds"] = json!([]);
        let mut row = x["visits"][0].clone();
        row["id"] = json!("bad id");
        x["visits"] = json!(vec![row.clone(); 1000]);
        let got = issues(&c, &b, &x);
        assert_eq!(got.len(), 1000);
        assert_eq!(got[999]["index"], 999);
        x["visits"].as_array_mut().unwrap().push(row);
        assert!(diagnose(&c, &b, &x, &json!("虚构导入")).is_err());
    }
}
