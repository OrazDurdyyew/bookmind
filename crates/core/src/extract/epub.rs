//! Извлечение текста из EPUB 2/3: главы по оглавлению (nav.xhtml или toc.ncx).

use std::collections::HashMap;
use std::path::Path;

use quick_xml::events::Event;

use super::xml::{attr, collapse_ws, event_text, open_zip, read_entry, reader};
use super::{file_stem, ExtractError};
use crate::model::{Block, Chapter, ExtractedBook};

type Zip = zip::ZipArchive<std::fs::File>;

/// Папка файла внутри архива: "OEBPS/text/a.xhtml" → "OEBPS/text".
fn dir_of(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

/// Ссылка из оглавления: файл и необязательный якорь (#id).
fn split_href(base_dir: &str, href: &str) -> (String, Option<String>) {
    let mut it = href.splitn(2, '#');
    let file = resolve(base_dir, it.next().unwrap_or(""));
    let frag = it.next().filter(|f| !f.is_empty()).map(String::from);
    (file, frag)
}

/// Склеивает относительную ссылку с папкой и убирает "./" и "../".
fn resolve(base_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("").replace("%20", " ");
    let mut parts: Vec<&str> = if base_dir.is_empty() { Vec::new() } else { base_dir.split('/').collect() };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Содержимое OPF: название, файлы spine по порядку, путь к nav и ncx.
struct Opf {
    title: Option<String>,
    spine: Vec<String>,
    nav: Option<String>,
    ncx: Option<String>,
}

fn parse_opf(xml: &str, opf_dir: &str) -> Opf {
    let mut items: HashMap<String, (String, String, String)> = HashMap::new(); // id → (href, properties, media-type)
    let mut spine_ids = Vec::new();
    let mut spine_toc: Option<String> = None;
    let mut title = String::new();
    let mut in_title = false;
    let mut r = reader(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().into_inner() {
                "title" => in_title = true,
                "item" => {
                    if let (Some(id), Some(href)) = (attr(&e, "id"), attr(&e, "href")) {
                        let props = attr(&e, "properties").unwrap_or_default();
                        let media = attr(&e, "media-type").unwrap_or_default();
                        items.insert(id, (resolve(opf_dir, &href), props, media));
                    }
                }
                "spine" => spine_toc = attr(&e, "toc"),
                "itemref" => {
                    if let Some(id) = attr(&e, "idref") {
                        spine_ids.push(id);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().into_inner() == "title" => in_title = false,
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) if in_title => {
                if let Some(t) = event_text(&ev) {
                    title.push_str(&t);
                }
            }
            _ => {}
        }
    }
    let nav = items.values().find(|(_, p, _)| p.split_whitespace().any(|x| x == "nav")).map(|i| i.0.clone());
    let ncx = spine_toc
        .and_then(|id| items.get(&id).map(|i| i.0.clone()))
        .or_else(|| items.values().find(|(_, _, m)| m == "application/x-dtbncx+xml").map(|i| i.0.clone()));
    let spine = spine_ids.iter().filter_map(|id| items.get(id).map(|i| i.0.clone())).collect();
    let title = collapse_ws(&title);
    Opf { title: (!title.is_empty()).then_some(title), spine, nav, ncx }
}

/// Пункт оглавления: название, файл и якорь внутри файла.
struct TocEntry {
    title: String,
    file: String,
    anchor: Option<String>,
    /// Уровень вложенности: 1 — верхний.
    depth: usize,
}

fn toc_entry(title: &str, base_dir: &str, href: &str, depth: usize) -> TocEntry {
    let (file, anchor) = split_href(base_dir, href);
    TocEntry { title: collapse_ws(title), file, anchor, depth }
}

/// Самый мелкий уровень оглавления, на котором не меньше 3 пунктов, — уровень глав.
/// Так книга, где все главы вложены в один пункт «Книга», всё равно делится на главы.
/// Если такого уровня нет (короткое оглавление), берём только верхний уровень.
fn chapter_level(mut toc: Vec<TocEntry>) -> Vec<TocEntry> {
    const MIN_ENTRIES: usize = 3;
    const MAX_DEPTH: usize = 3;
    let level = (1..=MAX_DEPTH).find(|l| toc.iter().filter(|e| e.depth == *l).count() >= MIN_ENTRIES).unwrap_or(1);
    toc.retain(|e| e.depth <= level);
    toc
}

/// Все пункты оглавления из nav.xhtml (с уровнем вложенности).
fn parse_nav(xml: &str, nav_dir: &str) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut in_toc = false;
    let mut ol_depth = 0;
    let mut current: Option<(String, String)> = None; // (href, текст)
    let mut r = reader(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match e.local_name().into_inner() {
                "nav" if attr(&e, "type").is_some_and(|t| t.contains("toc")) => in_toc = true,
                "ol" if in_toc => ol_depth += 1,
                "a" if in_toc => current = Some((attr(&e, "href").unwrap_or_default(), String::new())),
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().into_inner() {
                "nav" => in_toc = false,
                "ol" if in_toc => ol_depth -= 1,
                "a" => {
                    if let Some((href, text)) = current.take() {
                        out.push(toc_entry(&text, nav_dir, &href, ol_depth));
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) => {
                if let (Some((_, text)), Some(t)) = (current.as_mut(), event_text(&ev)) {
                    text.push_str(&t);
                }
            }
        }
    }
    out
}

/// Все пункты из toc.ncx (с уровнем вложенности).
fn parse_ncx(xml: &str, ncx_dir: &str) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut in_text = false;
    let mut label = String::new();
    let mut r = reader(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match e.local_name().into_inner() {
                "navPoint" => {
                    depth += 1;
                    label.clear();
                }
                "text" => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().into_inner() == "content" && depth > 0 => {
                out.push(toc_entry(&label, ncx_dir, &attr(&e, "src").unwrap_or_default(), depth));
            }
            Ok(Event::End(e)) => match e.local_name().into_inner() {
                "navPoint" => depth -= 1,
                "text" => in_text = false,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) if in_text => {
                if let Some(t) = event_text(&ev) {
                    label.push_str(&t);
                }
            }
            _ => {}
        }
    }
    out
}

/// Блок XHTML: абзац текста или подзаголовок (h2–h6).
enum Piece {
    Text(String),
    Heading(String),
    /// Элемент с id, на который ссылается оглавление, — здесь начинается глава.
    Anchor(String),
}

/// Закрывает накопленный текст: абзац, подзаголовок или (для h1) ничего.
fn flush(text: &mut String, heading: Option<u8>, out: &mut Vec<Piece>) {
    let t = collapse_ws(text);
    text.clear();
    if t.is_empty() {
        return;
    }
    match heading {
        Some(1) => {}
        Some(_) => out.push(Piece::Heading(t)),
        None => out.push(Piece::Text(t)),
    }
}

/// Если у элемента id, на который ссылается оглавление, — отмечаем начало главы.
/// Текст до якоря относится к предыдущей главе, поэтому сначала закрываем его.
fn push_anchor(e: &quick_xml::events::BytesStart, anchors: &[&str], text: &mut String, heading: Option<u8>, out: &mut Vec<Piece>) {
    if let Some(id) = attr(e, "id").filter(|id| anchors.contains(&id.as_str())) {
        flush(text, heading, out);
        out.push(Piece::Anchor(id));
    }
}

const BLOCK_TAGS: &[&str] = &["p", "li", "div", "blockquote", "td", "dd", "dt", "pre", "h1", "h2", "h3", "h4", "h5", "h6"];

/// Разбирает XHTML на абзацы и подзаголовки. h1 пропускается:
/// обычно он повторяет название главы из оглавления.
fn parse_xhtml(xml: &str, anchors: &[&str]) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut skip = 0; // внутри head/script/style
    let mut heading: Option<u8> = None;
    let mut r = reader(xml);

    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                if skip == 0 {
                    push_anchor(&e, anchors, &mut text, heading, &mut out);
                }
                let name = e.local_name().into_inner().to_string();
                if matches!(name.as_str(), "head" | "script" | "style") {
                    skip += 1;
                } else if BLOCK_TAGS.contains(&name.as_str()) {
                    flush(&mut text, heading, &mut out);
                    if let Some(level) = name.strip_prefix('h').and_then(|n| n.parse().ok()) {
                        heading = Some(level);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                if skip == 0 {
                    push_anchor(&e, anchors, &mut text, heading, &mut out);
                }
                if e.local_name().into_inner() == "br" {
                    text.push(' ');
                }
            }
            Ok(Event::End(e)) => {
                let name = e.local_name().into_inner().to_string();
                if matches!(name.as_str(), "head" | "script" | "style") {
                    skip -= 1;
                } else if BLOCK_TAGS.contains(&name.as_str()) {
                    flush(&mut text, heading, &mut out);
                    if name.starts_with('h') {
                        heading = None;
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) if skip == 0 => {
                if let Some(t) = event_text(&ev) {
                    text.push_str(&t);
                }
            }
            _ => {}
        }
    }
    flush(&mut text, heading, &mut out);
    out
}

fn container_rootfile(zip: &mut Zip) -> Result<String, ExtractError> {
    let xml = read_entry(zip, "META-INF/container.xml")
        .ok_or_else(|| ExtractError::Corrupt("нет META-INF/container.xml".into()))?;
    let mut r = reader(&xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().into_inner() == "rootfile" => {
                if let Some(p) = attr(&e, "full-path") {
                    return Ok(p);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    Err(ExtractError::Corrupt("в container.xml нет rootfile".into()))
}

pub fn extract_epub(path: &Path) -> Result<ExtractedBook, ExtractError> {
    let mut zip = open_zip(path)?;
    let opf_path = container_rootfile(&mut zip)?;
    let opf_xml = read_entry(&mut zip, &opf_path).ok_or_else(|| ExtractError::Corrupt(format!("нет {opf_path}")))?;
    let opf = parse_opf(&opf_xml, dir_of(&opf_path));

    // Оглавление: сначала nav.xhtml (EPUB 3), иначе toc.ncx (EPUB 2).
    let mut toc = opf
        .nav
        .as_ref()
        .and_then(|p| read_entry(&mut zip, p).map(|x| parse_nav(&x, dir_of(p))))
        .unwrap_or_default();
    if toc.is_empty() {
        toc = opf
            .ncx
            .as_ref()
            .and_then(|p| read_entry(&mut zip, p).map(|x| parse_ncx(&x, dir_of(p))))
            .unwrap_or_default();
    }

    let toc = chapter_level(toc);

    // Раскладываем документы spine по главам. Глава начинается либо с начала
    // своего файла, либо с элемента-якоря внутри файла.
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut front: Vec<Block> = Vec::new(); // текст до первой главы
    let mut subheading: Option<String> = None;
    let mut used: Vec<(String, Option<String>)> = Vec::new();
    let mut start_chapter = |title: &str, key: (String, Option<String>), chapters: &mut Vec<Chapter>, sub: &mut Option<String>| {
        if !used.contains(&key) {
            used.push(key);
            chapters.push(Chapter { title: Some(title.to_string()), blocks: Vec::new() });
            *sub = None;
        }
    };
    for doc_path in &opf.spine {
        let entries: Vec<&TocEntry> = toc.iter().filter(|e| e.file == *doc_path).collect();
        let anchors: Vec<&str> = entries.iter().filter_map(|e| e.anchor.as_deref()).collect();
        let pieces = read_entry(&mut zip, doc_path).map(|x| parse_xhtml(&x, &anchors)).unwrap_or_default();
        let found: Vec<&str> = pieces.iter().filter_map(|p| if let Piece::Anchor(a) = p { Some(a.as_str()) } else { None }).collect();
        // Пункты без якоря и с ненайденным якорем начинают главу с начала файла.
        if let Some(e) = entries.iter().find(|e| e.anchor.as_deref().is_none_or(|a| !found.contains(&a))) {
            start_chapter(&e.title, (e.file.clone(), e.anchor.clone()), &mut chapters, &mut subheading);
        }
        for piece in pieces {
            match piece {
                Piece::Anchor(id) => {
                    if let Some(e) = entries.iter().find(|e| e.anchor.as_deref() == Some(id.as_str())) {
                        start_chapter(&e.title, (e.file.clone(), e.anchor.clone()), &mut chapters, &mut subheading);
                    }
                }
                Piece::Heading(h) => subheading = Some(h),
                Piece::Text(text) => {
                    let block = Block { text, page: None, subheading: subheading.clone() };
                    match chapters.last_mut() {
                        Some(c) => c.blocks.push(block),
                        None => front.push(block),
                    }
                }
            }
        }
    }
    chapters.retain(|c| !c.blocks.is_empty());
    if chapters.is_empty() {
        chapters.push(Chapter { title: None, blocks: Vec::new() });
    }
    if !front.is_empty() {
        front.append(&mut chapters[0].blocks);
        chapters[0].blocks = front;
    }

    Ok(ExtractedBook { title: opf.title.unwrap_or_else(|| file_stem(path)), chapters, page_count: None })
}
