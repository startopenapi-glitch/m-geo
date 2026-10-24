//! Atomic, revision-bound updates of existing buying facts.
use super::*;
const HEADER: [&str; 9] = [
    "SKU",
    "资料版本",
    "价格",
    "币种",
    "库存状态",
    "购买链接",
    "图片链接",
    "依据",
    "有效期",
];
const KEYS: [&str; 5] = ["price", "currency", "availability", "purchase", "image"];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_rejects_bad_header_and_size() {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        assert!(preview(&c, "b", &json!({"content":"SKU\na"})).is_err());
        assert!(preview(&c, "b", &json!({"content":"x".repeat(2*1024*1024+1)})).is_err());
    }
}
fn prepare(c: &Connection, b: &str, p: &Value) -> Result<(Value, Vec<Value>)> {
    let content = p["content"].as_str().ok_or("请提供 CSV 内容")?;
    if content.len() > 2 * 1024 * 1024 || content.contains('\0') {
        return Err("CSV 最多 2 MB，不能含空字符".into());
    }
    let mut reader = csv::ReaderBuilder::new().from_reader(content.as_bytes());
    if reader
        .headers()
        .map_err(|_| "CSV 表头无效")?
        .iter()
        .ne(HEADER)
    {
        return Err("CSV 表头必须与批量更新模板完全一致".into());
    }
    let records = reader
        .records()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "CSV 行格式无效")?;
    if records.is_empty() || records.len() > 200 {
        return Err("CSV 必须包含 1 至 200 行".into());
    }
    let mut seen = HashSet::new();
    let mut rows = vec![];
    let mut updates = vec![];
    let mut versions = vec![];
    for (i, record) in records.iter().enumerate() {
        let cells: Vec<_> = record.iter().map(str::trim).collect();
        let sku = cells[0];
        let mut errors: Vec<String> = vec![];
        let mut changes = vec![];
        if !cells[8].is_empty()
            && (cells[8].len() != 10 || NaiveDate::parse_from_str(cells[8], "%Y-%m-%d").is_err())
        {
            errors.push("事实有效期必须为 YYYY-MM-DD".into());
        }
        if !seen.insert(sku.to_owned()) {
            errors.push("SKU 重复".into());
        }
        let stored: Option<String> = c
            .query_row(
                "SELECT data FROM m_products WHERE brand_id=?1 AND sku=?2",
                params![b, sku],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let mut name = String::new();
        if let Some(stored) = stored {
            let old: Value = serde_json::from_str(&stored).map_err(|_| "商品资料无效")?;
            name = old["name"].as_str().unwrap_or("").to_owned();
            versions.push(json!([sku, old["id"], old["revision"]]));
            if cells[1].parse::<i64>().ok().filter(|v| *v > 0) != old["revision"].as_i64() {
                errors.push("资料版本无效或已变化，请重新预览".into());
            }
            let mut next = old.clone();
            let mut fs = old["facts"].as_array().ok_or("商品事实无效")?.clone();
            for (column, key) in KEYS.iter().enumerate() {
                let value = cells[column + 2];
                if value.is_empty() {
                    continue;
                }
                let fid = format!("commerce:{key}");
                let pos = fs
                    .iter()
                    .position(|f| f["id"].as_str().is_some_and(|v| v.trim() == fid));
                let existing = pos.map(|n| fs[n].clone());
                let value_changed = !existing
                    .as_ref()
                    .is_some_and(|f| f["value"].as_str() == Some(value));
                let label = commerce::FIELDS.iter().find(|(k, _)| k == key).unwrap().1;
                let expiry = if cells[8].is_empty() {
                    existing
                        .as_ref()
                        .map(|f| f["validUntil"].clone())
                        .unwrap_or(Value::Null)
                } else {
                    json!(cells[8])
                };
                let source = if cells[7].is_empty() {
                    existing
                        .as_ref()
                        .map(|f| f["source"].clone())
                        .unwrap_or(json!(""))
                } else {
                    json!(cells[7])
                };
                let metadata_changed = existing
                    .as_ref()
                    .is_none_or(|f| f["source"] != source || f["validUntil"] != expiry);
                if !value_changed && !metadata_changed {
                    continue;
                }
                let f = json!({"id":fid,"label":label,"value":value,"source":source,"visibility":existing.as_ref().map(|f| f["visibility"].clone()).unwrap_or(json!("public")),"validUntil":expiry});
                if value_changed && ["price", "availability"].contains(key) && cells[8].is_empty() {
                    errors.push(format!("{label}修改需要有效期"));
                }
                if value_changed && cells[7].is_empty() {
                    errors.push(format!("{label}修改需要依据"));
                }
                if let Err(e) = commerce::freshness(&f) {
                    errors.push(e);
                }
                for (field, suffix) in [("value", ""), ("source", "依据"), ("validUntil", "有效期")]
                {
                    let before = existing
                        .as_ref()
                        .map(|v| v[field].clone())
                        .unwrap_or(Value::Null);
                    if before != f[field] {
                        changes.push(json!({"field":format!("{label}{suffix}"),"before":before,"after":f[field]}));
                    }
                }
                if let Some(n) = pos {
                    fs[n] = f;
                } else {
                    fs.push(f);
                }
            }
            next["facts"] = json!(fs);
            next["expectedRevision"] = old["revision"].clone();
            if let Err(e) = facts(&next) {
                errors.push(e);
            }
            if errors.is_empty() && !changes.is_empty() {
                updates.push(next);
            }
        } else {
            errors.push("当前品牌没有此 SKU".into());
            versions.push(json!([sku, null]));
        }
        rows.push(json!({"row":i+2,"sku":sku,"name":name,"status":if !errors.is_empty(){"error"}else if changes.is_empty(){"unchanged"}else{"changed"},"changes":changes,"errors":errors}));
    }
    let changed = rows.iter().filter(|r| r["status"] == "changed").count();
    let unchanged = rows.iter().filter(|r| r["status"] == "unchanged").count();
    let token = format!(
        "{:x}",
        Sha256::digest(
            json!([b, content, versions, updates])
                .to_string()
                .as_bytes()
        )
    );
    Ok((
        json!({"canApply":changed>0&&changed+unchanged==rows.len(),"token":token,"changedCount":changed,"unchangedCount":unchanged,"rows":rows}),
        updates,
    ))
}
pub(super) fn preview(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    Ok(prepare(c, b, p)?.0)
}
pub(super) fn apply(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    if !boolean(p, "confirm")? {
        return Err("请明确确认批量更新".into());
    }
    let (preview, updates) = prepare(c, b, p)?;
    if p["token"].as_str().is_none() || p["token"] != preview["token"] {
        return Err("预览已失效，请重新预览".into());
    }
    if preview["canApply"] != true {
        return Err("批量更新含错误或没有变化，请检查预览".into());
    }
    for update in &updates {
        save_product(c, b, update)?;
    }
    Ok(json!({"updated":updates.len(),"unchanged":preview["unchangedCount"]}))
}

#[cfg(test)]
mod behavioral_tests {
    use super::*;
    fn fixture() -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构批量测试"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let p=save_product(&c,&b,&json!({"name":"虚构商品","sku":"A","sourceText":"raw","privateNotes":"secret","facts":[{"id":"private","label":"内部","value":"secret","source":"内部","visibility":"private","validUntil":null},{"id":"public","label":"材质","value":"棉","source":"官方","visibility":"public","validUntil":null}]})).unwrap();
        (c, b, p)
    }
    fn csv(rows: &str) -> Value {
        json!({"content":format!("{}\n{}",HEADER.join(","),rows)})
    }
    #[test]
    fn batch_validation_blocks_all_rows_and_cross_brand() {
        let (c, b, p) = fixture();
        for row in [
            "A,1,-1,CNY,,,,依据,2099-01-01",
            "A,1,10,CNY,,,,依据,bad",
            "A,0,10,CNY,,,,依据,2099-01-01",
            "A,1,10,CNY,,,,,2099-01-01",
            "A,1,10,CNY,,,,依据,",
        ] {
            assert_eq!(
                preview(&c, &b, &csv(row)).unwrap()["canApply"],
                false,
                "{row}"
            );
        }
        let input = csv("A,1,10,CNY,,,,依据,2099-01-01\nUNKNOWN,1,10,CNY,,,,依据,2099-01-01");
        let pre = preview(&c, &b, &input).unwrap();
        let mut apply_input = input;
        apply_input["token"] = pre["token"].clone();
        apply_input["confirm"] = json!(true);
        assert!(apply(&c, &b, &apply_input).is_err());
        assert_eq!(
            load(&c, "m_products", &b, p["id"].as_str().unwrap()).unwrap(),
            p
        );
        assert_eq!(
            preview(&c, "another-brand", &csv("A,1,10,CNY,,,,依据,2099-01-01")).unwrap()["rows"][0]
                ["errors"],
            json!(["当前品牌没有此 SKU"])
        );
        assert_eq!(
            preview(&c, &b, &csv("A,1,,,,,,,\n A ,1,,,,,,,")).unwrap()["canApply"],
            false
        );
    }
    #[test]
    fn batch_preserves_private_and_unchanged_and_rejects_replay() {
        let (mut c, b, p) = fixture();
        let confirmed = handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":p["id"],"expectedRevision":1}),
        )
        .unwrap();
        assert_eq!(
            preview(&c, &b, &csv("A,1,,,,,,,")).unwrap()["unchangedCount"],
            1
        );
        assert_eq!(
            load(&c, "m_products", &b, p["id"].as_str().unwrap()).unwrap(),
            confirmed
        );
        let mut input = csv(" A ,1,10,CNY,,,,依据,2099-01-01");
        input["token"] = preview(&c, &b, &input).unwrap()["token"].clone();
        input["confirm"] = json!(true);
        assert_eq!(apply(&c, &b, &input).unwrap()["updated"], 1);
        let updated = load(&c, "m_products", &b, p["id"].as_str().unwrap()).unwrap();
        assert!(updated["confirmedAt"].is_null());
        assert_eq!(updated["revision"], 2);
        assert_eq!(updated["facts"][0], p["facts"][0]);
        assert_eq!(updated["privateNotes"], "secret");
        assert_eq!(updated["sourceText"], "raw");
        assert!(apply(&c, &b, &input).is_err());
        let reconfirmed = handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":p["id"],"expectedRevision":2}),
        )
        .unwrap();
        assert!(!reconfirmed["confirmedAt"].is_null());
        let mut repeat = csv("A,2,10,CNY,,,,different source,2099-12-31");
        repeat["token"] = preview(&c, &b, &repeat).unwrap()["token"].clone();
        assert_eq!(preview(&c, &b, &repeat).unwrap()["changedCount"], 1);
        repeat["confirm"] = json!(true);
        apply(&c, &b, &repeat).unwrap();
        let renewed = load(&c, "m_products", &b, p["id"].as_str().unwrap()).unwrap();
        assert_eq!(renewed["revision"], 3);
        assert!(renewed["confirmedAt"].is_null());
        assert_eq!(renewed["facts"][2]["validUntil"], "2099-12-31");
        repeat["content"]=json!("SKU,资料版本,价格,币种,库存状态,购买链接,图片链接,依据,有效期\nA,3,10,CNY,,,,different source,2099-12-31");
        assert_eq!(preview(&c, &b, &repeat).unwrap()["unchangedCount"], 1);
        let mut source_only = csv("A,3,10,,,,,new source,");
        let pre = preview(&c, &b, &source_only).unwrap();
        assert_eq!(pre["changedCount"], 1);
        source_only["token"] = pre["token"].clone();
        source_only["confirm"] = json!(true);
        apply(&c, &b, &source_only).unwrap();
        let sourced = load(&c, "m_products", &b, p["id"].as_str().unwrap()).unwrap();
        assert_eq!(sourced["facts"][2]["validUntil"], "2099-12-31");
        assert_eq!(sourced["facts"][2]["source"], "new source");
    }
    #[test]
    fn batch_stale_preview_after_concurrent_save() {
        let (c, b, mut p) = fixture();
        let mut input = csv("A,1,10,CNY,,,,依据,2099-01-01");
        input["token"] = preview(&c, &b, &input).unwrap()["token"].clone();
        input["confirm"] = json!(true);
        p["expectedRevision"] = json!(1);
        save_product(&c, &b, &p).unwrap();
        assert!(apply(&c, &b, &input).unwrap_err().contains("预览已失效"));
    }
    #[test]
    fn batch_update_makes_reviewed_draft_stale() {
        let (mut c, b, p) = fixture();
        handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":p["id"],"expectedRevision":1}),
        )
        .unwrap();
        let d = handle(
            &mut c,
            "draft.generateLocal",
            json!({"brandId":b,"productId":p["id"],"kind":"description"}),
        )
        .unwrap();
        handle(
            &mut c,
            "draft.review",
            json!({"brandId":b,"id":d["id"],"expectedRevision":1}),
        )
        .unwrap();
        let mut input = csv("A,1,10,CNY,,,,依据,2099-01-01");
        input["brandId"] = json!(b);
        input["token"] = preview(&c, &b, &input).unwrap()["token"].clone();
        input["confirm"] = json!(true);
        handle(&mut c, "commerce.applyBatch", input).unwrap();
        assert!(handle(
            &mut c,
            "draft.export",
            json!({"brandId":b,"id":d["id"],"expectedRevision":1,"format":"html"})
        )
        .is_err());
    }
    #[test]
    fn batch_outer_transaction_rolls_back_late_save_error() {
        let (mut c, b, p) = fixture();
        let q = save_product(&c, &b, &json!({"name":"另一虚构商品","sku":"B","facts":[]})).unwrap();
        let mut overflow = q.clone();
        overflow["revision"] = json!(i64::MAX);
        c.execute(
            "UPDATE m_products SET data=?1 WHERE id=?2",
            params![overflow.to_string(), q["id"].as_str().unwrap()],
        )
        .unwrap();
        let mut input = csv(&format!(
            "A,1,10,CNY,,,,依据,2099-01-01\nB,{},10,CNY,,,,依据,2099-01-01",
            i64::MAX
        ));
        input["brandId"] = json!(b);
        input["token"] = preview(&c, &b, &input).unwrap()["token"].clone();
        input["confirm"] = json!(true);
        assert!(handle(&mut c, "commerce.applyBatch", input).is_err());
        assert_eq!(
            load(&c, "m_products", &b, p["id"].as_str().unwrap()).unwrap(),
            p
        );
    }
}
