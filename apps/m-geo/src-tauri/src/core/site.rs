//! A local, review-gated static website. No network requests or publication side effects.
use super::*;
use std::collections::BTreeMap;
fn file(path: &str, content: String) -> Value {
    json!({"path":path,"content":content})
}
fn paragraphs(body: &str) -> String {
    body.lines()
        .filter(|s| !s.trim().is_empty())
        .map(|line| {
            if let Some(s) = line.strip_prefix("### ") {
                format!("<h3>{}</h3>", escape(s))
            } else if let Some(s) = line.strip_prefix("## ") {
                format!("<h2>{}</h2>", escape(s))
            } else if let Some(s) = line.strip_prefix("# ") {
                format!("<h2>{}</h2>", escape(s))
            } else {
                format!("<p>{}</p>", escape(line))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn page(
    title: &str,
    description: &str,
    canonical: Option<&str>,
    body: &str,
    structured: Option<Value>,
) -> String {
    let meta = canonical
        .map(|url| {
            format!(
                "<link rel=\"canonical\" href=\"{}\"><meta property=\"og:url\" content=\"{}\">",
                escape(url),
                escape(url)
            )
        })
        .unwrap_or_else(|| "<meta name=\"robots\" content=\"noindex,nofollow\">".into());
    let data = structured
        .map(|v| {
            format!(
                "<script type=\"application/ld+json\">{}</script>",
                v.to_string()
                    .replace('<', "\\u003c")
                    .replace('>', "\\u003e")
                    .replace('&', "\\u0026")
            )
        })
        .unwrap_or_default();
    format!("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>{}</title><meta name=\"description\" content=\"{}\"><meta property=\"og:title\" content=\"{}\"><meta property=\"og:description\" content=\"{}\">{}{}<style>body{{max-width:880px;margin:40px auto;padding:0 24px;font:17px/1.8 system-ui;color:#1c2940;background:#fff;overflow-wrap:anywhere}}a{{color:#195fca}}article,section{{border-top:1px solid #ddd;padding:24px 0}}dt{{font-weight:700}}dd{{margin:0 0 16px}}small{{color:#526174}}h1{{line-height:1.3}}</style></head><body>{}</body></html>",escape(title),escape(description),escape(title),escape(description),meta,data,body)
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let br = brand(c, b)?;
    let base = text(p, "baseUrl", 2000, false)?;
    let base = if base.is_empty() {
        None
    } else {
        web_url(&base, true)?;
        let mut u = url::Url::parse(&base).map_err(|_| "网站地址无效")?;
        if u.scheme() != "https" || u.query().is_some() || u.fragment().is_some() {
            return Err("官网地址需要 HTTPS，且不含查询参数或片段".into());
        }
        if !u.path().ends_with('/') {
            u.set_path(&format!("{}/", u.path()));
        }
        Some(u.to_string())
    };
    let selected = p["drafts"].as_array().ok_or("请选择已审核内容")?;
    if selected.is_empty() || selected.len() > 50 {
        return Err("一次导出1至50份已审核内容".into());
    }
    let mut grouped: BTreeMap<String, (Value, Vec<Value>)> = BTreeMap::new();
    let mut ids = HashSet::new();
    for row in selected {
        let did = guid(row, "id")?;
        if !ids.insert(did.clone()) {
            return Err("不能重复选择同一稿件".into());
        }
        let d = load(c, "m_drafts", b, &did)?;
        revision(&d, row)?;
        current_draft(c, b, &d, true)?;
        let pid = guid(&d, "productId")?;
        let prod = load(c, "m_products", b, &pid)?;
        grouped.entry(pid).or_insert((prod, vec![])).1.push(d);
    }
    let mut files = vec![];
    let mut completeness = vec![];
    let mut revisions = vec![];
    let mut catalog = vec![];
    let mut links = String::new();
    let mut urls = vec![];
    for (pid, (prod, drafts)) in grouped {
        let fs = confirmed_public(&prod)?;
        completeness.push((pid.clone(), fs.clone()));
        revisions.push(json!({"productId":pid,"productRevision":prod["revision"],"sourceRevision":prod["revision"],"drafts":drafts.iter().map(|d|json!({"id":d["id"],"revision":d["revision"],"sourceRevision":d["sourceRevision"]})).collect::<Vec<_>>()}));
        let path = format!("products/{pid}/index.html");
        let canonical = base.as_ref().map(|b| format!("{b}products/{pid}/"));
        let name = prod["name"].as_str().unwrap_or("");
        let description = format!(
            "{}。{} {}",
            name,
            commerce::get(&fs, "audience").unwrap_or(""),
            commerce::get(&fs, "scenario").unwrap_or("")
        );
        let description: String = description.chars().take(200).collect();
        let mut data = json!({"@context":"https://schema.org","@type":"Product","name":name,"sku":prod["sku"],"description":description});
        if let Some(brand) = catalog::public_brand(&fs, Utc::now().date_naive()) {
            data["brand"] = json!({"@type":"Brand","name":brand});
        }
        if let Some(url) = &canonical {
            data["url"] = json!(url);
            urls.push(url.clone());
        }
        if let Some(img) = commerce::get(&fs, "image") {
            data["image"] = json!(img);
        }
        if let (Some(price), Some(currency), Some(stock), Some(purchase)) = (
            commerce::get(&fs, "price"),
            commerce::get(&fs, "currency"),
            commerce::get(&fs, "availability"),
            commerce::get(&fs, "purchase"),
        ) {
            if stock != "Unknown" {
                let expiry = fs
                    .iter()
                    .find(|f| f["id"] == "commerce:price")
                    .and_then(|f| f["validUntil"].as_str())
                    .ok_or("价格缺少有效期")?;
                data["offers"] = json!({"@type":"Offer","price":price,"priceCurrency":currency,"availability":format!("https://schema.org/{stock}"),"url":purchase,"priceValidUntil":expiry});
            }
        }
        let mut body=format!("<nav><a href=\"../../index.html\">全部商品</a></nav><h1>{}</h1><p>{}</p><section><h2>商品购买信息与依据</h2><dl>",escape(name),escape(&description));
        for f in &fs {
            body.push_str(&format!(
                "<dt>{}</dt><dd>{}<br><small>依据：{}；有效至：{}</small></dd>",
                escape(f["label"].as_str().unwrap_or("")),
                escape(f["value"].as_str().unwrap_or("")),
                escape(f["source"].as_str().unwrap_or("")),
                escape(f["validUntil"].as_str().unwrap_or("未设定"))
            ));
        }
        body.push_str("</dl></section>");
        if let Some(purchase) = commerce::get(&fs, "purchase") {
            body.push_str(&format!(
                "<p><a href=\"{}\" rel=\"noreferrer\">查看商品购买页面</a></p>",
                escape(purchase)
            ));
        }
        for d in &drafts {
            body.push_str(&format!(
                "<article><h2>{}</h2>{}<small>稿件版本 {} · 资料版本 {}</small></article>",
                escape(d["title"].as_str().unwrap_or("")),
                paragraphs(d["body"].as_str().unwrap_or("")),
                d["revision"],
                d["sourceRevision"]
            ));
        }
        body.push_str(
            "<footer>价格和库存以购买页面当前信息为准。未列出的属性需向商家核实。</footer>",
        );
        files.push(file(
            &path,
            page(
                name,
                &description,
                canonical.as_deref(),
                &body,
                Some(data.clone()),
            ),
        ));
        links.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>",
            escape(&path),
            escape(name)
        ));
        catalog.push(json!({"id":pid,"sku":prod["sku"],"title":name,"url":canonical,"purchaseUrl":commerce::get(&fs,"purchase"),"imageUrl":commerce::get(&fs,"image"),"offer":data.get("offers"),"publicFacts":fs,"sourceRevision":prod["revision"]}));
    }
    let title = format!("{} · 商品资料", br["name"].as_str().unwrap_or(""));
    files.push(file(
        "index.html",
        page(
            &title,
            "商品购买信息、使用限制、常见问题与事实依据",
            base.as_deref(),
            &format!("<h1>{}</h1><ul>{}</ul>", escape(&title), links),
            None,
        ),
    ));
    if let Some(base) = &base {
        urls.insert(0, base.clone());
        files.push(file("sitemap.xml",format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{}</urlset>",urls.iter().map(|u|format!("<url><loc>{}</loc></url>",escape(u))).collect::<Vec<_>>().join(""))));
        files.push(file(
            "robots.txt",
            format!("User-agent: *\nAllow: /\nSitemap: {base}sitemap.xml\n"),
        ));
    } else {
        files.push(file("robots.txt", "User-agent: *\nDisallow: /\n".into()));
    }
    files.push(file("catalog.json",serde_json::to_string_pretty(&json!({"format":"m-geo-generic-catalog-v1","generatedAt":now(),"status":"local-preparation-not-submitted","products":catalog})).map_err(|_|"商品目录序列化失败")?));
    files.push(file("README.txt",format!("M-GEO 本地网站内容包\n状态：{}\n未上传、未公开发布、未提交到任何 AI 平台。\n1. 核对每项商品信息、来源、价格与库存有效期。\n2. 提供真实官网地址后重新导出；保留目录层级部署到指定路径。\n3. 人工批准发布后检查公开页面、canonical、sitemap、robots 和结构化数据。\n4. 登记真实发布网址，按固定问题复测。\nrobots.txt 必须部署或合并到网站根目录；子目录导出不代表根目录 robots 策略。\n catalog.json 与 catalog.csv 是通用商品目录准备文件；平台 Feed 需要商家资格、具体字段适配和独立提交验证。\n订单、流量、销量和推荐效果尚未接入；不得据此推断增长。\n",if base.is_some(){"已配置预期官网地址，待人工发布"}else{"本地预览，noindex；缺少官网地址"})));
    files.push(file("catalog.csv", readiness::catalog_csv(&catalog)));
    let readiness = readiness::evaluate(base.as_deref(), &completeness);
    let digests: Vec<Value> = files.iter().map(|f|json!({"path":f["path"],"sha256":hex::encode(Sha256::digest(f["content"].as_str().unwrap_or("").as_bytes()))})).collect();
    files.push(file("release-manifest.json", serde_json::to_string_pretty(&json!({"format":"m-geo-release-manifest-v1","generatedAt":now(),"publicationStatus":"not-published","previewOnly":base.is_none(),"baseUrl":base,"revisions":revisions,"files":digests,"expectedPublicUrls":urls,"catalogStatus":"generic-not-platform-qualified-not-submitted","manualPostPublicationChecks":["人工核对公开页面及购买信息","核验 canonical 与 sitemap 地址","robots.txt 必须部署或合并到网站根目录；子目录文件不代表根目录 robots 策略","人工检查结构化数据与公开可访问性；不代表收录或平台资格"]})).map_err(|_|"清单序列化失败")?));
    let size: usize = files
        .iter()
        .map(|f| f["content"].as_str().map(str::len).unwrap_or(0))
        .sum();
    enforce_package_size(size)?;
    Ok(
        json!({"fileName":"M-GEO-商品网站内容包.zip","previewOnly":base.is_none(),"productCount":catalog.len(),"readiness":readiness,"publicationStatus":"not-published","files":files}),
    )
}
fn enforce_package_size(size: usize) -> Result<()> {
    if size > 16 * 1024 * 1024 {
        return Err("网站包超过16MB，请减少选中的稿件".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn brand_fact(value: &str) -> Value {
        json!({"id":"catalog:brand","label":"商品品牌","value":value,"source":"Fictional public source","visibility":"public","validUntil":null})
    }
    fn reviewed_fixture(mut facts: Vec<Value>) -> (Connection, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(
            &mut c,
            "brand.create",
            json!({"name":"Fictional Multi-brand Store"}),
        )
        .unwrap();
        facts.push(json!({"id":"capacity","label":"容量","value":"1L","source":"Fictional capacity source","visibility":"public","validUntil":null}));
        let p = handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"Fictional Cup","sku":"CUP-A","category":"Cup","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":facts})).unwrap();
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
        (
            c,
            json!({"brandId":b["id"],"baseUrl":"","drafts":[{"id":d["id"],"expectedRevision":d["revision"]}]}),
        )
    }
    fn structured_product(package: &Value) -> Value {
        let html = package["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
            .unwrap()["content"]
            .as_str()
            .unwrap();
        let data = html
            .split("<script type=\"application/ld+json\">")
            .nth(1)
            .unwrap()
            .split("</script>")
            .next()
            .unwrap();
        serde_json::from_str(data).unwrap()
    }
    #[test]
    fn website_product_brand_uses_public_manufacturer_and_keeps_workspace_index() {
        let (c, input) = reviewed_fixture(vec![brand_fact("  Fictional Cup Maker  ")]);
        let package = prepare(&c, input["brandId"].as_str().unwrap(), &input).unwrap();
        let product = structured_product(&package);
        assert_eq!(
            product["brand"],
            json!({"@type":"Brand","name":"Fictional Cup Maker"})
        );
        assert_eq!(product["sku"], "CUP-A");
        assert_eq!(product["name"], "Fictional Cup");
        let index = package["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "index.html")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(index.contains("<title>Fictional Multi-brand Store · 商品资料</title>"));
        assert!(package["readiness"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == "brand" && c["status"] == "pass"));
    }
    #[test]
    fn website_omits_unusable_brand_and_all_private_sentinels() {
        let mut private = brand_fact("PRIVATE_BRAND_SECRET");
        private["visibility"] = json!("private");
        private["source"] = json!("PRIVATE_SOURCE_SECRET");
        for facts in [vec![], vec![private], vec![brand_fact(&"X".repeat(71))]] {
            let (c, input) = reviewed_fixture(facts);
            let package = prepare(&c, input["brandId"].as_str().unwrap(), &input).unwrap();
            assert!(structured_product(&package).get("brand").is_none());
            assert!(package["readiness"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["code"] == "brand" && c["status"] == "warning"));
            for secret in [
                "PRIVATE_BRAND_SECRET",
                "PRIVATE_SOURCE_SECRET",
                "RAW_SECRET",
                "NOTE_SECRET",
            ] {
                assert!(!package.to_string().contains(secret));
            }
        }
    }
    #[test]
    fn website_valid_brand_still_requires_current_draft_review() {
        let (mut c, mut input) = reviewed_fixture(vec![brand_fact("Fictional Cup Maker")]);
        let b = input["brandId"].as_str().unwrap().to_owned();
        let d = load(
            &c,
            "m_drafts",
            &b,
            input["drafts"][0]["id"].as_str().unwrap(),
        )
        .unwrap();
        let updated = handle(&mut c,"draft.save",json!({"brandId":b,"id":d["id"],"expectedRevision":d["revision"],"title":d["title"],"body":"Fictional edited description"})).unwrap();
        assert!(prepare(&c, &b, &input).is_err());
        input["drafts"][0]["expectedRevision"] = updated["revision"].clone();
        assert!(prepare(&c, &b, &input).is_err());
        handle(
            &mut c,
            "draft.review",
            json!({"brandId":b,"id":updated["id"],"expectedRevision":updated["revision"]}),
        )
        .unwrap();
        assert!(prepare(&c, &b, &input).is_ok());
    }
    #[test]
    fn website_brand_change_requires_reconfirmation_and_new_draft() {
        let (mut c, input) = reviewed_fixture(vec![brand_fact("Fictional Cup Maker")]);
        let b = input["brandId"].as_str().unwrap().to_owned();
        let d = load(
            &c,
            "m_drafts",
            &b,
            input["drafts"][0]["id"].as_str().unwrap(),
        )
        .unwrap();
        let mut product = load(&c, "m_products", &b, d["productId"].as_str().unwrap()).unwrap();
        product["expectedRevision"] = product["revision"].clone();
        product["facts"][0]["value"] = json!("Fictional Other Maker");
        let changed = handle(&mut c, "product.save", product).unwrap();
        assert!(prepare(&c, &b, &input).is_err());
        handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":changed["id"],"expectedRevision":changed["revision"]}),
        )
        .unwrap();
        assert!(prepare(&c, &b, &input).is_err());
    }
    #[test]
    fn website_expired_public_brand_is_rejected_before_optional_projection() {
        let (c, input) = reviewed_fixture(vec![brand_fact("Fictional Cup Maker")]);
        let b = input["brandId"].as_str().unwrap();
        let d = load(
            &c,
            "m_drafts",
            b,
            input["drafts"][0]["id"].as_str().unwrap(),
        )
        .unwrap();
        let mut product = load(&c, "m_products", b, d["productId"].as_str().unwrap()).unwrap();
        // Model the passage of time for an already-confirmed product.
        product["facts"][0]["validUntil"] = json!("2000-01-01");
        c.execute(
            "UPDATE m_products SET data=?1 WHERE id=?2",
            params![product.to_string(), product["id"].as_str()],
        )
        .unwrap();
        assert!(prepare(&c, b, &input).is_err());
    }
    #[test]
    fn package_size_boundary_includes_all_additions() {
        assert!(enforce_package_size(16 * 1024 * 1024).is_ok());
        assert!(enforce_package_size(16 * 1024 * 1024 + 1).is_err());
    }
    #[test]
    fn manifest_hashes_every_original_file_and_omits_private_sources() {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构验收"})).unwrap();
        let p=handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构杯","sku":"=TEST","category":"杯","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"f","label":"容量","value":"1L","source":"虚构依据","visibility":"public","validUntil":null}]})).unwrap();
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
        let mut input = json!({"brandId":b["id"],"baseUrl":"","drafts":[{"id":d["id"],"expectedRevision":d["revision"]}]});
        for base in ["", "https://example.com/shop/"] {
            input["baseUrl"] = json!(base);
            let package = prepare(&c, b["id"].as_str().unwrap(), &input).unwrap();
            assert_eq!(
                package["readiness"]["blockers"],
                if base.is_empty() { 1 } else { 0 }
            );
            let files = package["files"].as_array().unwrap();
            let manifest: Value = serde_json::from_str(
                files
                    .iter()
                    .find(|f| f["path"] == "release-manifest.json")
                    .unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["publicationStatus"], "not-published");
            assert_eq!(manifest["files"].as_array().unwrap().len(), files.len() - 1);
            for f in files
                .iter()
                .filter(|f| f["path"] != "release-manifest.json")
            {
                let item = manifest["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|i| i["path"] == f["path"])
                    .unwrap();
                assert_eq!(
                    item["sha256"],
                    hex::encode(Sha256::digest(f["content"].as_str().unwrap().as_bytes()))
                );
            }
            assert!(!package.to_string().contains("RAW_SECRET"));
            assert!(!package.to_string().contains("NOTE_SECRET"));
        }
        input["drafts"][0]["expectedRevision"] = json!(99);
        assert!(prepare(&c, b["id"].as_str().unwrap(), &input).is_err());
    }
}
