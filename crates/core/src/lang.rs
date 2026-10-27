//! Определение языка текста: туркменский, русский или английский.

use crate::model::Lang;

/// Буквы, которые есть в туркменской латинице, но не в английском.
const TURKMEN_LETTERS: &str = "äçňöşüýžÄÇŇÖŞÜÝŽ";

/// Определяет язык по доле кириллицы и туркменских букв.
/// Передавайте не больше первых ~5000 символов текста.
pub fn detect_lang(text: &str) -> Lang {
    let mut cyrillic = 0usize;
    let mut latin = 0usize;
    let mut turkmen = 0usize;
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyrillic += 1;
        } else if c.is_ascii_alphabetic() || TURKMEN_LETTERS.contains(c) {
            latin += 1;
            if TURKMEN_LETTERS.contains(c) {
                turkmen += 1;
            }
        }
    }
    let letters = cyrillic + latin;
    if letters == 0 {
        return Lang::En;
    }
    if cyrillic as f64 / letters as f64 > 0.5 {
        Lang::Ru
    } else if latin > 0 && turkmen as f64 / latin as f64 > 0.01 {
        Lang::Tm
    } else {
        Lang::En
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_three_languages() {
        assert_eq!(detect_lang("Фотосинтез — это процесс, при котором растения..."), Lang::Ru);
        assert_eq!(detect_lang("Photosynthesis is the process by which plants..."), Lang::En);
        assert_eq!(
            detect_lang("Türkmenistanyň paýtagty Aşgabat şäheridir. Ol gözel şäher."),
            Lang::Tm
        );
    }

    #[test]
    fn empty_text_falls_back_to_english() {
        assert_eq!(detect_lang(""), Lang::En);
    }
}
