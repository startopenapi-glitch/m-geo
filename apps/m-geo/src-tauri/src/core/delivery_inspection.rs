//! Read-only offline inspection of a bounded, decoded website artifact.
use super::*;
use std::collections::BTreeMap;

const MAX_REPORT: usize = 2 * 1024 * 1024;
const MAX_CHECKS: usize = 2048;
const CAVEAT: &str =
    "仅核对本地文件与当前品牌，不证明文件来源可信、官网可达、收录或推荐；哈希不等于数字签名。";

fn unique_json(s: &str) -> Result<Value> {
    strict_json::parse(s).map_err(|_| "清单或目录 JSON 无效或含重复字段".into())
}
fn shape(v: &Value, keys: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}
fn positive(v: &Value) -> bool {
    v.as_i64().is_some_and(|n| n > 0)
}
fn uuid(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| Uuid::parse_str(s).is_ok_and(|u| u.to_string() == s))
}
fn timestamp(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| DateTime::parse_from_rfc3339(s).is_ok())
}
fn string(v: &Value, max: usize, nonempty: bool) -> bool {
    v.as_str().is_some_and(|s| {
        (!nonempty || !s.trim().is_empty()) && s.chars().count() <= max && !s.contains('\0')
    })
}
fn nullable_string(v: &Value, max: usize) -> bool {
    v.is_null() || string(v, max, true)
}
fn safe_path(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        && !s.starts_with('/')
        && !s.split('/').any(|p| p.is_empty() || p == "." || p == "..")
}
fn hash(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn file_map(package: &Value) -> Result<BTreeMap<String, String>> {
    let rows = package["files"].as_array().ok_or("网站文件列表无效")?;
    let mut files = BTreeMap::new();
    let mut insensitive = HashSet::new();
    for row in rows {
        let path = row["path"].as_str().ok_or("网站文件路径无效")?;
        let content = row["content"].as_str().ok_or("网站文件内容无效")?;
        if !shape(row, &["path", "content"])
            || !safe_path(path)
            || !insensitive.insert(path.to_ascii_lowercase())
            || content.contains('\0')
            || content.starts_with('\u{feff}')
        {
            return Err("网站文件路径不安全、重复或文本无效".into());
        }
        files.insert(path.to_owned(), content.to_owned());
    }
    Ok(files)
}
fn processing_budget(package: &Value) -> Result<()> {
    if let Some(rows) = package["files"].as_array() {
        if rows.len() > 110 {
            return Err("网站文件数量超过110项处理上限".into());
        }
        let mut bytes = 0usize;
        let mut hrefs = 0usize;
        for row in rows {
            if let Some(s) = row["content"].as_str() {
                bytes = bytes
                    .checked_add(s.len())
                    .ok_or("网站文件超过16MiB处理上限")?;
                if bytes > 16 * 1024 * 1024 {
                    return Err("网站文件超过16MiB处理上限".into());
                }
                // A superset of the inherited template audit's href=\" occurrences.
                hrefs += s.matches("href").count();
                if hrefs > MAX_CHECKS {
                    return Err("网站 href 数量超过2048项处理上限".into());
                }
            }
        }
    }
    Ok(())
}
fn structure(files: &BTreeMap<String, String>) -> Result<(Value, Value)> {
    let fail = || "网站清单、目录结构或文件引用不符合当前导出格式".to_string();
    let m = unique_json(files.get("release-manifest.json").ok_or_else(fail)?)?;
    let cat = unique_json(files.get("catalog.json").ok_or_else(fail)?)?;
    if !shape(
        &m,
        &[
            "format",
            "generatedAt",
            "publicationStatus",
            "previewOnly",
            "baseUrl",
            "revisions",
            "files",
            "expectedPublicUrls",
            "catalogStatus",
            "manualPostPublicationChecks",
        ],
    ) || m["format"] != "m-geo-release-manifest-v1"
        || !timestamp(&m["generatedAt"])
        || m["publicationStatus"] != "not-published"
        || m["catalogStatus"] != "generic-not-platform-qualified-not-submitted"
    {
        return Err(fail());
    }
    let preview = m["previewOnly"].as_bool().ok_or_else(fail)?;
    let base = if preview {
        if !m["baseUrl"].is_null() {
            return Err(fail());
        }
        None
    } else {
        let s = m["baseUrl"].as_str().ok_or_else(fail)?;
        if s.len() > 2000 || web_url(s, true).is_err() {
            return Err(fail());
        }
        let u = url::Url::parse(s).map_err(|_| fail())?;
        if u.scheme() != "https"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.path().ends_with('/')
            || u.to_string() != s
        {
            return Err(fail());
        }
        Some(s)
    };
    if m["manualPostPublicationChecks"]
        != json!([
            "人工核对公开页面及购买信息",
            "核验 canonical 与 sitemap 地址",
            "robots.txt 必须部署或合并到网站根目录；子目录文件不代表根目录 robots 策略",
            "人工检查结构化数据与公开可访问性；不代表收录或平台资格"
        ])
    {
        return Err(fail());
    }
    let refs = m["revisions"].as_array().ok_or_else(fail)?;
    if refs.is_empty() || refs.len() > 50 {
        return Err(fail());
    }
    let mut product_ids = HashSet::new();
    let mut draft_ids = HashSet::new();
    let mut expected_files: HashSet<String> = [
        "index.html",
        "robots.txt",
        "catalog.json",
        "catalog.csv",
        "README.txt",
        "release-manifest.json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut expected_urls = vec![];
    if let Some(base) = base {
        expected_files.insert("sitemap.xml".into());
        expected_urls.push(json!(base));
    }
    for r in refs {
        if !shape(
            r,
            &["productId", "productRevision", "sourceRevision", "drafts"],
        ) || !uuid(&r["productId"])
            || !positive(&r["productRevision"])
            || r["sourceRevision"] != r["productRevision"]
            || !product_ids.insert(r["productId"].as_str().unwrap())
        {
            return Err(fail());
        }
        let pid = r["productId"].as_str().unwrap();
        expected_files.insert(format!("products/{pid}/index.html"));
        if let Some(base) = base {
            expected_urls.push(json!(format!("{base}products/{pid}/")));
        }
        let drafts = r["drafts"].as_array().ok_or_else(fail)?;
        if drafts.is_empty() || drafts.len() > 50 {
            return Err(fail());
        }
        for d in drafts {
            if !shape(d, &["id", "revision", "sourceRevision"])
                || !uuid(&d["id"])
                || !positive(&d["revision"])
                || d["sourceRevision"] != r["productRevision"]
                || !draft_ids.insert(d["id"].as_str().unwrap())
                || draft_ids.len() > 50
            {
                return Err(fail());
            }
        }
    }
    if m["expectedPublicUrls"] != json!(expected_urls)
        || expected_files != files.keys().cloned().collect()
    {
        return Err(fail());
    }
    let digests = m["files"].as_array().ok_or_else(fail)?;
    if digests.len() != files.len() - 1 {
        return Err(fail());
    }
    let mut listed = HashSet::new();
    for f in digests {
        let path = f["path"].as_str().ok_or_else(fail)?;
        if !shape(f, &["path", "sha256"])
            || !safe_path(path)
            || path == "release-manifest.json"
            || !hash(&f["sha256"])
            || !listed.insert(path)
            || !files.get(path).is_some_and(|content| {
                f["sha256"] == hex::encode(Sha256::digest(content.as_bytes()))
            })
        {
            return Err(fail());
        }
    }
    if !shape(&cat, &["format", "generatedAt", "status", "products"])
        || cat["format"] != "m-geo-generic-catalog-v1"
        || cat["status"] != "local-preparation-not-submitted"
        || !timestamp(&cat["generatedAt"])
    {
        return Err(fail());
    }
    let products = cat["products"].as_array().ok_or_else(fail)?;
    if products.len() != refs.len() {
        return Err(fail());
    }
    let mut catalog_ids = HashSet::new();
    let mut skus = HashSet::new();
    for p in products {
        if !shape(
            p,
            &[
                "id",
                "sku",
                "title",
                "url",
                "purchaseUrl",
                "imageUrl",
                "offer",
                "publicFacts",
                "sourceRevision",
            ],
        ) || !uuid(&p["id"])
            || !catalog_ids.insert(p["id"].as_str().unwrap())
            || !string(&p["title"], 200, true)
            || !string(&p["sku"], 100, true)
            || !skus.insert(p["sku"].as_str().unwrap())
            || !nullable_string(&p["purchaseUrl"], 3000)
            || !nullable_string(&p["imageUrl"], 3000)
        {
            return Err(fail());
        }
        let r = refs
            .iter()
            .find(|r| r["productId"] == p["id"])
            .ok_or_else(fail)?;
        if p["sourceRevision"] != r["productRevision"]
            || p["url"]
                != base
                    .map(|b| json!(format!("{b}products/{}/", p["id"].as_str().unwrap())))
                    .unwrap_or(Value::Null)
        {
            return Err(fail());
        }
        let facts = p["publicFacts"].as_array().ok_or_else(fail)?;
        let mut ids = HashSet::new();
        if facts.is_empty() || facts.len() > 100 {
            return Err(fail());
        }
        for f in facts {
            let legacy = shape(
                f,
                &["id", "label", "value", "source", "visibility", "validUntil"],
            );
            let reviewed = shape(
                f,
                &[
                    "id",
                    "label",
                    "value",
                    "source",
                    "visibility",
                    "validUntil",
                    "reviewStatus",
                    "reviewedAt",
                    "updatedAt",
                ],
            );
            if !(legacy || reviewed)
                || !string(&f["id"], 100, true)
                || !ids.insert(f["id"].as_str().unwrap())
                || !string(&f["label"], 200, true)
                || !string(&f["value"], 3000, true)
                || !string(&f["source"], 2000, true)
                || f["visibility"] != "public"
                || (reviewed
                    && (f["reviewStatus"] != "approved"
                        || !timestamp(&f["reviewedAt"])
                        || !timestamp(&f["updatedAt"])))
                || !(f["validUntil"].is_null()
                    || f["validUntil"].as_str().is_some_and(|s| {
                        s.len() == 10 && NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
                    }))
            {
                return Err(fail());
            }
        }
        if !p["offer"].is_null() {
            let o = &p["offer"];
            if !shape(
                o,
                &[
                    "@type",
                    "price",
                    "priceCurrency",
                    "availability",
                    "url",
                    "priceValidUntil",
                ],
            ) || o["@type"] != "Offer"
                || ![
                    "price",
                    "priceCurrency",
                    "availability",
                    "url",
                    "priceValidUntil",
                ]
                .iter()
                .all(|k| string(&o[*k], 3000, true))
            {
                return Err(fail());
            }
        }
    }
    Ok((m, cat))
}
fn check(code: &str, status: &str, message: &str) -> Value {
    json!({"code":code,"status":status,"message":message})
}
fn report_budget(report: &Value) -> Result<()> {
    if report["checks"]
        .as_array()
        .is_some_and(|a| a.len() > MAX_CHECKS)
    {
        return Err("验收报告检查数量超过2048项上限".into());
    }
    if serde_json::to_vec(report)
        .map_err(|_| "验收报告序列化失败")?
        .len()
        > MAX_REPORT
    {
        return Err("验收报告超过2MiB上限".into());
    }
    Ok(())
}
fn finish(mut report: Value) -> Result<Value> {
    let mut evidence = report.clone();
    evidence.as_object_mut().unwrap().remove("checkedAt");
    report["reportFingerprint"] = json!(fingerprint(&evidence));
    report_budget(&report)?;
    Ok(report)
}
fn normalized(path: &str, content: &str) -> Result<Value> {
    if path == "catalog.json" || path == "release-manifest.json" {
        let mut value = unique_json(content)?;
        let object = value.as_object_mut().ok_or("网站 JSON 格式无效")?;
        object.remove("generatedAt");
        if path == "release-manifest.json" {
            object.remove("files");
        }
        Ok(value)
    } else {
        Ok(json!(content))
    }
}
pub(super) fn inspect(c: &Connection, b: &str, package: &Value) -> Result<Value> {
    brand(c, b)?;
    processing_budget(package)?;
    let s = &package["source"];
    let source = json!({"fileName":s["fileName"],"sha256":s["sha256"],"compressedBytes":s["compressedBytes"],"uncompressedBytes":s["uncompressedBytes"],"fileCount":s["fileCount"]});
    let mut report = json!({"format":"m-geo-site-inspection-v1","brandId":b,"checkedAt":now(),"source":source,"packageStatus":"blocked","checks":[],"previewOnly":null,"baseUrl":null,"products":[],"currentStatus":"unavailable","remoteStatus":"not-verified","publicationStatus":"not-published"});
    let mut checks = vec![check("scope", "warning", CAVEAT)];
    let parsed =
        file_map(package).and_then(|files| structure(&files).map(|(m, cat)| (files, m, cat)));
    let (files, m, cat) = match parsed {
        Ok(v) => v,
        Err(_) => {
            checks.push(check(
                "structure",
                "blocked",
                "文件、发布清单或商品目录不符合当前支持的格式、哈希或引用要求",
            ));
            report["checks"] = json!(checks);
            return finish(report);
        }
    };
    checks.push(check(
        "structure",
        "pass",
        "发布清单、商品目录、文件集合与原始内容哈希逐项一致",
    ));
    let audit = site_audit::audit_package(package)?;
    checks.extend(
        audit["checks"]
            .as_array()
            .ok_or("本地检查报告无效")?
            .iter()
            .cloned(),
    );
    if checks.iter().any(|c| c["status"] == "blocked") {
        report["checks"] = json!(checks);
        return finish(report);
    }
    report["packageStatus"] = json!("pass");
    report["previewOnly"] = m["previewOnly"].clone();
    report["baseUrl"] = m["baseUrl"].clone();
    let mut products = vec![];
    let mut selection = vec![];
    let mut changed = false;
    let mut unavailable = false;
    for r in m["revisions"].as_array().unwrap() {
        let pid = r["productId"].as_str().unwrap();
        let current = load(c, "m_products", b, pid).ok();
        let mut status = match &current {
            None => "unavailable",
            Some(p) if p["revision"] == r["productRevision"] && confirmed_public(p).is_ok() => {
                "match"
            }
            Some(_) => "changed",
        };
        let mut drafts = vec![];
        for d in r["drafts"].as_array().unwrap() {
            let did = d["id"].as_str().unwrap();
            let current_d = load(c, "m_drafts", b, did).ok();
            let ds = match &current_d {
                None => "unavailable",
                Some(cd)
                    if current.is_some()
                        && cd["productId"] == r["productId"]
                        && cd["revision"] == d["revision"]
                        && cd["sourceRevision"] == d["sourceRevision"]
                        && current_draft(c, b, cd, true).is_ok() =>
                {
                    "match"
                }
                Some(_) => "changed",
            };
            if ds == "unavailable" {
                status = "unavailable";
            } else if ds == "changed" && status != "unavailable" {
                status = "changed";
            }
            drafts.push(json!({"id":did,"packageRevision":d["revision"],"currentRevision":current_d.as_ref().and_then(|d|d["revision"].as_i64()),"status":ds}));
            selection.push(json!({"id":did,"expectedRevision":d["revision"]}));
        }
        changed |= status == "changed";
        unavailable |= status == "unavailable";
        let catalog = cat["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == r["productId"])
            .unwrap();
        products.push(json!({"productId":pid,"name":catalog["title"],"sku":catalog["sku"],"packageRevision":r["productRevision"],"currentRevision":current.as_ref().and_then(|p|p["revision"].as_i64()),"status":status,"detail":match status {"match"=>"当前品牌的商品与所选已审核稿件版本一致","changed"=>"当前商品或稿件版本、确认、审核或资料有效性已变化",_=>"当前品牌中至少一项引用不可用"},"drafts":drafts}));
    }
    if !changed && !unavailable {
        match site::prepare(
            c,
            b,
            &json!({"drafts":selection,"baseUrl":m["baseUrl"].as_str().unwrap_or("")}),
        ) {
            Ok(regenerated) => {
                let regenerated = file_map(&regenerated)?;
                let same = files.len() == regenerated.len()
                    && files.iter().all(|(path, content)| {
                        regenerated.get(path).is_some_and(|other| {
                            normalized(path, content).ok() == normalized(path, other).ok()
                        })
                    });
                if !same {
                    changed = true;
                    checks.push(check(
                        "current-content",
                        "warning",
                        "当前品牌重新生成的文件内容与历史包不同；历史文件未被改写",
                    ));
                }
            }
            Err(_) => {
                changed = true;
                checks.push(check(
                    "current-preparation",
                    "warning",
                    "当前资料的有效性、确认或审核条件已变化，不能按原选择重新生成",
                ));
            }
        }
    }
    let status = if unavailable {
        "unavailable"
    } else if changed {
        "changed"
    } else {
        "match"
    };
    checks.push(check(
        "current-versions",
        if status == "match" { "pass" } else { "warning" },
        match status {
            "match" => "当前品牌版本及重新生成内容与本地包一致",
            "changed" => "历史包结构仍可核对，当前品牌资料或文件内容已有变化",
            _ => "当前品牌缺少部分历史引用；未查询或展示其他品牌资料",
        },
    ));
    report["products"] = json!(products);
    report["currentStatus"] = json!(status);
    report["checks"] = json!(checks);
    finish(report)
}
fn md(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\r' | '\n' => out.push(' '),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!'
            | '|' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}
fn csv_cell(v: &Value) -> String {
    let s = v
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if v.is_null() {
                String::new()
            } else {
                v.to_string()
            }
        })
        .replace(['\r', '\n'], " ");
    let s = if s.starts_with(|c: char| c.is_whitespace() || c.is_control())
        || s.starts_with(['=', '+', '-', '@'])
    {
        format!("'{s}")
    } else {
        s
    };
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn csv_row(values: &[Value]) -> String {
    values.iter().map(csv_cell).collect::<Vec<_>>().join(",") + "\r\n"
}
pub(super) fn export(report: &Value) -> Result<Value> {
    report_budget(report)?;
    let checks = report["checks"].as_array().ok_or("验收报告格式无效")?;
    let products = report["products"].as_array().ok_or("验收报告格式无效")?;
    let mut markdown=format!("# 网站包只读验收报告\n\n{CAVEAT}\n\n文件：{}\n\nSHA256：{}\n\n本地一致性：{}；当前版本：{}；官网：未验证；发布：未发布。\n\n",md(report["source"]["fileName"].as_str().unwrap_or("")),md(report["source"]["sha256"].as_str().unwrap_or("")),md(report["packageStatus"].as_str().unwrap_or("")),md(report["currentStatus"].as_str().unwrap_or("")));
    markdown.push_str(&format!(
        "品牌 ID：{}\n\n核对时间：{}\n\n报告指纹：{}\n\n包内模式：{}\n\n预期官网地址：{}\n\n",
        md(report["brandId"].as_str().unwrap_or("")),
        md(report["checkedAt"].as_str().unwrap_or("")),
        md(report["reportFingerprint"].as_str().unwrap_or("")),
        match report["previewOnly"].as_bool() {
            Some(true) => "本地预览（noindex）",
            Some(false) => "已配置预期官网地址",
            None => "结构不支持，无法判定",
        },
        md(report["baseUrl"].as_str().unwrap_or("未配置"))
    ));
    let mut check_csv = "code,status,message,path\r\n".to_string();
    let mut product_csv="productId,name,sku,packageRevision,currentRevision,status,draftId,draftPackageRevision,draftCurrentRevision,draftStatus\r\n".to_string();
    for c in checks {
        markdown.push_str(&format!(
            "- {}：{}\n",
            md(c["status"].as_str().unwrap_or("")),
            md(c["message"].as_str().unwrap_or(""))
        ));
        check_csv.push_str(&csv_row(&[
            c["code"].clone(),
            c["status"].clone(),
            c["message"].clone(),
            c["path"].clone(),
        ]));
    }
    for p in products {
        markdown.push_str(&format!(
            "\n## {}\n\nSKU：{}；商品 ID：{}；包内版本：{}；当前版本：{}；状态：{}\n",
            md(p["name"].as_str().unwrap_or("")),
            md(p["sku"].as_str().unwrap_or("")),
            md(p["productId"].as_str().unwrap_or("")),
            md(&p["packageRevision"].to_string()),
            md(&p["currentRevision"].to_string()),
            md(p["status"].as_str().unwrap_or(""))
        ));
        for d in p["drafts"].as_array().ok_or("验收报告稿件格式无效")? {
            markdown.push_str(&format!(
                "\n- 稿件 {}：包内版本 {}；当前版本 {}；状态 {}\n",
                md(d["id"].as_str().unwrap_or("")),
                md(&d["packageRevision"].to_string()),
                md(&d["currentRevision"].to_string()),
                md(d["status"].as_str().unwrap_or(""))
            ));
            product_csv.push_str(&csv_row(&[
                p["productId"].clone(),
                p["name"].clone(),
                p["sku"].clone(),
                p["packageRevision"].clone(),
                p["currentRevision"].clone(),
                p["status"].clone(),
                d["id"].clone(),
                d["packageRevision"].clone(),
                d["currentRevision"].clone(),
                d["status"].clone(),
            ]));
        }
    }
    let inspection = serde_json::to_string_pretty(report)
        .map_err(|_| "验收报告序列化失败")?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    let files = json!([{"path":"report.md","content":markdown},{"path":"inspection.json","content":inspection},{"path":"checks.csv","content":check_csv},{"path":"product-versions.csv","content":product_csv}]);
    if files
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["content"].as_str().unwrap().len())
        .sum::<usize>()
        > MAX_REPORT
    {
        return Err("验收报告导出内容超过2MiB上限".into());
    }
    Ok(json!({"fileName":"M-GEO-网站包验收报告.zip","files":files}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(base: &str) -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构验收品牌"})).unwrap();
        let p = handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构杯 <script> [名称]","sku":"=FORMULA","category":"杯","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"f","label":"容量","value":"PUBLIC_FACT_SECRET","source":"SOURCE_SECRET","visibility":"public","validUntil":null},{"id":"hidden","label":"私人","value":"PRIVATE_SECRET","source":"私人","visibility":"private","validUntil":null}]})).unwrap();
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
        let d = handle(
            &mut c,
            "draft.review",
            json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
        )
        .unwrap();
        let input =
            json!({"baseUrl":base,"drafts":[{"id":d["id"],"expectedRevision":d["revision"]}]});
        let site = site::prepare(&c, b["id"].as_str().unwrap(), &input).unwrap();
        let package = json!({"source":{"fileName":"fixture.zip","sha256":"a".repeat(64),"compressedBytes":1200,"uncompressedBytes":site["files"].as_array().unwrap().iter().map(|f|f["content"].as_str().unwrap().len()).sum::<usize>(),"fileCount":site["files"].as_array().unwrap().len()},"files":site["files"]});
        (c, b["id"].as_str().unwrap().into(), package)
    }
    fn content_mut<'a>(p: &'a mut Value, path: &str) -> &'a mut Value {
        &mut p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == path)
            .unwrap()["content"]
    }
    fn document(p: &Value, path: &str) -> Value {
        serde_json::from_str(
            p["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["path"] == path)
                .unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
    }
    fn rehash(p: &mut Value) {
        let hashes:Vec<Value>=p["files"].as_array().unwrap().iter().filter(|f|f["path"]!="release-manifest.json").map(|f|json!({"path":f["path"],"sha256":hex::encode(Sha256::digest(f["content"].as_str().unwrap().as_bytes()))})).collect();
        let mut m = document(p, "release-manifest.json");
        m["files"] = json!(hashes);
        *content_mut(p, "release-manifest.json") = json!(m.to_string());
    }
    fn dump(c: &Connection) -> String {
        let tables: Vec<String> = c
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let mut out = String::new();
        for table in tables {
            let mut q = c
                .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
                .unwrap();
            let n = q.column_count();
            let mut rows = q.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                out.push_str(&table);
                for i in 0..n {
                    out.push_str(&format!("{:?}", row.get_ref(i).unwrap()));
                }
            }
        }
        out
    }
    fn edit_brand_data(p: &mut Value, brand: Option<Value>) {
        let path = p["files"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|f| {
                f["path"]
                    .as_str()
                    .filter(|path| path.starts_with("products/"))
            })
            .unwrap()
            .to_string();
        let html = content_mut(p, &path).as_str().unwrap().to_owned();
        let (prefix, rest) = html
            .split_once("<script type=\"application/ld+json\">")
            .unwrap();
        let (data, suffix) = rest.split_once("</script>").unwrap();
        let mut data: Value = serde_json::from_str(data).unwrap();
        match brand {
            Some(brand) => {
                data["brand"] = brand;
            }
            None => {
                data.as_object_mut().unwrap().remove("brand");
            }
        }
        *content_mut(p, &path) = json!(format!(
            "{prefix}<script type=\"application/ld+json\">{data}</script>{suffix}"
        ));
    }
    fn decoded_zip(p: &Value) -> (Vec<u8>, Value) {
        let bytes = crate::files::site_zip(p).unwrap();
        let decoded =
            crate::site_package::decode_site_package("fictional-brand.zip", &bytes).unwrap();
        (bytes, decoded)
    }
    fn assert_check(r: &Value, code: &str, status: &str) {
        assert!(
            r["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == code && c["status"] == status),
            "{code} {status}: {r}"
        );
    }
    #[test]
    fn decoded_brand_packages_keep_integrity_semantics_and_current_state_separate() {
        let (c, b, mut p) = fixture("");
        let before = dump(&c);
        let (bytes, normal) = decoded_zip(&p);
        let normal_snapshot = normal.clone();
        let r = inspect(&c, &b, &normal).unwrap();
        assert_eq!(r["packageStatus"], "pass");
        assert_eq!(r["currentStatus"], "match");
        assert_check(&r, "hashes", "pass");
        assert_check(&r, "product-brand", "warning");
        assert_eq!(normal, normal_snapshot);
        assert_eq!(bytes, crate::files::site_zip(&p).unwrap());

        // Reproduce the historical v1 workspace fallback without touching its catalog facts.
        edit_brand_data(
            &mut p,
            Some(json!({"@type":"Brand","name":"UNSUPPORTED_WORKSPACE_SECRET"})),
        );
        rehash(&mut p);
        let (wrong_bytes, wrong) = decoded_zip(&p);
        let original = wrong.clone();
        let r = inspect(&c, &b, &wrong).unwrap();
        assert_eq!(r["packageStatus"], "blocked");
        assert_eq!(r["currentStatus"], "unavailable");
        assert_check(&r, "structure", "pass");
        assert_check(&r, "hashes", "pass");
        assert_check(&r, "product-brand", "blocked");
        let exported = export(&r).unwrap();
        for secret in [
            "UNSUPPORTED_WORKSPACE_SECRET",
            "RAW_SECRET",
            "NOTE_SECRET",
            "PRIVATE_SECRET",
            "SOURCE_SECRET",
        ] {
            assert!(!r.to_string().contains(secret));
            assert!(!exported.to_string().contains(secret));
        }
        assert_eq!(wrong, original);
        assert_eq!(wrong_bytes, crate::files::site_zip(&p).unwrap());
        assert_eq!(before, dump(&c));
    }
    #[test]
    fn decoded_historical_brand_valid_at_generation_stays_semantically_valid() {
        let (c, b, mut p) = fixture("");
        let before = dump(&c);
        let mut m = document(&p, "release-manifest.json");
        m["generatedAt"] = json!("2000-01-01T23:30:00-12:00");
        *content_mut(&mut p, "release-manifest.json") = json!(m.to_string());
        let mut cat = document(&p, "catalog.json");
        cat["generatedAt"] = m["generatedAt"].clone();
        cat["products"][0]["publicFacts"].as_array_mut().unwrap().push(json!({"id":"catalog:brand","label":"商品品牌","value":"虚构历史商品品牌","source":"HISTORICAL_SOURCE_SECRET","visibility":"public","validUntil":"2000-01-01"}));
        *content_mut(&mut p, "catalog.json") = json!(cat.to_string());
        edit_brand_data(
            &mut p,
            Some(json!({"@type":"Brand","name":"虚构历史商品品牌"})),
        );
        rehash(&mut p);
        let (bytes, decoded) = decoded_zip(&p);
        let original = decoded.clone();
        let r = inspect(&c, &b, &decoded).unwrap();
        assert_eq!(r["packageStatus"], "pass");
        assert_eq!(r["currentStatus"], "changed");
        assert_check(&r, "structure", "pass");
        assert_check(&r, "hashes", "pass");
        assert_check(&r, "product-brand", "pass");
        assert!(!r.to_string().contains("HISTORICAL_SOURCE_SECRET"));
        assert!(!export(&r)
            .unwrap()
            .to_string()
            .contains("HISTORICAL_SOURCE_SECRET"));
        assert_eq!(decoded, original);
        assert_eq!(bytes, crate::files::site_zip(&p).unwrap());
        assert_eq!(before, dump(&c));
    }
    #[test]
    fn generated_preview_and_domain_are_local_pass_readonly_and_private() {
        for base in ["", "https://example.invalid/shop/"] {
            let (c, b, p) = fixture(base);
            let before = dump(&c);
            let original = p.clone();
            let r = inspect(&c, &b, &p).unwrap();
            assert_eq!(r["packageStatus"], "pass", "{r}");
            assert_eq!(r["currentStatus"], "match", "{r}");
            assert_eq!(r["previewOnly"], base.is_empty());
            assert_eq!(r["remoteStatus"], "not-verified");
            assert_eq!(r["publicationStatus"], "not-published");
            let output = export(&r).unwrap();
            assert_eq!(output["files"].as_array().unwrap().len(), 4);
            for secret in [
                "RAW_SECRET",
                "NOTE_SECRET",
                "PRIVATE_SECRET",
                "PUBLIC_FACT_SECRET",
                "SOURCE_SECRET",
            ] {
                assert!(!r.to_string().contains(secret));
                assert!(!output.to_string().contains(secret));
            }
            assert_eq!(before, dump(&c));
            assert_eq!(p, original);
            assert_eq!(
                r["reportFingerprint"],
                inspect(&c, &b, &p).unwrap()["reportFingerprint"]
            );
        }
    }
    #[test]
    fn archived_product_and_draft_edits_are_changed_without_invalidating_package() {
        for kind in ["product", "draft", "brand", "review"] {
            let (c, b, p) = fixture("");
            let m = document(&p, "release-manifest.json");
            let pid = m["revisions"][0]["productId"].as_str().unwrap();
            let did = m["revisions"][0]["drafts"][0]["id"].as_str().unwrap();
            match kind {
                "product" => {
                    c.execute(
                        "UPDATE m_products SET data=json_set(data,'$.revision',2) WHERE id=?1",
                        [pid],
                    )
                    .unwrap();
                }
                "draft" => {
                    c.execute(
                        "UPDATE m_drafts SET data=json_set(data,'$.revision',99) WHERE id=?1",
                        [did],
                    )
                    .unwrap();
                }
                "review" => {
                    c.execute(
                        "UPDATE m_drafts SET data=json_set(data,'$.reviewedAt',NULL) WHERE id=?1",
                        [did],
                    )
                    .unwrap();
                }
                _ => {
                    c.execute(
                        "UPDATE m_brands SET data=json_set(data,'$.name','改名') WHERE id=?1",
                        [&b],
                    )
                    .unwrap();
                }
            }
            let r = inspect(&c, &b, &p).unwrap();
            assert_eq!(r["packageStatus"], "pass");
            assert_eq!(r["currentStatus"], "changed", "{kind}: {r}");
        }
    }
    #[test]
    fn wrong_brand_is_unavailable_without_current_other_brand_details() {
        let (mut c, _, p) = fixture("");
        let b = handle(&mut c, "brand.create", json!({"name":"另一品牌"})).unwrap();
        let r = inspect(&c, b["id"].as_str().unwrap(), &p).unwrap();
        assert_eq!(r["packageStatus"], "pass");
        assert_eq!(r["currentStatus"], "unavailable");
        assert!(r["products"][0]["currentRevision"].is_null());
        assert!(!r.to_string().contains("虚构验收品牌"));
        assert!(inspect(&c, &Uuid::new_v4().to_string(), &p).is_err());
    }
    #[test]
    fn tampered_missing_extra_duplicate_json_and_wrongformat_are_blocked() {
        let (c, b, p) = fixture("");
        for case in [
            "tamper",
            "missing",
            "extra",
            "duplicate",
            "nestedduplicate",
            "format",
            "revision",
            "reference",
            "path",
            "duplicatefile",
        ] {
            let mut altered = p.clone();
            match case {
                "tamper" => {
                    *content_mut(&mut altered, "index.html") = json!("tampered");
                }
                "missing" => {
                    altered["files"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|f| f["path"] != "index.html");
                }
                "extra" => {
                    altered["files"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"path":"extra.txt","content":"extra"}));
                    rehash(&mut altered);
                }
                "duplicate" => {
                    let s = content_mut(&mut altered, "catalog.json")
                        .as_str()
                        .unwrap()
                        .replacen('{', "{\"format\":\"ignored\",", 1);
                    *content_mut(&mut altered, "catalog.json") = json!(s);
                    rehash(&mut altered);
                }
                "nestedduplicate" => {
                    let s = content_mut(&mut altered, "catalog.json")
                        .as_str()
                        .unwrap()
                        .replacen(
                            "\"publicFacts\": [",
                            "\"publicFacts\": [{\"id\":\"first\",\"id\":\"second\"},",
                            1,
                        );
                    *content_mut(&mut altered, "catalog.json") = json!(s);
                    rehash(&mut altered);
                }
                "duplicatefile" => {
                    let f = altered["files"][0].clone();
                    altered["files"].as_array_mut().unwrap().push(f);
                }
                "path" => {
                    altered["files"][0]["path"] = json!("../private");
                }
                _ => {
                    let mut m = document(&altered, "release-manifest.json");
                    if case == "format" {
                        m["format"] = json!("future");
                    } else if case == "revision" {
                        m["revisions"][0]["sourceRevision"] = json!(99);
                    } else {
                        m["revisions"][0]["productId"] = json!(Uuid::new_v4().to_string());
                    }
                    *content_mut(&mut altered, "release-manifest.json") = json!(m.to_string());
                }
            }
            let r = inspect(&c, &b, &altered).unwrap();
            assert_eq!(r["packageStatus"], "blocked", "{case}: {r}");
            assert_eq!(r["currentStatus"], "unavailable");
            assert_eq!(r["products"], json!([]));
        }
    }
    #[test]
    fn rehashed_body_change_is_locally_consistent_but_current_changed() {
        let (c, b, mut p) = fixture("");
        let s = content_mut(&mut p, "index.html")
            .as_str()
            .unwrap()
            .replace("</body>", "<p>改动</p></body>");
        *content_mut(&mut p, "index.html") = json!(s);
        rehash(&mut p);
        let r = inspect(&c, &b, &p).unwrap();
        assert_eq!(r["packageStatus"], "pass");
        assert_eq!(r["currentStatus"], "changed");
    }
    #[test]
    fn regeneration_ignores_only_generated_times_and_the_derived_digest_list() {
        let (c, b, mut p) = fixture("");
        for path in ["catalog.json", "release-manifest.json"] {
            let mut d = document(&p, path);
            d["generatedAt"] = json!("2000-01-01T00:00:00Z");
            *content_mut(&mut p, path) = json!(serde_json::to_string_pretty(&d).unwrap());
        }
        rehash(&mut p);
        let r = inspect(&c, &b, &p).unwrap();
        assert_eq!(r["currentStatus"], "match", "{r}");
    }
    #[test]
    fn processing_and_report_budgets_reject_without_truncation() {
        let (c, b, mut p) = fixture("");
        let s = content_mut(&mut p, "index.html")
            .as_str()
            .unwrap()
            .to_owned()
            + &"href=\"#\"".repeat(2049);
        *content_mut(&mut p, "index.html") = json!(s);
        rehash(&mut p);
        assert!(inspect(&c, &b, &p).unwrap_err().contains("2048"));
    }
    #[test]
    fn exact_manifest_and_catalog_types_and_reference_uniqueness_are_required() {
        let (c, b, p) = fixture("");
        for case in [
            "unknown-key",
            "bad-time",
            "preview-type",
            "base-type",
            "zero-revision",
            "float-revision",
            "uuid",
            "duplicate-draft",
            "hash-case",
            "hash-duplicate",
            "catalog-id",
            "catalog-sku",
            "catalog-time",
            "catalog-fact",
            "catalog-extra",
        ] {
            let mut altered = p.clone();
            let mut m = document(&altered, "release-manifest.json");
            let mut cat = document(&altered, "catalog.json");
            match case {
                "unknown-key" => m["unknown"] = json!(true),
                "bad-time" => m["generatedAt"] = json!("yesterday"),
                "preview-type" => m["previewOnly"] = json!("true"),
                "base-type" => m["baseUrl"] = json!(false),
                "zero-revision" => m["revisions"][0]["productRevision"] = json!(0),
                "float-revision" => m["revisions"][0]["productRevision"] = json!(1.0),
                "uuid" => m["revisions"][0]["productId"] = json!("not-uuid"),
                "duplicate-draft" => {
                    let d = m["revisions"][0]["drafts"][0].clone();
                    m["revisions"][0]["drafts"].as_array_mut().unwrap().push(d);
                }
                "hash-case" => m["files"][0]["sha256"] = json!("A".repeat(64)),
                "hash-duplicate" => {
                    let d = m["files"][0].clone();
                    m["files"].as_array_mut().unwrap().push(d);
                }
                "catalog-id" => cat["products"][0]["id"] = json!(Uuid::new_v4().to_string()),
                "catalog-sku" => cat["products"][0]["sku"] = json!(""),
                "catalog-time" => cat["generatedAt"] = Value::Null,
                "catalog-fact" => {
                    cat["products"][0]["publicFacts"][0]["visibility"] = json!("private")
                }
                _ => cat["products"][0]["extra"] = json!(true),
            }
            *content_mut(&mut altered, "release-manifest.json") = json!(m.to_string());
            if case.starts_with("catalog-") {
                *content_mut(&mut altered, "catalog.json") = json!(cat.to_string());
                rehash(&mut altered);
            }
            let before = dump(&c);
            let r = inspect(&c, &b, &altered).unwrap();
            assert_eq!(r["packageStatus"], "blocked", "{case}: {r}");
            assert_eq!(r["products"], json!([]));
            assert_eq!(before, dump(&c));
        }
    }
    #[test]
    fn missing_current_draft_is_unavailable_and_report_limits_are_errors() {
        let (c, b, p) = fixture("");
        let m = document(&p, "release-manifest.json");
        let did = m["revisions"][0]["drafts"][0]["id"].as_str().unwrap();
        c.execute("DELETE FROM m_drafts WHERE id=?1", [did])
            .unwrap();
        let r = inspect(&c, &b, &p).unwrap();
        assert_eq!(r["packageStatus"], "pass");
        assert_eq!(r["currentStatus"], "unavailable");
        assert!(r["products"][0]["drafts"][0]["currentRevision"].is_null());
        let mut oversized = r.clone();
        oversized["checks"] = json!(vec![
            json!({"code":"x","status":"pass","message":"x"});
            2049
        ]);
        assert!(export(&oversized).is_err());
        oversized = r;
        oversized["source"]["fileName"] = json!("x".repeat(2 * 1024 * 1024));
        assert!(export(&oversized).is_err());
    }
    #[test]
    fn markdown_report_identifies_evidence_mode_and_draft_versions() {
        let (c, b, p) = fixture("https://example.invalid/shop/");
        let r = inspect(&c, &b, &p).unwrap();
        let out = export(&r).unwrap();
        let md = out["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "report.md")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(md.contains("https://example.invalid/shop/"));
        assert!(md.contains(r["reportFingerprint"].as_str().unwrap()));
        assert!(md
            .replace("\\-", "-")
            .contains(r["products"][0]["drafts"][0]["id"].as_str().unwrap()));
        assert!(md.contains("预期官网地址"));
        assert!(md.contains("稿件"));
    }
    #[test]
    fn report_exports_escape_markup_and_spreadsheet_formula() {
        let (c, b, p) = fixture("");
        let r = inspect(&c, &b, &p).unwrap();
        let out = export(&r).unwrap();
        let files = out["files"].as_array().unwrap();
        let md = files.iter().find(|f| f["path"] == "report.md").unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(!md.contains("<script>"));
        assert!(!md.contains("[名称]"));
        let csv = files
            .iter()
            .find(|f| f["path"] == "product-versions.csv")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(csv.contains("'=FORMULA"));
    }
}
