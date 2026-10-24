//! Read-only explanations. Legacy prepare remains the sole readiness authority.
use super::*;
use std::collections::BTreeMap;

fn issue(
    scope: &str,
    record: Option<usize>,
    field: Option<&str>,
    code: &str,
    message: &str,
) -> Value {
    json!({"scope":scope,"record":record,"field":field,"code":code,"message":message,"relatedRecords":[]})
}
fn csv_issue(record: Option<usize>, code: &str, message: &str) -> Value {
    issue("csv", record, None, code, message)
}
fn report(
    stage: &str,
    parsed: Option<&Parsed>,
    issues: Vec<Value>,
    prepared: Option<Value>,
) -> Value {
    let total = issues.len();
    let issues = issues.into_iter().take(200).collect::<Vec<_>>();
    json!({"format":"m-geo-csv-diagnostics-v1","stage":stage,"status":if total == 0 {"ready"} else {"blocked"},"headers":parsed.map(|p| &p.headers).cloned().unwrap_or_default(),"rowCount":parsed.map(|p|p.rows.len()),"issues":issues,"totalIssues":total,"issuesTruncated":total>200,"prepared":prepared})
}
fn input(p: &Value) -> Result<(&str, u8)> {
    let csv = p["csv"].as_str().ok_or(INVALID)?;
    let delimiter = match p["delimiter"].as_str() {
        Some("comma") => b',',
        Some("tab") => b'\t',
        _ => return Err(INVALID.into()),
    };
    Ok((csv, delimiter))
}
// Match the inherited strict quote grammar, while tracking logical CSV records.
fn grammar(input: &str, delimiter: u8) -> std::result::Result<(), Value> {
    #[derive(Clone, Copy)]
    enum State {
        Start,
        Bare,
        Quoted,
        Closed,
    }
    let mut state = State::Start;
    let mut empty = true;
    let mut logical = 0usize;
    let mut bytes = input.bytes().peekable();
    while let Some(byte) = bytes.next() {
        let record = (logical > 0).then_some(logical);
        if !matches!(state, State::Quoted) && matches!(byte, b'\r' | b'\n') {
            if empty {
                return Err(csv_issue(
                    record,
                    "csv.blankRecord",
                    "删除空白记录后重新检查",
                ));
            }
            if byte == b'\r' && bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
            empty = true;
            logical += 1;
        } else {
            empty = false;
        }
        state = match (state, byte) {
            (State::Start, b'"') => State::Quoted,
            (State::Start | State::Bare, b'\r' | b'\n') => State::Start,
            (State::Start | State::Bare, b) if b == delimiter => State::Start,
            (State::Bare, b'"') => {
                return Err(csv_issue(
                    record,
                    "csv.quote",
                    "检查引号配对与转义后重新检查",
                ))
            }
            (State::Start | State::Bare, _) => State::Bare,
            (State::Quoted, b'"') => State::Closed,
            (State::Quoted, _) => State::Quoted,
            (State::Closed, b'"') => State::Quoted,
            (State::Closed, b'\r' | b'\n') => State::Start,
            (State::Closed, b) if b == delimiter => State::Start,
            (State::Closed, _) => {
                return Err(csv_issue(
                    record,
                    "csv.quote",
                    "检查引号配对与转义后重新检查",
                ))
            }
        };
    }
    if matches!(state, State::Quoted) {
        return Err(csv_issue(
            (logical > 0).then_some(logical),
            "csv.quote",
            "检查引号配对与转义后重新检查",
        ));
    }
    Ok(())
}
fn structure(csv: &str, delimiter: u8) -> std::result::Result<Parsed, Value> {
    if csv.is_empty() {
        return Err(csv_issue(None, "csv.empty", "选择包含表头与数据记录的CSV"));
    }
    if csv.len() > MAX_BYTES {
        return Err(csv_issue(
            None,
            "csv.size",
            "CSV不得超过512KiB，请拆分后重新检查",
        ));
    }
    if csv.contains('\0') {
        return Err(csv_issue(None, "csv.nul", "删除无效空字符后重新检查"));
    }
    let input = csv.strip_prefix('\u{feff}').unwrap_or(csv);
    grammar(input, delimiter)?;
    let mut reader = ::csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(input.as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| csv_issue(None, "csv.header", "检查表头格式后重新检查"))?
        .iter()
        .map(|s| s.trim().to_owned())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    if headers.is_empty()
        || headers.len() > 40
        || headers
            .iter()
            .any(|s| s.is_empty() || s.chars().count() > 100 || !seen.insert(s.clone()))
    {
        return Err(csv_issue(
            None,
            "csv.header",
            "表头须非空、不重复、每项不超过100字且不超过40列",
        ));
    }
    let mut rows = Vec::new();
    for row in reader.records() {
        let record = rows.len() + 1;
        let row =
            row.map_err(|_| csv_issue(Some(record), "csv.record", "检查记录格式后重新检查"))?;
        if row.len() != headers.len() {
            return Err(csv_issue(Some(record), "csv.width", "记录列数须与表头一致"));
        }
        if rows.len() >= 1000 {
            return Err(csv_issue(
                Some(record),
                "csv.rowLimit",
                "数据记录不得超过1000条，请拆分后重新检查",
            ));
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(csv_issue(
            None,
            "csv.records",
            "添加至少一条数据记录后重新检查",
        ));
    }
    Ok(Parsed { headers, rows })
}
pub(super) fn inspect_detailed(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(c, b, p, &["brandId", "csv", "delimiter"])?;
    let (csv, delimiter) = input(p)?;
    Ok(match structure(csv, delimiter) {
        Ok(parsed) => report("structure", Some(&parsed), vec![], None),
        Err(issue) => report("structure", None, vec![issue], None),
    })
}
fn exchange(metadata: &Value) -> Value {
    json!({"format":"m-geo-attribution-v1","evidenceMode":metadata["evidenceMode"],"sourceLabel":metadata["sourceLabel"],"coverageStart":metadata["coverageStart"],"coverageEnd":metadata["coverageEnd"],"currency":metadata["currency"],"visits":[],"orders":[],"refunds":[]})
}
fn semantic_issues(issues: Vec<Value>, records: &BTreeMap<&str, Vec<usize>>) -> Vec<Value> {
    let location = |kind: &Value, index: &Value| -> Option<usize> {
        records
            .get(kind.as_str()?)?
            .get(index.as_u64()? as usize)
            .copied()
    };
    let mut output = Vec::new();
    for semantic in issues {
        let record = location(&semantic["kind"], &semantic["index"]);
        let field = semantic["field"]
            .as_str()
            .map(|s| if s == "amountMinor" { "amount" } else { s });
        let mut value = issue(
            semantic["scope"].as_str().unwrap_or("dataset"),
            record,
            field,
            semantic["code"].as_str().unwrap_or("dataset.validation"),
            semantic["message"].as_str().unwrap_or("修正数据后重新检查"),
        );
        let related = semantic["related"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| location(&v["kind"], &v["index"]))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(20)
            .collect::<Vec<_>>();
        value["relatedRecords"] = json!(related);
        output.push(value);
    }
    output.sort_by_key(|v| v["record"].as_u64());
    output
}
pub(super) fn diagnose(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(
        c,
        b,
        p,
        &["brandId", "name", "csv", "delimiter", "mapping", "metadata"],
    )?;
    let (csv, delimiter) = input(p)?;
    let parsed = match structure(csv, delimiter) {
        Ok(parsed) => parsed,
        Err(issue) => return Ok(report("mapped", None, vec![issue], None)),
    };
    // Advisory explanations can never authorize an import or modify accepted output.
    match prepare(c, b, p) {
        Ok(prepared) => return Ok(report("mapped", Some(&parsed), vec![], Some(prepared))),
        Err(error) if error == DB_ERROR => return Err(error),
        Err(_) => {}
    }
    let mapping = &p["mapping"];
    let mut issues = Vec::new();
    if exact(mapping, &FIELDS).is_err() {
        issues.push(issue(
            "mapping",
            None,
            None,
            "mapping.fields",
            "映射须包含全部标准字段，未使用字段请选择不映射",
        ));
        return Ok(report("mapped", Some(&parsed), issues, None));
    }
    let mut selected = BTreeSet::new();
    let mut columns = Vec::new();
    for field in FIELDS {
        let column = if mapping[field].is_null() {
            if ["kind", "id", "occurredAt"].contains(&field) {
                issues.push(issue(
                    "mapping",
                    None,
                    Some(field),
                    "mapping.required",
                    "选择必需字段对应的表头",
                ));
            }
            None
        } else if let Some(header) = mapping[field].as_str() {
            let index = parsed.headers.iter().position(|s| s == header);
            if index.is_none() {
                issues.push(issue(
                    "mapping",
                    None,
                    Some(field),
                    "mapping.header",
                    "重新选择当前CSV中存在的表头",
                ));
            }
            if !selected.insert(header) {
                issues.push(issue(
                    "mapping",
                    None,
                    Some(field),
                    "mapping.reused",
                    "每个表头只能映射到一个字段",
                ));
            }
            index
        } else {
            issues.push(issue(
                "mapping",
                None,
                Some(field),
                "mapping.value",
                "选择表头或不映射",
            ));
            None
        };
        columns.push(column);
    }
    if !issues.is_empty() {
        return Ok(report("mapped", Some(&parsed), issues, None));
    }
    let metadata = &p["metadata"];
    if exact(
        metadata,
        &[
            "evidenceMode",
            "sourceLabel",
            "coverageStart",
            "coverageEnd",
            "currency",
            "amountUnit",
        ],
    )
    .is_err()
    {
        issues.push(issue(
            "metadata",
            None,
            None,
            "metadata.fields",
            "填写全部标准元数据字段并移除额外字段",
        ));
    }
    let unit = metadata["amountUnit"]
        .as_str()
        .filter(|s| ["minor", "major"].contains(s));
    if unit.is_none() {
        issues.push(issue(
            "metadata",
            None,
            Some("amountUnit"),
            "metadata.amountUnit",
            "选择元或最小货币单位",
        ));
    }
    let mut mapped = exchange(metadata);
    let mut records = BTreeMap::from([("visits", vec![]), ("orders", vec![]), ("refunds", vec![])]);
    // Metadata must be repaired before attempting row conversion or relationship checks.
    let metadata_issues = attribution::diagnose_exchange(c, b, &mapped, &p["name"])?
        .into_iter()
        .filter(|v| v["scope"] == "metadata")
        .collect::<Vec<_>>();
    issues.extend(semantic_issues(metadata_issues, &records));
    if !issues.is_empty() {
        return Ok(report("mapped", Some(&parsed), issues, None));
    }
    let unit = unit.ok_or(INVALID)?;
    for (index, record) in parsed.rows.iter().enumerate() {
        let logical = Some(index + 1);
        let source = MappedRecord {
            record,
            columns: &columns,
        };
        let kind = source.cell("kind");
        let Some(used) = used_fields(kind) else {
            issues.push(issue(
                "row",
                logical,
                Some("kind"),
                "row.kind",
                "类型须为visits、orders或refunds",
            ));
            continue;
        };
        for field in FIELDS {
            if !used.contains(&field) && !source.cell(field).is_empty() {
                issues.push(issue(
                    "row",
                    logical,
                    Some(field),
                    "row.unused",
                    "清空此类型不使用的已映射字段",
                ));
            }
        }
        let row = match source.assemble(kind, unit) {
            Ok(row) => row,
            Err(RowConversionError::Amount) => {
                issues.push(issue(
                    "row",
                    logical,
                    Some("amount"),
                    "row.amount",
                    "填写有效正数金额，元最多两位小数且最小单位不超过十亿",
                ));
                continue;
            }
            Err(RowConversionError::SourcePair) => {
                issues.push(issue(
                    "row",
                    logical,
                    Some("sourceEvidence"),
                    "row.sourcePair",
                    "来源与来源依据须同时填写或同时留空",
                ));
                continue;
            }
        };
        mapped[kind].as_array_mut().ok_or(INVALID)?.push(row);
        records.get_mut(kind).ok_or(INVALID)?.push(index + 1);
    }
    if issues.is_empty() {
        issues = semantic_issues(
            attribution::diagnose_exchange(c, b, &mapped, &p["name"])?,
            &records,
        );
    }
    if issues.is_empty() {
        issues.push(issue(
            "dataset",
            None,
            None,
            "dataset.validation",
            "数据仍未通过完整校验，请检查数据集后重新检查",
        ));
    }
    Ok(report("mapped", Some(&parsed), issues, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    // Repeated, array-ordered references must become bounded sorted logical records.
    #[test]
    fn semantic_locations_map_money_and_bound_related_records() {
        let records = BTreeMap::from([("orders", (1..=25).rev().collect::<Vec<_>>())]);
        let related = (0..25)
            .chain([0, 1])
            .map(|index| json!({"kind":"orders","index":index}))
            .collect::<Vec<_>>();
        let result = semantic_issues(
            vec![
                json!({"scope":"reference","kind":"orders","index":0,"field":"amountMinor","code":"reference.refundExcess","message":"检查退款金额","related":related}),
            ],
            &records,
        );
        assert_eq!(result[0]["record"], 25);
        assert_eq!(result[0]["field"], "amount");
        assert_eq!(
            result[0]["relatedRecords"],
            json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20])
        );
    }
}
