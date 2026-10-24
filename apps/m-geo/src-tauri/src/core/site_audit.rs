//! Offline semantic checks. Nothing here requests or publishes a URL.
use super::*;
use std::collections::BTreeMap;

pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    audit_package(&site::prepare(c, b, p)?)
}
fn attr<'a>(html: &'a str, prefix: &str) -> Option<&'a str> {
    html.split_once(prefix)?.1.split_once('"').map(|x| x.0)
}
fn xml_urls(text: &str) -> Result<Vec<String>> {
    use quick_xml::{events::Event, Reader};
    let mut reader = Reader::from_str(text);
    let mut urls = vec![];
    let mut stack: Vec<Vec<u8>> = vec![];
    let mut root_seen = false;
    let mut root_closed = false;
    let mut loc_text = String::new();
    let mut loc_count = 0;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_vec();
                match stack.len() {
                    0 if !root_seen && name == b"urlset" => {
                        let attrs = e
                            .attributes()
                            .collect::<std::result::Result<Vec<_>, _>>()
                            .map_err(|_| "sitemap 属性无效")?;
                        if attrs.len() != 1
                            || attrs[0].key.as_ref() != b"xmlns"
                            || attrs[0].value.as_ref()
                                != b"http://www.sitemaps.org/schemas/sitemap/0.9"
                        {
                            return Err("sitemap 命名空间无效".into());
                        }
                        root_seen = true;
                    }
                    1 if name == b"url" && !root_closed => {
                        loc_count = 0;
                    }
                    2 if name == b"loc" && loc_count == 0 => {
                        loc_text.clear();
                        loc_count += 1;
                    }
                    _ => return Err("sitemap 层级无效".into()),
                }
                if !stack.is_empty() && e.attributes().next().is_some() {
                    return Err("sitemap 子元素属性无效".into());
                }
                stack.push(name);
            }
            Ok(Event::Text(e)) => {
                let value = e.unescape().map_err(|_| "sitemap 编码无效")?;
                if stack.len() == 3 {
                    loc_text.push_str(&value);
                } else if !value.trim().is_empty() {
                    return Err("sitemap 文本位置无效".into());
                }
            }
            Ok(Event::End(e)) => {
                let name = stack.pop().ok_or("sitemap 结束标签无效")?;
                if name.as_slice() != e.name().as_ref() {
                    return Err("sitemap 结束标签不匹配".into());
                }
                if name == b"loc" {
                    if loc_text.trim().is_empty() {
                        return Err("sitemap 地址为空".into());
                    }
                    urls.push(loc_text.clone());
                } else if name == b"url" && loc_count != 1 {
                    return Err("sitemap 地址缺失".into());
                } else if name == b"urlset" {
                    root_closed = true;
                }
            }
            Ok(Event::Decl(_)) if !root_seen => (),
            Ok(Event::Comment(_)) => (),
            Ok(Event::Eof) if root_seen && root_closed && stack.is_empty() => break,
            _ => return Err("sitemap XML 无效或未闭合".into()),
        }
    }
    Ok(urls)
}
fn robots_noindex(html: &str) -> bool {
    let head = html.split_once("</head>").map(|(h, _)| h).unwrap_or(html);
    head.split("<meta ")
        .skip(1)
        .filter_map(|s| s.split_once('>').map(|(tag, _)| tag))
        .any(|tag| {
            attr(tag, "name=\"").is_some_and(|name| name.eq_ignore_ascii_case("robots"))
                && attr(tag, "content=\"").is_some_and(|value| {
                    value.split(',').any(|v| {
                        v.trim().eq_ignore_ascii_case("noindex")
                            || v.trim().eq_ignore_ascii_case("none")
                    })
                })
        })
}
pub(super) fn audit_package(package: &Value) -> Result<Value> {
    let rows = package["files"].as_array().ok_or("网站文件列表无效")?;
    if rows.len() > 256 {
        return Err("网站文件过多".into());
    }
    let mut files = BTreeMap::new();
    let mut size = 0usize;
    for row in rows {
        let path = row["path"].as_str().ok_or("网站文件路径无效")?;
        let content = row["content"].as_str().ok_or("网站文件内容无效")?;
        if path.is_empty()
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || files.insert(path, content).is_some()
        {
            return Err("网站文件路径不安全或重复".into());
        }
        size = size.checked_add(content.len()).ok_or("网站文件过大")?;
        if size > 16 * 1024 * 1024 {
            return Err("网站文件超过16MiB".into());
        }
    }
    let mut checks = vec![];
    macro_rules! check { ($code:expr,$ok:expr,$message:expr,$path:expr) => { checks.push(json!({"code":$code,"status":if $ok {"pass"} else {"blocked"},"message":$message,"path":$path})); }; }
    let manifest = files
        .get("release-manifest.json")
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    check!(
        "manifest",
        manifest.is_some(),
        "发布清单必须存在且为有效 JSON",
        "release-manifest.json"
    );
    let manifest = manifest.unwrap_or(json!({}));
    let digests = manifest["files"].as_array();
    let mut listed = HashSet::new();
    let hashes_ok = digests
        .map(|ds| {
            ds.iter().all(|d| {
                let path = d["path"].as_str().unwrap_or("");
                listed.insert(path.to_string())
                    && files
                        .get(path)
                        .map(|s| d["sha256"] == hex::encode(Sha256::digest(s.as_bytes())))
                        .unwrap_or(false)
            }) && listed.len() == files.len().saturating_sub(1)
                && !listed.contains("release-manifest.json")
        })
        .unwrap_or(false);
    check!(
        "hashes",
        hashes_ok,
        "清单必须逐项匹配实际文件与 SHA-256",
        "release-manifest.json"
    );
    check!(
        "publication",
        manifest["publicationStatus"] == "not-published",
        "本地内容包状态必须为尚未发布",
        "release-manifest.json"
    );
    // The package's own calendar date preserves historical validity, including its offset.
    let generated_date = manifest["generatedAt"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|timestamp| timestamp.date_naive());
    if generated_date.is_none() {
        check!(
            "product-brand",
            false,
            "发布清单生成时间必须为有效 RFC3339 时间，才能核对商品品牌依据有效性",
            "release-manifest.json"
        );
    }
    let preview = manifest["previewOnly"].as_bool().unwrap_or(true);
    let expected: Vec<String> = manifest["expectedPublicUrls"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let catalog = files
        .get("catalog.json")
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    check!(
        "catalog",
        catalog
            .as_ref()
            .and_then(|c| c["products"].as_array())
            .is_some(),
        "商品目录必须包含有效商品列表",
        "catalog.json"
    );
    let mut actual_urls = vec![];
    for (path, html) in files.iter().filter(|(p, _)| p.ends_with(".html")) {
        let title = html
            .split_once("<title>")
            .and_then(|(_, s)| s.split_once("</title>"))
            .map(|x| x.0)
            .unwrap_or("");
        check!(
            "page-fields",
            !title.trim().is_empty()
                && attr(html, "<meta name=\"description\" content=\"")
                    .is_some_and(|s| !s.trim().is_empty())
                && html.contains("name=\"viewport\""),
            "页面需要非空标题、描述与移动端 viewport",
            path
        );
        let canonical = attr(html, "<link rel=\"canonical\" href=\"");
        let page_url = if *path == "index.html" {
            manifest["baseUrl"].as_str().map(str::to_owned)
        } else {
            manifest["baseUrl"]
                .as_str()
                .map(|b| format!("{}{}", b, path.trim_end_matches("index.html")))
        };
        check!(
            "canonical",
            if preview {
                canonical.is_none() && robots_noindex(html)
            } else {
                canonical == page_url.as_deref()
                    && canonical.is_some_and(|u| expected.contains(&u.to_string()))
                    && !robots_noindex(html)
            },
            "预览需要 noindex；配置地址后 canonical 必须精确匹配清单",
            path
        );
        if let Some(u) = canonical {
            actual_urls.push(u.to_string());
        }
        for rest in html.split("href=\"").skip(1) {
            let href = rest.split('"').next().unwrap_or("");
            if href.starts_with("https://") || href.starts_with("http://") || href.starts_with('#')
            {
                continue;
            }
            let base = url::Url::parse(&format!("https://local.invalid/{path}")).unwrap();
            let target = base.join(href).ok();
            check!(
                "local-link",
                target
                    .as_ref()
                    .is_some_and(|u| u.host_str() == Some("local.invalid")
                        && files.contains_key(u.path().trim_start_matches('/'))),
                "本地链接必须指向包内文件",
                path
            );
        }
        if path.starts_with("products/") {
            let data = html
                .split_once("<script type=\"application/ld+json\">")
                .and_then(|(_, s)| s.split_once("</script>"))
                .and_then(|(s, _)| serde_json::from_str::<Value>(s).ok());
            let id = path.split('/').nth(1).unwrap_or("");
            let product = catalog
                .as_ref()
                .and_then(|c| c["products"].as_array())
                .and_then(|ps| ps.iter().find(|p| p["id"] == id));
            let brand_status = match (generated_date, data.as_ref(), product) {
                (Some(as_of), Some(d), Some(p)) => {
                    let expected_brand = p["publicFacts"]
                        .as_array()
                        .and_then(|facts| catalog::public_brand(facts, as_of));
                    match (expected_brand, d.get("brand")) {
                        (Some(expected), Some(claimed))
                            if claimed.as_object().is_some_and(|object| object.len() == 2)
                                && claimed["@type"] == "Brand"
                                && claimed["name"].as_str() == Some(expected) =>
                        {
                            "pass"
                        }
                        (None, None) => "warning",
                        _ => "blocked",
                    }
                }
                _ => "blocked",
            };
            checks.push(json!({
                "code":"product-brand",
                "status":brand_status,
                "message": if brand_status == "warning" {
                    "未提供可用公开商品品牌，页面应省略品牌；请补充有效公开依据"
                } else {
                    "商品品牌须与清单生成日期有效的唯一公开品牌依据一致，使用仅含类型和名称的 Brand 对象"
                },
                "path":path
            }));
            let semantic = data.as_ref().zip(product).is_some_and(|(d, p)| {
                let fields = p["publicFacts"].as_array();
                let fact = |key: &str| {
                    fields
                        .and_then(|fs| fs.iter().find(|f| f["id"] == format!("commerce:{key}")))
                        .and_then(|f| f["value"].as_str())
                };
                let complete = ["price", "currency", "availability", "purchase"]
                    .iter()
                    .all(|k| fact(k).is_some_and(|s| !s.trim().is_empty()))
                    && fact("availability") != Some("Unknown");
                let offer = d.get("offers");
                title == escape(d["name"].as_str().unwrap_or(""))
                    && d["@type"] == "Product"
                    && d["sku"] == p["sku"]
                    && d["sku"].as_str().is_some_and(|s| !s.trim().is_empty())
                    && d["name"] == p["title"]
                    && d["name"].as_str().is_some_and(|s| !s.trim().is_empty())
                    && d["description"]
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty())
                    && if preview {
                        d.get("url").unwrap_or(&Value::Null).is_null() && p["url"].is_null()
                    } else {
                        page_url.as_deref().is_some_and(|expected| {
                            d["url"].as_str() == Some(expected)
                                && p["url"].as_str() == Some(expected)
                        })
                    }
                    && offer.unwrap_or(&Value::Null) == &p["offer"]
                    && if complete {
                        offer.is_some_and(|o| {
                            o["price"] == fact("price").unwrap()
                                && o["priceCurrency"] == fact("currency").unwrap()
                                && o["url"] == fact("purchase").unwrap()
                                && o["availability"]
                                    == format!(
                                        "https://schema.org/{}",
                                        fact("availability").unwrap()
                                    )
                                && o["priceValidUntil"].as_str().is_some_and(|s| !s.is_empty())
                                && fields
                                    .and_then(|fs| fs.iter().find(|f| f["id"] == "commerce:price"))
                                    .is_some_and(|f| o["priceValidUntil"] == f["validUntil"])
                        })
                    } else {
                        offer.is_none()
                    }
            });
            check!(
                "product-data",
                semantic,
                "商品结构化数据的名称、SKU、网址和报价须与公开目录一致，资料不全不得生成报价",
                path
            );
        }
    }
    actual_urls.sort();
    let mut expected_sorted = expected.clone();
    expected_sorted.sort();
    check!(
        "expected-pages",
        files.contains_key("index.html") && actual_urls == expected_sorted,
        "清单地址必须与全部页面一致",
        "release-manifest.json"
    );
    let sitemap_ok = if preview {
        !files.contains_key("sitemap.xml")
    } else {
        files
            .get("sitemap.xml")
            .and_then(|s| xml_urls(s).ok())
            .is_some_and(|mut urls| {
                urls.sort();
                urls == expected_sorted
            })
    };
    check!(
        "sitemap",
        sitemap_ok,
        "预览不得提供 sitemap；配置地址后 sitemap 必须与页面一致",
        "sitemap.xml"
    );
    checks.push(json!({"code":"remote-unverified","status":"warning","message":"预期官网地址尚未测试；本地检查不代表公开可访问、抓取许可、收录或推荐"}));
    let remote: Vec<Value> = [
        "HTTP 状态与重定向",
        "最终页面 canonical",
        "网站根目录 robots 策略",
        "线上 noindex",
        "结构化数据与可见正文",
        "公开页面链接",
        "价格与库存有效期",
        "手机浏览布局",
    ]
    .iter()
    .map(|label| json!({"check":label,"status":"pending","observedEvidence":"","observedAt":""}))
    .collect();
    let summary = json!({"pass":checks.iter().filter(|c| c["status"] == "pass").count(),"warning":checks.iter().filter(|c| c["status"] == "warning").count(),"blocked":checks.iter().filter(|c| c["status"] == "blocked").count(),"publicationStatus":"not-published"});
    let report = json!({"checks":checks,"summary":summary,"remoteChecks":remote,"publicationStatus":"not-published"});
    let mut csv = "check,status,observedEvidence,observedAt\r\n".to_string();
    for row in &remote {
        csv.push_str(&format!("{},pending,,\r\n", row["check"].as_str().unwrap()));
    }
    let markdown = format!(
        "# 本地页面检查\n\n尚未发布。预期官网地址尚未测试。所有部署后检查均待人工完成。\n\n{}\n",
        checks
            .iter()
            .map(|c| format!(
                "- {}：{}",
                match c["status"].as_str().unwrap() {
                    "pass" => "通过",
                    "warning" => "提醒",
                    _ => "阻碍",
                },
                c["message"].as_str().unwrap()
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let exported = json!([{"path":"audit.md","content":markdown},{"path":"audit.json","content":serde_json::to_string_pretty(&report).map_err(|_|"检查报告序列化失败")?},{"path":"deployment-checklist.csv","content":csv}]);
    if exported
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["content"].as_str().unwrap().len())
        .sum::<usize>()
        > 16 * 1024 * 1024
    {
        return Err("检查报告超过16MiB".into());
    }
    Ok(
        json!({"checks":checks,"summary":summary,"remoteChecks":remote,"files":exported,"fileName":"M-GEO-部署检查单.zip"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(base: &str) -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构检查品牌"})).unwrap();
        let p = handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构杯","sku":"TEST","category":"杯","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"f","label":"容量","value":"1L","source":"虚构依据","visibility":"public","validUntil":null},{"id":"private","label":"秘密","value":"PRIVATE_SECRET","source":"虚构依据","visibility":"private","validUntil":null}]})).unwrap();
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
        let input = json!({"brandId":b["id"],"baseUrl":base,"drafts":[{"id":d["id"],"expectedRevision":d["revision"]}]});
        (c, b["id"].as_str().unwrap().to_string(), input)
    }
    fn blocked(a: &Value, code: &str) -> bool {
        a["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["code"] == code && c["status"] == "blocked")
    }
    fn rehash(p: &mut Value) {
        let fs = p["files"].as_array_mut().unwrap();
        let hashes: Vec<Value> = fs.iter().filter(|f|f["path"] != "release-manifest.json").map(|f|json!({"path":f["path"],"sha256":hex::encode(Sha256::digest(f["content"].as_str().unwrap().as_bytes()))})).collect();
        let m = fs
            .iter_mut()
            .find(|f| f["path"] == "release-manifest.json")
            .unwrap();
        let mut v: Value = serde_json::from_str(m["content"].as_str().unwrap()).unwrap();
        v["files"] = json!(hashes);
        m["content"] = json!(v.to_string());
    }
    fn edit_json_file(p: &mut Value, path: &str, edit: impl FnOnce(&mut Value)) {
        let f = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == path)
            .unwrap();
        let mut data: Value = serde_json::from_str(f["content"].as_str().unwrap()).unwrap();
        edit(&mut data);
        f["content"] = json!(data.to_string());
    }
    fn edit_product_data(p: &mut Value, edit: impl FnOnce(&mut Value)) {
        let f = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
            .unwrap();
        let (prefix, rest) = f["content"]
            .as_str()
            .unwrap()
            .split_once("<script type=\"application/ld+json\">")
            .unwrap();
        let (data, suffix) = rest.split_once("</script>").unwrap();
        let mut data: Value = serde_json::from_str(data).unwrap();
        edit(&mut data);
        f["content"] = json!(format!(
            "{prefix}<script type=\"application/ld+json\">{data}</script>{suffix}"
        ));
    }
    fn brand_fact() -> Value {
        json!({"id":"catalog:brand","label":"商品品牌","value":" 虚构商品品牌 ","source":"BRAND_SOURCE_SECRET","visibility":"public","validUntil":"2000-01-01"})
    }
    fn brand_package() -> Value {
        let (c, b, input) = fixture("");
        let mut p = site::prepare(&c, &b, &input).unwrap();
        edit_json_file(&mut p, "release-manifest.json", |m| {
            m["generatedAt"] = json!("2000-01-01T23:30:00-12:00")
        });
        edit_json_file(&mut p, "catalog.json", |cat| {
            cat["products"][0]["publicFacts"]
                .as_array_mut()
                .unwrap()
                .push(brand_fact())
        });
        edit_product_data(&mut p, |d| {
            d["brand"] = json!({"@type":"Brand","name":"虚构商品品牌"})
        });
        rehash(&mut p);
        p
    }
    fn brand_status(a: &Value) -> &str {
        a["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["code"] == "product-brand")
            .and_then(|c| c["status"].as_str())
            .unwrap_or("missing")
    }
    #[test]
    fn rehashed_wrong_product_brand_is_semantic_block_not_hash_corruption() {
        let mut p = brand_package();
        edit_product_data(&mut p, |d| d["brand"]["name"] = json!("虚构工作区名称"));
        rehash(&mut p);
        let original = p.clone();
        let a = audit_package(&p).unwrap();
        assert!(!blocked(&a, "hashes"), "{a}");
        assert_eq!(brand_status(&a), "blocked", "{a}");
        assert_eq!(p, original);
        for secret in [
            "虚构工作区名称",
            "BRAND_SOURCE_SECRET",
            "RAW_SECRET",
            "PRIVATE_SECRET",
        ] {
            assert!(!a.to_string().contains(secret));
        }
    }
    #[test]
    fn historical_brand_uses_manifest_calendar_date_and_exact_object_shape() {
        let original = brand_package();
        assert_eq!(brand_status(&audit_package(&original).unwrap()), "pass");
        for claimed in [
            Value::Null,
            json!("虚构商品品牌"),
            json!([{"@type":"Brand","name":"虚构商品品牌"}]),
            json!({"@type":"Organization","name":"虚构商品品牌"}),
            json!({"@type":"Brand","name":"虚构商品品牌","url":"https://fictional.invalid/"}),
            json!({"@type":"Brand","name":" 虚构商品品牌 "}),
        ] {
            let mut p = original.clone();
            edit_product_data(&mut p, |d| d["brand"] = claimed);
            rehash(&mut p);
            assert_eq!(brand_status(&audit_package(&p).unwrap()), "blocked");
        }
        let mut p = original.clone();
        edit_product_data(&mut p, |d| {
            d.as_object_mut().unwrap().remove("brand");
        });
        rehash(&mut p);
        assert_eq!(brand_status(&audit_package(&p).unwrap()), "blocked");
        for generated in [
            json!("yesterday"),
            json!("2000-01-01"),
            json!("2000-02-30T00:00:00Z"),
            Value::Null,
        ] {
            let mut p = original.clone();
            edit_json_file(&mut p, "release-manifest.json", |m| {
                m["generatedAt"] = generated
            });
            let a = audit_package(&p).unwrap();
            assert!(!blocked(&a, "hashes"));
            assert_eq!(brand_status(&a), "blocked");
        }
    }
    #[test]
    fn invalid_manifest_date_blocks_even_without_product_html() {
        let mut p = brand_package();
        p["files"]
            .as_array_mut()
            .unwrap()
            .retain(|f| !f["path"].as_str().unwrap().starts_with("products/"));
        edit_json_file(&mut p, "release-manifest.json", |m| {
            m["generatedAt"] = json!("invalid")
        });
        rehash(&mut p);
        let a = audit_package(&p).unwrap();
        assert!(!blocked(&a, "hashes"));
        assert!(blocked(&a, "product-brand"));
    }
    #[test]
    fn unsupported_brand_claims_block_but_conservative_omission_warns() {
        let original = brand_package();
        let mut unusable = vec![vec![], vec![brand_fact(), brand_fact()]];
        for (key, value) in [
            ("visibility", json!("private")),
            ("source", json!(" ")),
            ("value", json!(" ")),
            ("value", json!("x".repeat(71))),
            ("value", json!("虚构\n品牌")),
            ("value", json!(23)),
            ("validUntil", json!("1999-12-31")),
            ("validUntil", json!("2000-1-1")),
            ("validUntil", json!(false)),
        ] {
            let mut f = brand_fact();
            f[key] = value;
            unusable.push(vec![f]);
        }
        for facts in unusable {
            for asserted in [false, true] {
                let mut p = original.clone();
                edit_json_file(&mut p, "catalog.json", |cat| {
                    cat["products"][0]["publicFacts"] = json!(facts)
                });
                if !asserted {
                    edit_product_data(&mut p, |d| {
                        d.as_object_mut().unwrap().remove("brand");
                    });
                }
                rehash(&mut p);
                let a = audit_package(&p).unwrap();
                assert!(!blocked(&a, "hashes"));
                assert_eq!(
                    brand_status(&a),
                    if asserted { "blocked" } else { "warning" },
                    "{a}"
                );
                assert!(!a.to_string().contains("BRAND_SOURCE_SECRET"));
            }
        }
        for expiry in [None, Some(Value::Null)] {
            let mut p = original.clone();
            edit_json_file(&mut p, "catalog.json", |cat| {
                let f = &mut cat["products"][0]["publicFacts"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .as_object_mut()
                    .unwrap();
                if let Some(v) = expiry {
                    f.insert("validUntil".into(), v);
                } else {
                    f.remove("validUntil");
                }
            });
            rehash(&mut p);
            assert_eq!(brand_status(&audit_package(&p).unwrap()), "pass");
        }
    }
    #[test]
    fn preview_configured_privacy_and_stale_revision() {
        for base in ["", "https://fictional.example/shop/"] {
            let (c, b, mut input) = fixture(base);
            let a = prepare(&c, &b, &input).unwrap();
            assert_eq!(a["summary"]["blocked"], 0, "{a}");
            assert!(a["remoteChecks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["status"] == "pending" && r["observedAt"] == ""));
            for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
                assert!(!a.to_string().contains(secret));
            }
            input["drafts"][0]["expectedRevision"] = json!(99);
            assert!(prepare(&c, &b, &input).is_err());
        }
    }
    #[test]
    fn detects_hash_and_semantic_tampering_independently() {
        let (c, b, input) = fixture("https://fictional.example/shop/");
        let original = site::prepare(&c, &b, &input).unwrap();
        for (before, after, code) in [
            ("../../index.html", "../../missing.html", "local-link"),
            ("\"sku\":\"TEST\"", "\"sku\":\"WRONG\"", "product-data"),
            ("<title>虚构杯</title>", "<title></title>", "page-fields"),
            (
                "<link rel=\"canonical\" href=\"https://fictional.example/shop/products/",
                "<link rel=\"canonical\" href=\"https://wrong.example/products/",
                "canonical",
            ),
        ] {
            let mut p = original.clone();
            let f = p["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
                .unwrap();
            let text = f["content"].as_str().unwrap().replace(before, after);
            f["content"] = json!(text);
            assert!(blocked(&audit_package(&p).unwrap(), "hashes"));
            rehash(&mut p);
            let a = audit_package(&p).unwrap();
            assert!(!blocked(&a, "hashes"));
            assert!(blocked(&a, code), "{code}: {a}");
        }
        let mut p = original.clone();
        p["files"]
            .as_array_mut()
            .unwrap()
            .retain(|f| f["path"] != "release-manifest.json");
        assert!(blocked(&audit_package(&p).unwrap(), "manifest"));
        let mut p = original.clone();
        p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == "release-manifest.json")
            .unwrap()["content"] = json!("{");
        assert!(blocked(&audit_package(&p).unwrap(), "manifest"));
        let mut p = original.clone();
        let duplicate = p["files"][0].clone();
        p["files"].as_array_mut().unwrap().push(duplicate);
        assert!(audit_package(&p).is_err());
        let mut p = original;
        p["files"][0]["path"] = json!("../escape");
        assert!(audit_package(&p).is_err());
    }
    #[test]
    fn rejects_invented_offer_and_mismatched_sitemap_after_rehash() {
        let (c, b, input) = fixture("https://fictional.example/shop/");
        let mut p = site::prepare(&c, &b, &input).unwrap();
        let catalog = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == "catalog.json")
            .unwrap();
        let mut data: Value = serde_json::from_str(catalog["content"].as_str().unwrap()).unwrap();
        data["products"][0]["offer"] = json!({"price":"99","priceCurrency":"CNY"});
        catalog["content"] = json!(data.to_string());
        rehash(&mut p);
        assert!(blocked(&audit_package(&p).unwrap(), "product-data"));
        let sitemap = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == "sitemap.xml")
            .unwrap();
        sitemap["content"] = json!("<urlset><url><loc>https://wrong.example/</loc></url></urlset>");
        rehash(&mut p);
        assert!(blocked(&audit_package(&p).unwrap(), "sitemap"));
        let (c, b, input) = fixture("");
        let mut p = site::prepare(&c, &b, &input).unwrap();
        p["files"]
            .as_array_mut()
            .unwrap()
            .push(json!({"path":"sitemap.xml","content":"<urlset/>"}));
        rehash(&mut p);
        assert!(blocked(&audit_package(&p).unwrap(), "sitemap"));
    }
    #[test]
    fn price_mismatch_and_empty_sku_are_semantically_blocked() {
        let (c, b, input) = fixture("https://fictional.example/");
        let original = site::prepare(&c, &b, &input).unwrap();
        for replacement in [
            "\"sku\":\"\"",
            "\"sku\":null",
            "\"sku\":\"TEST\",\"offers\":{\"price\":\"19\"}",
        ] {
            let mut p = original.clone();
            let f = p["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
                .unwrap();
            f["content"] = json!(f["content"]
                .as_str()
                .unwrap()
                .replace("\"sku\":\"TEST\"", replacement));
            rehash(&mut p);
            assert!(blocked(&audit_package(&p).unwrap(), "product-data"));
        }
    }
    #[test]
    fn review_regressions_for_robots_urls_and_sitemap_structure() {
        let (c, b, input) = fixture("https://fictional.example/");
        let original = site::prepare(&c, &b, &input).unwrap();
        let mut p = original.clone();
        let f = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == "index.html")
            .unwrap();
        f["content"] = json!(f["content"]
            .as_str()
            .unwrap()
            .replace("</body>", "<p>noindex 是本 FAQ 的讨论主题</p></body>"));
        rehash(&mut p);
        assert!(!blocked(&audit_package(&p).unwrap(), "canonical"));
        let f = p["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == "index.html")
            .unwrap();
        f["content"] = json!(f["content"].as_str().unwrap().replace(
            "</head>",
            "<meta name=\"robots\" content=\"follow, noindex\"></head>"
        ));
        rehash(&mut p);
        assert!(blocked(&audit_package(&p).unwrap(), "canonical"));
        for base in ["https://fictional.example/", ""] {
            let (c, b, input) = fixture(base);
            let mut p = site::prepare(&c, &b, &input).unwrap();
            let catalog = p["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"] == "catalog.json")
                .unwrap();
            let mut cat: Value =
                serde_json::from_str(catalog["content"].as_str().unwrap()).unwrap();
            let old = cat["products"][0]["url"].clone();
            cat["products"][0]["url"] = json!("https://wrong.example/");
            catalog["content"] = json!(cat.to_string());
            let f = p["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"].as_str().unwrap().starts_with("products/"))
                .unwrap();
            let html = f["content"].as_str().unwrap();
            let (prefix, rest) = html
                .split_once("<script type=\"application/ld+json\">")
                .unwrap();
            let (structured, suffix) = rest.split_once("</script>").unwrap();
            let mut data: Value = serde_json::from_str(structured).unwrap();
            assert_eq!(data.get("url").unwrap_or(&Value::Null), &old);
            data["url"] = json!("https://wrong.example/");
            f["content"] = json!(format!(
                "{prefix}<script type=\"application/ld+json\">{data}</script>{suffix}"
            ));
            rehash(&mut p);
            let a = audit_package(&p).unwrap();
            assert!(!blocked(&a, "hashes"));
            assert!(blocked(&a, "product-data"));
        }
        let manifest: Value = serde_json::from_str(
            original["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["path"] == "release-manifest.json")
                .unwrap()["content"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let locs = manifest["expectedPublicUrls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| format!("<loc>{}</loc>", u.as_str().unwrap()))
            .collect::<String>();
        for xml in [
            format!("<not-a-sitemap>{locs}</not-a-sitemap>"),
            format!(
                "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{locs}</urlset>"
            ),
            format!("<urlset><url>{locs}</url></urlset>"),
            "<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"><url>".to_string(),
        ] {
            let mut p = original.clone();
            p["files"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["path"] == "sitemap.xml")
                .unwrap()["content"] = json!(xml);
            rehash(&mut p);
            let a = audit_package(&p).unwrap();
            assert!(!blocked(&a, "hashes"));
            assert!(blocked(&a, "sitemap"));
        }
    }
}
