//! Bounded, deterministic local helpers. This module has no I/O capabilities.
use serde_json::{json, Value};

const MAX_HTML_BYTES: usize = 256 * 1024;
const MAX_HTML_TAGS: usize = 10_000;
const REWRITE_HEADING: &str = "## 原文资料（本地结构整理）\n\n";

pub(super) fn simulate_monitor(
    questions: &[String],
    repetitions: u32,
    brand_name: &str,
    aliases: &[String],
    product_name: &str,
) -> Result<Value, String> {
    if questions.is_empty()
        || questions.len() > 10
        || !(1..=3).contains(&repetitions)
        || questions.iter().any(|q| !bounded_text(q, 500))
        || !bounded_text(brand_name, 500)
        || !bounded_text(product_name, 500)
        || aliases.len() > 20
        || aliases.iter().any(|alias| !bounded_text(alias, 500))
    {
        return Err("模拟问题、重复次数或品牌商品资料为空或超过上限".into());
    }
    // The same fixture is used on both rounds. Nothing here observes a real model.
    let display_brand = aliases.first().map(String::as_str).unwrap_or(brand_name);
    let mut samples = Vec::with_capacity(questions.len() * repetitions as usize);
    let mut mention_count = 0;
    let mut citation_count = 0;
    for question in questions {
        for _ in 0..repetitions {
            let answer = format!(
                "【模拟夹具，非真实模型回答】问题：{question}\n本地虚构候选：{display_brand} / {product_name}。\n虚构引用：https://example.invalid/m-geo/fixture 。仅用于离线流程演示，不代表实际推荐、效果或引用。"
            );
            let normalized = answer.to_lowercase();
            let mentioned = std::iter::once(brand_name)
                .chain(aliases.iter().map(String::as_str))
                .any(|name| normalized.contains(&name.trim().to_lowercase()));
            let citations = ["https://example.invalid/m-geo/fixture"];
            mention_count += usize::from(mentioned);
            citation_count += citations.len();
            samples.push(json!({
                "question": question, "answer": answer, "citations": citations,
                "mentioned": mentioned, "evidenceMode": "simulated", "surface": "fixture"
            }));
        }
    }
    Ok(json!({
        "sampleCount": samples.len(), "mentionCount": mention_count,
        "citationCount": citation_count, "samples": samples,
        "networkRequests": 0, "modelCalls": 0
    }))
}

fn bounded_text(value: &str, max_chars: usize) -> bool {
    !value.trim().is_empty() && !value.contains('\0') && value.chars().count() <= max_chars
}

pub(super) fn rewrite_body(original: &str) -> Result<String, String> {
    if original.trim().is_empty()
        || original.contains('\0')
        || original.len() > 30_000 - REWRITE_HEADING.len()
    {
        return Err("待整理原文为空、包含空字符或超过稿件长度上限".into());
    }
    // Do not split, trim, rephrase or normalize even one original claim/line ending.
    Ok(format!("{REWRITE_HEADING}{original}"))
}

struct Tag<'a> {
    name: &'a str,
    closing: bool,
    attrs: Vec<(&'a str, &'a str)>,
}

impl Tag<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
    }
}

// A deliberately small scanner, not a browser/DOM parser. ASCII comparisons leave
// UTF-8 byte offsets unchanged. Quoted '>' characters never terminate a tag.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, byte) in html.as_bytes()[start..].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(q), b) if q == b => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn parse_tag(source: &str) -> Tag<'_> {
    let bytes = source.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    let closing = bytes.get(pos) == Some(&b'/');
    if closing {
        pos += 1;
    }
    let start = pos;
    while pos < bytes.len()
        && (bytes[pos].is_ascii_alphanumeric() || matches!(bytes[pos], b'-' | b':' | b'_'))
    {
        pos += 1;
    }
    let name = &source[start..pos];
    let mut attrs = Vec::new();
    while pos < bytes.len() {
        while pos < bytes.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b'/') {
            pos += 1;
        }
        let start = pos;
        while pos < bytes.len()
            && !bytes[pos].is_ascii_whitespace()
            && !matches!(bytes[pos], b'=' | b'/')
        {
            pos += 1;
        }
        let key = &source[start..pos];
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let mut value = "";
        if bytes.get(pos) == Some(&b'=') {
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if let Some(quote @ (b'\'' | b'"')) = bytes.get(pos).copied() {
                pos += 1;
                let start = pos;
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                value = &source[start..pos];
                if pos < bytes.len() {
                    pos += 1;
                }
            } else {
                let start = pos;
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                value = &source[start..pos];
            }
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
        // An isolated '=' is malformed but must not make the scanner loop forever.
        if pos == start {
            pos += 1;
        }
    }
    Tag {
        name,
        closing,
        attrs,
    }
}

fn raw_close(lower: &str, start: usize, name: &str) -> Option<usize> {
    let marker = format!("</{name}");
    let mut pos = start;
    while let Some(offset) = lower[pos..].find(&marker) {
        let found = pos + offset;
        let next = lower.as_bytes().get(found + marker.len()).copied();
        if next.is_some_and(|b| b.is_ascii_whitespace() || b == b'>') {
            return Some(found);
        }
        pos = found + marker.len();
    }
    None
}

pub(super) fn audit_html(html: &str) -> Result<Value, String> {
    if html.trim().is_empty() || html.len() > MAX_HTML_BYTES || html.contains('\0') {
        return Err("HTML 为空、包含空字符或超过 256KiB 上限".into());
    }
    let lower = html.to_ascii_lowercase();
    let (mut pos, mut tags) = (0, 0);
    let (mut in_h1, mut in_body) = (false, false);
    let mut template_depth = 0usize;
    let (mut title, mut h1, mut body) = (String::new(), String::new(), String::new());
    let mut description = false;
    let (mut json_count, mut json_invalid) = (0, false);
    while pos < html.len() {
        let next = html[pos..]
            .find('<')
            .map(|offset| pos + offset)
            .unwrap_or(html.len());
        let text = &html[pos..next];
        if template_depth == 0 && in_h1 {
            h1.push_str(text);
        }
        if template_depth == 0 && in_body {
            body.push_str(text);
        }
        if next == html.len() {
            break;
        }
        if html[next..].starts_with("<!--") {
            pos = html[next + 4..]
                .find("-->")
                .map(|offset| next + 4 + offset + 3)
                .unwrap_or(html.len());
            continue;
        }
        tags += 1;
        if tags > MAX_HTML_TAGS {
            return Err("HTML 标签数量超过本地检查上限".into());
        }
        let end = tag_end(html, next + 1).ok_or("HTML 标签未闭合，无法安全检查")?;
        let tag = parse_tag(&html[next + 1..end]);
        pos = end + 1;
        let name = tag.name.to_ascii_lowercase();
        if template_depth > 0 {
            match name.as_str() {
                "template" if tag.closing => template_depth -= 1,
                "template" => template_depth += 1,
                "script" | "style" | "title" | "textarea" | "noscript" if !tag.closing => {
                    // A spelling such as </template> inside raw/RCDATA text does
                    // not close the containing inert template.
                    pos = raw_close(&lower, pos, &name)
                        .and_then(|close| tag_end(html, close + 2))
                        .map(|end| end + 1)
                        .unwrap_or(html.len());
                }
                _ => {}
            }
            continue;
        }
        match name.as_str() {
            "template" if !tag.closing => template_depth = 1,
            "title" | "textarea" if !tag.closing => {
                // RCDATA may spell markup, but it cannot create document tags.
                let close = raw_close(&lower, pos, &name);
                let text = &html[pos..close.unwrap_or(html.len())];
                if name == "title" {
                    title.push_str(text);
                } else if in_body {
                    // Textarea values count as body text, never metadata/headings.
                    body.push_str(text);
                }
                pos = close
                    .and_then(|close| tag_end(html, close + 2))
                    .map(|end| end + 1)
                    .unwrap_or(html.len());
            }
            "h1" => in_h1 = !tag.closing,
            "body" => in_body = !tag.closing,
            "meta" if !tag.closing => {
                if tag
                    .attr("name")
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case("description"))
                    && tag
                        .attr("content")
                        .is_some_and(|content| !content.trim().is_empty())
                {
                    description = true;
                }
            }
            "script" | "style" | "noscript" if !tag.closing => {
                let close = raw_close(&lower, pos, &name);
                if name == "script"
                    && tag.attr("type").is_some_and(|value| {
                        value.trim().eq_ignore_ascii_case("application/ld+json")
                    })
                {
                    json_count += 1;
                    let data = &html[pos..close.unwrap_or(html.len())];
                    // serde_json's default recursion limit also bounds adversarial JSON.
                    json_invalid |= close.is_none() || serde_json::from_str::<Value>(data).is_err();
                }
                pos = match close {
                    Some(close) => tag_end(html, close + 2)
                        .map(|end| end + 1)
                        .unwrap_or(html.len()),
                    None => html.len(),
                };
            }
            _ => {}
        }
    }
    let checks = vec![
        audit_check(
            "title",
            "页面标题",
            if title.trim().is_empty() {
                "fail"
            } else {
                "pass"
            },
            "仅检查非空 title 文本，不代表可收录或排名",
        ),
        audit_check(
            "description",
            "页面描述",
            if description { "pass" } else { "warning" },
            "仅检查 description 元数据是否存在且非空",
        ),
        audit_check(
            "h1",
            "一级标题",
            if h1.trim().is_empty() { "fail" } else { "pass" },
            "仅检查非空 h1 文本，不代表内容质量评分",
        ),
        audit_check(
            "body",
            "正文",
            if body.trim().is_empty() {
                "fail"
            } else {
                "pass"
            },
            "仅检查 body 文本；脚本、样式与注释不作为正文",
        ),
        audit_check(
            "json_ld",
            "结构化 JSON-LD 语法",
            if json_invalid {
                "fail"
            } else if json_count == 0 {
                "warning"
            } else {
                "pass"
            },
            "仅解析本地 JSON 语法；不抓取 context、URL，不核验外部实体",
        ),
        audit_check(
            "product_consistency",
            "商品事实一致性提示",
            "warning",
            "此检查未提供已核验商品事实，需人工核对名称、数字与主张；不代表真实性、收录或引用",
        ),
    ];
    let count = |status: &str| {
        checks
            .iter()
            .filter(|check| check["status"] == status)
            .count()
    };
    Ok(json!({
        "scope": "offline_html", "passed": count("pass"),
        "warning": count("warning"), "blocking": count("fail"),
        "checks": checks, "networkRequests": 0
    }))
}

fn audit_check(id: &str, label: &str, status: &str, detail: &str) -> Value {
    json!({"id": id, "label": label, "status": status, "detail": detail})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check<'a>(result: &'a Value, id: &str) -> &'a Value {
        result["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()
    }

    #[test]
    fn monitor_three_questions_twice_have_explicit_simulated_provenance() {
        let questions = vec![
            "如何挑选桌面灯？".into(),
            "小房间适合什么？".into(),
            "如何整理书桌？".into(),
        ];
        let result = simulate_monitor(&questions, 2, "虚构星灯", &[], "纸月灯").unwrap();
        assert_eq!(result["sampleCount"], 6);
        assert_eq!(result["mentionCount"], 6);
        assert_eq!(result["citationCount"], 6);
        assert_eq!(result["networkRequests"], 0);
        assert_eq!(result["modelCalls"], 0);
        let samples = result["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 6);
        for question in questions {
            assert_eq!(
                samples
                    .iter()
                    .filter(|sample| sample["question"] == question)
                    .count(),
                2
            );
        }
        for sample in samples {
            assert_eq!(sample["evidenceMode"], "simulated");
            assert_eq!(sample["surface"], "fixture");
            assert_eq!(sample["mentioned"], true);
            let answer = sample["answer"].as_str().unwrap();
            assert!(answer.contains("模拟"));
            assert!(answer.contains("虚构引用"));
            assert_eq!(
                sample["citations"],
                json!(["https://example.invalid/m-geo/fixture"])
            );
        }
    }

    #[test]
    fn monitor_repeated_baseline_and_followup_are_identical() {
        let questions = vec!["桌面用品怎么选？".into()];
        let baseline = simulate_monitor(&questions, 1, "虚构星灯", &[], "纸月灯").unwrap();
        let followup = simulate_monitor(&questions, 1, "虚构星灯", &[], "纸月灯").unwrap();
        assert_eq!(baseline, followup);
    }

    #[test]
    fn monitor_scores_local_alias_instead_of_requiring_canonical_brand() {
        let result = simulate_monitor(
            &["桌面用品怎么选？".into()],
            1,
            "虚构星灯",
            &["StarLight".into()],
            "纸月灯",
        )
        .unwrap();
        assert!(result["samples"][0]["answer"]
            .as_str()
            .unwrap()
            .contains("StarLight"));
        assert_eq!(result["mentionCount"], 1);
        assert_eq!(result["samples"][0]["mentioned"], true);
    }

    #[test]
    fn monitor_rejects_empty_and_excessive_inputs() {
        for questions in [
            vec![],
            vec![" ".into()],
            vec!["问".repeat(501)],
            vec!["问题".into(); 11],
        ] {
            assert!(simulate_monitor(&questions, 1, "虚构品牌", &[], "虚构商品").is_err());
        }
        for repetitions in [0, 4, u32::MAX] {
            assert!(
                simulate_monitor(&["问题".into()], repetitions, "虚构品牌", &[], "虚构商品")
                    .is_err()
            );
        }
        assert!(simulate_monitor(&["问题".into()], 1, "", &[], "虚构商品").is_err());
        assert!(simulate_monitor(&["问题".into()], 1, "虚构品牌", &[], " ").is_err());
    }

    #[test]
    fn rewrite_retains_original_numeric_claims_and_line_bytes_exactly() {
        let original = "  # 商品公开资料\r\n型号：F-02；容量：300 mL。\r\n\r\n材质：纸；不承诺防水。  \n已核对事实：仅含 2 件，不含电池。\n";
        let rewritten = rewrite_body(original).unwrap();
        assert_ne!(rewritten, original);
        assert!(rewritten.ends_with(original));
        assert_eq!(rewritten.matches(original).count(), 1);
        assert_eq!(rewritten.matches("300 mL").count(), 1);
        assert_eq!(rewritten.matches("仅含 2 件，不含电池").count(), 1);
    }

    #[test]
    fn rewrite_rejects_empty_nul_and_oversized_original() {
        for original in [
            "".to_owned(),
            " \r\n".into(),
            "商品\0内容".into(),
            "x".repeat(30_001),
        ] {
            assert!(rewrite_body(&original).is_err());
        }
    }

    #[test]
    fn audit_handles_uppercase_attributes_without_executing_or_fetching() {
        let result = audit_html(r#"<!doctype html><HTML><HEAD><TITLE>虚构商品</TITLE><META CONTENT='虚构资料' NAME=DESCRIPTION><SCRIPT TYPE='APPLICATION/LD+JSON'>{"@context":"https://schema.org","@type":"Product","name":"虚构商品"}</SCRIPT><SCRIPT SRC='https://example.invalid/never-fetch'>throw new Error('never run')</SCRIPT></HEAD><BODY ONLOAD='neverRun()'><H1>虚构商品</H1><P>本地样例。</P><IMG SRC='https://example.invalid/never-fetch.png'></BODY></HTML>"#).unwrap();
        assert_eq!(result["scope"], "offline_html");
        assert_eq!(result["networkRequests"], 0);
        for id in ["title", "description", "h1", "body", "json_ld"] {
            assert_eq!(check(&result, id)["status"], "pass", "{id}");
        }
        assert_eq!(check(&result, "product_consistency")["status"], "warning");
        assert_eq!(result["blocking"], 0);
        assert_eq!(result["passed"], 5);
        assert_eq!(result["warning"], 1);
        assert!(check(&result, "product_consistency")["detail"]
            .as_str()
            .unwrap()
            .contains("人工"));
    }

    #[test]
    fn audit_missing_fields_are_reported_as_local_hints() {
        let result = audit_html("<html><head></head><body> </body></html>").unwrap();
        for id in ["title", "h1", "body"] {
            assert_eq!(check(&result, id)["status"], "fail", "{id}");
        }
        assert_eq!(check(&result, "description")["status"], "warning");
        assert_eq!(check(&result, "json_ld")["status"], "warning");
        assert_eq!(result["blocking"], 3);
    }

    #[test]
    fn audit_invalid_or_excessively_nested_json_ld_is_blocking() {
        for json_ld in [
            "{invalid}".to_owned(),
            format!("{}0{}", "[".repeat(150), "]".repeat(150)),
        ] {
            let html = format!("<script type='application/ld+json'>{json_ld}</script>");
            let result = audit_html(&html).unwrap();
            assert_eq!(check(&result, "json_ld")["status"], "fail");
        }
    }

    #[test]
    fn audit_ignores_markup_inside_comments_scripts_and_styles() {
        let result = audit_html(r#"<!-- <title>fake</title><h1>fake</h1><meta name='description' content='fake'> --><script>const fake='<title>fake</title><h1>fake</h1>';</script><body><style>h1::before{content:'fake'}</style><script>fake()</script></body>"#).unwrap();
        for id in ["title", "h1", "body"] {
            assert_eq!(check(&result, id)["status"], "fail", "{id}");
        }
        assert_eq!(check(&result, "description")["status"], "warning");
    }

    #[test]
    fn audit_attribute_scanning_preserves_quoted_greater_than_and_name_boundaries() {
        let result = audit_html("<titlex>fake</titlex><bodyx>fake</bodyx><meta xname='description' content='fake'><meta content='a > b' name='description'><body><h1>真实标题</h1>正文</body>").unwrap();
        assert_eq!(check(&result, "title")["status"], "fail");
        assert_eq!(check(&result, "description")["status"], "pass");
        assert_eq!(check(&result, "body")["status"], "pass");
    }

    #[test]
    fn audit_rejects_empty_nul_oversized_and_pathological_inputs() {
        for html in [
            "".to_owned(),
            " ".into(),
            "<body>\0</body>".into(),
            "x".repeat(262_145),
            "<i>".repeat(10_001),
        ] {
            assert!(audit_html(&html).is_err());
        }
        let result = audit_html(&format!("<body>{}</body>", "字".repeat(10_000))).unwrap();
        assert_eq!(check(&result, "body")["status"], "pass");
    }

    #[test]
    fn audit_accepts_exactly_256_kib_and_rejects_the_next_byte() {
        let html = format!("<body>{}</body>", "a".repeat(262_144 - 13));
        assert!(audit_html(&html).is_ok());
        assert!(audit_html(&(html + "a")).is_err());
    }

    #[test]
    fn audit_nested_templates_cannot_supply_document_checks() {
        let result = audit_html("<template><template></template><title>inert</title><meta name=description content=inert><h1>inert</h1><body>inert</body></template>").unwrap();
        for id in ["title", "h1", "body"] {
            assert_eq!(check(&result, id)["status"], "fail", "{id}");
        }
        assert_eq!(check(&result, "description")["status"], "warning");
        assert_eq!(result["passed"], 0);
    }

    #[test]
    fn audit_title_rcdata_cannot_supply_heading_or_body() {
        let result = audit_html(
            "<title><h1>not an actual heading</h1><body>not an actual body</body></title>",
        )
        .unwrap();
        assert_eq!(check(&result, "title")["status"], "pass");
        assert_eq!(check(&result, "h1")["status"], "fail");
        assert_eq!(check(&result, "body")["status"], "fail");
    }

    #[test]
    fn audit_textarea_rcdata_cannot_supply_document_metadata_or_heading() {
        let result = audit_html("<body><textarea><title>not title</title><h1>not heading</h1><meta name=description content=notmeta></textarea></body>").unwrap();
        assert_eq!(check(&result, "title")["status"], "fail");
        assert_eq!(check(&result, "h1")["status"], "fail");
        assert_eq!(check(&result, "description")["status"], "warning");
    }
}
