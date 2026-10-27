//! Живой тест с настоящим llama-server. Запуск:
//! BOOKMIND_TEST_CHAT_MODEL=... BOOKMIND_TEST_EMBED_MODEL=... cargo test --test llama_live -- --ignored

use std::path::PathBuf;

use bookmind_core::llm::{ChatRequest, EmbedKind, EmbedPrefixes, LlamaHttp, Llm, Message, Role};
use bookmind_core::server::{free_port, LlamaServer, Pooling, ServerConfig};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/llama").join(format!("llama-server{}", std::env::consts::EXE_SUFFIX))
}

#[tokio::test]
#[ignore]
async fn live_chat_json_and_embeddings() {
    let chat_model = PathBuf::from(std::env::var("BOOKMIND_TEST_CHAT_MODEL").expect("BOOKMIND_TEST_CHAT_MODEL"));
    let embed_model = PathBuf::from(std::env::var("BOOKMIND_TEST_EMBED_MODEL").expect("BOOKMIND_TEST_EMBED_MODEL"));
    let chat = LlamaServer::spawn(&ServerConfig {
        binary: binary(), model: chat_model, port: free_port(), ctx_size: 4096, threads: Some(4), gpu_layers: 0, embedding: None,
    }).await.unwrap();
    let embed = LlamaServer::spawn(&ServerConfig {
        binary: binary(), model: embed_model, port: free_port(), ctx_size: 2048, threads: Some(4), gpu_layers: 0, embedding: Some(Pooling::Mean),
    }).await.unwrap();
    let llm = LlamaHttp::new(&chat.base_url(), &embed.base_url(), EmbedPrefixes { query: "query: ".into(), passage: "passage: ".into() });

    let v = llm.embed(EmbedKind::Passage, &["Фотосинтез идёт в листьях.".into()]).await.unwrap();
    assert!(v[0].len() >= 128);
    let len: f32 = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((len - 1.0).abs() < 1e-3);

    let req = ChatRequest { messages: vec![Message { role: Role::User, content: "Say hello.".into() }], temperature: 0.0, max_tokens: 32 };
    let mut pieces = 0;
    let (text, stats) = llm.chat_stream(&req, &mut |_| pieces += 1).await.unwrap();
    assert!(!text.trim().is_empty());
    assert!(stats.ttft_ms > 0 && pieces > 0);

    let schema = serde_json::json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]});
    let req = ChatRequest { messages: vec![Message { role: Role::User, content: "Reply with ok=true.".into() }], temperature: 0.0, max_tokens: 32 };
    let j = llm.chat_json(&req, &schema).await.unwrap();
    assert!(j["ok"].is_boolean());
}
