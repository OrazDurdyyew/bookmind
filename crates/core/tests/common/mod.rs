//! Помощники для тестов: создают маленькие DOCX и EPUB прямо в тесте.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

pub fn write_zip(path: &Path, files: &[(&str, String)]) {
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

/// Один XHTML-документ книги EPUB. `toc_title: None` — документ не в оглавлении.
pub struct EpubDoc<'a> {
    pub toc_title: Option<&'a str>,
    pub body: &'a str,
    /// Дополнительные пункты оглавления, указывающие на якоря внутри документа: (название, id).
    pub anchors: &'a [(&'a str, &'a str)],
}

/// Создаёт EPUB 3 (nav.xhtml + toc.ncx) с главами (название, тело xhtml).
pub fn make_epub(dir: &Path, title: &str, chapters: &[(&str, &str)]) -> PathBuf {
    let docs: Vec<EpubDoc> = chapters.iter().map(|(t, b)| EpubDoc { toc_title: Some(t), body: b, anchors: &[] }).collect();
    make_epub_with(dir, title, &docs, true)
}

/// Полный вариант: `epub3 = false` — только toc.ncx (EPUB 2).
pub fn make_epub_with(dir: &Path, title: &str, docs: &[EpubDoc], epub3: bool) -> PathBuf {
    let mut files: Vec<(String, String)> = vec![
        ("mimetype".into(), "application/epub+zip".into()),
        (
            "META-INF/container.xml".into(),
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.into(),
        ),
    ];
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut nav_items = String::new();
    let mut ncx_points = String::new();
    for (i, d) in docs.iter().enumerate() {
        let name = format!("text/ch{i}.xhtml");
        files.push((
            format!("OEBPS/{name}"),
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>x</title><style>p {{}}</style></head><body>{}</body></html>"#, d.body),
        ));
        manifest.push_str(&format!(r#"<item id="c{i}" href="{name}" media-type="application/xhtml+xml"/>"#));
        spine.push_str(&format!(r#"<itemref idref="c{i}"/>"#));
        if let Some(t) = d.toc_title {
            nav_items.push_str(&format!(r#"<li><a href="{name}">{t}</a></li>"#));
            ncx_points.push_str(&format!(r#"<navPoint id="n{i}"><navLabel><text>{t}</text></navLabel><content src="{name}"/></navPoint>"#));
        }
        for (j, (t, id)) in d.anchors.iter().enumerate() {
            nav_items.push_str(&format!(r#"<li><a href="{name}#{id}">{t}</a></li>"#));
            ncx_points.push_str(&format!(r#"<navPoint id="n{i}a{j}"><navLabel><text>{t}</text></navLabel><content src="{name}#{id}"/></navPoint>"#));
        }
    }
    if epub3 {
        files.push((
            "OEBPS/nav.xhtml".into(),
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol>{nav_items}</ol></nav></body></html>"#),
        ));
        manifest.push_str(r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#);
    }
    files.push((
        "OEBPS/toc.ncx".into(),
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><navMap>{ncx_points}</navMap></ncx>"#),
    ));
    manifest.push_str(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#);
    let version = if epub3 { "3.0" } else { "2.0" };
    files.push((
        "OEBPS/content.opf".into(),
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="{version}"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title></metadata><manifest>{manifest}</manifest><spine toc="ncx">{spine}</spine></package>"#),
    ));
    let refs: Vec<(&str, String)> = files.iter().map(|(n, c)| (n.as_str(), c.clone())).collect();
    let path = dir.join("book.epub");
    write_zip(&path, &refs);
    path
}
