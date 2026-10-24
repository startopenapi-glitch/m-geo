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
    let mut inside_run = false;
    let mut has_table = false;
    loop {
        match reader.read_event().map_err(|_| "DOCX正文结构无效")? {
            quick_xml::events::Event::DocType(_) => {
                return Err("DOCX包含不支持的外部实体定义".into())
            }
            quick_xml::events::Event::Start(e) => match e.local_name().as_ref() {
                b"t" => inside_text = true,
                b"r" => inside_run = true,
                b"tbl" => has_table = true,
                b"tab" if inside_run => out.push('\t'),
                b"br" | b"cr" if inside_run => out.push('\n'),
                _ => {}
            },
            quick_xml::events::Event::Empty(e) => match e.local_name().as_ref() {
                b"tab" if inside_run => out.push('\t'),
                b"br" | b"cr" if inside_run => out.push('\n'),
                b"p" => out.push('\n'),
                _ => {}
            },
            quick_xml::events::Event::End(e) => match e.local_name().as_ref() {
                b"t" => inside_text = false,
                b"r" => inside_run = false,
                b"p" => out.push('\n'),
                b"tc" => {
                    // The final paragraph terminates the cell; keep earlier paragraphs.
                    if out.ends_with('\n') {
                        out.pop();
                    }
                    out.push('\t');
                }
                b"tr" => {
                    // Separate rows without adding a spurious trailing column.
                    if out.ends_with('\t') {
                        out.pop();
                    }
                    out.push('\n');
                }
                _ => {}
            },
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
    // In table projections, edge tabs encode empty columns and text whitespace matters.
    // Remove only the final paragraph/row delimiter; retain the old trim for plain prose.
    if has_table {
        Ok(out.strip_suffix('\n').unwrap_or(&out).into())
    } else {
        Ok(out.trim().into())
    }
}
/// Encodes only domain-generated paths; never accepts arbitrary filesystem paths.
pub fn site_zip(package: &serde_json::Value) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let files = package["files"].as_array().ok_or("网站包格式错误")?;
    if files.len() > 110 {
        return Err("网站文件过多".into());
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut size = 0;
    for file in files {
        let path = file["path"].as_str().ok_or("文件名缺失")?;
        if path.starts_with('/') || path.contains("..") || path.contains('\\') || path.contains(':')
        {
            return Err("网站文件路径不安全".into());
        }
        let content = file["content"].as_str().ok_or("文件内容缺失")?;
        size += content.len();
        if size > 16 * 1024 * 1024 {
            return Err("网站包过大".into());
        }
        out.start_file(
            path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(|_| "网站压缩失败")?;
        out.write_all(content.as_bytes())
            .map_err(|_| "网站写入失败")?;
    }
    Ok(out.finish().map_err(|_| "网站包未完成")?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document_zip(body: &str) -> Vec<u8> {
        use std::io::Write;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(body.as_bytes()).unwrap();
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn preserves_docx_inline_tabs_and_line_breaks() {
        let bytes = document_zip(
            r#"<word:document xmlns:word="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <word:body><word:p><word:r>
                    <word:t>名称</word:t><word:tab/><word:t>价格</word:t>
                    <word:br/><word:t>第一行</word:t><word:cr/><word:t>第二行</word:t>
                    <word:tab></word:tab><word:t xml:space="preserve"> A &amp; </word:t>
                    <word:t>B</word:t><word:br></word:br><word:t>结束</word:t>
                </word:r></word:p></word:body>
            </word:document>"#,
        );
        assert_eq!(
            decode_document("facts.docx", &bytes).unwrap(),
            "名称\t价格\n第一行\n第二行\t A & B\n结束"
        );
    }

    #[test]
    fn ignores_docx_tab_stop_formatting_outside_runs() {
        let bytes = document_zip(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body>
                    <w:p><w:r><w:t>开头</w:t></w:r></w:p>
                    <w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr>
                        <w:r><w:t>名称</w:t><w:tab/><w:t>价格</w:t></w:r></w:p>
                </w:body>
            </w:document>"#,
        );
        assert_eq!(
            decode_document("facts.docx", &bytes).unwrap(),
            "开头\n名称\t价格"
        );
    }

    #[test]
    fn preserves_docx_table_cells_rows_and_empty_cells() {
        let bytes = document_zip(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body><w:tbl>
                    <w:tr>
                        <w:tc><w:p><w:r><w:t>名称</w:t></w:r></w:p></w:tc>
                        <w:tc><w:p><w:r><w:t>价格</w:t></w:r></w:p></w:tc>
                    </w:tr>
                    <w:tr>
                        <w:tc><w:p><w:r><w:t>商品</w:t></w:r></w:p>
                            <w:p><w:r><w:t>说明</w:t></w:r></w:p></w:tc>
                        <w:tc><w:p/></w:tc>
                        <w:tc><w:p><w:r><w:t>10元</w:t></w:r></w:p></w:tc>
                    </w:tr>
                </w:tbl></w:body>
            </w:document>"#,
        );
        assert_eq!(
            decode_document("facts.docx", &bytes).unwrap(),
            "名称\t价格\n商品\n说明\t\t10元"
        );
    }

    #[test]
    fn preserves_docx_table_edge_empty_cells_and_text_whitespace() {
        let bytes = document_zip(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body><w:tbl>
                    <w:tr>
                        <w:tc><w:p/></w:tc>
                        <w:tc><w:p><w:r><w:t xml:space="preserve"> 商品 </w:t></w:r></w:p></w:tc>
                    </w:tr>
                    <w:tr>
                        <w:tc><w:p><w:r><w:t xml:space="preserve"> 商品 </w:t></w:r></w:p></w:tc>
                        <w:tc><w:p/></w:tc>
                    </w:tr>
                </w:tbl></w:body>
            </w:document>"#,
        );
        assert_eq!(
            decode_document("facts.docx", &bytes).unwrap(),
            "\t 商品 \n 商品 \t"
        );
    }

    #[test]
    fn trims_docx_without_tables_as_before() {
        let bytes = document_zip(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body><w:p><w:r><w:t xml:space="preserve">   普通正文 </w:t><w:tab/></w:r></w:p></w:body>
            </w:document>"#,
        );
        assert_eq!(decode_document("facts.docx", &bytes).unwrap(), "普通正文");
    }

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
