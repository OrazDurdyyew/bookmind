//! Извлечение текста из DOCX: главы по заголовкам 1-го уровня,
//! подзаголовки — по заголовкам 2-го уровня и ниже.

use std::collections::HashMap;
use std::path::Path;

use quick_xml::events::Event;

use super::xml::{attr, collapse_ws, event_text, open_zip, read_entry, reader};
use super::{file_stem, ExtractError};
use crate::model::{Block, Chapter, ExtractedBook};

/// styleId → уровень заголовка (0 = глава) из word/styles.xml.
fn outline_levels(styles_xml: &str) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    let mut r = reader(styles_xml);
    let mut current: Option<String> = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().into_inner() {
                "style" => current = attr(&e, "styleId"),
                "outlineLvl" => {
                    if let (Some(id), Some(v)) = (&current, attr(&e, "val").and_then(|v| v.parse().ok())) {
                        map.insert(id.clone(), v);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Запасной вариант: стиль `Heading{N}` → уровень N-1.
fn level_from_style_name(style: &str) -> Option<u32> {
    let n: u32 = style.strip_prefix("Heading")?.parse().ok()?;
    n.checked_sub(1)
}

/// Название из docProps/core.xml (dc:title).
fn core_title(core_xml: &str) -> Option<String> {
    let mut r = reader(core_xml);
    let mut in_title = false;
    let mut title = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().into_inner() == "title" => in_title = true,
            Ok(Event::End(e)) if e.local_name().into_inner() == "title" => break,
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) if in_title => {
                if let Some(t) = event_text(&ev) {
                    title.push_str(&t);
                }
            }
            _ => {}
        }
    }
    let title = collapse_ws(&title);
    (!title.is_empty()).then_some(title)
}

pub fn extract_docx(path: &Path) -> Result<ExtractedBook, ExtractError> {
    let mut zip = open_zip(path)?;
    let document = read_entry(&mut zip, "word/document.xml")
        .ok_or_else(|| ExtractError::Corrupt("нет word/document.xml".into()))?;
    let levels = read_entry(&mut zip, "word/styles.xml").map(|s| outline_levels(&s)).unwrap_or_default();
    let title = read_entry(&mut zip, "docProps/core.xml")
        .and_then(|s| core_title(&s))
        .unwrap_or_else(|| file_stem(path));

    let mut chapters = vec![Chapter { title: None, blocks: Vec::new() }];
    let mut subheading: Option<String> = None;

    // Состояние текущего абзаца <w:p>.
    let mut text = String::new();
    let mut style: Option<String> = None;
    let mut direct_level: Option<u32> = None;
    let mut in_t = false;

    let mut r = reader(&document);
    loop {
        let ev = r.read_event().map_err(|e| ExtractError::Corrupt(e.to_string()))?;
        match &ev {
            Event::Start(e) | Event::Empty(e) => match e.local_name().into_inner() {
                "p" if matches!(ev, Event::Start(_)) => {
                    text.clear();
                    style = None;
                    direct_level = None;
                }
                "pStyle" => style = attr(e, "val"),
                "outlineLvl" => direct_level = attr(e, "val").and_then(|v| v.parse().ok()),
                "t" if matches!(ev, Event::Start(_)) => in_t = true,
                "tab" | "br" => text.push(' '),
                _ => {}
            },
            Event::End(e) => match e.local_name().into_inner() {
                "t" => in_t = false,
                "p" => {
                    let para = collapse_ws(&text);
                    if para.is_empty() {
                        continue;
                    }
                    let level = direct_level.or_else(|| {
                        style.as_ref().and_then(|s| levels.get(s).copied().or_else(|| level_from_style_name(s)))
                    });
                    match level {
                        Some(0) => {
                            chapters.push(Chapter { title: Some(para), blocks: Vec::new() });
                            subheading = None;
                        }
                        Some(_) => subheading = Some(para),
                        None => chapters.last_mut().unwrap().blocks.push(Block {
                            text: para,
                            page: None,
                            subheading: subheading.clone(),
                        }),
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            other if in_t => {
                if let Some(t) = event_text(other) {
                    text.push_str(&t);
                }
            }
            _ => {}
        }
    }

    if chapters.len() > 1 && chapters[0].blocks.is_empty() {
        chapters.remove(0);
    }
    Ok(ExtractedBook { title, chapters, page_count: None })
}
