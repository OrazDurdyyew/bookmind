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

/// Пункты оглавления верхнего уровня из nav.xhtml: (название, файл).
fn parse_nav(xml: &str, nav_dir: &str) -> Vec<(String, String)> {
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
                "a" if in_toc && ol_depth == 1 => current = Some((attr(&e, "href").unwrap_or_default(), String::new())),
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().into_inner() {
                "nav" => in_toc = false,
                "ol" if in_toc => ol_depth -= 1,
                "a" => {
                    if let Some((href, text)) = current.take() {
                        out.push((collapse_ws(&text), resolve(nav_dir, &href)));
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

/// Пункты верхнего уровня из toc.ncx: (название, файл).
fn parse_ncx(xml: &str, ncx_dir: &str) -> Vec<(String, String)> {
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
                    if depth == 1 {
                        label.clear();
                    }
                }
                "text" if depth == 1 => in_text = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().into_inner() == "content" && depth == 1 => {
                out.push((collapse_ws(&label), resolve(ncx_dir, &attr(&e, "src").unwrap_or_default())));
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
}

const BLOCK_TAGS: &[&str] = &["p", "li", "div", "blockquote", "td", "dd", "dt", "pre", "h1", "h2", "h3", "h4", "h5", "h6"];

/// Разбирает XHTML на абзацы и подзаголовки. h1 пропускается:
/// обычно он повторяет название главы из оглавления.
fn parse_xhtml(xml: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut skip = 0; // внутри head/script/style
    let mut heading: Option<u8> = None;
    let mut r = reader(xml);

    let flush = |text: &mut String, heading: Option<u8>, out: &mut Vec<Piece>| {
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
    };

    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
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
            Ok(Event::Empty(e)) if e.local_name().into_inner() == "br" => text.push(' '),
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

    // Начало каждой главы — позиция её файла в spine.
    let mut starts: Vec<(usize, String)> = Vec::new();
    for (title, file) in toc {
        if let Some(i) = opf.spine.iter().position(|s| *s == file) {
            if !starts.iter().any(|(j, _)| *j == i) {
                starts.push((i, title));
            }
        }
    }
    starts.sort_by_key(|(i, _)| *i);

    let docs: Vec<Vec<Piece>> = opf
        .spine
        .iter()
        .map(|p| read_entry(&mut zip, p).map(|x| parse_xhtml(&x)).unwrap_or_default())
        .collect();

    // Раскладываем документы по главам.
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut front: Vec<Block> = Vec::new(); // документы до первой главы
    let mut subheading: Option<String> = None;
    for (i, pieces) in docs.into_iter().enumerate() {
        if let Some((_, title)) = starts.iter().find(|(j, _)| *j == i) {
            chapters.push(Chapter { title: Some(title.clone()), blocks: Vec::new() });
            subheading = None;
        }
        for piece in pieces {
            match piece {
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
