//! Небольшие помощники для разбора XML через quick-xml.

use std::io::Read;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::ExtractError;

/// Значение атрибута по локальному имени (без префикса `w:` и т. п.).
pub fn attr(e: &BytesStart, local: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.local_name().into_inner() == local {
            Some(a.value.to_string())
        } else {
            None
        }
    })
}

/// Текст для ссылки на сущность XML (`&amp;`, `&#1092;`, `&nbsp;`).
pub fn entity_text(name: &str) -> String {
    match name {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        "nbsp" => " ".into(),
        _ => {
            let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse().ok()
            } else {
                None
            };
            code.and_then(char::from_u32).map(String::from).unwrap_or_default()
        }
    }
}

/// Текст события Text или GeneralRef; для остальных событий — None.
pub fn event_text(ev: &Event) -> Option<String> {
    match ev {
        Event::Text(t) => Some(t.as_ref().to_string()),
        Event::CData(t) => Some(t.as_ref().to_string()),
        Event::GeneralRef(r) => Some(entity_text(r.as_ref())),
        _ => None,
    }
}

/// Читатель XML из строки с сохранением пробелов.
pub fn reader(xml: &str) -> Reader<&[u8]> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    r
}

/// Открывает zip-архив; если это не zip — ошибка Corrupt.
pub fn open_zip(path: &std::path::Path) -> Result<zip::ZipArchive<std::fs::File>, ExtractError> {
    let f = std::fs::File::open(path)?;
    zip::ZipArchive::new(f).map_err(|e| ExtractError::Corrupt(e.to_string()))
}

/// Читает файл из архива как строку; None, если его нет.
pub fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let mut s = String::new();
    entry.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Схлопывает пробелы: несколько пробелов и переводов строк → один пробел.
pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
