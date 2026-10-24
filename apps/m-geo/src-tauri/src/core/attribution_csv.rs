//! Explicit local CSV mapping into the immutable attribution exchange.
use super::*;
use std::collections::BTreeSet;
#[path = "csv_diagnostics.rs"]
mod diagnostics;
pub(super) fn inspect_detailed(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    diagnostics::inspect_detailed(c, b, p)
}
pub(super) fn diagnose(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    diagnostics::diagnose(c, b, p)
}

const MAX_BYTES: usize = 512 * 1024;
const INVALID: &str = "CSV归因映射无效，请检查表头、映射、字段、时间与金额";
const FIELDS: [&str; 10] = [
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
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let object = v.as_object().ok_or(INVALID)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(INVALID.into());
    }
    Ok(())
}
fn request(c: &Connection, b: &str, p: &Value, keys: &[&str]) -> Result<()> {
    exact(p, keys)?;
    if p["brandId"].as_str() != Some(b) {
        return Err(INVALID.into());
    }
    brand(c, b)?;
    Ok(())
}
// The csv crate deliberately accepts some malformed quoting. Enforce a strict CSV
// field grammar first, while leaving escaping and record decoding to that crate.
fn quotes(input: &str, delimiter: u8) -> Result<()> {
    #[derive(Clone, Copy)]
    enum State {
        Start,
        Bare,
        Quoted,
        Closed,
    }
    let mut state = State::Start;
    let mut record_empty = true;
    let mut bytes = input.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if !matches!(state, State::Quoted) && matches!(byte, b'\r' | b'\n') {
            // csv::Reader silently skips bare empty records. Reject them here so
            // neither header selection nor raw-row accounting can ignore input.
            if record_empty {
                return Err(INVALID.into());
            }
            if byte == b'\r' && bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
            record_empty = true;
        } else {
            record_empty = false;
        }
        state = match (state, byte) {
            (State::Start, b'"') => State::Quoted,
            (State::Start | State::Bare, b'\r' | b'\n') => State::Start,
            (State::Start | State::Bare, b) if b == delimiter => State::Start,
            (State::Bare, b'"') => return Err(INVALID.into()),
            (State::Start | State::Bare, _) => State::Bare,
            (State::Quoted, b'"') => State::Closed,
            (State::Quoted, _) => State::Quoted,
            (State::Closed, b'"') => State::Quoted,
            (State::Closed, b'\r' | b'\n') => State::Start,
            (State::Closed, b) if b == delimiter => State::Start,
            (State::Closed, _) => return Err(INVALID.into()),
        };
    }
    if matches!(state, State::Quoted) {
        return Err(INVALID.into());
    }
    Ok(())
}
struct Parsed {
    headers: Vec<String>,
    rows: Vec<::csv::StringRecord>,
}
fn parse(p: &Value) -> Result<Parsed> {
    let csv = p["csv"].as_str().ok_or(INVALID)?;
    if csv.is_empty() || csv.len() > MAX_BYTES || csv.contains('\0') {
        return Err(INVALID.into());
    }
    let delimiter = match p["delimiter"].as_str() {
        Some("comma") => b',',
        Some("tab") => b'\t',
        _ => return Err(INVALID.into()),
    };
    let input = csv.strip_prefix('\u{feff}').unwrap_or(csv);
    quotes(input, delimiter)?;
    let mut reader = ::csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(input.as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| INVALID)?
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
        return Err(INVALID.into());
    }
    let mut rows = Vec::new();
    for row in reader.records() {
        let row = row.map_err(|_| INVALID)?;
        if row.len() != headers.len() || rows.len() >= 1000 {
            return Err(INVALID.into());
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(INVALID.into());
    }
    Ok(Parsed { headers, rows })
}
pub(super) fn inspect(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(c, b, p, &["brandId", "csv", "delimiter"])?;
    let parsed = parse(p)?;
    Ok(json!({"headers":parsed.headers,"rowCount":parsed.rows.len()}))
}
fn amount(cell: &str, unit: &str) -> Result<i64> {
    fn integer(s: &str) -> Result<i64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(INVALID.into());
        }
        s.bytes().try_fold(0i64, |value, digit| {
            value
                .checked_mul(10)
                .and_then(|n| n.checked_add(i64::from(digit - b'0')))
                .ok_or_else(|| INVALID.into())
        })
    }
    let value = if unit == "minor" {
        integer(cell)?
    } else {
        let mut parts = cell.split('.');
        let whole = integer(parts.next().ok_or(INVALID)?)?;
        let fractional = match parts.next() {
            None => 0,
            Some(s) if s.len() == 1 => integer(s)?.checked_mul(10).ok_or(INVALID)?,
            Some(s) if s.len() == 2 => integer(s)?,
            Some(_) => return Err(INVALID.into()),
        };
        if parts.next().is_some() {
            return Err(INVALID.into());
        }
        whole
            .checked_mul(100)
            .and_then(|n| n.checked_add(fractional))
            .ok_or(INVALID)?
    };
    if !(1..=1_000_000_000).contains(&value) {
        return Err(INVALID.into());
    }
    Ok(value)
}
// Shared conversion rules keep the authoritative import and its explanations aligned.
struct MappedRecord<'a> {
    record: &'a ::csv::StringRecord,
    columns: &'a [Option<usize>],
}
impl<'a> MappedRecord<'a> {
    fn cell(&self, field: &str) -> &'a str {
        let index = FIELDS.iter().position(|s| *s == field).unwrap();
        self.columns[index]
            .and_then(|n| self.record.get(n))
            .unwrap_or("")
    }
    fn assemble(&self, kind: &str, unit: &str) -> std::result::Result<Value, RowConversionError> {
        let mut row = json!({"id":self.cell("id"),"occurredAt":self.cell("occurredAt")});
        if kind == "refunds" {
            row["orderId"] = json!(self.cell("orderId"));
            row["amountMinor"] =
                json!(amount(self.cell("amount"), unit).map_err(|_| RowConversionError::Amount)?);
        } else {
            row["sku"] = json!(self.cell("sku"));
            row["sessionId"] = if kind == "orders" && self.cell("sessionId").is_empty() {
                Value::Null
            } else {
                json!(self.cell("sessionId"))
            };
            if kind == "visits" {
                let (source, evidence) = (self.cell("source"), self.cell("sourceEvidence"));
                if source.is_empty() != evidence.is_empty() {
                    return Err(RowConversionError::SourcePair);
                }
                row["source"] = json!(if source.is_empty() { "unknown" } else { source });
                row["sourceEvidence"] = json!(if evidence.is_empty() {
                    "unknown"
                } else {
                    evidence
                });
            } else {
                row["status"] = json!(self.cell("status"));
                row["amountMinor"] =
                    json!(amount(self.cell("amount"), unit)
                        .map_err(|_| RowConversionError::Amount)?);
            }
        }
        Ok(row)
    }
}
enum RowConversionError {
    Amount,
    SourcePair,
}
fn used_fields(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "visits" => Some(&[
            "kind",
            "id",
            "sessionId",
            "sku",
            "occurredAt",
            "source",
            "sourceEvidence",
        ]),
        "orders" => Some(&[
            "kind",
            "id",
            "sessionId",
            "sku",
            "occurredAt",
            "status",
            "amount",
        ]),
        "refunds" => Some(&["kind", "id", "occurredAt", "amount", "orderId"]),
        _ => None,
    }
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(
        c,
        b,
        p,
        &["brandId", "name", "csv", "delimiter", "mapping", "metadata"],
    )?;
    let parsed = parse(p)?;
    let mapping = &p["mapping"];
    exact(mapping, &FIELDS)?;
    let mut selected = BTreeSet::new();
    let mut columns = Vec::new();
    for field in FIELDS {
        let column = if mapping[field].is_null() {
            None
        } else {
            let header = mapping[field].as_str().ok_or(INVALID)?;
            if !selected.insert(header) {
                return Err(INVALID.into());
            }
            Some(
                parsed
                    .headers
                    .iter()
                    .position(|s| s == header)
                    .ok_or(INVALID)?,
            )
        };
        if column.is_none() && ["kind", "id", "occurredAt"].contains(&field) {
            return Err(INVALID.into());
        }
        columns.push(column);
    }
    let metadata = &p["metadata"];
    exact(
        metadata,
        &[
            "evidenceMode",
            "sourceLabel",
            "coverageStart",
            "coverageEnd",
            "currency",
            "amountUnit",
        ],
    )?;
    let unit = metadata["amountUnit"]
        .as_str()
        .filter(|s| ["minor", "major"].contains(s))
        .ok_or(INVALID)?;
    let mut exchange = json!({"format":"m-geo-attribution-v1","evidenceMode":metadata["evidenceMode"],"sourceLabel":metadata["sourceLabel"],"coverageStart":metadata["coverageStart"],"coverageEnd":metadata["coverageEnd"],"currency":metadata["currency"],"visits":[],"orders":[],"refunds":[]});
    for record in parsed.rows {
        let mapped = MappedRecord {
            record: &record,
            columns: &columns,
        };
        let kind = mapped.cell("kind");
        let used = used_fields(kind).ok_or(INVALID)?;
        if FIELDS
            .iter()
            .any(|field| !used.contains(field) && !mapped.cell(field).is_empty())
        {
            return Err(INVALID.into());
        }
        let row = mapped.assemble(kind, unit).map_err(|_| INVALID)?;
        exchange[kind].as_array_mut().ok_or(INVALID)?.push(row);
    }
    let raw = exchange.to_string();
    // Preview the raw exchange before canonical deduplication so callers can see
    // the supplied duplicate count. Only canonical deidentified content leaves here.
    let preview = attribution::preview(c, b, &json!({"brandId":b,"name":p["name"],"content":raw}))?;
    let content = attribution::normalize_exchange(&raw)?.to_string();
    if content.len() > MAX_BYTES {
        return Err(INVALID.into());
    }
    Ok(json!({"content":content,"preview":preview,"mapping":mapping,"metadata":metadata}))
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEADER: &str =
        "kind,id,sessionId,sku,occurredAt,source,sourceEvidence,status,amount,orderId,private";
    fn fixture() -> (Connection, String) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构CSV品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构茶杯","sku":"SIM-1","facts":[]}),
        )
        .unwrap();
        (c, b)
    }
    fn metadata() -> Value {
        json!({"evidenceMode":"simulated","sourceLabel":"虚构CSV演练","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","amountUnit":"major"})
    }
    fn mapping() -> Value {
        json!({"kind":"kind","id":"id","sessionId":"sessionId","sku":"sku","occurredAt":"occurredAt","source":"source","sourceEvidence":"sourceEvidence","status":"status","amount":"amount","orderId":"orderId"})
    }
    fn payload(b: &str, csv: &str) -> Value {
        json!({"brandId":b,"name":"虚构CSV","csv":csv,"delimiter":"comma","mapping":mapping(),"metadata":metadata()})
    }
    fn input(v: &Value) -> Value {
        serde_json::from_str(v["content"].as_str().unwrap()).unwrap()
    }
    fn unchanged(c: &Connection, before: i64) {
        assert_eq!(c.total_changes() as i64, before);
    }
    fn order(amount: &str) -> String {
        format!("{HEADER}\norders,o1,,SIM-1,2026-10-03T00:00:00Z,,,paid,{amount},,PRIVATE_MARKER")
    }

    // Omitting strict bounds/header/quote checks would accept ambiguous or truncated files.
    #[test]
    fn csv_headers_rows_limits() {
        let (c, b) = fixture();
        let before = c.total_changes() as i64;
        let inspect_csv =
            |csv: &str| inspect(&c, &b, &json!({"brandId":b,"csv":csv,"delimiter":"comma"}));
        assert_eq!(
            inspect_csv("\u{feff}  key , value \r\n\"a,b\",\"two\nlines\"\r\n").unwrap(),
            json!({"headers":["key","value"],"rowCount":1})
        );
        for csv in [
            "",
            "a,b\n1",
            "a,b\n1,2,3",
            "a, a \n1,2",
            ",b\n1,2",
            "a,b\n\"unterminated,2",
            "a,b\nno\"quote,2",
            "a,b\n\"closed\"junk,2",
            "a,b\n1,\0",
        ] {
            assert!(inspect_csv(csv).is_err(), "invalid CSV accepted");
        }
        assert!(inspect_csv(&format!("{}\n1", "h".repeat(101))).is_err());
        let header = (0..40)
            .map(|n| format!("h{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let cells = vec!["x"; 40].join(",");
        assert!(inspect_csv(&format!("{header}\n{cells}")).is_ok());
        assert!(inspect_csv(&format!("{header},extra\n{cells},x")).is_err());
        assert_eq!(
            inspect_csv(&format!("a\n{}", "x\n".repeat(1000))).unwrap()["rowCount"],
            1000
        );
        assert!(inspect_csv(&format!("a\n{}", "x\n".repeat(1001))).is_err());
        let boundary = format!("a\n{}", "x".repeat(512 * 1024 - 2));
        assert!(inspect_csv(&boundary).is_ok());
        assert!(inspect_csv(&(boundary + "x")).is_err());
        let tab = inspect(
            &c,
            &b,
            &json!({"brandId":b,"csv":"a\tb\r\n1\t2","delimiter":"tab"}),
        )
        .unwrap();
        assert_eq!(tab, json!({"headers":["a","b"],"rowCount":1}));
        assert!(inspect(&c, &b, &json!({"brandId":b,"csv":"a\n1","delimiter":";"})).is_err());
        unchanged(&c, before);
    }

    // Floating conversion, ignored row kinds, or dedup-before-preview would break these results.
    #[test]
    fn csv_mixed_mapping_exact_money() {
        let (c, b) = fixture();
        let before = c.total_changes() as i64;
        let csv=format!("{HEADER}\r\nvisits,v1,s1,SIM-1,2026-10-02T08:00:00+08:00,doubao,tagged,,,,PRIVATE_MARKER\r\norders,o1,,SIM-1,2026-10-03T00:00:00Z,,,paid,12.3,,PRIVATE_MARKER\r\nrefunds,r1,,,2026-10-04T00:00:00Z,,,,0.01,o1,PRIVATE_MARKER");
        let result = prepare(&c, &b, &payload(&b, &csv)).unwrap();
        let x = input(&result);
        assert_eq!(
            result["preview"]["counts"],
            json!({"visits":1,"orders":1,"refunds":1,"duplicates":0})
        );
        assert_eq!(x["orders"][0]["amountMinor"], 1230);
        assert_eq!(x["refunds"][0]["amountMinor"], 1);
        assert!(x["orders"][0]["sessionId"].is_null());
        assert_eq!(x["visits"][0]["occurredAt"], "2026-10-02T00:00:00Z");
        assert_eq!(result["mapping"], mapping());
        assert_eq!(result["metadata"], metadata());
        assert!(!result.to_string().contains("PRIVATE_MARKER"));
        for valid in [("0.01", 1), ("12.3", 1230), ("10000000.00", 1_000_000_000)] {
            assert_eq!(
                input(&prepare(&c, &b, &payload(&b, &order(valid.0))).unwrap())["orders"][0]
                    ["amountMinor"],
                valid.1
            );
        }
        for bad in [
            "12.345",
            "1e2",
            "+1",
            "-1",
            "1,000",
            ".01",
            "1.",
            "１２",
            "0",
            "10000000.01",
            " 12.3",
            "12.3 ",
            "999999999999999999999999999",
        ] {
            assert!(
                prepare(&c, &b, &payload(&b, &order(bad))).is_err(),
                "invalid amount accepted"
            );
        }
        let mut minor = payload(&b, &order("1230"));
        minor["metadata"]["amountUnit"] = json!("minor");
        assert_eq!(
            input(&prepare(&c, &b, &minor).unwrap())["orders"][0]["amountMinor"],
            1230
        );
        minor["csv"] = json!(order("12.3"));
        assert!(prepare(&c, &b, &minor).is_err());
        unchanged(&c, before);
    }

    // Mapped unused cells and personal identifiers must reject the entire mapping, without echoes.
    #[test]
    fn csv_privacy_conditional_fields() {
        let (c, b) = fixture();
        let before = c.total_changes() as i64;
        let valid = order("1");
        assert!(prepare(&c, &b, &payload(&b, &valid)).is_ok());
        for csv in [
            valid.replace("orders,o1", "mystery,o1"),
            valid.replace("orders,o1,,", "orders,o1,alice@example.com,"),
            valid.replace(",,,paid", ",doubao,,paid"),
            valid.replace(",,PRIVATE_MARKER", ",unexpected,PRIVATE_MARKER"),
            valid.replace(",o1,", ",,"),
        ] {
            let error = prepare(&c, &b, &payload(&b, &csv)).unwrap_err();
            assert!(!error.contains("alice@example.com"));
            assert!(!error.contains("PRIVATE_MARKER"));
        }
        for source in [
            ("", "", "unknown", "unknown"),
            ("unknown", "unknown", "unknown", "unknown"),
            ("baidu", "referrer", "baidu", "referrer"),
        ] {
            let csv = format!(
                "{HEADER}\nvisits,v1,s1,SIM-1,2026-10-02T00:00:00Z,{},{},,,,PRIVATE_MARKER",
                source.0, source.1
            );
            let x = input(&prepare(&c, &b, &payload(&b, &csv)).unwrap());
            assert_eq!(x["visits"][0]["source"], source.2);
            assert_eq!(x["visits"][0]["sourceEvidence"], source.3);
        }
        for pair in [
            ("doubao", ""),
            ("", "tagged"),
            ("doubao", "unknown"),
            ("unknown", "tagged"),
            ("google", "tagged"),
        ] {
            let csv = format!(
                "{HEADER}\nvisits,v1,s1,SIM-1,2026-10-02T00:00:00Z,{},{},,,,PRIVATE_MARKER",
                pair.0, pair.1
            );
            assert!(prepare(&c, &b, &payload(&b, &csv)).is_err());
        }
        let mut p = payload(&b, &valid);
        p["mapping"]["sessionId"] = Value::Null;
        p["mapping"]["source"] = Value::Null;
        p["mapping"]["sourceEvidence"] = Value::Null;
        p["mapping"]["orderId"] = Value::Null;
        assert!(prepare(&c, &b, &p).is_ok());
        for field in ["kind", "id", "occurredAt", "sku", "status", "amount"] {
            let mut p = payload(&b, &valid);
            p["mapping"][field] = Value::Null;
            assert!(prepare(&c, &b, &p).is_err());
        }
        let mut p = payload(&b, &valid);
        p["mapping"]["status"] = json!("sku");
        assert!(prepare(&c, &b, &p).is_err());
        let mut p = payload(&b, &valid);
        p["mapping"]["status"] = json!("missing");
        assert!(prepare(&c, &b, &p).is_err());
        for field in ["mapping", "metadata"] {
            let mut p = payload(&b, &valid);
            p[field]["extra"] = json!("PRIVATE_MARKER");
            assert!(prepare(&c, &b, &p).is_err());
        }
        unchanged(&c, before);
    }

    // Catalog binding and canonical UTC duplicate identity must survive reordered CSV and quoted SKU.
    #[test]
    fn csv_offsets_duplicates_and_cross_brand() {
        let (mut c, b) = fixture();
        let before = c.total_changes() as i64;
        let csv = format!(
            "{}\n{}",
            order("1"),
            "orders,o1,,SIM-1,2026-10-03T08:00:00+08:00,,,paid,1,,PRIVATE_MARKER"
        );
        let result = prepare(&c, &b, &payload(&b, &csv)).unwrap();
        assert_eq!(result["preview"]["counts"]["duplicates"], 1);
        assert_eq!(input(&result)["orders"].as_array().unwrap().len(), 1);
        let canonical_preview = attribution::preview(
            &c,
            &b,
            &json!({"brandId":b,"name":"虚构CSV","content":result["content"]}),
        )
        .unwrap();
        assert_eq!(
            canonical_preview["contentHash"],
            result["preview"]["contentHash"]
        );
        assert_eq!(canonical_preview["counts"]["duplicates"], 0);
        let conflict = csv.replacen("08:00:00+08:00,,,paid,1", "08:00:00+08:00,,,paid,2", 1);
        assert!(prepare(&c, &b, &payload(&b, &conflict)).is_err());
        for time in [
            "2026-10-03T00:00:00",
            "2026-10-03T00:00:00.000Z",
            "2026-11-01T00:00:00Z",
        ] {
            assert!(prepare(
                &c,
                &b,
                &payload(&b, &order("1").replace("2026-10-03T00:00:00Z", time))
            )
            .is_err());
        }
        unchanged(&c, before);
        let other = handle(&mut c, "brand.create", json!({"name":"另一虚构品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let before = c.total_changes() as i64;
        assert!(prepare(&c, &other, &payload(&other, &order("1"))).is_err());
        assert!(prepare(&c, &b, &payload(&other, &order("1"))).is_err());
        unchanged(&c, before);
        let sku = "SIM,\"variant\"\nBLUE\tL";
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构变体","sku":sku,"facts":[]}),
        )
        .unwrap();
        let csv = format!(
            "money,when,product,record,type\r\n0.01,2026-10-03T00:00:00Z,\"{}\",o2,orders",
            sku.replace('"', "\"\"")
        );
        let mut p = payload(&b, &csv);
        p["mapping"] = json!({"kind":"type","id":"record","sessionId":null,"sku":"product","occurredAt":"when","source":null,"sourceEvidence":null,"status":null,"amount":"money","orderId":null});
        assert!(prepare(&c, &b, &p).is_err()); // status remains mandatory even in a reordered map.
        p["csv"] = json!(csv.replace("type\r\n", "type,state\r\n") + ",paid");
        p["mapping"]["status"] = json!("state");
        let before = c.total_changes() as i64;
        assert_eq!(
            input(&prepare(&c, &b, &p).unwrap())["orders"][0]["sku"],
            sku
        );
        unchanged(&c, before);
    }
    #[test]
    fn csv_exact_requests_metadata_and_late_row_rejection() {
        let (c, b) = fixture();
        let before = c.total_changes() as i64;
        let valid = payload(&b, &order("1"));
        for (key, value) in [
            ("evidenceMode", json!("guessed")),
            ("currency", json!("GBP")),
            ("amountUnit", json!("auto")),
            ("coverageStart", json!("2026-10-01T00:00:00")),
            ("sourceLabel", json!("")),
        ] {
            let mut p = valid.clone();
            p["metadata"][key] = value;
            assert!(prepare(&c, &b, &p).is_err());
        }
        for name in [
            json!(""),
            json!("x".repeat(101)),
            json!("private\nname"),
            Value::Null,
        ] {
            let mut p = valid.clone();
            p["name"] = name;
            assert!(prepare(&c, &b, &p).is_err());
        }
        let mut p = valid.clone();
        p["extra"] = json!("PRIVATE_MARKER");
        assert!(prepare(&c, &b, &p).is_err());
        let mut p = valid.clone();
        p["mapping"]["sessionId"] = json!(12);
        assert!(prepare(&c, &b, &p).is_err());
        let mut p = valid.clone();
        p["metadata"]["evidenceMode"] = json!("recorded");
        p["metadata"]["currency"] = json!("EUR");
        let result = prepare(&c, &b, &p).unwrap();
        assert_eq!(result["preview"]["evidenceMode"], "recorded");
        assert_eq!(result["preview"]["currency"], "EUR");
        let csv = format!("{HEADER}\nvisits,v1,s1,SIM-1,2026-10-02T00:00:00Z,,,,,,PRIVATE_MARKER");
        let mut p = payload(&b, &csv);
        for key in ["source", "sourceEvidence"] {
            p["mapping"][key] = Value::Null;
        }
        assert_eq!(
            input(&prepare(&c, &b, &p).unwrap())["visits"][0]["source"],
            "unknown"
        );
        for key in ["sessionId", "sku"] {
            let mut p = payload(&b, &csv);
            p["mapping"][key] = Value::Null;
            assert!(prepare(&c, &b, &p).is_err());
        }
        let late = format!(
            "{}\norders,o2,,NO-SKU,2026-10-03T00:00:00Z,,,paid,1,,alice@example.com",
            order("1")
        );
        let error = prepare(&c, &b, &payload(&b, &late)).unwrap_err();
        assert!(!error.contains("NO-SKU"));
        assert!(!error.contains("alice@example.com"));
        assert!(!error.contains("PRIVATE_MARKER"));
        let mut p = valid.clone();
        p["metadata"]["amountUnit"] = json!("minor");
        p["csv"] = json!(order("1000000000"));
        assert_eq!(
            input(&prepare(&c, &b, &p).unwrap())["orders"][0]["amountMinor"],
            1_000_000_000
        );
        p["csv"] = json!(order("1000000001"));
        assert!(prepare(&c, &b, &p).is_err());
        unchanged(&c, before);
    }
    #[test]
    fn csv_bare_empty_records_reject_quoted_blank_lines_survive() {
        let (c, b) = fixture();
        let before = c.total_changes() as i64;
        for (delimiter, separator) in [("comma", ","), ("tab", "\t")] {
            for newline in ["\n", "\r\n"] {
                let header = format!("a{separator}b");
                let row = format!("1{separator}2");
                let inspect_csv = |csv: String| {
                    inspect(
                        &c,
                        &b,
                        &json!({"brandId":b,"csv":csv,"delimiter":delimiter}),
                    )
                };
                for csv in [
                    format!("{newline}{header}{newline}{row}"),
                    format!("{header}{newline}{newline}{row}"),
                    format!("{header}{newline}{row}{newline}{newline}"),
                ] {
                    assert!(
                        inspect_csv(csv).is_err(),
                        "bare empty record must reject for {delimiter}"
                    );
                }
                let quoted = format!(
                    "{header}{newline}\"first{newline}{newline}last\"{separator}2{newline}"
                );
                assert_eq!(inspect_csv(quoted).unwrap()["rowCount"], 1);
                assert_eq!(
                    inspect_csv(format!("{header}{newline}{row}{newline}")).unwrap()["rowCount"],
                    1
                );
            }
        }
        for newline in ["\n", "\r\n"] {
            let csv = order("1").replace('\n', newline);
            let valid = format!("{csv}{newline}");
            assert!(prepare(&c, &b, &payload(&b, &valid)).is_ok());
            let invalid=format!("{csv}{newline}{newline}orders,o2,,SIM-1,2026-10-03T00:00:00Z,,,paid,1,,PRIVATE_MARKER");
            assert!(prepare(&c, &b, &payload(&b, &invalid)).is_err());
        }
        // A quoted empty cell is an actual one-field record, rather than a skipped line.
        assert_eq!(
            inspect(
                &c,
                &b,
                &json!({"brandId":b,"csv":"a\n\"\"\n","delimiter":"comma"})
            )
            .unwrap()["rowCount"],
            1
        );
        unchanged(&c, before);
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    fn fixture() -> (Connection, String) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构诊断"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构杯","sku":"SIM-1","facts":[]}),
        )
        .unwrap();
        (c, b)
    }
    const HEADER: &str =
        "kind,id,sessionId,sku,occurredAt,source,sourceEvidence,status,amount,orderId";
    fn order(id: &str, amount: &str) -> String {
        format!("orders,{id},,SIM-1,2026-10-03T00:00:00Z,,,paid,{amount},")
    }
    fn payload(b: &str, rows: &str) -> Value {
        json!({"brandId":b,"name":"虚构CSV","csv":format!("{HEADER}\n{rows}"),"delimiter":"comma","mapping":{"kind":"kind","id":"id","sessionId":"sessionId","sku":"sku","occurredAt":"occurredAt","source":"source","sourceEvidence":"sourceEvidence","status":"status","amount":"amount","orderId":"orderId"},"metadata":{"evidenceMode":"simulated","sourceLabel":"虚构演练","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","amountUnit":"major"}})
    }
    fn check(c: &Connection, b: &str, csv: &str) -> Value {
        inspect_detailed(c, b, &json!({"brandId":b,"csv":csv,"delimiter":"comma"})).unwrap()
    }
    #[test]
    fn diagnostics_structure_safe_logical_records_and_limits() {
        let (c, b) = fixture();
        let before = c.total_changes();
        for (csv, code, record) in [
            ("a,b\n1,\"SECRET", "csv.quote", Some(1)),
            ("a,b\n1,2\n\n", "csv.blankRecord", Some(2)),
            ("a, a \n1,2", "csv.header", None),
            ("a,b\n1", "csv.width", Some(1)),
            ("", "csv.empty", None),
        ] {
            let d = check(&c, &b, csv);
            assert_eq!(d["status"], "blocked");
            assert_eq!(d["headers"], json!([]));
            assert!(d["rowCount"].is_null());
            assert_eq!(d["issues"][0]["code"], code);
            assert_eq!(d["issues"][0]["record"], json!(record));
            assert!(!d.to_string().contains("SECRET"));
        }
        let d = check(&c, &b, "\u{feff}a,b\r\n\"one\r\n\r\ntwo\",2\r\n3\r\n");
        assert_eq!(d["issues"][0]["record"], 2);
        let csv = "\u{feff}a,b\r\n\"one\r\n\r\ntwo\",2\r\n3,4\r\n";
        let d = check(&c, &b, csv);
        assert_eq!(d["status"], "ready");
        assert_eq!(d["rowCount"], 2);
        assert!(d["prepared"].is_null());
        assert_eq!(
            check(&c, &b, &format!("a\n{}", "x\n".repeat(1000)))["status"],
            "ready"
        );
        assert_eq!(
            check(&c, &b, &format!("a\n{}", "x\n".repeat(1001)))["issues"][0]["code"],
            "csv.rowLimit"
        );
        let boundary = format!("a\n{}", "界".repeat((MAX_BYTES - 2) / 3));
        let boundary = format!("{boundary}{}", "x".repeat(MAX_BYTES - boundary.len()));
        assert_eq!(check(&c, &b, &boundary)["status"], "ready");
        assert_eq!(
            check(&c, &b, &(boundary + "x"))["issues"][0]["code"],
            "csv.size"
        );
        assert_eq!(c.total_changes(), before);
    }
    #[test]
    fn diagnostics_mapping_collects_safe_independent_conflicts() {
        let (c, b) = fixture();
        let mut p = payload(&b, &order("o1", "1"));
        p["mapping"]["kind"] = Value::Null;
        p["mapping"]["id"] = json!("SECRET");
        p["mapping"]["status"] = json!("sku");
        let d = diagnose(&c, &b, &p).unwrap();
        assert_eq!(d["totalIssues"], 3);
        assert_eq!(d["issues"][0]["scope"], "mapping");
        assert!(!d.to_string().contains("SECRET"));
        assert!(d["prepared"].is_null());
    }
    #[test]
    fn diagnostics_conversion_collects_input_order_and_rechecks() {
        let (c, b) = fixture();
        let rows = format!(
            "{}\n{}\n{}",
            order("o1", "1.234"),
            "mystery,SECRET,,,,,,,,",
            "visits,v1,s1,SIM-1,2026-10-03T00:00:00Z,doubao,,paid,,"
        );
        let d = diagnose(&c, &b, &payload(&b, &rows)).unwrap();
        assert_eq!(d["totalIssues"], 4);
        assert_eq!(
            d["issues"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["record"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 3]
        );
        assert!(!d.to_string().contains("SECRET"));
        let p = payload(&b, &order("o1", "1"));
        let d = diagnose(&c, &b, &p).unwrap();
        assert_eq!(d["status"], "ready");
        assert_eq!(d["prepared"], prepare(&c, &b, &p).unwrap());
        assert_eq!(
            d["headers"],
            inspect(
                &c,
                &b,
                &json!({"brandId":b,"csv":p["csv"],"delimiter":"comma"})
            )
            .unwrap()["headers"]
        );
    }
    #[test]
    fn diagnostics_semantic_provenance_interleaved_and_parity() {
        let (c, b) = fixture();
        let before = c.total_changes();
        let rows=format!("refunds,r1,,,2026-10-04T00:00:00Z,,,,0.01,o1\nvisits,v1,s1,SIM-1,2026-10-02T00:00:00Z,doubao,tagged,,,\n{}\n{}", order("o1","1"),order("o1","2"));
        let d = diagnose(&c, &b, &payload(&b, &rows)).unwrap();
        let conflict = d["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["record"] == 4)
            .unwrap();
        assert_eq!(conflict["relatedRecords"], json!([3]));
        let rows = format!(
            "{}\n{}",
            order("o1", "1"),
            order("o1", "1").replace("00:00:00Z", "08:00:00+08:00")
        );
        let p = payload(&b, &rows);
        assert_eq!(
            diagnose(&c, &b, &p).unwrap()["prepared"],
            prepare(&c, &b, &p).unwrap()
        );
        let d = diagnose(
            &c,
            &b,
            &payload(&b, &order("o1", "1").replace("SIM-1", "SECRET-SKU")),
        )
        .unwrap();
        assert_eq!(d["issues"][0]["record"], 1);
        assert_eq!(d["issues"][0]["scope"], "catalog");
        assert!(!d.to_string().contains("SECRET"));
        assert_eq!(c.total_changes(), before);
    }
    #[test]
    fn diagnostics_exact_truncation_and_metadata_location() {
        let (c, b) = fixture();
        for count in [200, 201] {
            let rows = (0..count)
                .map(|i| order(&format!("o{i}"), "1.234"))
                .collect::<Vec<_>>()
                .join("\n");
            let d = diagnose(&c, &b, &payload(&b, &rows)).unwrap();
            assert_eq!(d["totalIssues"], count);
            assert_eq!(d["issues"].as_array().unwrap().len(), 200);
            assert_eq!(d["issuesTruncated"], count > 200);
        }
        let mut p = payload(&b, &order("o1", "1"));
        p["name"] = json!("SECRET\n");
        let d = diagnose(&c, &b, &p).unwrap();
        assert_eq!(d["status"], "blocked");
        assert_eq!(d["issues"][0]["scope"], "metadata");
        assert!(d["issues"][0]["record"].is_null());
        assert!(!d.to_string().contains("SECRET"));
    }
    // Reusing semantic-array indices as CSV records would mislocate this multiline fixture.
    #[test]
    fn diagnostics_multiline_sku_semantic_locations_and_accepted_parity() {
        let (mut c, b) = fixture();
        let sku = "SIM,\"BLUE\"\nVARIANT\tL";
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构变体","sku":sku,"facts":[]}),
        )
        .unwrap();
        let row = format!(
            "orders,o2,,\"{}\",2026-10-03T00:00:00Z,,,paid,1,",
            sku.replace('"', "\"\"")
        );
        let rows = format!("{}\r\n{}", order("o1", "1"), row);
        let mut p = payload(&b, &rows);
        p["csv"] = json!(format!("\u{feff}{HEADER}\r\n{rows}\r\n"));
        let before = c.total_changes();
        let d = diagnose(&c, &b, &p).unwrap();
        assert_eq!(d["status"], "ready");
        assert_eq!(d["rowCount"], 2);
        assert_eq!(d["prepared"], prepare(&c, &b, &p).unwrap());
        p["csv"] = json!(p["csv"]
            .as_str()
            .unwrap()
            .replacen("orders,o2", "orders,bad@SECRET", 1));
        let d = diagnose(&c, &b, &p).unwrap();
        assert_eq!(d["issues"][0]["record"], 2);
        assert_eq!(d["issues"][0]["field"], "id");
        assert!(!d["issues"].to_string().contains("SECRET"));
        for (unit, money) in [("minor", "1000000000"), ("major", "0.01")] {
            let mut p = payload(&b, &order("o1", money));
            p["metadata"]["amountUnit"] = json!(unit);
            assert_eq!(
                diagnose(&c, &b, &p).unwrap()["prepared"],
                prepare(&c, &b, &p).unwrap()
            );
        }
        assert_eq!(c.total_changes(), before);
    }
    // Semantic errors must also retain the exact examined-stage total after display truncation.
    #[test]
    fn diagnostics_semantic_truncation_and_source_pair_order() {
        let (c, b) = fixture();
        let rows = (0..201)
            .map(|i| {
                order(&format!("o{i}"), "1").replace("2026-10-03T00:00:00Z", "BAD_SECRET_TIME")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let d = diagnose(&c, &b, &payload(&b, &rows)).unwrap();
        assert_eq!(d["totalIssues"], 201);
        assert_eq!(d["issues"].as_array().unwrap().len(), 200);
        assert_eq!(d["issues"][199]["record"], 200);
        assert_eq!(d["issuesTruncated"], true);
        assert!(!d["issues"].to_string().contains("SECRET"));
        let rows="visits,v1,s1,SIM-1,2026-10-03T00:00:00Z,doubao,unknown,,,\nvisits,v2,s2,SIM-1,2026-10-03T00:00:00Z,unknown,tagged,,,";
        let d = diagnose(&c, &b, &payload(&b, rows)).unwrap();
        assert_eq!(d["totalIssues"], 2);
        assert_eq!(d["issues"][0]["record"], 1);
        assert_eq!(d["issues"][1]["record"], 2);
        assert_eq!(d["issues"][0]["field"], "sourceEvidence");
    }
    #[test]
    fn diagnostics_dataset_lookup_failure_preserves_authoritative_safe_error() {
        let (mut c, b) = fixture();
        let p = payload(&b, &order("o1", "1"));
        let prepared = handle(&mut c, "attribution.csvPrepare", p.clone()).unwrap();
        assert_eq!(
            handle(&mut c, "attribution.csvDiagnose", p.clone()).unwrap()["prepared"],
            prepared
        );
        // Remove only the isolated fixture's dataset lookup, leaving its valid catalog intact.
        c.execute("DROP TABLE m_attribution_datasets", []).unwrap();
        let before = c.total_changes();
        let legacy = handle(&mut c, "attribution.csvPrepare", p.clone()).unwrap_err();
        assert_eq!(
            legacy,
            "本地资料操作失败，请重试；如持续失败请恢复备份或联系支持"
        );
        let diagnostic = handle(&mut c, "attribution.csvDiagnose", p);
        assert_eq!(c.total_changes(), before);
        assert_eq!(diagnostic, Err(legacy));
    }
    // Empty advisory output must never override an authoritative expanded-content rejection.
    #[test]
    fn diagnostics_unexplained_legacy_failure_stays_blocked() {
        let (mut c, b) = fixture();
        let sku = format!("V{}", "\u{0001}".repeat(99));
        handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构编码边界","sku":sku,"facts":[]}),
        )
        .unwrap();
        let rows = (0..1000)
            .map(|i| order(&format!("o{i}"), "1").replace("SIM-1", &sku))
            .collect::<Vec<_>>()
            .join("\n");
        let p = payload(&b, &rows);
        assert!(p["csv"].as_str().unwrap().len() < MAX_BYTES);
        assert!(prepare(&c, &b, &p).is_err());
        let before = c.total_changes();
        let d = diagnose(&c, &b, &p).unwrap();
        assert_eq!(d["status"], "blocked");
        assert!(d["prepared"].is_null());
        assert_eq!(d["totalIssues"], 1);
        assert_eq!(d["issues"][0]["scope"], "dataset");
        assert_eq!(d["issues"][0]["code"], "dataset.validation");
        assert_eq!(c.total_changes(), before);
    }
}
