//! Ports of `models/test_embed_content.py` plain functions (Developer API).

use gemini_genai::types::{EmbedContentConfig, Part};
use wiremock::MockServer;

use super::common::test_client;
use super::support::{TestResult, mount_json, received};

fn embedding_reply(dim: usize) -> serde_json::Value {
    let values = vec![0.5_f64; dim];
    serde_json::json!({"embeddings": [{"values": values}]})
}

fn dimensionality(n: i64) -> EmbedContentConfig {
    EmbedContentConfig {
        output_dimensionality: Some(n),
        ..Default::default()
    }
}

// upstream-test: models/test_embed_content.py::test_gemini_embedding_2_content_combination
#[tokio::test]
async fn test_gemini_embedding_2_content_combination() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "POST", 200, &embedding_reply(100)).await;
    let client = test_client(server.uri());
    // upstream passes [str, Part(bytes), Part(uri)] which is folded into a
    // single user Content with three parts
    let parts = vec![
        Part::from_text("The jetpack is cool"),
        Part::from_bytes(vec![0x89, b'P', b'N', b'G'], "image/png"),
        Part::from_uri(
            "gs://generativeai-downloads/images/scones.jpg",
            "image/jpeg",
        ),
    ];
    let response = client
        .models()
        .embed_content(
            "gemini-embedding-2-preview",
            parts,
            Some(dimensionality(100)),
        )
        .await?;
    let embeddings = response.embeddings.ok_or("no embeddings")?;
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].values.as_ref().map(Vec::len), Some(100));
    let body: serde_json::Value = received(&server).await?[0].body_json()?;
    let sent_parts = &body["requests"][0]["content"]["parts"];
    assert_eq!(sent_parts.as_array().map(Vec::len), Some(3), "{body}");
    Ok(())
}

// upstream-test: models/test_embed_content.py::test_async
#[tokio::test]
async fn test_async() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "POST", 200, &embedding_reply(10)).await;
    let client = test_client(server.uri());
    let response = client
        .models()
        .embed_content(
            "gemini-embedding-001",
            "What is your name?",
            Some(dimensionality(10)),
        )
        .await?;
    assert_eq!(response.embeddings.map(|e| e.len()), Some(1));
    Ok(())
}

// upstream-test: models/test_embed_content.py::test_async_new_api
#[tokio::test]
async fn test_async_new_api() -> TestResult {
    let server = MockServer::start().await;
    mount_json(&server, "POST", 200, &embedding_reply(10)).await;
    let client = test_client(server.uri());
    let response = client
        .models()
        .embed_content(
            "gemini-embedding-2-preview",
            "What is your name?",
            Some(dimensionality(10)),
        )
        .await?;
    assert_eq!(response.embeddings.map(|e| e.len()), Some(1));
    Ok(())
}
