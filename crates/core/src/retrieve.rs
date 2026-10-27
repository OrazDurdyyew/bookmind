//! Гибридный поиск: полнотекстовый (FTS5) + векторный, объединение через RRF.

use crate::llm::{EmbedKind, Llm};
use crate::model::{BookId, ChunkId, StoredChunk};
use crate::store::Store;

/// Сколько кандидатов берём из каждого вида поиска.
pub const CANDIDATES: usize = 20;
/// Константа k в формуле RRF: 1 / (k + место).
pub const RRF_K: f32 = 60.0;
/// Сколько фрагментов отдаём модели для ответа.
pub const DEFAULT_TOP_K: usize = 6;

/// Reciprocal Rank Fusion: объединяет несколько рейтингов в один.
/// Оценка id = сумма 1 / (k + место), место начинается с 1.
/// При равной оценке сохраняется порядок первого появления.
pub fn rrf(lists: &[Vec<ChunkId>], k: f32) -> Vec<ChunkId> {
    let mut scores: Vec<(ChunkId, f32)> = Vec::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            let add = 1.0 / (k + rank as f32 + 1.0);
            match scores.iter_mut().find(|(x, _)| x == id) {
                Some((_, s)) => *s += add,
                None => scores.push((*id, add)),
            }
        }
    }
    // sort_by стабилен: при равенстве остаётся порядок появления.
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores.into_iter().map(|(id, _)| id).collect()
}

/// Находит `top_k` фрагментов книги, лучше всего подходящих к вопросу.
pub async fn retrieve<L: Llm>(store: &Store, llm: &L, book: BookId, query: &str, top_k: usize) -> anyhow::Result<Vec<StoredChunk>> {
    let text_hits = store.fts_search(book, query, CANDIDATES)?;

    let q = llm.embed(EmbedKind::Query, &[query.to_string()]).await?;
    let q = q.first().ok_or_else(|| anyhow::anyhow!("пустой эмбеддинг вопроса"))?;
    let mut scored: Vec<(ChunkId, f32)> = store
        .vectors(book)?
        .into_iter()
        .map(|(id, v)| (id, v.iter().zip(q).map(|(a, b)| a * b).sum()))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let vector_hits: Vec<ChunkId> = scored.into_iter().take(CANDIDATES).map(|(id, _)| id).collect();

    let ids: Vec<ChunkId> = rrf(&[text_hits, vector_hits], RRF_K).into_iter().take(top_k).collect();
    store.chunks(&ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeLlm;
    use crate::{ChunkDraft, Lang};

    #[test]
    fn rrf_fuses_rankings() {
        assert_eq!(rrf(&[vec![1, 2, 3], vec![3, 1]], 60.0), vec![1, 3, 2]);
        assert_eq!(rrf(&[vec![7], vec![]], 60.0), vec![7]);
        assert_eq!(rrf(&[vec![], vec![5, 6]], 60.0), vec![5, 6]);
    }

    #[tokio::test]
    async fn retrieve_puts_best_matching_chunk_first() {
        let store = Store::open_in_memory().unwrap();
        let book = store.insert_book("b", "b.txt", Lang::Ru, None, &[None]).unwrap();
        let texts = [
            "животные двигаются быстро",
            "листья бывают зелёные и жёлтые",
            "фотосинтез идёт в листьях растений",
            "вода нужна всем",
            "солнце светит днём",
            "корни держат растение",
            "семена прорастают весной",
            "цветы привлекают пчёл",
        ];
        let drafts: Vec<ChunkDraft> = texts
            .iter()
            .map(|t| ChunkDraft { chapter_index: 0, subheading: None, page_start: None, page_end: None, text: t.to_string() })
            .collect();
        let embs: Vec<Vec<f32>> = texts.iter().map(|t| FakeLlm::vector(t)).collect();
        store.insert_chunks(book, &drafts, &embs).unwrap();

        let found = retrieve(&store, &FakeLlm::new(), book, "где идёт фотосинтез в листьях", 3).await.unwrap();
        assert!(found.len() <= 3);
        assert_eq!(found[0].text, "фотосинтез идёт в листьях растений");
    }
}
