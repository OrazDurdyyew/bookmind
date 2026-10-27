mod common;

use bookmind_core::extract::extract_epub;
use common::{make_epub, make_epub_with, EpubDoc};

fn titles(book: &bookmind_core::ExtractedBook) -> Vec<Option<String>> {
    book.chapters.iter().map(|c| c.title.clone()).collect()
}

#[test]
fn chapters_come_from_toc_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_epub(dir.path(), "Biology", &[("Chapter 1", "<p>Cells.</p>"), ("Chapter 2", "<p>Plants.</p>")]);
    let book = extract_epub(&path).unwrap();
    assert_eq!(book.title, "Biology");
    assert_eq!(titles(&book), vec![Some("Chapter 1".into()), Some("Chapter 2".into())]);
    assert_eq!(book.chapters[1].blocks[0].text, "Plants.");
}

#[test]
fn headings_set_subheading_and_list_items_are_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let body = "<h1>Chapter 1</h1><p>Intro &amp; more.</p><h2>Photosynthesis</h2><p>Light   is\n used.</p><ul><li>Leaves</li></ul><script>var x;</script>";
    let path = make_epub(dir.path(), "B", &[("Chapter 1", body)]);
    let blocks = &extract_epub(&path).unwrap().chapters[0].blocks;
    let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, vec!["Intro & more.", "Light is used.", "Leaves"]);
    assert_eq!(blocks[0].subheading, None);
    assert_eq!(blocks[1].subheading.as_deref(), Some("Photosynthesis"));
    assert_eq!(blocks[2].subheading.as_deref(), Some("Photosynthesis"));
}

#[test]
fn documents_before_first_toc_entry_go_to_chapter_one() {
    let dir = tempfile::tempdir().unwrap();
    let docs = [
        EpubDoc { toc_title: None, body: "<p>Preface.</p>", anchors: &[] },
        EpubDoc { toc_title: Some("Chapter 1"), body: "<p>Body.</p>", anchors: &[] },
    ];
    let book = extract_epub(&make_epub_with(dir.path(), "B", &docs, true)).unwrap();
    assert_eq!(titles(&book), vec![Some("Chapter 1".into())]);
    let texts: Vec<&str> = book.chapters[0].blocks.iter().map(|b| b.text.as_str()).collect();
    assert_eq!(texts, vec!["Preface.", "Body."]);
}

#[test]
fn toc_entry_with_empty_document_makes_no_chapter() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_epub(dir.path(), "B", &[("Cover", ""), ("Chapter 1", "<p>Body.</p>")]);
    assert_eq!(titles(&extract_epub(&path).unwrap()), vec![Some("Chapter 1".into())]);
}

#[test]
fn epub2_with_ncx_only_works() {
    let dir = tempfile::tempdir().unwrap();
    let docs = [
        EpubDoc { toc_title: Some("Bap 1"), body: "<p>Tekst.</p>", anchors: &[] },
        EpubDoc { toc_title: Some("Bap 2"), body: "<p>Başga.</p>", anchors: &[] },
    ];
    let book = extract_epub(&make_epub_with(dir.path(), "Kitap", &docs, false)).unwrap();
    assert_eq!(titles(&book), vec![Some("Bap 1".into()), Some("Bap 2".into())]);
}

#[test]
fn extract_book_dispatches_epub() {
    let dir = tempfile::tempdir().unwrap();
    let path = make_epub(dir.path(), "B", &[("C", "<p>x</p>")]);
    assert!(bookmind_core::extract_book(&path).is_ok());
}

/// Как в книгах Project Gutenberg: один большой файл, главы — якоря внутри него.
#[test]
fn toc_anchors_inside_one_document_split_chapters() {
    let body = r#"<p>Title page.</p><h2 id="c1">CHAPTER I.</h2><p>Cells.</p><div><h2 id="c2">CHAPTER II.</h2></div><p>Bones.</p><p>Muscles.</p>"#;
    for epub3 in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let docs = [EpubDoc { toc_title: None, body, anchors: &[("CHAPTER I.", "c1"), ("CHAPTER II.", "c2")] }];
        let book = extract_epub(&make_epub_with(dir.path(), "B", &docs, epub3)).unwrap();
        assert_eq!(titles(&book), vec![Some("CHAPTER I.".into()), Some("CHAPTER II.".into())], "epub3={epub3}");
        let texts = |i: usize| book.chapters[i].blocks.iter().map(|b| b.text.clone()).collect::<Vec<_>>();
        assert_eq!(texts(0), vec!["Title page.", "Cells."]);
        assert_eq!(texts(1), vec!["Bones.", "Muscles."]);
    }
}

/// Оглавление, где все главы вложены в один пункт верхнего уровня
/// (так устроен nav.xhtml многих книг Project Gutenberg).
#[test]
fn nested_toc_with_single_top_entry_uses_next_level() {
    let dir = tempfile::tempdir().unwrap();
    let body = r#"<p>Preface.</p><h3 id="a">CHAPTER I.</h3><p>One.</p><h3 id="b">CHAPTER II.</h3><p>Two.</p><h3 id="c">CHAPTER III.</h3><p>Three.</p>"#;
    let nav = r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol>
        <li><a href="text.xhtml#top">THE BOOK</a><ol>
            <li><a href="text.xhtml#a">CHAPTER I.</a><ol><li><a href="text.xhtml#a">Section</a></li></ol></li>
            <li><a href="text.xhtml#b">CHAPTER II.</a></li>
            <li><a href="text.xhtml#c">CHAPTER III.</a></li>
        </ol></li></ol></nav></body></html>"#;
    let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>B</dc:title></metadata><manifest><item id="t" href="text.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="t"/></spine></package>"#;
    let container = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#;
    let text = format!(r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><div id="top">{body}</div></body></html>"#);
    let path = dir.path().join("nested.epub");
    common::write_zip(&path, &[
        ("META-INF/container.xml", container.to_string()),
        ("content.opf", opf.to_string()),
        ("nav.xhtml", nav.to_string()),
        ("text.xhtml", text),
    ]);
    let book = extract_epub(&path).unwrap();
    assert_eq!(
        titles(&book),
        vec![Some("THE BOOK".into()), Some("CHAPTER I.".into()), Some("CHAPTER II.".into()), Some("CHAPTER III.".into())]
    );
    assert_eq!(book.chapters[3].blocks[0].text, "Three.");
}
