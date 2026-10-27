//! Сводка результатов по языкам и отчёт report.md.

use bookmind_core::Lang;

use crate::config::load_config;
use crate::judge::apply_overrides;
use crate::paths;
use crate::records::{read_jsonl, Judged, Record};

/// Порог качества: 85% по каждой метрике.
pub const PASS: f32 = 0.85;

/// Итоги по одному языку.
#[derive(Clone, Debug, PartialEq)]
pub struct LangSummary {
    pub lang: Lang,
    /// Доля верных ответов на вопросы, ответ на которые есть в книге.
    pub answer_acc: Option<f32>,
    /// Доля честных «нет в книге» на вопросы без ответа.
    pub refusal_acc: Option<f32>,
    /// Доля годных вопросов теста (недостающие считаются негодными).
    pub quiz_validity: Option<f32>,
    pub ttft_median_ms: u64,
    pub tokens_per_sec: f32,
    pub n_answerable: usize,
    pub n_unanswerable: usize,
    pub n_quiz: usize,
    pub passed: bool,
}

fn ratio(good: usize, total: usize) -> Option<f32> {
    (total > 0).then(|| good as f32 / total as f32)
}

/// Считает метрики по каждому языку (в порядке tm, ru, en).
pub fn summarize(judged: &[Judged]) -> Vec<LangSummary> {
    let mut out = Vec::new();
    for lang in [Lang::Tm, Lang::Ru, Lang::En] {
        let (mut ans_ok, mut ans_n, mut ref_ok, mut ref_n) = (0, 0, 0, 0);
        let (mut quiz_ok, mut quiz_n, mut requested) = (0, 0, 0);
        let mut ttfts = Vec::new();
        let mut speeds = Vec::new();
        let mut any = false;
        for j in judged {
            match &j.record {
                Record::Answer(a) if a.lang == lang => {
                    any = true;
                    if a.answerable {
                        ans_n += 1;
                        ans_ok += j.correct as usize;
                    } else {
                        ref_n += 1;
                        ref_ok += j.correct as usize;
                    }
                    if a.found {
                        ttfts.push(a.ttft_ms);
                        speeds.push(a.tokens_per_sec);
                    }
                }
                Record::Quiz(q) if q.lang == lang => {
                    any = true;
                    quiz_n += 1;
                    quiz_ok += j.correct as usize;
                }
                Record::QuizBatch(b) if b.lang == lang => {
                    any = true;
                    requested += b.requested;
                }
                _ => {}
            }
        }
        if !any {
            continue;
        }
        ttfts.sort_unstable();
        let ttft_median_ms = if ttfts.is_empty() { 0 } else { ttfts[(ttfts.len() - 1) / 2] };
        let tokens_per_sec = if speeds.is_empty() { 0.0 } else { speeds.iter().sum::<f32>() / speeds.len() as f32 };
        let answer_acc = ratio(ans_ok, ans_n);
        let refusal_acc = ratio(ref_ok, ref_n);
        let quiz_validity = ratio(quiz_ok, quiz_n.max(requested));
        let passed = [answer_acc, refusal_acc, quiz_validity].iter().all(|m| m.is_some_and(|v| v >= PASS));
        out.push(LangSummary {
            lang, answer_acc, refusal_acc, quiz_validity, ttft_median_ms, tokens_per_sec,
            n_answerable: ans_n, n_unanswerable: ref_n, n_quiz: quiz_n, passed,
        });
    }
    out
}

fn pct(v: Option<f32>) -> String {
    v.map(|x| format!("{:.0}%", x * 100.0)).unwrap_or_else(|| "—".into())
}

/// Команда `report`: собирает все прогоны в eval/out/report.md.
pub fn report() -> anyhow::Result<()> {
    let root = paths::root()?;
    let cfg = load_config(&root.join("eval/models.toml"))?;
    let out = paths::out_dir(&root);
    let mut rows: Vec<(String, u64, LangSummary)> = Vec::new(); // (прогон, размер модели, итоги)
    let mut dirs: Vec<_> = std::fs::read_dir(&out)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("judged.jsonl").is_file()).collect();
    dirs.sort();
    for dir in dirs {
        let run = dir.file_name().unwrap().to_string_lossy().to_string();
        let mut judged: Vec<Judged> = read_jsonl(&dir.join("judged.jsonl"))?;
        if let Ok(csv) = std::fs::read_to_string(dir.join("review.csv")) {
            apply_overrides(&mut judged, &csv)?;
        }
        let candidate = run.split("__").next().unwrap_or("");
        let size = cfg
            .candidates
            .iter()
            .find(|c| c.model.name == candidate)
            .and_then(|c| std::fs::metadata(paths::model_path(&root, &c.model.file)).ok())
            .map(|m| m.len())
            .unwrap_or(u64::MAX);
        for s in summarize(&judged) {
            rows.push((run.clone(), size, s));
        }
    }

    let mut md = String::from("# Проверка моделей BookMind\n\n");
    md.push_str("| Прогон | Язык | Ответы | Отказы «нет в книге» | Вопросы теста | TTFT (медиана) | ток/с | Итог |\n");
    md.push_str("|---|---|---|---|---|---|---|---|\n");
    for (run, _, s) in &rows {
        md.push_str(&format!(
            "| {run} | {} | {} ({}) | {} ({}) | {} ({}) | {:.1} с | {:.1} | {} |\n",
            s.lang.code(), pct(s.answer_acc), s.n_answerable, pct(s.refusal_acc), s.n_unanswerable,
            pct(s.quiz_validity), s.n_quiz, s.ttft_median_ms as f64 / 1000.0, s.tokens_per_sec,
            if s.passed { "✅" } else { "❌" }
        ));
    }
    md.push_str("\n**Самая маленькая прошедшая модель:**\n\n");
    for lang in [Lang::Tm, Lang::Ru, Lang::En] {
        let best = rows.iter().filter(|(_, _, s)| s.lang == lang && s.passed).min_by_key(|(_, size, _)| *size);
        if rows.iter().any(|(_, _, s)| s.lang == lang) {
            md.push_str(&format!("- {}: {}\n", lang.code(), best.map(|(r, _, _)| r.as_str()).unwrap_or("нет")));
        }
    }
    let path = out.join("report.md");
    std::fs::write(&path, md)?;
    println!("{}", std::fs::read_to_string(&path)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::tests::ans;
    use crate::records::{QuizBatch, QuizResult};
    use bookmind_core::quiz::{CorrectAnswer, QuestionKind, QuizQuestion};

    fn j(record: Record, correct: bool) -> Judged {
        Judged { record, correct, reason: String::new() }
    }

    fn quiz(id: &str) -> Record {
        Record::Quiz(QuizResult {
            id: id.into(), lang: Lang::Ru, book: "b.pdf".into(), source_text: "t".into(),
            question: QuizQuestion {
                kind: QuestionKind::TrueFalse, question: "q".into(), options: vec![], correct: CorrectAnswer::Bool(true),
                source_chunk: 1, page: None, chapter_index: 0,
            },
        })
    }

    #[test]
    fn summarize_computes_metrics_per_language() {
        let mut a1 = ans("1", true, true);
        a1.ttft_ms = 2000;
        let mut a3 = ans("3", true, true);
        a3.ttft_ms = 4000;
        let judged = vec![
            j(Record::Answer(a1), true),
            j(Record::Answer(ans("2", true, true)), false),
            j(Record::Answer(a3), true),
            j(Record::Answer(ans("4", false, false)), true),
            j(quiz("q1"), true),
            j(quiz("q2"), true),
            j(quiz("q3"), false),
            // Просили 4 вопроса, получилось 3 — недостающий считается неверным.
            j(Record::QuizBatch(QuizBatch { lang: Lang::Ru, book: "b.pdf".into(), requested: 4, generated: 3 }), true),
        ];
        let s = summarize(&judged);
        assert_eq!(s.len(), 1);
        let ru = &s[0];
        assert_eq!(ru.lang, Lang::Ru);
        assert!((ru.answer_acc.unwrap() - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(ru.refusal_acc, Some(1.0));
        assert!((ru.quiz_validity.unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(ru.ttft_median_ms, 2000); // медиана 1000, 2000, 4000 (отказ не считается)
        assert!(!ru.passed);
    }

    #[test]
    fn summarize_passes_when_all_metrics_reach_threshold() {
        let judged = vec![
            j(Record::Answer(ans("1", true, true)), true),
            j(Record::Answer(ans("2", false, false)), true),
            j(quiz("q1"), true),
        ];
        assert!(summarize(&judged)[0].passed);
    }

    #[test]
    fn missing_metric_means_not_passed() {
        let judged = vec![j(Record::Answer(ans("1", true, true)), true)];
        let s = summarize(&judged);
        assert_eq!(s[0].refusal_acc, None);
        assert!(!s[0].passed);
    }
}
