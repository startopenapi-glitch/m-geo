//! Pure offline rehearsal contracts. No transport, model, filesystem, or database I/O.
use super::*;
use std::collections::BTreeMap;
use url::Url;

fn zero_effects() -> Value {
    json!({"networkRequests":0,"modelCalls":0,"publications":0})
}
fn shape(v: &Value, keys: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or("fixture 对象无效")?;
    if o.len() != keys.len() || o.keys().any(|k| !keys.contains(&k.as_str())) {
        return Err("fixture 字段缺失或包含未知字段".into());
    }
    Ok(())
}
fn bounded(v: &Value, key: &str, max: usize, required: bool) -> Result<String> {
    raw(v, key, max, required)
}
fn digest(v: &Value) -> String {
    hex::encode(Sha256::digest(v.to_string().as_bytes()))
}
fn mode_label(mode: &str) -> Result<&'static str> {
    match mode {
        "natural_discovery" => Ok("模拟 API 自然发现请求；不是消费者平台自然结果"),
        "informed_qa" => Ok("模拟 API 知情问答；不能作为自然发现证据"),
        _ => Err("不支持的监测模式".into()),
    }
}
pub(super) fn monitor_request(
    mode: &str,
    question: &str,
    brand: &Value,
    product: &Value,
) -> Result<Value> {
    let label = mode_label(mode)?;
    let question = text(&json!({"question":question}), "question", 500, true)?;
    if brand.to_string().len() > 2 * 1024 * 1024 || product.to_string().len() > 2 * 1024 * 1024 {
        return Err("商品或品牌资料过大".into());
    }
    let brand_name = text(brand, "name", 100, true)?;
    let product_name = text(product, "name", 200, true)?;
    let sku = text(product, "sku", 100, false)?;
    if brand["id"].as_str().is_none() || product["brandId"] != brand["id"] {
        return Err("商品与品牌不匹配".into());
    }
    if mode == "natural_discovery" {
        let q = question.to_lowercase();
        let mut identifiers = vec![brand_name.clone(), product_name.clone(), sku.clone()];
        for entity in [&brand["profile"], product] {
            if let Some(aliases) = entity["aliases"].as_array() {
                for alias in aliases {
                    if let Some(a) = alias.as_str() {
                        identifiers.push(a.to_owned());
                    }
                }
            }
            if let Some(url) = entity["officialUrl"]
                .as_str()
                .and_then(|u| Url::parse(u).ok())
            {
                if let Some(host) = url.host_str() {
                    identifiers.push(host.to_owned());
                }
            }
        }
        for name in identifiers {
            if !name.is_empty() && q.contains(&name.to_lowercase()) {
                return Err("自然发现问题不得包含品牌、商品名称、别名、官网或 SKU".into());
            }
        }
    }
    let context = if mode == "informed_qa" {
        let facts = confirmed_public(product)?;
        if facts.len() > 200 {
            return Err("公开事实过多".into());
        }
        let facts=facts.iter().map(|f|Ok(json!({"label":text(f,"label",100,true)?,"value":text(f,"value",2000,true)?,"source":text(f,"source",1000,true)?}))).collect::<Result<Vec<Value>>>()?;
        json!({"brandName":brand_name,"productName":product_name,"sku":sku,"facts":facts})
    } else {
        Value::Null
    };
    let prompt = monitor_prompt(&question, &context);
    let mut r = json!({"mode":mode,"modeLabel":label,"question":question,"contextIncluded":mode=="informed_qa","context":context,"transport":"fixture","evidenceMode":"simulated","request":{"prompt":prompt},"realEffects":zero_effects()});
    r["requestHash"] = json!(digest(&r));
    Ok(r)
}
fn monitor_prompt(question: &str, context: &Value) -> String {
    if context.is_null() {
        format!("这是本地模拟 API 自然发现请求，不代表消费者平台结果。\n仅回答问题：{question}")
    } else {
        format!("这是已提供公开资料的本地模拟 API 知情问答，不得视为自然发现证据。\n公开资料：{}\n问题：{question}",context)
    }
}
pub(super) fn monitor_fixture(request: &Value) -> Result<Value> {
    if request.to_string().len() > 512 * 1024 {
        return Err("模拟请求过大".into());
    }
    shape(
        request,
        &[
            "mode",
            "modeLabel",
            "question",
            "contextIncluded",
            "context",
            "transport",
            "evidenceMode",
            "request",
            "realEffects",
            "requestHash",
        ],
    )?;
    let mode = text(request, "mode", 30, true)?;
    let label = mode_label(&mode)?;
    let question = text(request, "question", 500, true)?;
    shape(&request["request"], &["prompt"])?;
    bounded(&request["request"], "prompt", 256 * 1024, true)?;
    let mut unsigned = request.clone();
    unsigned.as_object_mut().unwrap().remove("requestHash");
    if request["transport"] != "fixture"
        || request["evidenceMode"] != "simulated"
        || request["realEffects"] != zero_effects()
        || request["modeLabel"] != label
        || request["contextIncluded"] != (mode == "informed_qa")
        || request["requestHash"] != digest(&unsigned)
        || (mode == "natural_discovery" && !request["context"].is_null())
    {
        return Err("模拟请求来源、模式或完整性无效".into());
    }
    if mode == "informed_qa" {
        let context = &request["context"];
        shape(context, &["brandName", "productName", "sku", "facts"])?;
        text(context, "brandName", 100, true)?;
        text(context, "productName", 200, true)?;
        text(context, "sku", 100, false)?;
        let facts = context["facts"]
            .as_array()
            .filter(|f| !f.is_empty() && f.len() <= 200)
            .ok_or("模拟知情问答公开事实无效")?;
        for f in facts {
            shape(f, &["label", "value", "source"])?;
            text(f, "label", 100, true)?;
            text(f, "value", 2000, true)?;
            text(f, "source", 1000, true)?;
        }
    }
    if request["request"]["prompt"] != monitor_prompt(&question, &request["context"]) {
        return Err("模拟请求 prompt 与问题或公开上下文不一致".into());
    }
    let result_text = format!("本地模拟回答（无模型调用，无自然推荐证据）：{question}");
    Ok(
        json!({"mode":mode,"question":question,"contextIncluded":request["contextIncluded"],"surface":"fixture","sourceKind":"simulated","transport":"fixture","evidenceMode":"simulated","resultText":result_text,"resultIdentity":digest(&json!({"requestHash":request["requestHash"],"resultText":result_text})),"requestHash":request["requestHash"],"realEffects":zero_effects()}),
    )
}
fn fixture_url(s: &str) -> Result<Url> {
    if s.len() > 2000 || s.contains('\0') {
        return Err("fixture URL 过长或无效".into());
    }
    let u = Url::parse(s).map_err(|_| "fixture URL 无效")?;
    if u.scheme() != "https"
        || !u
            .host_str()
            .is_some_and(|h| h.ends_with(".invalid") && h.len() > 8)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || u.fragment().is_some()
    {
        return Err("仅允许无凭据的 HTTPS .invalid fixture 地址".into());
    }
    Ok(u)
}
pub(super) fn site_fixture(url: &str, title: &str, body: &str) -> Value {
    // Invalid inputs remain invalid and are rejected by the audit boundary; no silent fallback.
    let article = json!({"@context":"https://schema.org","@type":"Article","headline":title,"description":"local simulated article data; not real evidence","url":url,"articleBody":body});
    let data = article
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    json!({"url":url,"finalUrl":url,"statusCode":200,"headers":{"contentType":"text/html; charset=utf-8","xRobotsTag":""},"html":format!("<!doctype html><html><head><title>{}</title><meta name=\"description\" content=\"Local simulated article data; not real evidence.\"><link rel=\"canonical\" href=\"{}\"><script type=\"application/ld+json\">{}</script></head><body><p>Local simulated article / local simulated article data; not real evidence.</p><h1>{}</h1><article>{}</article></body></html>",escape(title),escape(url),data,escape(title),escape(body)),"robotsTxt":"User-agent: *\nAllow: /\n","sitemapXml":format!("<?xml version=\"1.0\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\"><url><loc>{}</loc></url></urlset>",escape(url)),"expectedText":body,"transport":"fixture"})
}

#[derive(Default)]
struct HtmlEvidence {
    visible: String,
    canonicals: Vec<String>,
    titles: Vec<String>,
    descriptions: Vec<String>,
    headings: Vec<String>,
    json_ld: Vec<String>,
    noindex: bool,
    valid: bool,
}
fn decode(s: &str) -> String {
    quick_xml::escape::unescape(&s.replace("&nbsp;", "&#160;"))
        .map(|s| s.into_owned())
        .unwrap_or_default()
}
fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn attributes(s: &str) -> Option<BTreeMap<String, String>> {
    let mut rest = s.trim();
    let mut attrs = BTreeMap::new();
    while !rest.is_empty() {
        if rest == "/" {
            break;
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        let key = rest[..end].to_ascii_lowercase();
        rest = rest[end..].trim_start();
        let mut value = String::new();
        if let Some(r) = rest.strip_prefix('=') {
            rest = r.trim_start();
            if rest.starts_with(['\'', '"']) {
                let quote = rest.chars().next()?;
                rest = &rest[1..];
                let end = rest.find(quote)?;
                value = decode(&rest[..end]);
                rest = &rest[end + 1..];
            } else {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                if end == 0 {
                    return None;
                }
                value = decode(&rest[..end]);
                rest = &rest[end..];
            }
        }
        if attrs.insert(key, value).is_some() {
            return None;
        }
        rest = rest.trim_start();
    }
    Some(attrs)
}
// Conservative quote-aware tokenizer. Raw text, comments and hidden/template subtrees
// never yield text or metadata. Unsupported/malformed markup fails the structure check.
fn html_evidence(html: &str) -> HtmlEvidence {
    let mut e = HtmlEvidence {
        valid: true,
        ..Default::default()
    };
    let mut rest = html;
    let mut stack: Vec<(String, bool)> = vec![];
    let mut title_slot: Option<usize> = None;
    let mut heading_slot: Option<usize> = None;
    let mut json_script = false;
    while !rest.is_empty() {
        if let Some((raw, _)) = stack
            .last()
            .filter(|(n, _)| ["script", "style", "noscript"].contains(&n.as_str()))
        {
            let lower = rest.to_ascii_lowercase();
            let close = format!("</{raw}");
            match lower.find(&close) {
                Some(i) => {
                    if json_script {
                        e.json_ld.push(rest[..i].to_owned());
                        json_script = false;
                    }
                    rest = &rest[i..];
                }
                None => {
                    e.valid = false;
                    break;
                }
            }
        }
        let start = rest.find('<').unwrap_or(rest.len());
        let text = decode(&rest[..start]);
        if let Some(index) = title_slot {
            if stack.last().is_some_and(|(name, _)| name == "title") {
                e.titles[index].push_str(&text);
            }
        }
        if !stack.iter().any(|(_, hidden)| *hidden) {
            if let Some(index) = heading_slot {
                e.headings[index].push_str(&text);
                e.headings[index].push(' ');
            }
            e.visible.push_str(&text);
            e.visible.push(' ');
        }
        rest = &rest[start..];
        if rest.is_empty() {
            break;
        }
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(i) => {
                    rest = &rest[i + 3..];
                    continue;
                }
                None => {
                    e.valid = false;
                    break;
                }
            }
        }
        let mut quote = None;
        let mut end = None;
        for (i, c) in rest.char_indices().skip(1) {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '\'' || c == '"' {
                quote = Some(c);
            } else if c == '>' {
                end = Some(i);
                break;
            }
        }
        let Some(end) = end else {
            e.valid = false;
            break;
        };
        let tag = rest[1..end].trim();
        rest = &rest[end + 1..];
        if tag.to_ascii_lowercase().starts_with("!doctype ") {
            continue;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            let closing = closing.trim().to_ascii_lowercase();
            if closing == "title" {
                title_slot = None;
            }
            if closing == "h1" {
                heading_slot = None;
            }
            if stack.last().is_some_and(|(n, _)| *n == closing) {
                stack.pop();
            } else {
                e.valid = false;
            }
            continue;
        }
        let split = tag
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = tag[..split].to_ascii_lowercase();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            e.valid = false;
            break;
        }
        let Some(attrs) = attributes(&tag[split..]) else {
            e.valid = false;
            break;
        };
        let inherited = stack.iter().any(|(_, h)| *h);
        let metadata_allowed =
            !stack.iter().any(|(n, h)| *h && n != "head") && stack.iter().any(|(n, _)| n == "head");
        if metadata_allowed && name == "title" {
            title_slot = Some(e.titles.len());
            e.titles.push(String::new());
        }
        if metadata_allowed
            && name == "meta"
            && attrs
                .get("name")
                .is_some_and(|v| v.eq_ignore_ascii_case("description"))
        {
            e.descriptions
                .push(attrs.get("content").cloned().unwrap_or_default());
        }
        if (metadata_allowed || !inherited)
            && name == "script"
            && attrs
                .get("type")
                .is_some_and(|v| v.eq_ignore_ascii_case("application/ld+json"))
        {
            json_script = true;
        }
        if metadata_allowed
            && name == "link"
            && attrs.get("rel").is_some_and(|r| {
                r.split_whitespace()
                    .any(|s| s.eq_ignore_ascii_case("canonical"))
            })
        {
            e.canonicals
                .push(attrs.get("href").cloned().unwrap_or_default());
        }
        if metadata_allowed
            && name == "meta"
            && attrs.get("name").is_some_and(|n| {
                ["robots", "googlebot", "bingbot", "m-geo-p0-fixture"]
                    .contains(&n.to_ascii_lowercase().as_str())
            })
            && attrs.get("content").is_some_and(|c| has_noindex(c))
        {
            e.noindex = true;
        }
        let style = attrs
            .get("style")
            .map(|s| s.to_ascii_lowercase().replace(char::is_whitespace, ""))
            .unwrap_or_default();
        let hidden = inherited
            || ["head", "script", "style", "noscript", "template"].contains(&name.as_str())
            || attrs.contains_key("hidden")
            || attrs.contains_key("inert")
            || attrs
                .get("aria-hidden")
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || style.contains("display:none")
            || style.contains("visibility:hidden");
        if name == "h1" && !hidden {
            heading_slot = Some(e.headings.len());
            e.headings.push(String::new());
        }
        if ![
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
            "source", "track", "wbr",
        ]
        .contains(&name.as_str())
        {
            if tag.ends_with('/') {
                e.valid = false;
                break;
            }
            stack.push((name, hidden));
            if stack.len() > 128 {
                e.valid = false;
                break;
            }
        }
    }
    if !stack.is_empty() {
        e.valid = false;
    }
    e.visible = normalized(&e.visible);
    e
}
fn has_noindex(s: &str) -> bool {
    s.to_ascii_lowercase()
        .split(|c: char| c == ',' || c.is_whitespace() || c == ':')
        .any(|p| p == "noindex" || p == "none")
}
fn sitemap_urls(text: &str) -> Result<Vec<String>> {
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

fn sitemap_matches(xml: &str, target: &Url) -> bool {
    // Reuse the package inspector's strict quick-xml grammar (no entities/CDATA/DTD).
    sitemap_urls(xml)
        .is_ok_and(|urls| urls.len() == 1 && fixture_url(&urls[0]).is_ok_and(|u| u == *target))
}
fn robots_path(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                if v.is_ascii_alphanumeric() || b"-._~".contains(&v) {
                    out.push(v as char);
                } else {
                    out.push_str(&format!("%{v:02X}"));
                }
                i += 3;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
fn robots_pattern_matches(rule: &str, path: &str) -> bool {
    let anchored = rule.ends_with('$');
    let mut pattern = rule.strip_suffix('$').unwrap_or(rule).as_bytes().to_vec();
    if !anchored {
        pattern.push(b'*');
    }
    let path = path.as_bytes();
    let (mut p, mut at) = (0, 0);
    let mut star = None;
    let mut retry = 0;
    while at < path.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = at;
        } else if p < pattern.len() && pattern[p] == path[at] {
            p += 1;
            at += 1;
        } else if let Some(saved) = star {
            retry += 1;
            at = retry;
            p = saved + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
fn robots_allowed(text: &str, target: &Url) -> bool {
    #[derive(Default)]
    struct Group {
        agents: Vec<String>,
        rules: Vec<(bool, String)>,
    }
    let mut groups: Vec<Group> = vec![];
    let mut group = Group::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return false;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "user-agent" => {
                if !group.rules.is_empty() {
                    groups.push(group);
                    group = Group::default();
                }
                group.agents.push(value.to_ascii_lowercase());
            }
            "allow" | "disallow" => {
                if group.agents.is_empty() {
                    return false;
                }
                group
                    .rules
                    .push((key.trim().eq_ignore_ascii_case("allow"), value.to_owned()));
            }
            "sitemap" | "crawl-delay" => (),
            _ => return false,
        }
    }
    if !group.agents.is_empty() {
        groups.push(group);
    }
    let specific = groups.iter().any(|g| {
        g.agents
            .iter()
            .any(|a| !a.is_empty() && a != "*" && "m-geo-p0-fixture".starts_with(a))
    });
    let mut rules = vec![];
    for g in groups {
        if g.agents.iter().any(|a| {
            if specific {
                !a.is_empty() && a != "*" && "m-geo-p0-fixture".starts_with(a)
            } else {
                a == "*"
            }
        }) {
            rules.extend(g.rules);
        }
    }
    let path = match target.query() {
        Some(q) => format!("{}?{}", target.path(), q),
        None => target.path().to_owned(),
    };
    let path = robots_path(&path);
    let mut best: Option<(usize, bool)> = None;
    for (allow, rule) in rules {
        let rule = robots_path(&rule);
        if rule.is_empty() {
            continue;
        }
        if robots_pattern_matches(&rule, &path) {
            let score = rule.trim_end_matches('$').replace('*', "").len();
            if best.is_none_or(|(n, a)| score > n || (score == n && allow && !a)) {
                best = Some((score, allow));
            }
        }
    }
    best.is_none_or(|(_, allow)| allow)
}
pub(super) fn audit_site_fixture(f: &Value) -> Result<Value> {
    if f.to_string().len() > 2 * 1024 * 1024 {
        return Err("fixture 超过 2MiB".into());
    }
    shape(
        f,
        &[
            "url",
            "finalUrl",
            "statusCode",
            "headers",
            "html",
            "robotsTxt",
            "sitemapXml",
            "expectedText",
            "transport",
        ],
    )?;
    if f["transport"] != "fixture" {
        return Err("仅允许 fixture transport".into());
    }
    let url = fixture_url(&bounded(f, "url", 2000, true)?)?;
    let final_url = fixture_url(&bounded(f, "finalUrl", 2000, true)?)?;
    let status = f["statusCode"]
        .as_u64()
        .filter(|s| (100..=599).contains(s))
        .ok_or("fixture HTTP 状态无效")?;
    shape(&f["headers"], &["contentType", "xRobotsTag"])?;
    let content_type = bounded(&f["headers"], "contentType", 200, true)?;
    let robots_header = bounded(&f["headers"], "xRobotsTag", 2000, false)?;
    let html = bounded(f, "html", 512 * 1024, true)?;
    let robots = bounded(f, "robotsTxt", 64 * 1024, false)?;
    let sitemap = bounded(f, "sitemapXml", 128 * 1024, true)?;
    let expected = bounded(f, "expectedText", 128 * 1024, true)?;
    // Raw byte bounds also cap tokenizer processing for multibyte input.
    if html.len() > 512 * 1024
        || robots.len() > 64 * 1024
        || sitemap.len() > 128 * 1024
        || expected.len() > 128 * 1024
    {
        return Err("fixture 文本超过大小限制".into());
    }
    let evidence = html_evidence(&html);
    let mut checks = vec![];
    macro_rules! check {($id:expr,$ok:expr,$label:expr,$detail:expr)=>{checks.push(json!({"id":$id,"label":$label,"status":if $ok{"pass"}else{"fail"},"detail":$detail}));}}
    check!(
        "target",
        url == final_url,
        "模拟目标与重定向",
        "最终地址必须与原始 .invalid 目标一致"
    );
    check!(
        "status",
        status == 200,
        "模拟 HTTP 状态",
        "fixture 状态必须为 200"
    );
    check!(
        "content_type",
        content_type
            .split(';')
            .next()
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("text/html")),
        "HTML 类型",
        "仅检查 fixture HTML 类型"
    );
    check!(
        "html_structure",
        evidence.valid,
        "保守 HTML 解析",
        "不支持或不完整的标签结构不能构成成功证据"
    );
    check!(
        "title",
        evidence.valid
            && evidence.titles.len() == 1
            && !normalized(&evidence.titles[0]).is_empty()
            && evidence.titles[0].chars().count() <= 300,
        "页面标题",
        "有效 head 需要一个非空标题（最多 300 字）；仅为本地页面完整性检查"
    );
    check!(
        "description",
        evidence.valid
            && evidence.descriptions.len() == 1
            && !normalized(&evidence.descriptions[0]).is_empty()
            && evidence.descriptions[0].chars().count() <= 500,
        "页面描述",
        "有效 head 需要一个非空 description（最多 500 字）；不代表搜索效果"
    );
    check!(
        "h1",
        evidence.valid
            && evidence.headings.len() == 1
            && !normalized(&evidence.headings[0]).is_empty()
            && evidence.headings[0].chars().count() <= 300,
        "可见主标题",
        "需要一个非空可见 H1（最多 300 字）"
    );
    let article = if evidence.json_ld.len() == 1 {
        strict_json::parse(&evidence.json_ld[0]).ok()
    } else {
        None
    };
    let article_ok = article.as_ref().is_some_and(|a| {
        a.is_object()
            && a["@context"] == "https://schema.org"
            && a["@type"] == "Article"
            && a["headline"].as_str().is_some_and(|h| {
                !normalized(h).is_empty()
                    && evidence.titles.len() == 1
                    && normalized(h) == normalized(&evidence.titles[0])
            })
            && a["articleBody"]
                .as_str()
                .is_some_and(|b| normalized(b) == normalized(&expected))
            && a["url"]
                .as_str()
                .is_some_and(|u| fixture_url(u).is_ok_and(|u| u == url))
            && a["description"]
                .as_str()
                .is_some_and(|d| d.to_ascii_lowercase().contains("local simulated article"))
            && evidence
                .visible
                .to_ascii_lowercase()
                .contains("local simulated article")
    });
    check!("json_ld",evidence.valid && article_ok,"本地模拟 Article JSON-LD","需要有效的模拟 Article JSON-LD，标题、正文和 .invalid URL 与页面一致，并在数据和可见页面明确标注 local simulated article；不代表真实证据");
    check!(
        "indexability",
        !has_noindex(&robots_header) && !evidence.noindex,
        "模拟索引许可",
        "fixture header 与有效 head meta 不得声明 noindex/none"
    );
    check!(
        "robots",
        robots_allowed(&robots, &url),
        "模拟 robots 许可",
        "按 M-GEO-P0-Fixture 对应组及最长路径匹配检查"
    );
    check!(
        "canonical",
        evidence.valid
            && evidence.canonicals.len() == 1
            && fixture_url(&evidence.canonicals[0]).is_ok_and(|u| u == url),
        "模拟 canonical",
        "有效 head 必须仅包含一个精确匹配目标的 canonical"
    );
    check!(
        "sitemap",
        sitemap_matches(&sitemap, &url),
        "模拟 sitemap",
        "有效 XML 必须仅包含该 fixture URL"
    );
    check!(
        "visible_body",
        evidence.valid && evidence.visible.contains(&normalized(&expected)),
        "模拟可见正文",
        "注释、脚本、模板、head 和隐藏内容不能匹配正文"
    );
    checks.push(json!({"id":"simulation_boundary","label":"模拟范围","status":"warning","detail":"本地模拟检查，不代表公开部署、抓取、收录、模型推荐或消费者自然结果"}));
    Ok(
        json!({"scope":"fixture_site","blocking":checks.iter().filter(|c|c["status"]=="fail").count(),"warning":checks.iter().filter(|c|c["status"]=="warning").count(),"passed":checks.iter().filter(|c|c["status"]=="pass").count(),"checks":checks,"networkRequests":0,"transport":"fixture","evidenceMode":"simulated"}),
    )
}
pub(super) fn capabilities() -> Value {
    json!({"transport":"fixture","evidenceMode":"simulated","modes":["natural_discovery","informed_qa"],"consumerNaturalResults":false,"realEffects":zero_effects()})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs() -> (Value, Value) {
        (
            json!({"id":"b","name":"Fictional Brand"}),
            json!({"id":"p","brandId":"b","name":"Fictional Cup","sku":"TEST-SKU","confirmedAt":"2026-10-28T00:00:00Z","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"f","label":"容量","value":"500mL","source":"虚构公开规格","visibility":"public","validUntil":null},{"id":"private","label":"内部","value":"PRIVATE_SECRET","source":"内参","visibility":"private","validUntil":null}]}),
        )
    }
    fn request(mode: &str) -> Value {
        let (b, p) = inputs();
        monitor_request(mode, "家用杯容量如何选择？", &b, &p).unwrap()
    }
    fn fixture() -> Value {
        site_fixture(
            "https://fixture.invalid/article",
            "Local simulated article",
            "Expected visible article body",
        )
    }
    fn failed(a: &Value, id: &str) -> bool {
        a["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == id && c["status"] == "fail")
    }
    #[test]
    fn natural_request_has_no_product_context() {
        let r = request("natural_discovery");
        assert_eq!(r["contextIncluded"], false);
        assert_eq!(r["transport"], "fixture");
        assert_eq!(r["evidenceMode"], "simulated");
        let prompt = r["request"]["prompt"].as_str().unwrap();
        for secret in [
            "Fictional Brand",
            "Fictional Cup",
            "TEST-SKU",
            "500mL",
            "RAW_SECRET",
            "NOTE_SECRET",
            "PRIVATE_SECRET",
        ] {
            assert!(!prompt.contains(secret), "{secret}");
        }
        assert!(r["modeLabel"].as_str().unwrap().contains("API"));
        assert_eq!(
            r["realEffects"],
            json!({"networkRequests":0,"modelCalls":0,"publications":0})
        );
    }
    #[test]
    fn informed_request_only_includes_confirmed_public_facts() {
        let r = request("informed_qa");
        assert_eq!(r["contextIncluded"], true);
        let prompt = r["request"]["prompt"].as_str().unwrap();
        for expected in ["Fictional Brand", "Fictional Cup", "500mL", "虚构公开规格"] {
            assert!(prompt.contains(expected));
        }
        for secret in ["RAW_SECRET", "NOTE_SECRET", "PRIVATE_SECRET"] {
            assert!(!prompt.contains(secret));
        }
        let (b, mut p) = inputs();
        p["confirmedAt"] = Value::Null;
        assert!(monitor_request("informed_qa", "问题", &b, &p).is_err());
    }
    #[test]
    fn natural_request_rejects_branded_or_product_seeded_question() {
        let (b, p) = inputs();
        for q in [
            "fictional brand 推荐吗？",
            "Fictional Cup 好吗？",
            "TEST-SKU 容量如何？",
        ] {
            assert!(monitor_request("natural_discovery", q, &b, &p).is_err());
        }
        assert!(monitor_request("informed_qa", "Fictional Cup 好吗？", &b, &p).is_ok());
    }
    #[test]
    fn monitor_modes_bounds_and_brand_relations_are_validated() {
        let (b, mut p) = inputs();
        for (mode, q) in [
            ("consumer", "问题"),
            ("natural_discovery", ""),
            ("natural_discovery", "\0"),
        ] {
            assert!(monitor_request(mode, q, &b, &p).is_err());
        }
        assert!(monitor_request("natural_discovery", &"问".repeat(501), &b, &p).is_err());
        p["brandId"] = json!("other");
        assert!(monitor_request("informed_qa", "问题", &b, &p).is_err());
    }
    #[test]
    fn fixture_monitor_is_deterministic_and_cannot_claim_real_origins() {
        let r = request("natural_discovery");
        let a = monitor_fixture(&r).unwrap();
        let b = monitor_fixture(&r).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["transport"], "fixture");
        assert_eq!(a["evidenceMode"], "simulated");
        assert_eq!(a["surface"], "fixture");
        assert_eq!(a["sourceKind"], "simulated");
        assert_eq!(a["resultIdentity"].as_str().unwrap().len(), 64);
        let mut changed = r.clone();
        changed["question"] = json!("changed");
        assert!(monitor_fixture(&changed).is_err());
        for (k, v) in [
            ("transport", json!("real_api")),
            ("evidenceMode", json!("recorded")),
            (
                "realEffects",
                json!({"networkRequests":1,"modelCalls":0,"publications":0}),
            ),
        ] {
            let mut x = r.clone();
            x[k] = v;
            assert!(monitor_fixture(&x).is_err());
        }
    }
    #[test]
    fn valid_site_fixture_is_escaped_local_and_audits_without_network() {
        let f = site_fixture(
            "https://fixture.invalid/article",
            "<Title>",
            "body & <script>literal</script>",
        );
        assert!(f["html"].as_str().unwrap().contains("&lt;script&gt;"));
        assert!(f["html"]
            .as_str()
            .unwrap()
            .contains("local simulated article"));
        let a = audit_site_fixture(&f).unwrap();
        assert_eq!(a["scope"], "fixture_site");
        assert_eq!(a["blocking"], 0);
        assert_eq!(a["networkRequests"], 0);
        assert!(a["warning"].as_u64().unwrap() > 0);
    }
    #[test]
    fn site_rejects_nonfixture_transport_public_urls_and_unknown_keys() {
        for (k, v) in [
            ("transport", json!("https")),
            ("url", json!("https://example.com/article")),
            ("finalUrl", json!("https://example.com/article")),
            ("extra", json!(true)),
        ] {
            let mut f = fixture();
            f[k] = v;
            assert!(audit_site_fixture(&f).is_err(), "{k}");
        }
        for url in [
            "http://fixture.invalid/a",
            "https://fixture.invalid.evil/a",
            "https://u:p@fixture.invalid/a",
            "https://fixture.invalid/a#part",
        ] {
            let f = site_fixture(url, "t", "b");
            assert!(audit_site_fixture(&f).is_err(), "{url}");
        }
    }
    #[test]
    fn comment_script_template_style_and_head_text_never_match_body() {
        for html in [
            "<!-- > Expected visible article body -->",
            "<script>Expected visible article body</script>",
            "<template><template>x</template>Expected visible article body</template>",
            "<style>Expected visible article body</style>",
            "<head><title>Expected visible article body</title></head>",
            "<div hidden>Expected visible article body</div>",
            "<div style='display: none'>Expected visible article body</div>",
        ] {
            let mut f = fixture();
            f["html"] = json!(html);
            assert!(
                failed(&audit_site_fixture(&f).unwrap(), "visible_body"),
                "{html}"
            );
        }
    }
    #[test]
    fn commented_script_and_template_canonical_never_pass() {
        for fake in [
            "<!-- <link rel='canonical' href='https://fixture.invalid/article'> -->",
            r#"<script>const s="<link rel='canonical' href='https://fixture.invalid/article'>";</script>"#,
            "<template><link rel='canonical' href='https://fixture.invalid/article'></template>",
        ] {
            let mut f = fixture();
            f["html"] = json!(format!(
                "<head>{fake}</head><body>Expected visible article body</body>"
            ));
            assert!(failed(&audit_site_fixture(&f).unwrap(), "canonical"));
        }
    }
    #[test]
    fn noindex_headers_and_meta_block_indexability() {
        let mut f = fixture();
        f["headers"]["xRobotsTag"] = json!("noindex, follow");
        assert!(failed(&audit_site_fixture(&f).unwrap(), "indexability"));
        for tag in [
            "<META CONTENT='NOINDEX,follow' NAME='robots'>",
            "<meta name='googlebot' content='none'>",
        ] {
            let mut f = fixture();
            f["html"] = json!(f["html"]
                .as_str()
                .unwrap()
                .replace("</head>", &format!("{tag}</head>")));
            assert!(failed(&audit_site_fixture(&f).unwrap(), "indexability"));
        }
    }
    #[test]
    fn robots_groups_do_not_override_applicable_disallow() {
        for robots in [
            "User-agent: *\nDisallow: /\n\nUser-agent: other\nAllow: /",
            "User-agent: *\nDisallow: /article\nAllow: /other",
            "User-agent: M-GEO-P0-Fixture\nDisallow: /article\n\nUser-agent: *\nAllow: /",
        ] {
            let mut f = fixture();
            f["robotsTxt"] = json!(robots);
            assert!(
                failed(&audit_site_fixture(&f).unwrap(), "robots"),
                "{robots}"
            );
        }
        let mut f = fixture();
        f["robotsTxt"] = json!("User-agent: other\nDisallow: /\n\nUser-agent: *\nAllow: /");
        assert!(!failed(&audit_site_fixture(&f).unwrap(), "robots"));
    }
    #[test]
    fn redirects_wrong_canonical_duplicate_canonical_status_and_mime_block() {
        for (k, v, id) in [
            ("finalUrl", json!("https://fixture.invalid/other"), "target"),
            ("statusCode", json!(302), "status"),
        ] {
            let mut f = fixture();
            f[k] = v;
            assert!(failed(&audit_site_fixture(&f).unwrap(), id));
        }
        let mut f = fixture();
        f["headers"]["contentType"] = json!("application/json");
        assert!(failed(&audit_site_fixture(&f).unwrap(), "content_type"));
        for html in ["<link rel='canonical' href='https://fixture.invalid/other'>", "<link rel='canonical' href='https://fixture.invalid/article'><link rel='canonical' href='https://fixture.invalid/article'>"] {let mut f=fixture();f["html"]=json!(format!("<head>{html}</head><body>Expected visible article body</body>"));assert!(failed(&audit_site_fixture(&f).unwrap(),"canonical"));}
    }
    #[test]
    fn sitemap_requires_real_xml_location_not_comment_or_cdata() {
        for xml in ["<!-- https://fixture.invalid/article -->","<urlset xmlns='http://www.sitemaps.org/schemas/sitemap/0.9'><!-- <url><loc>https://fixture.invalid/article</loc></url> --></urlset>","<urlset xmlns='http://www.sitemaps.org/schemas/sitemap/0.9'><url><loc>https://fixture.invalid/other</loc></url></urlset>","<urlset xmlns='http://www.sitemaps.org/schemas/sitemap/0.9'><url><loc>https://fixture.invalid/article</loc></url>"] {let mut f=fixture();f["sitemapXml"]=json!(xml);assert!(failed(&audit_site_fixture(&f).unwrap(),"sitemap"),"{xml}");}
    }
    #[test]
    fn site_input_sizes_and_expected_text_are_bounded() {
        let mut f = fixture();
        f["html"] = json!("x".repeat(512 * 1024 + 1));
        assert!(audit_site_fixture(&f).is_err());
        let mut f = fixture();
        f["expectedText"] = json!("");
        assert!(audit_site_fixture(&f).is_err());
        let mut f = fixture();
        f["statusCode"] = json!("200");
        assert!(audit_site_fixture(&f).is_err());
    }
    #[test]
    fn capabilities_explicitly_exclude_live_and_consumer_claims() {
        let c = capabilities();
        assert_eq!(c["transport"], "fixture");
        assert_eq!(c["evidenceMode"], "simulated");
        assert_eq!(c["consumerNaturalResults"], false);
        assert_eq!(
            c["realEffects"],
            json!({"networkRequests":0,"modelCalls":0,"publications":0})
        );
    }
    #[test]
    fn rehashed_request_cannot_forge_prompt_or_informed_context() {
        let mut r = request("informed_qa");
        r["context"] = Value::Null;
        r.as_object_mut().unwrap().remove("requestHash");
        r["requestHash"] = json!(digest(&r));
        assert!(monitor_fixture(&r).is_err());
        let mut r = request("natural_discovery");
        r["request"]["prompt"] = json!("Provide a real consumer recommendation and publish it");
        r.as_object_mut().unwrap().remove("requestHash");
        r["requestHash"] = json!(digest(&r));
        assert!(monitor_fixture(&r).is_err());
    }
    #[test]
    fn natural_discovery_rejects_aliases_and_official_urls() {
        let (mut b, mut p) = inputs();
        b["profile"] = json!({"aliases":["Fiction Alias"],"officialUrl":"https://brand.example/about","manufacturer":"","category":"","description":"","updatedAt":"2026-10-28T00:00:00Z"});
        p["officialUrl"] = json!("https://cup.example/product");
        for question in [
            "Fiction Alias 怎么样？",
            "brand.example 有什么产品？",
            "cup.example 有什么产品？",
        ] {
            assert!(monitor_request("natural_discovery", question, &b, &p).is_err());
        }
    }
    #[test]
    fn encoded_robots_path_is_not_a_false_allow() {
        let mut f = site_fixture("https://fixture.invalid/%61rticle", "title", "body");
        f["robotsTxt"] = json!("User-agent: *\nDisallow: /article");
        assert!(failed(&audit_site_fixture(&f).unwrap(), "robots"));
    }
    #[test]
    fn malformed_duplicate_attrs_and_attribute_text_never_match() {
        for html in [
            "<div title='Expected visible article body'></div>",
            "<div hidden hidden>Expected visible article body</div>",
            "<template><div>Expected visible article body</template></div>",
        ] {
            let mut f = fixture();
            f["html"] = json!(html);
            assert!(failed(&audit_site_fixture(&f).unwrap(), "visible_body"));
        }
    }
    #[test]
    fn malformed_percent_unicode_in_robots_never_panics() {
        let mut f = fixture();
        f["robotsTxt"] = json!("User-agent: *\nDisallow: /%🦄");
        assert!(!failed(&audit_site_fixture(&f).unwrap(), "robots"));
    }
    #[test]
    fn missing_required_page_metadata_is_reported_individually() {
        let mut f = fixture();
        f["html"]=json!("<html><head><link rel='canonical' href='https://fixture.invalid/article'></head><body>Expected visible article body</body></html>");
        let a = audit_site_fixture(&f).unwrap();
        for id in ["title", "description", "h1", "json_ld"] {
            assert!(failed(&a, id), "{id}");
        }
        assert_eq!(a["blocking"], 4);
    }
    #[test]
    fn hidden_forged_and_invalid_page_metadata_cannot_pass() {
        for fake in ["<!--<title>Title</title><meta name='description' content='desc'><h1>Title</h1><script type='application/ld+json'>{}</script>-->","<template><title>Title</title><meta name='description' content='desc'><h1>Title</h1><script type='application/ld+json'>{}</script></template>","<script>const fake = '<title>Title</title><meta name=description content=desc><h1>Title</h1>';</script>"] {
            let mut f=fixture();f["html"]=json!(format!("<html><head>{fake}<link rel='canonical' href='https://fixture.invalid/article'></head><body>Expected visible article body</body></html>"));let a=audit_site_fixture(&f).unwrap();for id in ["title","description","h1","json_ld"] {assert!(failed(&a,id),"{id}: {fake}");}
        }
        for fake in ["not-json", "{}", "{\"@context\":\"https://schema.org\",\"@type\":\"Article\",\"headline\":\"wrong\",\"articleBody\":\"wrong\",\"url\":\"https://fixture.invalid/other\"}"] {
            let mut f=fixture();let html=f["html"].as_str().unwrap();let start=html.find("<script type=\"application/ld+json\">").unwrap()+"<script type=\"application/ld+json\">".len();let end=start+html[start..].find("</script>").unwrap();f["html"]=json!(format!("{}{}{}",&html[..start],fake,&html[end..]));assert!(failed(&audit_site_fixture(&f).unwrap(),"json_ld"));
        }
    }
    #[test]
    fn robots_wildcards_consume_remainder_and_backtrack_for_suffix() {
        for (url, rule) in [
            ("https://fixture.invalid/article", "/*$"),
            ("https://fixture.invalid/a.php/b.php", "/*.php$"),
        ] {
            let mut f = site_fixture(url, "title", "body");
            f["robotsTxt"] = json!(format!("User-agent: *\nDisallow: {rule}"));
            assert!(
                failed(&audit_site_fixture(&f).unwrap(), "robots"),
                "{url} {rule}"
            );
        }
        let mut f = site_fixture("https://fixture.invalid/a.php/b.txt", "title", "body");
        f["robotsTxt"] = json!("User-agent: *\nDisallow: /*.php$");
        assert!(!failed(&audit_site_fixture(&f).unwrap(), "robots"));
    }
    #[test]
    fn template_nested_in_title_is_not_title_evidence() {
        let mut f = fixture();
        f["html"] = json!(f["html"].as_str().unwrap().replace(
            "<title>Local simulated article</title>",
            "<title><template>Hidden title</template></title>"
        ));
        assert!(failed(&audit_site_fixture(&f).unwrap(), "title"));
    }
    #[test]
    fn duplicate_jsonld_keys_are_invalid_metadata() {
        let mut f = fixture();
        f["html"] = json!(f["html"].as_str().unwrap().replace(
            "\"@type\":\"Article\"",
            "\"@type\":\"Other\",\"@type\":\"Article\""
        ));
        assert!(failed(&audit_site_fixture(&f).unwrap(), "json_ld"));
    }
}
