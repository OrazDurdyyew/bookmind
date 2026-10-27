//! Ответ на вопрос строго по тексту книги, со ссылками на источники.

use std::sync::OnceLock;

use regex::Regex;

use crate::llm::{ChatRequest, ChatStats, Llm, Message, Role};
use crate::model::{BookId, ChunkId, StoredChunk};
use crate::retrieve::{retrieve, DEFAULT_TOP_K};
use crate::store::Store;

/// Так модель отвечает, если в источниках нет ответа.
pub const NOT_IN_BOOK: &str = "NOT_IN_BOOK";
/// Сколько последних ходов разговора передаём модели.
const HISTORY_TURNS: usize = 2;

/// Инструкция модели. Специально на английском: маленькие модели лучше
/// слушаются английских инструкций, а язык ответа задаётся правилом.
const SYSTEM_PROMPT: &str = "You answer questions about a book using ONLY the numbered sources provided.
Rules:
- Use only facts stated in the sources. Do not use outside knowledge.
- If the sources do not contain the answer, reply with exactly: NOT_IN_BOOK
- Answer in the same language as the question.
- After each fact, cite its source number in square brackets, e.g. [2].
- Be concise: 1-5 sentences.";

/// Прошлый ход разговора: вопрос и ответ.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub question: String,
    pub answer: String,
}

/// Ссылка на место в книге, откуда взят ответ.
#[derive(Clone, Debug, PartialEq)]
pub struct Citation {
    pub chunk_id: ChunkId,
    pub chapter_title: Option<String>,
    pub chapter_index: u32,
    pub subheading: Option<String>,
    pub page_start: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// false — в книге нет ответа.
    pub found: bool,
    /// Текст ответа без меток [n].
    pub text: String,
    pub citations: Vec<Citation>,
    pub stats: ChatStats,
}

/// Подпись источника: «Глава 4, стр. 87» (или часть/раздел, если страниц нет).
fn source_label(c: &StoredChunk) -> String {
    let mut parts = Vec::new();
    match &c.chapter_title {
        Some(t) => parts.push(t.clone()),
        None => parts.push(format!("Part {}", c.chapter_index + 1)),
    }
    if let Some(s) = &c.subheading {
        parts.push(s.clone());
    }
    if let Some(p) = &c.page_start {
        parts.push(format!("p. {p}"));
    }
    parts.join(", ")
}

/// Сообщения для модели: инструкция, последние ходы, источники и вопрос.
pub fn build_answer_messages(question: &str, history: &[Turn], sources: &[StoredChunk]) -> Vec<Message> {
    let mut msgs = vec![Message { role: Role::System, content: SYSTEM_PROMPT.to_string() }];
    for t in history.iter().skip(history.len().saturating_sub(HISTORY_TURNS)) {
        msgs.push(Message { role: Role::User, content: t.question.clone() });
        msgs.push(Message { role: Role::Assistant, content: t.answer.clone() });
    }
    let mut user = String::from("Sources:\n");
    for (i, c) in sources.iter().enumerate() {
        user.push_str(&format!("[{}] ({})\n{}\n\n", i + 1, source_label(c), c.text));
    }
    user.push_str(&format!("Question: {question}"));
    msgs.push(Message { role: Role::User, content: user });
    msgs
}

fn marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s*\[(\d+(?:\s*,\s*\d+)*)\]").unwrap())
}

fn space_before_punct_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+([.,;:!?])").unwrap())
}

/// Разбирает ответ модели: найдено ли, чистый текст и ссылки.
/// Метки вне диапазона источников ([9] при 3 источниках) отбрасываются.
pub fn parse_answer(raw: &str, sources: &[StoredChunk]) -> (bool, String, Vec<Citation>) {
    let trimmed = raw.trim();
    if trimmed.starts_with(NOT_IN_BOOK) || trimmed.is_empty() {
        return (false, String::new(), Vec::new());
    }
    let mut cited: Vec<usize> = Vec::new();
    for cap in marker_re().captures_iter(trimmed) {
        for n in cap[1].split(',').filter_map(|x| x.trim().parse::<usize>().ok()) {
            if (1..=sources.len()).contains(&n) && !cited.contains(&n) {
                cited.push(n);
            }
        }
    }
    let text = marker_re().replace_all(trimmed, "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = space_before_punct_re().replace_all(&text, "$1").trim().to_string();
    let citations = cited
        .into_iter()
        .map(|n| {
            let c = &sources[n - 1];
            Citation {
                chunk_id: c.id,
                chapter_title: c.chapter_title.clone(),
                chapter_index: c.chapter_index,
                subheading: c.subheading.clone(),
                page_start: c.page_start.clone(),
            }
        })
        .collect();
    (true, text, citations)
}

/// Отвечает на вопрос по книге. `on_token` получает текст по мере генерации.
pub async fn answer<L: Llm>(
    store: &Store,
    llm: &L,
    book: BookId,
    question: &str,
    history: &[Turn],
    on_token: &mut (dyn FnMut(&str) + Send),
) -> anyhow::Result<Answer> {
    let sources = retrieve(store, llm, book, question, DEFAULT_TOP_K).await?;
    if sources.is_empty() {
        return Ok(Answer { found: false, text: String::new(), citations: Vec::new(), stats: ChatStats::default() });
    }
    let req = ChatRequest { messages: build_answer_messages(question, history, &sources), temperature: 0.2, max_tokens: 512 };
    let (raw, stats) = llm.chat_stream(&req, on_token).await?;
    let (found, text, citations) = parse_answer(&raw, &sources);
    Ok(Answer { found, text, citations, stats })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeLlm;
    use crate::llm::Role;
    use crate::{ChunkDraft, Lang};

    fn src(id: ChunkId, page: &str, text: &str) -> StoredChunk {
        StoredChunk {
            id, book_id: 1, ord: id as u32, chapter_index: 3, chapter_title: Some("Глава 4".into()), subheading: None,
            page_start: Some(page.into()), page_end: Some(page.into()), text: text.into(),
        }
    }

    fn three() -> Vec<StoredChunk> {
        vec![src(10, "87", "Фотосинтез идёт в хлоропластах."), src(11, "88", "Хлоропласты есть в листьях."), src(12, "90", "Корни.")]
    }

    #[test]
    fn messages_number_sources_and_limit_history() {
        let history: Vec<Turn> = (0..4).map(|i| Turn { question: format!("q{i}"), answer: format!("a{i}") }).collect();
        let msgs = build_answer_messages("Где идёт фотосинтез?", &history, &three());
        assert_eq!(msgs[0].role, Role::System);
        assert!(msgs[0].content.contains(NOT_IN_BOOK));
        // system + 2 хода истории (по 2 сообщения) + вопрос
        assert_eq!(msgs.len(), 1 + 4 + 1);
        assert_eq!(msgs[1].content, "q2");
        assert_eq!(msgs[4].content, "a3");
        let last = &msgs.last().unwrap().content;
        assert!(last.contains("[1]") && last.contains("[3]") && !last.contains("[4]"));
        assert!(last.contains("Глава 4") && last.contains("87"));
        assert!(last.contains("Где идёт фотосинтез?"));
    }

    #[test]
    fn parses_citations_and_strips_markers() {
        let (found, text, cites) = parse_answer("Фотосинтез идёт в хлоропластах [1][2].", &three());
        assert!(found);
        assert_eq!(text, "Фотосинтез идёт в хлоропластах.");
        assert_eq!(cites.iter().map(|c| c.chunk_id).collect::<Vec<_>>(), vec![10, 11]);
        assert_eq!(cites[0].page_start.as_deref(), Some("87"));
        assert_eq!(cites[0].chapter_title.as_deref(), Some("Глава 4"));
    }

    #[test]
    fn handles_broken_model_output() {
        assert!(!parse_answer("NOT_IN_BOOK", &three()).0);
        assert!(!parse_answer("  NOT_IN_BOOK. Sorry, I could not find it.", &three()).0);
        let (found, text, cites) = parse_answer("Ответ [9] и [2]", &three());
        assert!(found);
        assert_eq!(text, "Ответ и");
        assert_eq!(cites.iter().map(|c| c.chunk_id).collect::<Vec<_>>(), vec![11]);
        let (found, _, cites) = parse_answer("Просто ответ без ссылок.", &three());
        assert!(found && cites.is_empty());
        assert_eq!(parse_answer("Да [1][1], [1, 3].", &three()).2.iter().map(|c| c.chunk_id).collect::<Vec<_>>(), vec![10, 12]);
    }

    #[tokio::test]
    async fn answer_streams_and_parses() {
        let store = Store::open_in_memory().unwrap();
        let book = store.insert_book("b", "b.txt", Lang::Ru, None, &[Some("Глава 1".into())]).unwrap();
        let text = "фотосинтез идёт в хлоропластах листьев";
        let d = ChunkDraft { chapter_index: 0, subheading: None, page_start: Some("5".into()), page_end: Some("5".into()), text: text.into() };
        store.insert_chunks(book, &[d], &[FakeLlm::vector(text)]).unwrap();
        let llm = FakeLlm::new();
        llm.chat_replies.lock().unwrap().push_back("В хлоропластах [1].".into());
        let mut streamed = String::new();
        let a = answer(&store, &llm, book, "где идёт фотосинтез", &[], &mut |t| streamed.push_str(t)).await.unwrap();
        assert_eq!(streamed, "В хлоропластах [1].");
        assert!(a.found);
        assert_eq!(a.text, "В хлоропластах.");
        assert_eq!(a.citations[0].page_start.as_deref(), Some("5"));
        assert_eq!(a.citations[0].chapter_title.as_deref(), Some("Глава 1"));
    }

    #[tokio::test]
    async fn empty_book_is_not_found_without_calling_model() {
        let store = Store::open_in_memory().unwrap();
        let book = store.insert_book("b", "b.txt", Lang::Ru, None, &[None]).unwrap();
        let llm = FakeLlm::new();
        llm.chat_replies.lock().unwrap().push_back("выдумка".into());
        let a = answer(&store, &llm, book, "вопрос", &[], &mut |_| {}).await.unwrap();
        assert!(!a.found);
        assert_eq!(llm.chat_replies.lock().unwrap().len(), 1);
    }
}
