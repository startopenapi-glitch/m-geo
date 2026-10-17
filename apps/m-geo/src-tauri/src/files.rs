use std::io::{Cursor, Read};
const MAX_INPUT: usize = 2 * 1024 * 1024;
pub fn decode_document(name: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT {
        return Err("请选择非空文件，最大2MB".into());
    }
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if extension == "docx" {
        return docx(bytes);
    }
    if !["txt", "md", "markdown", "csv", "json"].contains(&extension.as_str()) {
        return Err("仅支持 TXT、Markdown、DOCX、CSV 或迁移 JSON 文件".into());
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (s, _, errors) = encoding_rs::GB18030.decode(bytes);
            if errors {
                return Err("文件编码无法识别，请另存为UTF-8后重试".into());
            }
            s.into_owned()
        }
    };
    let text = text.trim_start_matches('\u{feff}').to_string();
    if text.trim().is_empty() || text.contains('\0') {
        return Err("文件为空或不是有效文本".into());
    }
    Ok(text)
}
fn docx(bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "DOCX文件无法解析")?;
    if archive.len() > 200 {
        return Err("DOCX包含过多条目，请导出为文本再导入".into());
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|_| "DOCX条目无效")?;
        total = total.checked_add(entry.size()).ok_or("DOCX体积异常")?;
        if total > 16 * 1024 * 1024
            || entry.size() > 8 * 1024 * 1024
            || entry.enclosed_name().is_none()
            || entry.name().to_lowercase().contains("vbaproject")
        {
            return Err("DOCX包含不支持的内容或解压后过大".into());
        }
    }
    let entry = archive
        .by_name("word/document.xml")
        .map_err(|_| "DOCX缺少正文")?;
    let mut xml = String::new();
    entry
        .take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut xml)
        .map_err(|_| "DOCX正文编码无效")?;
    if xml.len() > 8 * 1024 * 1024 {
        return Err("DOCX正文过大".into());
    }
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut out = String::new();
    let mut inside_text = false;
    loop {
        match reader.read_event().map_err(|_| "DOCX正文结构无效")? {
            quick_xml::events::Event::DocType(_) => {
                return Err("DOCX包含不支持的外部实体定义".into())
            }
            quick_xml::events::Event::Start(e) => {
                if e.local_name().as_ref() == b"t" {
                    inside_text = true
                }
            }
            quick_xml::events::Event::End(e) => {
                if e.local_name().as_ref() == b"t" {
                    inside_text = false
                }
                if e.local_name().as_ref() == b"p" {
                    out.push('\n')
                }
            }
            quick_xml::events::Event::Text(t) if inside_text => {
                out.push_str(&t.unescape().map_err(|_| "DOCX文字无效")?)
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    if out.trim().is_empty() || out.chars().count() > 100000 {
        return Err("DOCX正文为空或超过100000字，请分段导入".into());
    }
    Ok(out.trim().into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_chinese_and_rejects_non_documents() {
        assert_eq!(
            decode_document("facts.txt", "商品资料".as_bytes()).unwrap(),
            "商品资料"
        );
        assert!(decode_document("payload.exe", b"abc").is_err());
        assert!(decode_document("empty.txt", b"").is_err());
    }
    #[test]
    fn caps_input_size() {
        assert!(decode_document("big.txt", &vec![b'a'; 2 * 1024 * 1024 + 1]).is_err());
    }
    #[test]
    fn parses_docx_text_but_rejects_dtd() {
        use std::io::{Cursor, Write};
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(b"<w:document xmlns:w=\"x\"><w:p><w:r><w:t>Hello &amp; world</w:t></w:r></w:p></w:document>").unwrap();
        let bytes = z.finish().unwrap().into_inner();
        assert!(decode_document("facts.docx", &bytes)
            .unwrap()
            .contains("Hello & world"));
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(b"<!DOCTYPE foo SYSTEM 'file:///etc/passwd'><w:document/>")
            .unwrap();
        assert!(decode_document("bad.docx", &z.finish().unwrap().into_inner()).is_err());
    }
}
