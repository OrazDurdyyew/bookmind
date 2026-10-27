//! Генерация вопросов теста по тексту книги.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ingest::CancelToken;
use crate::llm::{ChatRequest, Llm, Message, Role};
use crate::model::{BookId, ChunkId, Lang};
use crate::store::Store;

/// Сколько раз переспрашивать модель, если вопрос получился плохим.
const RETRIES: u32 = 2;

/// Фрагменты короче — это заголовки, строки оглавления, титульный лист:
/// по ним нормальный вопрос не составить, поэтому источниками их не берём.
const MIN_SOURCE_WORDS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestionKind {
    MultipleChoice,
    Open,
    TrueFalse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

/// Правильный ответ: номер варианта, да/нет или текст.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CorrectAnswer {
    Option(usize),
    Bool(bool),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub kind: QuestionKind,
    pub question: String,
    /// Варианты ответа (только для MultipleChoice).
    pub options: Vec<String>,
    pub correct: CorrectAnswer,
    pub source_chunk: ChunkId,
    pub page: Option<String>,
    pub chapter_index: u32,
}

#[derive(Clone, Debug)]
pub struct QuizRequest {
    pub book: BookId,
    /// None — вся книга.
    pub chapters: Option<Vec<u32>>,
    /// Типы вопросов; идут по кругу в этом порядке.
    pub kinds: Vec<QuestionKind>,
    pub count: usize,
    pub difficulty: Difficulty,
    pub lang: Lang,
}

/// Выбирает фрагменты-источники: поровну из каждой главы, равномерно
/// внутри главы, главы чередуются. Если вопросов больше, чем фрагментов,
/// каждый фрагмент берётся по разу, а затем всё повторяется сначала.
pub fn plan_sources(by_chapter: &[(u32, Vec<ChunkId>)], count: usize) -> Vec<ChunkId> {
    let total: usize = by_chapter.iter().map(|(_, ids)| ids.len()).sum();
    if total == 0 || count == 0 {
        return Vec::new();
    }
    if count > total {
        let once = plan_sources(by_chapter, total);
        return once.iter().cycle().take(count).copied().collect();
    }
    // Квоты: раздаём по одному вопросу по кругу, пока не наберём count.
    let mut quota = vec![0usize; by_chapter.len()];
    let mut assigned = 0;
    while assigned < count {
        for (i, (_, ids)) in by_chapter.iter().enumerate() {
            if assigned < count && quota[i] < ids.len() {
                quota[i] += 1;
                assigned += 1;
            }
        }
    }
    // Равномерные позиции внутри главы, главы чередуются.
    let max_quota = quota.iter().copied().max().unwrap_or(0);
    let mut out = Vec::with_capacity(count);
    for round in 0..max_quota {
        for (i, (_, ids)) in by_chapter.iter().enumerate() {
            if round < quota[i] {
                let pos = ((round as f64 + 0.5) * ids.len() as f64 / quota[i] as f64).floor() as usize;
                out.push(ids[pos.min(ids.len() - 1)]);
            }
        }
    }
    out
}

/// Нормализация для сравнения: нижний регистр, лишние пробелы убраны.
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Проверяет, что вопрос годится в тест.
pub fn validate(q: &QuizQuestion, earlier: &[QuizQuestion]) -> Result<(), &'static str> {
    if norm(&q.question).is_empty() {
        return Err("пустой вопрос");
    }
    if earlier.iter().any(|e| norm(&e.question) == norm(&q.question)) {
        return Err("вопрос повторяется");
    }
    match (q.kind, &q.correct) {
        (QuestionKind::MultipleChoice, CorrectAnswer::Option(i)) => {
            if q.options.len() != 4 {
                return Err("нужно ровно 4 варианта");
            }
            if q.options.iter().any(|o| norm(o).is_empty()) {
                return Err("пустой вариант");
            }
            let mut seen: Vec<String> = q.options.iter().map(|o| norm(o)).collect();
            seen.sort();
            seen.dedup();
            if seen.len() != 4 {
                return Err("варианты повторяются");
            }
            // Маленькие модели иногда пишут в варианты другие вопросы.
            if q.options.iter().any(|o| o.trim_end().ends_with('?')) {
                return Err("вариант — это вопрос");
            }
            if *i >= 4 {
                return Err("номер правильного варианта вне диапазона");
            }
        }
        (QuestionKind::TrueFalse, CorrectAnswer::Bool(_)) | (QuestionKind::Open, CorrectAnswer::Text(_)) => {
            if !q.options.is_empty() {
                return Err("у этого типа не бывает вариантов");
            }
            if let CorrectAnswer::Text(t) = &q.correct {
                if norm(t).is_empty() {
                    return Err("пустой ответ");
                }
            }
        }
        _ => return Err("тип вопроса не совпадает с типом ответа"),
    }
    Ok(())
}

fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Tm => "Turkmen (Latin script)",
        Lang::Ru => "Russian",
        Lang::En => "English",
    }
}

/// JSON-схема ответа модели для каждого типа вопроса.
/// Для выбора из вариантов модель пишет правильный ответ текстом и три неправильных:
/// номер правильного варианта маленькие модели путают, поэтому место выбираем сами.
fn schema(kind: QuestionKind) -> Value {
    match kind {
        QuestionKind::MultipleChoice => json!({
            "type": "object",
            "properties": {
                "question": {"type": "string"},
                "correct_answer": {"type": "string", "description": "the right answer, stated in the text"},
                "wrong_answers": {"type": "array", "items": {"type": "string"}, "minItems": 3, "maxItems": 3,
                    "description": "three plausible but wrong answers to the same question"}
            },
            "required": ["question", "correct_answer", "wrong_answers"]
        }),
        QuestionKind::TrueFalse => json!({
            "type": "object",
            "properties": {"question": {"type": "string"}, "answer": {"type": "boolean"}},
            "required": ["question", "answer"]
        }),
        QuestionKind::Open => json!({
            "type": "object",
            "properties": {"question": {"type": "string"}, "answer": {"type": "string"}},
            "required": ["question", "answer"]
        }),
    }
}

/// Место правильного варианта среди `wrong + 1`: зависит от текста вопроса,
/// поэтому повторяется от запуска к запуску, а по тесту распределяется примерно поровну.
fn correct_position(question: &str, wrong: usize) -> usize {
    let sum: usize = question.bytes().map(usize::from).sum();
    sum % (wrong + 1)
}

/// Превращает JSON модели в вопрос; None, если полей не хватает.
fn from_json(kind: QuestionKind, v: &Value) -> Option<(String, Vec<String>, CorrectAnswer)> {
    let question = v["question"].as_str()?.trim().to_string();
    match kind {
        QuestionKind::MultipleChoice => {
            let correct = v["correct_answer"].as_str()?.trim().to_string();
            let mut options = v["wrong_answers"].as_array()?.iter().map(|o| o.as_str().map(|s| s.trim().to_string())).collect::<Option<Vec<_>>>()?;
            let pos = correct_position(&question, options.len());
            options.insert(pos, correct);
            Some((question, options, CorrectAnswer::Option(pos)))
        }
        QuestionKind::TrueFalse => Some((question, Vec::new(), CorrectAnswer::Bool(v["answer"].as_bool()?))),
        QuestionKind::Open => Some((question, Vec::new(), CorrectAnswer::Text(v["answer"].as_str()?.trim().to_string()))),
    }
}

/// Создаёт один вопрос по фрагменту `source`. None — модель так и не дала
/// годный вопрос (после повторов). Используется и для «Заменить другим».
pub async fn generate_one<L: Llm>(
    store: &Store,
    llm: &L,
    source: ChunkId,
    kind: QuestionKind,
    difficulty: Difficulty,
    lang: Lang,
    earlier: &[QuizQuestion],
) -> anyhow::Result<Option<QuizQuestion>> {
    let chunk = store.chunks(&[source])?.remove(0);
    let kind_text = match kind {
        QuestionKind::MultipleChoice => "multiple-choice (4 options, exactly one correct)",
        QuestionKind::TrueFalse => "true/false",
        QuestionKind::Open => "open-ended (short answer)",
    };
    let difficulty_text = match difficulty {
        Difficulty::Easy => "easy",
        Difficulty::Medium => "medium",
        Difficulty::Hard => "hard",
    };
    let prompt = format!(
        "Text:\n{}\n\nWrite one {difficulty_text} {kind_text} question in {} that can be answered using ONLY this text. The correct answer must be stated in the text. Answers are short statements, not questions.",
        chunk.text,
        lang_name(lang)
    );
    for attempt in 0..=RETRIES {
        let req = ChatRequest {
            messages: vec![Message { role: Role::User, content: prompt.clone() }],
            temperature: 0.4 + 0.2 * attempt as f32,
            max_tokens: 400,
        };
        // Ошибку разбора JSON считаем неудачной попыткой.
        let Ok(v) = llm.chat_json(&req, &schema(kind)).await else { continue };
        let Some((question, options, correct)) = from_json(kind, &v) else { continue };
        let q = QuizQuestion {
            kind, question, options, correct, source_chunk: chunk.id, page: chunk.page_start.clone(), chapter_index: chunk.chapter_index,
        };
        if validate(&q, earlier).is_ok() {
            return Ok(Some(q));
        }
    }
    Ok(None)
}

/// Убирает из источников короткие фрагменты и опустевшие главы.
/// Если короткие все (крошечная книга), оставляет всё как было.
fn drop_short_sources(store: &Store, by_chapter: &[(u32, Vec<ChunkId>)]) -> anyhow::Result<Vec<(u32, Vec<ChunkId>)>> {
    let mut out = Vec::new();
    for (chapter, ids) in by_chapter {
        let long: Vec<ChunkId> = store
            .chunks(ids)?
            .into_iter()
            .filter(|c| c.text.split_whitespace().count() >= MIN_SOURCE_WORDS)
            .map(|c| c.id)
            .collect();
        if !long.is_empty() {
            out.push((*chapter, long));
        }
    }
    if out.is_empty() {
        return Ok(by_chapter.to_vec());
    }
    Ok(out)
}

/// Создаёт тест. `on_question(n, q)` вызывается после каждого готового вопроса
/// (n — сколько уже готово). При отмене возвращает то, что успели создать.
pub async fn generate_quiz<L: Llm>(
    store: &Store,
    llm: &L,
    req: &QuizRequest,
    cancel: &CancelToken,
    on_question: &mut (dyn FnMut(usize, &QuizQuestion) + Send),
) -> anyhow::Result<Vec<QuizQuestion>> {
    let all = store.chunk_ids_by_chapter(req.book, req.chapters.as_deref())?;
    let by_chapter = drop_short_sources(store, &all)?;
    let planned = plan_sources(&by_chapter, req.count);
    // Запасные источники — остальные фрагменты по порядку книги.
    let backups = by_chapter.iter().flat_map(|(_, ids)| ids.iter().copied()).filter(|id| !planned.contains(id));
    let kinds = if req.kinds.is_empty() { vec![QuestionKind::MultipleChoice] } else { req.kinds.clone() };

    let mut out: Vec<QuizQuestion> = Vec::new();
    for source in planned.clone().into_iter().chain(backups) {
        if out.len() >= req.count || cancel.is_cancelled() {
            break;
        }
        let kind = kinds[out.len() % kinds.len()];
        if let Some(q) = generate_one(store, llm, source, kind, req.difficulty, req.lang, &out).await? {
            out.push(q);
            on_question(out.len(), out.last().unwrap());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeLlm;
    use crate::ChunkDraft;

    fn mc(q: &str, options: &[&str], correct: usize) -> QuizQuestion {
        QuizQuestion {
            kind: QuestionKind::MultipleChoice, question: q.into(), options: options.iter().map(|s| s.to_string()).collect(),
            correct: CorrectAnswer::Option(correct), source_chunk: 1, page: None, chapter_index: 0,
        }
    }

    #[test]
    fn plan_spreads_evenly_and_interleaves_chapters() {
        let by: Vec<(u32, Vec<ChunkId>)> = (0..3).map(|c| (c, (c as i64 * 10..c as i64 * 10 + 10).collect())).collect();
        assert_eq!(plan_sources(&by, 6), vec![2, 12, 22, 7, 17, 27]);
    }

    #[test]
    fn plan_repeats_when_more_questions_than_chunks() {
        let by = vec![(0, vec![1, 2]), (1, vec![3])];
        assert_eq!(plan_sources(&by, 5), vec![1, 3, 2, 1, 3]);
        assert!(plan_sources(&[], 3).is_empty());
    }

    #[test]
    fn validate_rejects_bad_questions() {
        let good = mc("Где идёт фотосинтез?", &["В корнях", "В хлоропластах", "В ядре", "В стебле"], 1);
        assert!(validate(&good, &[]).is_ok());
        assert!(validate(&mc("q?", &["a", "b", "c"], 0), &[]).is_err());
        assert!(validate(&mc("q?", &["a", " A ", "c", "d"], 0), &[]).is_err());
        assert!(validate(&mc("q?", &["a", "b", "c", "d"], 4), &[]).is_err());
        assert!(validate(&mc("q?", &["a", "", "c", "d"], 0), &[]).is_err());
        assert!(validate(&mc("Где  идёт фотосинтез? ", &["1", "2", "3", "4"], 0), &[good.clone()]).is_err());
        let tf_bad = QuizQuestion { kind: QuestionKind::TrueFalse, options: vec!["x".into()], correct: CorrectAnswer::Bool(true), ..good.clone() };
        assert!(validate(&tf_bad, &[]).is_err());
        let tf_good = QuizQuestion { kind: QuestionKind::TrueFalse, options: vec![], correct: CorrectAnswer::Bool(true), ..good.clone() };
        assert!(validate(&tf_good, &[]).is_ok());
        let open_empty = QuizQuestion { kind: QuestionKind::Open, options: vec![], correct: CorrectAnswer::Text(" ".into()), ..good };
        assert!(validate(&open_empty, &[]).is_err());
        // Варианты-вопросы вместо ответов.
        assert!(validate(&mc("Кто автор?", &["Кто автор?", "Где место?", "Какая тема?", "Сколько лет?"], 0), &[]).is_err());
    }

    #[test]
    fn correct_position_is_stable_and_spread() {
        assert_eq!(correct_position("Где идёт фотосинтез?", 3), correct_position("Где идёт фотосинтез?", 3));
        let mut counts = [0; 4];
        for i in 0..40 {
            counts[correct_position(&format!("Вопрос номер {i}?"), 3)] += 1;
        }
        assert!(counts.iter().all(|&c| c >= 5), "{counts:?}");
    }

    #[tokio::test]
    async fn generate_quiz_skips_short_sources() {
        let store = Store::open_in_memory().unwrap();
        let book = store.insert_book("b", "b.txt", Lang::Ru, None, &[None, None]).unwrap();
        let long = "слово ".repeat(MIN_SOURCE_WORDS);
        let drafts = vec![
            ChunkDraft { chapter_index: 0, subheading: None, page_start: Some("1".into()), page_end: None, text: "ГЛАВА I. ОГЛАВЛЕНИЕ 5".into() },
            ChunkDraft { chapter_index: 1, subheading: None, page_start: Some("2".into()), page_end: None, text: long },
        ];
        let embs: Vec<Vec<f32>> = drafts.iter().map(|d| FakeLlm::vector(&d.text)).collect();
        store.insert_chunks(book, &drafts, &embs).unwrap();

        let llm = FakeLlm::new();
        llm.json_replies.lock().unwrap().push_back(json!({"question": "Что там?", "answer": "Слова"}));
        let req = QuizRequest { book, chapters: None, kinds: vec![QuestionKind::Open], count: 1, difficulty: Difficulty::Easy, lang: Lang::Ru };
        let qs = generate_quiz(&store, &llm, &req, &CancelToken::default(), &mut |_, _| {}).await.unwrap();
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].chapter_index, 1);
    }

    #[tokio::test]
    async fn generate_quiz_retries_invalid_and_alternates_kinds() {
        let store = Store::open_in_memory().unwrap();
        let book = store.insert_book("b", "b.txt", Lang::Ru, None, &[None, None]).unwrap();
        let drafts: Vec<ChunkDraft> = (0..4)
            .map(|i| ChunkDraft { chapter_index: i / 2, subheading: None, page_start: Some(format!("{}", i + 1)), page_end: None, text: format!("текст {i}") })
            .collect();
        let embs: Vec<Vec<f32>> = drafts.iter().map(|d| FakeLlm::vector(&d.text)).collect();
        store.insert_chunks(book, &drafts, &embs).unwrap();

        let llm = FakeLlm::new();
        llm.json_replies.lock().unwrap().extend([
            json!({"question": "Плохой?", "correct_answer": "a", "wrong_answers": ["b", "c"]}),
            json!({"question": "Где идёт фотосинтез?", "correct_answer": "В листьях", "wrong_answers": ["В корнях", "В ядре", "В стебле"]}),
            json!({"question": "Растения дышат?", "answer": true}),
        ]);
        let req = QuizRequest {
            book, chapters: None, kinds: vec![QuestionKind::MultipleChoice, QuestionKind::TrueFalse], count: 2,
            difficulty: Difficulty::Medium, lang: Lang::Ru,
        };
        let mut seen = Vec::new();
        let qs = generate_quiz(&store, &llm, &req, &CancelToken::default(), &mut |n, _| seen.push(n)).await.unwrap();
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0].kind, QuestionKind::MultipleChoice);
        let CorrectAnswer::Option(i) = qs[0].correct else { panic!("{:?}", qs[0].correct) };
        assert_eq!(qs[0].options[i], "В листьях");
        assert_eq!(qs[0].options.len(), 4);
        assert_eq!(qs[1].kind, QuestionKind::TrueFalse);
        assert_eq!(qs[1].correct, CorrectAnswer::Bool(true));
        assert_eq!(seen, vec![1, 2]);
        // Вопросы взяты из разных глав, страница — из фрагмента-источника.
        assert_ne!(qs[0].chapter_index, qs[1].chapter_index);
        assert!(qs[0].page.is_some());
    }

    #[tokio::test]
    async fn generate_quiz_stops_when_sources_run_out() {
        let store = Store::open_in_memory().unwrap();
        let book = store.insert_book("b", "b.txt", Lang::Ru, None, &[None]).unwrap();
        let d = ChunkDraft { chapter_index: 0, subheading: None, page_start: None, page_end: None, text: "текст".into() };
        store.insert_chunks(book, &[d], &[FakeLlm::vector("текст")]).unwrap();
        let llm = FakeLlm::new(); // ни одного JSON-ответа — все попытки неудачны
        let req = QuizRequest { book, chapters: None, kinds: vec![QuestionKind::Open], count: 3, difficulty: Difficulty::Easy, lang: Lang::En };
        let qs = generate_quiz(&store, &llm, &req, &CancelToken::default(), &mut |_, _| {}).await.unwrap();
        assert!(qs.is_empty());
    }
}
