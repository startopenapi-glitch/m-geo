use super::*;

fn csv(s: &str) -> String {
    let safe = if s.trim_start().starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{}", s)
    } else {
        s.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let ids = p["productIds"]
        .as_array()
        .filter(|a| (1..=3).contains(&a.len()))
        .ok_or("请选择1至3个商品")?;
    let br = brand(c, b)?;
    let mut seen = HashSet::new();
    let mut products = vec![];
    let mut revisions = vec![];
    let mut facts_csv = String::from("商品,事实,公开值,来源,有效期,状态\n");
    let mut missing_csv = String::from("商品,待补材料\n");
    for value in ids {
        let pid = value.as_str().ok_or("商品标识格式错误")?;
        Uuid::parse_str(pid).map_err(|_| "商品标识格式错误")?;
        if !seen.insert(pid) {
            return Err("请勿重复选择商品".into());
        }
        let prod = load(c, "m_products", b, pid)?;
        revisions.push(json!([pid, prod["revision"]]));
        let mut gaps = vec!["网站与渠道授权：外部待确认".to_owned()];
        if prod["confirmedAt"].is_null() {
            gaps.push("当前商品版本尚未确认".into());
        }
        if prod["officialUrl"].as_str().unwrap_or("").is_empty() {
            gaps.push("缺少官网链接".into());
        }
        let facts = prod["facts"].as_array().ok_or("商品事实格式错误")?;
        let mut public = vec![];
        for f in facts {
            if f["visibility"] != "public" {
                continue;
            }
            let expired = f["validUntil"]
                .as_str()
                .is_some_and(|s| s < Utc::now().date_naive().to_string().as_str());
            let missing = f["source"].as_str().unwrap_or("").trim().is_empty();
            let mut statuses = vec![];
            if expired {
                statuses.push("已过期，不可用于确认");
            }
            if missing {
                statuses.push("缺少来源");
            }
            if statuses.is_empty() {
                statuses.push("公开资料；使用前须确认当前版本");
            }
            let status = statuses.join("；");
            if let Err(reason) = commerce::freshness(f) {
                gaps.push(format!("公开事实时效待核对：{}", reason));
            }
            if expired || missing {
                gaps.push(format!(
                    "{}：{}",
                    f["label"].as_str().unwrap_or("事实"),
                    status
                ));
            }
            let safe = json!({"id":f["id"],"label":f["label"],"value":f["value"],"source":f["source"],"validUntil":f["validUntil"],"status":status});
            facts_csv.push_str(
                &[
                    prod["name"].as_str().unwrap_or(""),
                    f["label"].as_str().unwrap_or(""),
                    f["value"].as_str().unwrap_or(""),
                    f["source"].as_str().unwrap_or(""),
                    f["validUntil"].as_str().unwrap_or("未注明"),
                    status.as_str(),
                ]
                .map(csv)
                .join(","),
            );
            facts_csv.push('\n');
            public.push(safe);
        }
        if public.is_empty() {
            gaps.push("缺少公开事实（私有事实不导出）".into());
        }
        let today = Utc::now().date_naive();
        for (field, label) in commerce::FIELDS {
            let usable = facts.iter().any(|f| {
                f["id"] == format!("commerce:{field}")
                    && f["visibility"] == "public"
                    && f["value"]
                        .as_str()
                        .is_some_and(|v| !v.trim().is_empty() && v != "Unknown")
                    && f["source"].as_str().is_some_and(|s| !s.trim().is_empty())
                    && commerce::value(f).is_ok()
                    && commerce::freshness(f).is_ok()
                    && (f["validUntil"].is_null()
                        || f["validUntil"]
                            .as_str()
                            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                            .is_some_and(|expiry| expiry >= today))
            });
            if !usable {
                gaps.push(format!("购买资料待补：{label}"));
            }
        }
        let mut stmt = c
            .prepare("SELECT data FROM m_drafts WHERE brand_id=?1 AND product_id=?2")
            .map_err(db_error)?;
        let drafts = stmt
            .query_map(params![b, pid], |r| r.get::<_, String>(0))
            .map_err(db_error)?;
        let mut reviewed = 0;
        for d in drafts {
            let d = parse_data(d.map_err(db_error)?)?;
            if !d["reviewedAt"].is_null() && d["sourceRevision"] == prod["revision"] {
                reviewed += 1;
            }
        }
        if reviewed == 0 {
            gaps.push("缺少当前商品版本已审核稿件".into());
        }
        for gap in &gaps {
            missing_csv.push_str(&format!(
                "{},{}\n",
                csv(prod["name"].as_str().unwrap_or("")),
                csv(gap)
            ));
        }
        products.push(json!({"id":pid,"name":prod["name"],"sku":prod["sku"],"category":prod["category"],"officialUrl":prod["officialUrl"],"revision":prod["revision"],"confirmed":!prod["confirmedAt"].is_null(),"facts":public,"gaps":gaps,"reviewedDraftCount":reviewed}));
    }
    let templates = [
        ("怎么选适合自己的{}？", "selection"),
        ("购买{}前要核对哪些参数？", "selection"),
        ("不同{}应该如何比较？", "comparison"),
        ("{}日常使用有哪些注意事项？", "usage"),
        ("{}售后需要确认哪些条款？", "after_sales"),
        ("第一次购买{}要考虑什么？", "selection"),
        ("同预算的{}如何取舍？", "comparison"),
        ("{}适合哪些使用场景？", "usage"),
        ("{}保养时应该注意什么？", "usage"),
        ("{}退换货前需要准备什么？", "after_sales"),
    ];
    let mut candidates = vec![];
    let mut questions_csv = String::from("商品标识,候选问题,意图,来源,确认状态\n");
    for (i, (template, intent)) in templates.iter().enumerate() {
        let prod = &products[i % products.len()];
        let category = prod["category"].as_str().unwrap_or("");
        let blocked = category.is_empty()
            || [
                br["name"].as_str().unwrap_or(""),
                prod["name"].as_str().unwrap_or(""),
                prod["sku"].as_str().unwrap_or(""),
            ]
            .iter()
            .any(|s| !s.is_empty() && normalize(category).contains(&normalize(s)));
        let noun = if blocked { "这类商品" } else { category };
        let question = template.replace("{}", noun);
        candidates.push(json!({"productId":prod["id"],"text":question,"intent":intent}));
        questions_csv.push_str(
            &[
                prod["id"].as_str().unwrap_or(""),
                &question,
                intent,
                "M-GEO 本地试点模板；非真实搜索数据",
                "未确认",
            ]
            .map(csv)
            .join(","),
        );
        questions_csv.push('\n');
    }
    let token = fingerprint(&json!({"brandId":b,"revisions":revisions,"candidates":candidates}));
    let markdown = format!("# 本地试点准备 · 未发布\n\n仅整理本地公开资料与模板问题；不代表真实查询需求、公开可达、收录或推荐。网站与渠道授权始终为外部待确认。\n\n{}",serde_json::to_string_pretty(&products).map_err(|_| "报告生成失败")?);
    let files = json!([{"path":"pilot.md","content":markdown},{"path":"public-facts.csv","content":facts_csv},{"path":"missing-materials.csv","content":missing_csv},{"path":"question-candidates.csv","content":questions_csv}]);
    if files
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["content"].as_str().unwrap().len())
        .sum::<usize>()
        > 16 * 1024 * 1024
    {
        return Err("导出内容超过16MiB".into());
    }
    Ok(
        json!({"products":products,"candidates":candidates,"token":token,"files":files,"fileName":"m-geo-pilot.zip"}),
    )
}
pub(super) fn create_questions(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    if p["confirm"] != true {
        return Err("请明确确认保存候选问题".into());
    }
    let prepared = prepare(c, b, p)?;
    if p["token"] != prepared["token"] {
        return Err("商品资料或候选问题已变化，请重新准备".into());
    }
    let mut created = 0;
    let mut skipped = 0;
    let mut questions = vec![];
    for candidate in prepared["candidates"].as_array().unwrap() {
        let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM m_questions WHERE brand_id=?1 AND product_id=?2 AND normalized=?3)",params![b,candidate["productId"].as_str(),normalize(candidate["text"].as_str().unwrap())],|r|r.get(0)).map_err(db_error)?;
        let mut q = candidate.clone();
        q["origin"] = json!("suggestion");
        q["source"] = json!("M-GEO 本地试点模板；非真实搜索数据");
        q["confirmed"] = json!(false);
        q["branded"] = json!(false);
        questions.push(save_question(c, b, &q)?);
        if exists {
            skipped += 1;
        } else {
            created += 1;
        }
    }
    Ok(json!({"created":created,"skipped":skipped,"questions":questions}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Connection, Value, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构试点品牌"})).unwrap();
        let p=handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构商品","sku":"FICTIONAL-01","category":"虚构试点品牌专属","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"f","label":"公开参数","value":"=10","source":"","visibility":"public","validUntil":"2000-01-01"},{"id":"private","label":"私有","value":"PRIVATE_SECRET","source":"内部","visibility":"private","validUntil":null}]})).unwrap();
        (c, b, p)
    }
    #[test]
    fn pilot_privacy_gaps_balance_and_explicit_save() {
        let (mut c, b, p) = setup();
        let payload = json!({"brandId":b["id"],"productIds":[p["id"]]});
        let prepared = handle(&mut c, "pilot.prepare", payload.clone()).unwrap();
        assert_eq!(prepared["candidates"].as_array().unwrap().len(), 10);
        for q in prepared["candidates"].as_array().unwrap() {
            assert!(!mentions_brand(q["text"].as_str().unwrap(), "虚构试点品牌"));
        }
        for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
            assert!(!prepared.to_string().contains(secret));
        }
        let gaps = prepared["products"][0]["gaps"].as_array().unwrap();
        assert!(gaps
            .iter()
            .any(|gap| gap.as_str().unwrap().contains("已过期")));
        assert!(gaps
            .iter()
            .any(|gap| gap.as_str().unwrap().contains("缺少来源")));
        let status = prepared["products"][0]["facts"][0]["status"]
            .as_str()
            .unwrap();
        assert!(status.contains("已过期") && status.contains("缺少来源"));
        for index in [1, 2] {
            let csv = prepared["files"][index]["content"].as_str().unwrap();
            assert!(csv.contains("已过期") && csv.contains("缺少来源"));
        }
        assert!(prepared.to_string().contains("已过期"));
        assert!(prepared.to_string().contains("外部待确认"));
        assert!(prepared["files"][1]["content"]
            .as_str()
            .unwrap()
            .contains("'=10"));
        assert!(handle(&mut c, "pilot.createQuestions", payload.clone()).is_err());
        let mut save = payload.clone();
        save["token"] = prepared["token"].clone();
        save["confirm"] = json!(true);
        let first = handle(&mut c, "pilot.createQuestions", save.clone()).unwrap();
        assert_eq!(first["created"], 10);
        assert!(first["questions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|q| q["confirmed"] == false));
        let mut confirmed = first["questions"][0].clone();
        confirmed["confirmed"] = json!(true);
        update(&c, "m_questions", &confirmed).unwrap();
        let again = handle(&mut c, "pilot.createQuestions", save.clone()).unwrap();
        assert_eq!(again["skipped"], 10);
        assert_eq!(again["questions"][0]["confirmed"], true);
        let mut changed = p.clone();
        changed["revision"] = json!(2);
        update(&c, "m_products", &changed).unwrap();
        assert!(handle(&mut c, "pilot.createQuestions", save).is_err());
    }
    #[test]
    fn pilot_rejects_duplicate_foreign_and_balances_selected_order() {
        let (mut c, b, p) = setup();
        assert!(handle(
            &mut c,
            "pilot.prepare",
            json!({"brandId":b["id"],"productIds":[p["id"],p["id"]]})
        )
        .is_err());
        let other = handle(&mut c, "brand.create", json!({"name":"另一虚构品牌"})).unwrap();
        assert!(handle(
            &mut c,
            "pilot.prepare",
            json!({"brandId":other["id"],"productIds":[p["id"]]})
        )
        .is_err());
        let mut private_only = p.clone();
        private_only["facts"] = json!([{"id":"secret","label":"私有事实","value":"PRIVATE_SECRET","source":"内部","visibility":"private","validUntil":null}]);
        update(&c, "m_products", &private_only).unwrap();
        let missing = handle(
            &mut c,
            "pilot.prepare",
            json!({"brandId":b["id"],"productIds":[p["id"]]}),
        )
        .unwrap();
        assert!(missing.to_string().contains("缺少公开事实"));
        assert!(!missing.to_string().contains("PRIVATE_SECRET"));
        let mut ids = vec![p["id"].clone()];
        for i in 2..=3 {
            let mut x = p.clone();
            x["id"] = Value::Null;
            x["sku"] = json!(format!("FICTIONAL-{}", i));
            x["name"] = json!(format!("虚构商品{}", i));
            let x = handle(&mut c, "product.save", x).unwrap();
            ids.push(x["id"].clone());
        }
        let result = handle(
            &mut c,
            "pilot.prepare",
            json!({"brandId":b["id"],"productIds":ids}),
        )
        .unwrap();
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(
                result["candidates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|q| q["productId"] == *id)
                    .count(),
                if i == 0 { 4 } else { 3 }
            );
        }
    }

    fn buying_facts() -> Vec<Value> {
        [
            ("price", "价格", "59.90"),
            ("currency", "币种", "CNY"),
            ("availability", "库存状态", "InStock"),
            ("audience", "适用人群", "成年通勤者"),
            ("scenario", "使用场景", "办公桌"),
            ("limits", "使用限制", "不盛放沸水"),
            ("purchase", "购买链接", "https://example.com/buy"),
            ("afterSales", "售后说明", "虚构售后条款"),
            ("image", "商品图片", "https://example.com/image.png"),
        ]
        .iter()
        .map(|(key, label, value)| json!({"id":format!("commerce:{key}"),"label":label,"value":value,"source":"虚构官方资料","visibility":"public","validUntil":"2099-01-01"}))
        .collect()
    }
    fn buying_report(facts: Vec<Value>, confirm_and_review: bool) -> Value {
        let (mut c, b, mut p) = setup();
        p["facts"] = json!(facts);
        p["officialUrl"] = json!("https://example.com/product");
        update(&c, "m_products", &p).unwrap();
        if confirm_and_review {
            handle(
                &mut c,
                "product.confirm",
                json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
            )
            .unwrap();
            let d = handle(
                &mut c,
                "draft.generateLocal",
                json!({"brandId":b["id"],"productId":p["id"],"kind":"description"}),
            )
            .unwrap();
            handle(
                &mut c,
                "draft.review",
                json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
            )
            .unwrap();
        }
        handle(
            &mut c,
            "pilot.prepare",
            json!({"brandId":b["id"],"productIds":[p["id"]]}),
        )
        .unwrap()
    }
    fn assert_buying_gap(report: &Value, label: &str) {
        let gap = format!("购买资料待补：{label}");
        assert!(
            report["products"][0]["gaps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == &gap),
            "missing {label}: {report}"
        );
        let csv = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "missing-materials.csv")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(csv.contains(&gap), "missing exported {label}");
    }
    #[test]
    fn pilot_enumerates_absent_buying_fields_even_for_confirmed_reviewed_product() {
        let report = buying_report(
            vec![
                json!({"id":"material","label":"材质","value":"棉","source":"虚构官方资料","visibility":"public","validUntil":null}),
            ],
            true,
        );
        assert_eq!(report["products"][0]["confirmed"], true);
        assert_eq!(report["products"][0]["reviewedDraftCount"], 1);
        for label in [
            "价格",
            "币种",
            "库存状态",
            "适用人群",
            "使用场景",
            "使用限制",
            "购买链接",
            "售后说明",
            "商品图片",
        ] {
            assert_buying_gap(&report, label);
        }
    }
    #[test]
    fn pilot_private_buying_fields_do_not_fill_public_material_gaps() {
        let mut facts = buying_facts();
        for fact in &mut facts {
            fact["visibility"] = json!("private");
            fact["source"] = json!("PRIVATE_BUYING_SOURCE");
        }
        let report = buying_report(facts, false);
        for label in [
            "价格",
            "币种",
            "库存状态",
            "适用人群",
            "使用场景",
            "使用限制",
            "购买链接",
            "售后说明",
            "商品图片",
        ] {
            assert_buying_gap(&report, label);
        }
        for secret in [
            "PRIVATE_BUYING_SOURCE",
            "59.90",
            "https://example.com/buy",
            "虚构售后条款",
        ] {
            assert!(!report.to_string().contains(secret));
        }
    }
    #[test]
    fn pilot_unusable_buying_facts_stay_missing() {
        let mut facts = buying_facts();
        facts[0]["validUntil"] = Value::Null;
        facts[1]["source"] = json!("");
        facts[2]["value"] = json!("Unknown");
        facts[3]["validUntil"] = json!("2000-01-01");
        facts[4]["value"] = json!("   ");
        let report = buying_report(facts, false);
        for label in ["价格", "币种", "库存状态", "适用人群", "使用场景"] {
            assert_buying_gap(&report, label);
        }
        for label in ["使用限制", "购买链接", "售后说明", "商品图片"] {
            assert!(!report["products"][0]["gaps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == &format!("购买资料待补：{label}")));
        }
    }
    #[test]
    fn pilot_complete_usable_buying_facts_leave_only_external_authorization() {
        let report = buying_report(buying_facts(), true);
        assert_eq!(
            report["products"][0]["gaps"],
            json!(["网站与渠道授权：外部待确认"])
        );
    }
}
