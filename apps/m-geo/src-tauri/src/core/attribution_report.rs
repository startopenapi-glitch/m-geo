use super::*;
use chrono::Datelike;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const METRIC_KEYS: &[&str] = &[
    "visits",
    "visitingSessions",
    "paidOrders",
    "cancelledOrders",
    "attributedOrders",
    "unknownOrders",
    "convertedSessions",
    "grossMinor",
    "refundMinor",
    "netMinor",
    "attributedNetMinor",
    "unknownNetMinor",
    "sessionConversionRate",
    "unknownOrderRate",
    "lookbackCoverageIncomplete",
];
const MAX_EXPORT: usize = 16 * 1024 * 1024;
type Pair = (String, String);

pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let expected = ["brandId", "datasetId", "from", "to", "lookbackDays"];
    if !p
        .as_object()
        .is_some_and(|o| o.len() == expected.len() && expected.iter().all(|k| o.contains_key(*k)))
        || p["brandId"] != b
    {
        return Err("归因报告参数无效".into());
    }
    let dataset = attribution::load_dataset(c, b, &text(p, "datasetId", 80, true)?)?;
    calculate(&dataset, p)
}

// Sibling-only entry: validation is mandatory before entering the calculator.
pub(super) fn calculate_frozen(
    brand_id: &str,
    dataset: &Value,
    parameters: &Value,
) -> Result<Value> {
    attribution::validate_portable_dataset(brand_id, dataset)?;
    calculate(dataset, parameters)
}

#[derive(Default)]
struct Metrics {
    visits: i64,
    sessions: BTreeSet<Pair>,
    converted: BTreeSet<Pair>,
    paid: i64,
    cancelled: i64,
    attributed: i64,
    unknown: i64,
    gross: i64,
    refunds: i64,
    attributed_net: i64,
    unknown_net: i64,
    incomplete: bool,
}
impl Metrics {
    fn visit(&mut self, pair: &Pair) {
        self.visits += 1;
        self.sessions.insert(pair.clone());
    }
    fn order(&mut self, paid: bool, known: bool, amount: i64, refund: i64, incomplete: bool) {
        if !paid {
            self.cancelled += 1;
            return;
        }
        self.paid += 1;
        self.gross += amount;
        self.refunds += refund;
        self.incomplete |= incomplete;
        if known {
            self.attributed += 1;
            self.attributed_net += amount - refund;
        } else {
            self.unknown += 1;
            self.unknown_net += amount - refund;
        }
    }
    fn value(&self) -> Value {
        json!({"visits":self.visits,"visitingSessions":self.sessions.len(),
            "paidOrders":self.paid,"cancelledOrders":self.cancelled,
            "attributedOrders":self.attributed,"unknownOrders":self.unknown,
            "convertedSessions":self.converted.len(),"grossMinor":self.gross,
            "refundMinor":self.refunds,"netMinor":self.gross-self.refunds,
            "attributedNetMinor":self.attributed_net,"unknownNetMinor":self.unknown_net,
            "sessionConversionRate":if self.sessions.is_empty() {None} else {Some(self.converted.len() as f64/self.sessions.len() as f64)},
            "unknownOrderRate":if self.paid==0 {None} else {Some(self.unknown as f64/self.paid as f64)},
            "lookbackCoverageIncomplete":self.incomplete})
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn rows<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    v[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| "归因数据格式无效".into())
}
fn timestamp(value: &str) -> Result<i64> {
    let dt = DateTime::parse_from_rfc3339(value).map_err(|_| "归因时间格式无效")?;
    if value.contains('.') || dt.timestamp_subsec_nanos() != 0 {
        return Err("归因时间必须精确到整秒".into());
    }
    Ok(dt.timestamp())
}
fn utc(time: i64) -> String {
    DateTime::from_timestamp(time, 0)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}
fn week(time: i64) -> String {
    let date = DateTime::from_timestamp(time, 0).unwrap().date_naive();
    (date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)).to_string()
}
fn pair(v: &Value) -> Pair {
    (str_field(v, "sessionId").into(), str_field(v, "sku").into())
}

// Called only after live loading or calculate_frozen validates the exchange and snapshots.
fn calculate(dataset: &Value, p: &Value) -> Result<Value> {
    let input = &dataset["input"];
    let from = timestamp(&text(p, "from", 80, true)?)?;
    let to = timestamp(&text(p, "to", 80, true)?)?;
    let coverage_start = timestamp(str_field(input, "coverageStart"))?;
    let coverage_end = timestamp(str_field(input, "coverageEnd"))?;
    let lookback = p["lookbackDays"]
        .as_i64()
        .filter(|n| (1..=30).contains(n))
        .ok_or("回溯天数必须为 1 到 30 的整数")?;
    if from >= to || from < coverage_start || to > coverage_end {
        return Err("报告区间必须在数据覆盖范围内，且开始时间早于结束时间".into());
    }
    let visits = rows(input, "visits")?;
    let all_orders = rows(input, "orders")?;
    let refunds = rows(input, "refunds")?;
    let product_names: BTreeMap<&str, &str> = rows(dataset, "products")?
        .iter()
        .map(|v| (str_field(v, "sku"), str_field(v, "name")))
        .collect();
    let mut total = Metrics::default();
    let mut sources = BTreeMap::<String, Metrics>::new();
    let mut products = BTreeMap::<String, Metrics>::new();
    let mut weeks = BTreeMap::<String, Metrics>::new();
    for visit in visits {
        let time = timestamp(str_field(visit, "occurredAt"))?;
        if time < from || time >= to {
            continue;
        }
        let pair = pair(visit);
        total.visit(&pair);
        sources
            .entry(str_field(visit, "source").into())
            .or_default()
            .visit(&pair);
        products
            .entry(str_field(visit, "sku").into())
            .or_default()
            .visit(&pair);
        weeks.entry(week(time)).or_default().visit(&pair);
    }
    let mut refund_totals = BTreeMap::<&str, i64>::new();
    for refund in refunds {
        // Validation guarantees all refunds precede the exclusive coverageEnd cutoff.
        *refund_totals
            .entry(str_field(refund, "orderId"))
            .or_default() += refund["amountMinor"].as_i64().ok_or("归因金额无效")?;
    }
    let mut period_orders: Vec<_> = all_orders
        .iter()
        .map(|order| Ok((timestamp(str_field(order, "occurredAt"))?, order)))
        .collect::<Result<_>>()?;
    period_orders.retain(|(time, _)| *time >= from && *time < to);
    period_orders.sort_by(|(a_time, a), (b_time, b)| {
        a_time
            .cmp(b_time)
            .then_with(|| str_field(a, "id").cmp(str_field(b, "id")))
    });
    let timed_visits: Vec<_> = visits
        .iter()
        .map(|v| Ok((timestamp(str_field(v, "occurredAt"))?, v)))
        .collect::<Result<_>>()?;
    let mut ledger = Vec::new();
    for (time, order) in period_orders {
        let paid = order["status"] == "paid";
        let amount = order["amountMinor"].as_i64().ok_or("归因金额无效")?;
        let sku = str_field(order, "sku");
        let mut source = "unknown";
        let mut evidence = "unknown";
        let mut chosen = None;
        let reason;
        if !paid {
            reason = "cancelled";
        } else if order["sessionId"].is_null() {
            reason = "missing_session";
        } else {
            let mut candidates: Vec<_> = timed_visits
                .iter()
                .filter(|(visit_time, visit)| {
                    visit["sessionId"] == order["sessionId"]
                        && str_field(visit, "sku") == sku
                        && *visit_time <= time
                        && *visit_time >= time - lookback * 86400
                })
                .collect();
            candidates.sort_by(|(a_time, a), (b_time, b)| {
                b_time
                    .cmp(a_time)
                    .then_with(|| str_field(a, "id").cmp(str_field(b, "id")))
            });
            if let Some((latest_time, latest)) = candidates.first() {
                if candidates
                    .iter()
                    .take_while(|(t, _)| t == latest_time)
                    .any(|(_, v)| {
                        v["source"] != latest["source"]
                            || v["sourceEvidence"] != latest["sourceEvidence"]
                    })
                {
                    reason = "ambiguous_touch";
                } else {
                    chosen = Some((*latest_time, *latest));
                    source = str_field(latest, "source");
                    if source == "unknown" {
                        reason = "unknown_source";
                    } else {
                        evidence = str_field(latest, "sourceEvidence");
                        reason = "matched";
                    }
                }
            } else {
                reason = "no_matching_visit";
            }
        }
        let refund = if paid {
            *refund_totals.get(str_field(order, "id")).unwrap_or(&0)
        } else {
            0
        };
        let incomplete = paid && time - lookback * 86400 < coverage_start;
        let known = source != "unknown";
        total.order(paid, known, amount, refund, incomplete);
        sources
            .entry(source.into())
            .or_default()
            .order(paid, known, amount, refund, incomplete);
        products
            .entry(sku.into())
            .or_default()
            .order(paid, known, amount, refund, incomplete);
        weeks
            .entry(week(time))
            .or_default()
            .order(paid, known, amount, refund, incomplete);
        let visit_in_period = chosen.is_some_and(|(t, _)| t >= from && t < to);
        if visit_in_period {
            let (visit_time, visit) = chosen.unwrap();
            let pair = pair(visit);
            total.converted.insert(pair.clone());
            sources
                .entry(source.into())
                .or_default()
                .converted
                .insert(pair.clone());
            products
                .entry(sku.into())
                .or_default()
                .converted
                .insert(pair.clone());
            // Conversion belongs to its denominator's visit week, not the payment week.
            weeks
                .entry(week(visit_time))
                .or_default()
                .converted
                .insert(pair);
        }
        ledger.push(json!({"orderId":order["id"],"sku":sku,"occurredAt":order["occurredAt"],
            "status":order["status"],"amountMinor":amount,"refundMinor":refund,
            "netMinor":if paid {amount-refund} else {0},"source":source,"sourceEvidence":evidence,
            "visitId":chosen.map(|(_,v)|str_field(v,"id")),"reason":reason,"visitInPeriod":visit_in_period}));
    }
    let source_rows: Vec<_> = sources
        .into_iter()
        .map(|(key, metrics)| {
            let mut v = metrics.value();
            v["source"] = json!(key);
            v
        })
        .collect();
    let product_rows: Vec<_> = products
        .into_iter()
        .map(|(key, metrics)| {
            let mut v = metrics.value();
            v["name"] = json!(product_names.get(key.as_str()).copied().unwrap_or(""));
            v["sku"] = json!(key);
            v
        })
        .collect();
    let week_rows: Vec<_> = weeks
        .into_iter()
        .map(|(key, metrics)| {
            let mut v = metrics.value();
            v["weekStart"] = json!(key);
            v
        })
        .collect();
    let mut report = json!({"datasetId":dataset["id"],"contentHash":dataset["contentHash"],
    "evidenceMode":input["evidenceMode"],"currency":input["currency"],
    "period":{"from":utc(from),"to":utc(to),"refundAsOf":input["coverageEnd"],"lookbackDays":lookback},
    "summary":total.value(),"bySource":source_rows,"byProduct":product_rows,"weekly":week_rows,"orders":ledger,
    "limitations":[
        "本地独立数据集；simulated 是虚构演练，recorded 是调用方提供的记录；来源由调用方声明，未验证平台推荐或销售因果关系。",
        "访问会话按 (sessionId, sku) 去重；source/week visitingSessions 和 convertedSessions 是 non-additive，不能把分组会话数相加。",
        "转化是报告区间内所选访问会话的历史付款观察，周转化按所选访问的 UTC 周一归组；订单数和金额按订单付款周归组。",
        "退款跟随原始付款订单队列，统计至 refundAsOf 的覆盖截止（不含）；可能含报告结束后的退款。全额退款不抹去历史付款或会话转化。",
        "取消订单单独计数，不贡献付款、收入或转化；未知最新访问不回退旧来源；相互冲突的最新触点没有选定访问。",
        "回溯可能早于 coverageStart；lookbackCoverageIncomplete 只说明覆盖不足，不证明更早没有访问。",
        "金额为整数最小货币单位（100/单位），不合并数据集、证据模式或币种；不计算汇率、退款现金流、利润、ROI 或增长。"
    ]});
    let files = export_files(dataset, &report)?;
    report["files"] = json!(files);
    Ok(report)
}

fn serialized(value: &Value, pretty: bool) -> Result<String> {
    (if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    })
    .map_err(|_| "归因报告序列化失败".into())
}
fn csv_cell(value: &str) -> String {
    let trimmed = value.trim_start_matches(|c: char| c.is_whitespace() || c.is_control());
    let dangerous =
        value.starts_with(['\t', '\r', '\n']) || trimmed.starts_with(['=', '+', '-', '@']);
    format!(
        "\"{}{}\"",
        if dangerous { "'" } else { "" },
        value.replace('"', "\"\"")
    )
}
fn csv_line(values: impl IntoIterator<Item = String>) -> String {
    values
        .into_iter()
        .map(|s| csv_cell(&s))
        .collect::<Vec<_>>()
        .join(",")
        + "\r\n"
}
fn csv_table(metadata: &[(String, String)], data: &[Value], keys: &[&str]) -> String {
    let mut result = csv_line(["metadataKey".into(), "metadataValue".into()]);
    for (key, value) in metadata {
        result.push_str(&csv_line([key.clone(), value.clone()]));
    }
    result.push_str("\r\n");
    result.push_str(&csv_line(keys.iter().map(|s| s.to_string())));
    for row in data {
        result.push_str(&csv_line(keys.iter().map(|key| match &row[*key] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })));
    }
    result
}
fn bound_export(size: usize) -> Result<()> {
    if size > MAX_EXPORT {
        Err("归因导出包超过 16 MiB，请缩小报告范围".into())
    } else {
        Ok(())
    }
}
fn export_files(dataset: &Value, report: &Value) -> Result<Vec<Value>> {
    let input = &dataset["input"];
    let input_json = serialized(input, false)?;
    if input_json.len() > 512 * 1024 {
        return Err("归因交换文件超过 512 KiB".into());
    }
    let mut metadata = Vec::new();
    for (key, value) in [
        ("datasetId", &dataset["id"]),
        ("contentHash", &dataset["contentHash"]),
        ("evidenceMode", &input["evidenceMode"]),
        ("sourceLabel", &input["sourceLabel"]),
        ("currency", &input["currency"]),
        ("coverageStart", &input["coverageStart"]),
        ("coverageEnd", &input["coverageEnd"]),
        ("from", &report["period"]["from"]),
        ("to", &report["period"]["to"]),
        ("refundAsOf", &report["period"]["refundAsOf"]),
        ("lookbackDays", &report["period"]["lookbackDays"]),
    ] {
        metadata.push((
            key.into(),
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string()),
        ));
    }
    metadata.push(("sourceNature".into(),"caller-declared tagged/referrer; simulated=fictional; recorded=caller-supplied; no verified AI-sales causality".into()));
    let mut files = vec![json!({"path":"summary.json","content":serialized(report,true)?})];
    let order_keys = [
        "orderId",
        "sku",
        "occurredAt",
        "status",
        "amountMinor",
        "refundMinor",
        "netMinor",
        "source",
        "sourceEvidence",
        "visitId",
        "reason",
        "visitInPeriod",
    ];
    for (path, group, base) in [
        ("orders.csv", "orders", order_keys.as_slice()),
        ("sources.csv", "bySource", ["source"].as_slice()),
        ("products.csv", "byProduct", ["sku", "name"].as_slice()),
        ("weeks.csv", "weekly", ["weekStart"].as_slice()),
    ] {
        let mut keys = base.to_vec();
        if group != "orders" {
            keys.extend_from_slice(METRIC_KEYS);
        }
        files.push(json!({"path":path,"content":csv_table(&metadata,rows(report,group)?,&keys)}));
    }
    files.push(json!({"path":"dataset.json","content":serialized(dataset,true)?}));
    files.push(json!({"path":"input.json","content":input_json}));
    let metadata_text = metadata
        .iter()
        .map(|(k, v)| format!("{k}: {v}\n"))
        .collect::<String>();
    let limitations = rows(report, "limitations")?
        .iter()
        .map(|v| format!("- {}\n", v.as_str().unwrap_or("")))
        .collect::<String>();
    files.push(json!({"path":"methodology.txt","content":format!(
        "M-GEO 本地归因审计方法\n{metadata_text}\n报告与覆盖区间均为 [start,end)，时间规范化为 UTC 整秒。refundAsOf 为 exclusive coverageEnd。\n最后触点：同一非空 sessionId + 同一 SKU；visitTime <= paidTime 且 visitTime >= paidTime - lookbackDays*86400。边界相等可匹配；选最新时间，证据冲突则 ambiguous_touch 且无 visitId；同证据并列选字典序最小 id。未知来源保持未知。\nconvertedSessions 是报告期内所选访问的 (sessionId,sku) 子集；同一组内多次付款只记一个会话。source/week 会话 non-additive；来源切换及跨周访问可同时进入多组。访问数、付款/取消数和金额可跨分组核对，但访问周转化与付款周收入不必同周发生。\n退款按原付款订单队列归属，使用整个已验证数据集直到 coverageEnd 的全部退款；不输出独立退款现金流。最小货币单位为整数，100/单位；paidOrders 是订单行数。\nCSV 为 metadataKey/metadataValue 元数据前导段、一个空行、数据表。所有单元格均加引号，文本公式前缀加单引号。JSON 保留原始规范化值；dataset.json 为不可变审计快照，input.json 为可重新导入的紧凑交换格式。空分母率为 null。\n{limitations}") }));
    files.push(json!({"path":"external-requirements.txt","content":
        "真实业务接入前仍需：经授权的网站域名及事件采集机制；三件真实商品 SKU 与订单行映射；可合法使用的匿名会话关联；实际付款/取消/退款来源；币种、时区、报告截止及业务回溯规则；数据访问与使用授权和平台资质。\n当前导出仅完成本地确定性演练/调用方记录计算；未连接账户、未采集网站、未调用模型、未付费、未对外传输或发布，不证明 AI 推荐带来销量增长。\n"}));
    bound_export(
        files
            .iter()
            .map(|f| str_field(f, "path").len() + str_field(f, "content").len())
            .sum(),
    )?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn visit(id: &str, session: &str, sku: &str, time: &str, source: &str) -> Value {
        json!({"id":id,"sessionId":session,"sku":sku,"occurredAt":time,"source":source,"sourceEvidence":if source=="unknown" {"unknown"} else {"tagged"}})
    }
    fn order(
        id: &str,
        session: Option<&str>,
        sku: &str,
        time: &str,
        status: &str,
        amount: i64,
    ) -> Value {
        json!({"id":id,"sessionId":session,"sku":sku,"occurredAt":time,"status":status,"amountMinor":amount})
    }
    fn dataset(visits: Vec<Value>, orders: Vec<Value>, refunds: Vec<Value>) -> Value {
        json!({"id":"dataset","brandId":"brand","name":"虚构演练","createdAt":"2026-10-01T00:00:00Z","contentHash":"frozen-hash","input":{"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":"虚构演练","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","visits":visits,"orders":orders,"refunds":refunds},"products":[{"id":"p1","sku":"A","name":"原始商品A","revision":1},{"id":"p2","sku":"B","name":"原始商品B","revision":1},{"id":"p3","sku":"C","name":"原始商品C","revision":1}]})
    }
    fn params() -> Value {
        json!({"brandId":"brand","datasetId":"dataset","from":"2026-10-12T00:00:00Z","to":"2026-10-26T00:00:00Z","lookbackDays":7})
    }
    fn row<'a>(r: &'a Value, group: &str, key: &str, value: &str) -> &'a Value {
        r[group]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[key] == value)
            .unwrap()
    }
    fn ledger<'a>(r: &'a Value, id: &str) -> &'a Value {
        row(r, "orders", "orderId", id)
    }

    #[test]
    fn report_last_touch_window_ties_unknown() {
        let d = dataset(
            vec![
                visit("old", "s", "A", "2026-10-12T00:00:00Z", "doubao"),
                visit("latest", "s", "A", "2026-10-18T00:00:00Z", "unknown"),
                visit("a", "tie", "A", "2026-10-18T01:00:00Z", "qianwen"),
                visit("b", "tie", "A", "2026-10-18T01:00:00Z", "doubao"),
                visit("z", "same", "A", "2026-10-18T01:00:00Z", "baidu"),
                visit("c", "same", "A", "2026-10-18T01:00:00Z", "baidu"),
                visit("edge", "edge", "A", "2026-10-12T10:00:00Z", "yuanbao"),
                visit("future", "edge", "A", "2026-10-19T10:00:01Z", "doubao"),
                visit("cross", "cross", "B", "2026-10-18T01:00:00Z", "doubao"),
                visit("stale", "stale", "A", "2026-10-12T09:59:59Z", "doubao"),
            ],
            vec![
                order(
                    "unknown",
                    Some("s"),
                    "A",
                    "2026-10-19T10:00:00Z",
                    "paid",
                    100,
                ),
                order("tie", Some("tie"), "A", "2026-10-19T10:00:00Z", "paid", 100),
                order(
                    "same",
                    Some("same"),
                    "A",
                    "2026-10-19T10:00:00Z",
                    "paid",
                    100,
                ),
                order(
                    "edge",
                    Some("edge"),
                    "A",
                    "2026-10-19T10:00:00Z",
                    "paid",
                    100,
                ),
                order(
                    "cross",
                    Some("cross"),
                    "A",
                    "2026-10-19T10:00:00Z",
                    "paid",
                    100,
                ),
                order(
                    "stale",
                    Some("stale"),
                    "A",
                    "2026-10-19T10:00:00Z",
                    "paid",
                    100,
                ),
                order("missing", None, "A", "2026-10-19T10:00:00Z", "paid", 100),
            ],
            vec![],
        );
        let r = calculate(&d, &params()).unwrap();
        assert_eq!(ledger(&r, "unknown")["reason"], "unknown_source");
        assert_eq!(ledger(&r, "unknown")["visitId"], "latest");
        assert_eq!(ledger(&r, "tie")["reason"], "ambiguous_touch");
        assert!(ledger(&r, "tie")["visitId"].is_null());
        assert_eq!(ledger(&r, "same")["visitId"], "c");
        assert_eq!(ledger(&r, "edge")["visitId"], "edge");
        for id in ["cross", "stale"] {
            assert_eq!(ledger(&r, id)["reason"], "no_matching_visit");
        }
        assert_eq!(ledger(&r, "missing")["reason"], "missing_session");
        assert_eq!(r["summary"]["attributedOrders"], 2);
        assert_eq!(r["summary"]["unknownOrders"], 5);
        assert_eq!(r["summary"]["convertedSessions"], 3);
        let mut evidence_tie = d.clone();
        evidence_tie["input"]["visits"][3]["source"] = json!("qianwen");
        evidence_tie["input"]["visits"][3]["sourceEvidence"] = json!("referrer");
        assert_eq!(
            ledger(&calculate(&evidence_tie, &params()).unwrap(), "tie")["reason"],
            "ambiguous_touch"
        );
    }
    #[test]
    fn report_weekly_visit_cohort_denominator() {
        let d = dataset(
            vec![
                visit("sun", "s", "A", "2026-10-18T23:59:59Z", "doubao"),
                visit("mon", "s", "A", "2026-10-19T02:00:00Z", "yuanbao"),
            ],
            vec![
                order("a", Some("s"), "A", "2026-10-19T00:00:00Z", "paid", 101),
                order("b", Some("s"), "A", "2026-10-19T01:00:00Z", "paid", 202),
                order("c", Some("s"), "A", "2026-10-20T00:00:00Z", "paid", 303),
            ],
            vec![],
        );
        let r = calculate(&d, &params()).unwrap();
        assert_eq!(r["summary"]["visitingSessions"], 1);
        assert_eq!(r["summary"]["convertedSessions"], 1);
        assert_eq!(r["summary"]["sessionConversionRate"], 1.0);
        let sun = row(&r, "weekly", "weekStart", "2026-10-12");
        let mon = row(&r, "weekly", "weekStart", "2026-10-19");
        assert_eq!(sun["convertedSessions"], 1);
        assert_eq!(sun["paidOrders"], 0);
        assert_eq!(sun["sessionConversionRate"], 1.0);
        assert_eq!(mon["convertedSessions"], 1);
        assert_eq!(mon["paidOrders"], 3);
        assert_eq!(mon["grossMinor"], 606);
        for source in ["doubao", "yuanbao"] {
            assert_eq!(
                row(&r, "bySource", "source", source)["convertedSessions"],
                1
            );
        }
        assert_eq!(row(&r, "bySource", "source", "doubao")["paidOrders"], 2);
    }
    #[test]
    fn report_refunds_cancelled_and_currency() {
        let mut d = dataset(
            vec![visit("pre", "s", "A", "2026-10-11T23:00:00Z", "doubao")],
            vec![
                order("paid", Some("s"), "A", "2026-10-12T00:00:00Z", "paid", 1001),
                order("full", None, "B", "2026-10-20T00:00:00Z", "paid", 999),
                order(
                    "cancel",
                    Some("s"),
                    "C",
                    "2026-10-20T00:00:00Z",
                    "cancelled",
                    800,
                ),
                order("end", None, "A", "2026-10-26T00:00:00Z", "paid", 700),
            ],
            vec![
                json!({"id":"r1","orderId":"paid","occurredAt":"2026-10-30T00:00:00Z","amountMinor":501}),
                json!({"id":"r2","orderId":"full","occurredAt":"2026-10-20T00:00:00Z","amountMinor":999}),
            ],
        );
        d["input"]["currency"] = json!("EUR");
        let r = calculate(&d, &params()).unwrap();
        assert_eq!(r["currency"], "EUR");
        assert_eq!(r["period"]["refundAsOf"], "2026-11-01T00:00:00Z");
        let s = &r["summary"];
        for (key, value) in [
            ("paidOrders", 2),
            ("cancelledOrders", 1),
            ("grossMinor", 2000),
            ("refundMinor", 1500),
            ("netMinor", 500),
            ("attributedNetMinor", 500),
            ("unknownNetMinor", 0),
            ("convertedSessions", 0),
        ] {
            assert_eq!(s[key], value, "{key}");
        }
        assert!(s["sessionConversionRate"].is_null());
        assert_eq!(s["unknownOrderRate"], 0.5);
        assert_eq!(ledger(&r, "paid")["visitInPeriod"], false);
        assert_eq!(ledger(&r, "full")["netMinor"], 0);
        let cancelled = ledger(&r, "cancel");
        assert_eq!(cancelled["amountMinor"], 800);
        assert_eq!(cancelled["netMinor"], 0);
        assert_eq!(cancelled["refundMinor"], 0);
        assert_eq!(cancelled["source"], "unknown");
        let mut p = params();
        p["from"] = json!("2026-10-01T00:00:00Z");
        let r = calculate(&d, &p).unwrap();
        assert_eq!(r["summary"]["lookbackCoverageIncomplete"], false);
        d["input"]["orders"].as_array_mut().unwrap().push(order(
            "under",
            None,
            "A",
            "2026-10-02T00:00:00Z",
            "paid",
            1,
        ));
        let r = calculate(&d, &p).unwrap();
        assert_eq!(r["summary"]["lookbackCoverageIncomplete"], true);
        assert_eq!(
            row(&r, "bySource", "source", "unknown")["lookbackCoverageIncomplete"],
            true
        );
    }
    #[test]
    fn report_ranges_group_reconciliation() {
        let d = dataset(
            vec![
                visit("v", "s", "A", "2026-10-12T00:00:00Z", "doubao"),
                visit("only", "other", "B", "2026-10-13T00:00:00Z", "other"),
            ],
            vec![
                order("a", Some("s"), "A", "2026-10-12T00:00:00Z", "paid", 123),
                order("c", None, "C", "2026-10-20T00:00:00Z", "cancelled", 10),
            ],
            vec![],
        );
        let r = calculate(&d, &params()).unwrap();
        for group in ["bySource", "byProduct", "weekly"] {
            for key in [
                "visits",
                "paidOrders",
                "cancelledOrders",
                "attributedOrders",
                "unknownOrders",
                "grossMinor",
                "refundMinor",
                "netMinor",
                "attributedNetMinor",
                "unknownNetMinor",
            ] {
                let sum: i64 = r[group]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x[key].as_i64().unwrap())
                    .sum();
                assert_eq!(sum, r["summary"][key].as_i64().unwrap(), "{group} {key}");
            }
            for v in r[group].as_array().unwrap() {
                assert!(
                    v["convertedSessions"].as_u64().unwrap()
                        <= v["visitingSessions"].as_u64().unwrap()
                );
            }
        }
        assert_eq!(row(&r, "byProduct", "sku", "B")["paidOrders"], 0);
        assert_eq!(row(&r, "byProduct", "sku", "C")["cancelledOrders"], 1);
        assert!(row(&r, "bySource", "source", "other")["unknownOrderRate"].is_null());
        for (key, value) in [
            ("from", json!("2026-09-30T00:00:00Z")),
            ("to", json!("2026-11-02T00:00:00Z")),
            ("lookbackDays", json!(0)),
            ("lookbackDays", json!(31)),
            ("lookbackDays", json!(1.5)),
            ("from", json!("2026-10-12T00:00:00.1Z")),
            ("to", json!("2026-10-12T00:00:00Z")),
        ] {
            let mut p = params();
            p[key] = value;
            assert!(calculate(&d, &p).is_err(), "{key}");
        }
    }
    #[test]
    fn report_csv_json_privacy_bounds() {
        let mut d = dataset(
            vec![visit("v", "opaque", "A", "2026-10-12T00:00:00Z", "doubao")],
            vec![order(
                "o",
                Some("opaque"),
                "A",
                "2026-10-12T00:00:00Z",
                "paid",
                100,
            )],
            vec![],
        );
        d["input"]["sourceLabel"] = json!("  =SUM(1,2)\nfictional");
        d["products"][0]["name"] = json!("=1+1,\"old\"\nname");
        d["products"][0]["sku"] = json!("+A");
        d["input"]["visits"][0]["sku"] = json!("+A");
        d["input"]["orders"][0]["sku"] = json!("+A");
        let r = calculate(&d, &params()).unwrap();
        assert_eq!(r, calculate(&d, &params()).unwrap());
        assert_eq!(r["byProduct"][0]["name"], d["products"][0]["name"]);
        let files = r["files"].as_array().unwrap();
        let names: Vec<_> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "summary.json",
                "orders.csv",
                "sources.csv",
                "products.csv",
                "weeks.csv",
                "dataset.json",
                "input.json",
                "methodology.txt",
                "external-requirements.txt"
            ]
        );
        let file = |name: &str| {
            files.iter().find(|f| f["path"] == name).unwrap()["content"]
                .as_str()
                .unwrap()
        };
        let summary: Value = serde_json::from_str(file("summary.json")).unwrap();
        let mut expected = r.clone();
        expected.as_object_mut().unwrap().remove("files");
        assert_eq!(summary, expected);
        assert_eq!(
            serde_json::from_str::<Value>(file("dataset.json")).unwrap(),
            d
        );
        assert_eq!(
            serde_json::from_str::<Value>(file("input.json")).unwrap(),
            d["input"]
        );
        assert!(file("input.json").len() <= 512 * 1024);
        assert!(!file("input.json").contains("\n  "));
        for name in ["orders.csv", "sources.csv", "products.csv", "weeks.csv"] {
            let csv = file(name);
            assert!(csv.contains("simulated"));
            assert!(csv.contains("2026-11-01T00:00:00Z"));
            assert!(csv.contains("'  =SUM(1,2)"));
            assert!(!csv.contains("sessionId"));
        }
        assert!(file("products.csv").contains("\"'+A\""));
        assert!(file("products.csv").contains("'=1+1,"));
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(file("products.csv").as_bytes());
        let records: Vec<_> = reader
            .records()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let parsed = records
            .iter()
            .find(|record| record.get(0) == Some("'+A"))
            .unwrap();
        assert_eq!(parsed.get(1), Some("'=1+1,\"old\"\nname"));
        assert!(file("methodology.txt").contains("non-additive"));
    }
    #[test]
    fn report_prepare_revalidates_frozen_snapshot_and_request() {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构归因品牌"})).unwrap();
        let product=handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"原始商品A","sku":"A","category":"","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[]})).unwrap();
        let d = dataset(
            vec![visit("v", "opaque", "A", "2026-10-12T00:00:00Z", "doubao")],
            vec![order(
                "o",
                Some("opaque"),
                "A",
                "2026-10-12T00:00:00Z",
                "paid",
                100,
            )],
            vec![],
        );
        let mut import =
            json!({"brandId":b["id"],"name":"虚构演练","content":d["input"].to_string()});
        let preview = attribution::preview(&c, b["id"].as_str().unwrap(), &import).unwrap();
        import["expectedHash"] = preview["contentHash"].clone();
        let saved = attribution::apply(&c, b["id"].as_str().unwrap(), &import).unwrap();
        let mut p = params();
        p["brandId"] = b["id"].clone();
        p["datasetId"] = saved["datasetId"].clone();
        let before = c.total_changes();
        let original = prepare(&c, b["id"].as_str().unwrap(), &p).unwrap();
        assert_eq!(c.total_changes(), before);
        let exchange = original["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "input.json")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        let reimport = json!({"brandId":b["id"],"name":"可重入演练","content":exchange});
        let repreview = attribution::preview(&c, b["id"].as_str().unwrap(), &reimport).unwrap();
        assert_eq!(repreview["contentHash"], preview["contentHash"]);
        assert_eq!(repreview["alreadyImported"], true);
        let mut offset = p.clone();
        offset["from"] = json!("2026-10-12T08:00:00+08:00");
        offset["to"] = json!("2026-10-26T08:00:00+08:00");
        assert_eq!(
            original,
            prepare(&c, b["id"].as_str().unwrap(), &offset).unwrap()
        );
        for secret in ["RAW_SECRET", "NOTE_SECRET"] {
            assert!(!original.to_string().contains(secret));
        }
        let mut update = product.clone();
        update["name"] = json!("MUTABLE_LABEL_SECRET");
        update["sku"] = json!("CHANGED");
        update["expectedRevision"] = product["revision"].clone();
        handle(&mut c, "product.save", update).unwrap();
        assert_eq!(
            original,
            prepare(&c, b["id"].as_str().unwrap(), &p).unwrap()
        );
        let mut wrong = p.clone();
        wrong["brandId"] = json!("wrong-brand");
        assert!(prepare(&c, b["id"].as_str().unwrap(), &wrong).is_err());
        let mut wrong = p.clone();
        wrong["extra"] = json!("not-allowed");
        assert!(prepare(&c, b["id"].as_str().unwrap(), &wrong).is_err());
        assert!(prepare(&c, "another-brand", &p).is_err());
        c.execute(
            "UPDATE m_attribution_datasets SET content_hash='broken' WHERE id=?1",
            [saved["datasetId"].as_str().unwrap()],
        )
        .unwrap();
        assert!(prepare(&c, b["id"].as_str().unwrap(), &p).is_err());
    }
    #[test]
    fn report_export_boundaries_and_empty_metadata() {
        assert!(bound_export(16 * 1024 * 1024).is_ok());
        assert!(bound_export(16 * 1024 * 1024 + 1).is_err());
        let mut d = dataset(
            vec![visit("v", "opaque", "A", "2026-10-11T00:00:00Z", "doubao")],
            vec![],
            vec![],
        );
        let r = calculate(&d, &params()).unwrap();
        assert_eq!(r["summary"]["visits"], 0);
        assert!(r["summary"]["sessionConversionRate"].is_null());
        for f in r["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["path"].as_str().unwrap().ends_with(".csv"))
        {
            let content = f["content"].as_str().unwrap();
            assert!(content.contains("2026-11-01T00:00:00Z"));
            assert!(content.contains("simulated"));
        }
        d["input"]["sourceLabel"] = json!("x".repeat(512 * 1024));
        assert!(calculate(&d, &params()).is_err());
        let mut p = params();
        p["from"] = json!("2026-10-12T00:00:00.000Z");
        assert!(calculate(&dataset(vec![], vec![], vec![]), &p).is_err());
    }

    #[test]
    fn report_large_integer_money_and_deterministic_sort() {
        let mut orders: Vec<_> = (0..999)
            .rev()
            .map(|i| {
                order(
                    &format!("o{i:04}"),
                    Some("s"),
                    "A",
                    "2026-10-12T00:00:00Z",
                    "paid",
                    1_000_000_000,
                )
            })
            .collect();
        let d = dataset(
            vec![visit("v", "s", "A", "2026-10-12T00:00:00Z", "doubao")],
            orders.clone(),
            vec![],
        );
        let r = calculate(&d, &params()).unwrap();
        assert_eq!(r["summary"]["grossMinor"], 999_000_000_000_i64);
        assert_eq!(r["summary"]["netMinor"], 999_000_000_000_i64);
        assert_eq!(r["summary"]["paidOrders"], 999);
        assert_eq!(r["summary"]["convertedSessions"], 1);
        assert_eq!(r["orders"][0]["orderId"], "o0000");
        assert_eq!(r["orders"][998]["orderId"], "o0998");
        let keys: BTreeSet<_> = r["orders"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "orderId",
                "sku",
                "occurredAt",
                "status",
                "amountMinor",
                "refundMinor",
                "netMinor",
                "source",
                "sourceEvidence",
                "visitId",
                "reason",
                "visitInPeriod"
            ]
            .into_iter()
            .collect()
        );
        orders.reverse();
        let mut reordered = d.clone();
        reordered["input"]["orders"] = json!(orders);
        let reordered_report = calculate(&reordered, &params()).unwrap();
        for key in ["summary", "bySource", "byProduct", "weekly", "orders"] {
            assert_eq!(r[key], reordered_report[key]);
        }
    }
}
