//! Помощники для тестов: создают маленькие DOCX и EPUB прямо в тесте.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

fn write_zip(path: &Path, files: &[(&str, String)]) {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    for (name, content) in files {
        z.start_file(*name, SimpleFileOptions::default()).unwrap();
        z.write_all(content.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Создаёт минимальный .docx. `paras` — пары (styleId или "", текст).
/// В styles.xml есть Heading1 (уровень 0), Heading2 (уровень 1) и
/// «локализованный» стиль a1 с уровнем 0.
pub fn make_docx(dir: &Path, title: Option<&str>, paras: &[(&str, &str)]) -> PathBuf {
    let body: String = paras
        .iter()
        .map(|(style, text)| {
            let ppr = if style.is_empty() {
                String::new()
            } else {
                format!(r#"<w:pPr><w:pStyle w:val="{style}"/></w:pPr>"#)
            };
            format!(r#"<w:p>{ppr}<w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#, xml_escape(text))
        })
        .collect();
    let ns = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let document = format!(r#"<?xml version="1.0" encoding="UTF-8"?><w:document {ns}><w:body>{body}</w:body></w:document>"#);
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:styles {ns}>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="a1"><w:name w:val="Заголовок 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>
</w:styles>"#
    );
    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let mut files = vec![
        ("[Content_Types].xml", content_types.to_string()),
        ("word/document.xml", document),
        ("word/styles.xml", styles),
    ];
    if let Some(t) = title {
        files.push((
            "docProps/core.xml",
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title></cp:coreProperties>"#, xml_escape(t)),
        ));
    }
    let path = dir.join("book.docx");
    write_zip(&path, &files);
    path
}
