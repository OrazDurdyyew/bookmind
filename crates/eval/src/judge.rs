//! Оценка результатов: правила без судьи, судья-модель, правки человека.

use std::path::Path;

use bookmind_core::llm::{ChatRequest, EmbedPrefixes, LlamaHttp, Llm, Message, Role};
use bookmind_core::quiz::CorrectAnswer;
use serde_json::json;

use crate::config::load_config;
use crate::paths;
use crate::records::{read_jsonl, AnswerResult, Judged, Record};
use crate::run::spawn_chat;

/// Можно ли оценить ответ без судьи.
#[derive(Debug, PartialEq, Eq)]
pub enum Score {
    Correct,
    Incorrect,
    NeedsJudge,
}

/// Правила: вопрос без ответа в книге → правильно только «не найдено»;
/// вопрос с ответом → «не найдено» неправильно, иначе решает судья.
pub fn score_answer(r: &AnswerResult) -> Score {
    match (r.answerable, r.found) {
        (false, false) => Score::Correct,
        (false, true) => Score::Incorrect,
        (true, false) => Score::Incorrect,
        (true, true) => Score::NeedsJudge,
    }
}

/// Применяет правки человека из review.csv: колонка human_override = 1 / 0.
pub fn apply_overrides(judged: &mut [Judged], csv_text: &str) -> anyhow::Result<()> {
    let mut rdr = csv::Reader::from_reader(csv_text.as_bytes());
    let headers = rdr.headers()?.clone();
    let col = |name: &str| headers.iter().position(|h| h == name).ok_or_else(|| anyhow::anyhow!("в review.csv нет колонки {name}"));
    let (id_col, ov_col) = (col("id")?, col("human_override")?);
    for row in rdr.records() {
        let row = row?;
        let verdict = match row.get(ov_col).map(str::trim) {
            Some("1") => true,
            Some("0") => false,
            _ => continue,
        };
        let id = row.get(id_col).unwrap_or("");
        for j in judged.iter_mut().filter(|j| j.record.id() == id) {
            j.correct = verdict;
            j.reason = format!("human override ({})", j.reason);
        }
    }
    Ok(())
}

/// Сколько раз спрашивать судью, если его JSON не разобрался.
const JUDGE_ATTEMPTS: u32 = 3;

/// Спрашивает судью. Длинное объяснение может оборваться на лимите токенов,
/// тогда JSON не разбирается — переспрашиваем с чуть большей температурой,
/// иначе модель ответила бы точно так же.
async fn ask_judge<L: Llm>(llm: &L, content: String, schema: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let content = format!("{content}\nKeep the reason to one short sentence.");
    let mut last_err = None;
    for attempt in 0..JUDGE_ATTEMPTS {
        let req = ChatRequest {
            messages: vec![Message { role: Role::User, content: content.clone() }],
            temperature: 0.3 * attempt as f32,
            max_tokens: 1000,
        };
        match llm.chat_json(&req, schema).await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}

/// Судья сравнивает ответ модели с эталоном.
async fn judge_answer<L: Llm>(llm: &L, a: &AnswerResult) -> anyhow::Result<(bool, String)> {
    let schema = json!({"type":"object","properties":{"correct":{"type":"boolean"},"reason":{"type":"string"}},"required":["correct","reason"]});
    let prompt = format!(
        "Question: {}\nExpected answer: {}\nModel answer: {}\n\nIs the model answer factually consistent with the expected answer? Minor wording differences are fine; missing the key fact is incorrect.",
        a.question, a.expected.as_deref().unwrap_or(""), a.answer
    );
    let v = ask_judge(llm, prompt, &schema).await?;
    Ok((v["correct"].as_bool().unwrap_or(false), v["reason"].as_str().unwrap_or("").to_string()))
}

/// Судья проверяет вопрос теста по тексту источника.
async fn judge_quiz<L: Llm>(llm: &L, source: &str, q: &bookmind_core::quiz::QuizQuestion) -> anyhow::Result<(bool, String)> {
    let schema = json!({"type":"object","properties":{
        "correct_is_right":{"type":"boolean"},"exactly_one_correct":{"type":"boolean"},
        "answerable_from_text":{"type":"boolean"},"reason":{"type":"string"}},
        "required":["correct_is_right","exactly_one_correct","answerable_from_text","reason"]});
    let answer = match &q.correct {
        CorrectAnswer::Option(i) => q.options.get(*i).cloned().unwrap_or_default(),
        CorrectAnswer::Bool(b) => b.to_string(),
        CorrectAnswer::Text(t) => t.clone(),
    };
    let options: String = q.options.iter().enumerate().map(|(i, o)| format!("{}) {o}\n", i + 1)).collect();
    let prompt = format!(
        "Source text:\n{source}\n\nQuiz question: {}\nOptions:\n{options}Marked correct answer: {answer}\n\nCheck: is the marked answer right according to the text? Is exactly one option correct? Can the question be answered from the text alone?",
        q.question
    );
    let v = ask_judge(llm, prompt, &schema).await?;
    let ok = ["correct_is_right", "exactly_one_correct", "answerable_from_text"].iter().all(|k| v[*k].as_bool().unwrap_or(false));
    Ok((ok, v["reason"].as_str().unwrap_or("").to_string()))
}

/// Команда `judge`: оценивает results.jsonl, пишет judged.jsonl и review.csv.
pub async fn judge(dir: &Path) -> anyhow::Result<()> {
    let root = paths::root()?;
    let cfg = load_config(&root.join("eval/models.toml"))?;
    let records: Vec<Record> = read_jsonl(&dir.join("results.jsonl"))?;
    println!("запускаю судью {}…", cfg.judge.model.name);
    let server = spawn_chat(&root, &cfg.judge, None).await?;
    let llm = LlamaHttp::new(&server.base_url(), &server.base_url(), EmbedPrefixes::default());

    let mut judged = Vec::with_capacity(records.len());
    for (i, r) in records.into_iter().enumerate() {
        let verdict = match &r {
            Record::Answer(a) => match score_answer(a) {
                Score::Correct => Ok((true, "rule: correct refusal".into())),
                Score::Incorrect => Ok((false, if a.answerable { "rule: answer exists but model refused" } else { "rule: answered unanswerable question" }.into())),
                Score::NeedsJudge => judge_answer(&llm, a).await,
            },
            Record::Quiz(q) => judge_quiz(&llm, &q.source_text, &q.question).await,
            Record::QuizBatch(_) => Ok((true, String::new())),
        };
        // Если судья так и не ответил, запись считается неверной и попадает в review.csv —
        // её можно поправить вручную, а остальная оценка не теряется.
        let (correct, reason) = verdict.unwrap_or_else(|e| {
            eprintln!("  судья не ответил на {}: {e}", r.id());
            (false, format!("judge error: {e}"))
        });
        println!("  [{}] {} → {}", i + 1, r.id(), if correct { "ok" } else { "НЕТ" });
        judged.push(Judged { record: r, correct, reason });
    }

    let mut out = String::new();
    for j in &judged {
        out.push_str(&serde_json::to_string(j)?);
        out.push('\n');
    }
    std::fs::write(dir.join("judged.jsonl"), out)?;

    let mut w = csv::Writer::from_path(dir.join("review.csv"))?;
    w.write_record(["id", "lang", "kind", "question", "expected", "model_output", "judge_verdict", "reason", "human_override"])?;
    for j in &judged {
        let verdict = if j.correct { "1" } else { "0" };
        match &j.record {
            Record::Answer(a) => w.write_record([
                a.id.as_str(), a.lang.code(), "answer", &a.question, a.expected.as_deref().unwrap_or(""),
                if a.found { &a.answer } else { "NOT_IN_BOOK" }, verdict, &j.reason, "",
            ])?,
            Record::Quiz(q) => w.write_record([
                q.id.as_str(), q.lang.code(), "quiz", &q.question.question, &format!("{:?}", q.question.correct),
                &q.question.options.join(" | "), verdict, &j.reason, "",
            ])?,
            Record::QuizBatch(_) => {}
        }
    }
    w.flush()?;
    println!("готово: {}", dir.join("review.csv").display());
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bookmind_core::Lang;

    pub fn ans(id: &str, answerable: bool, found: bool) -> AnswerResult {
        AnswerResult {
            id: id.into(), lang: Lang::Ru, question: "?".into(), expected: answerable.then(|| "x".into()), answerable, found,
            answer: "a".into(), cited_pages: vec![], ttft_ms: 1000, tokens_per_sec: 10.0,
        }
    }

    #[test]
    fn score_answer_rules() {
        assert_eq!(score_answer(&ans("1", false, false)), Score::Correct);
        assert_eq!(score_answer(&ans("2", false, true)), Score::Incorrect);
        assert_eq!(score_answer(&ans("3", true, false)), Score::Incorrect);
        assert_eq!(score_answer(&ans("4", true, true)), Score::NeedsJudge);
    }

    #[test]
    fn human_overrides_replace_judge_verdict() {
        let mut judged = vec![
            Judged { record: Record::Answer(ans("ru-001", true, true)), correct: false, reason: "judge".into() },
            Judged { record: Record::Answer(ans("ru-002", true, true)), correct: true, reason: "judge".into() },
            Judged { record: Record::Answer(ans("ru-003", true, true)), correct: true, reason: "judge".into() },
        ];
        let csv = "id,lang,kind,question,expected,model_output,judge_verdict,reason,human_override\n\
                   ru-001,ru,answer,?,x,a,0,judge,1\n\
                   ru-002,ru,answer,?,x,a,1,judge,0\n\
                   ru-003,ru,answer,?,x,a,1,judge,\n";
        apply_overrides(&mut judged, csv).unwrap();
        assert!(judged[0].correct);
        assert!(!judged[1].correct);
        assert!(judged[2].correct);
        assert!(judged[0].reason.contains("human"));
    }
}
