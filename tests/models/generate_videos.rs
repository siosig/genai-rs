//! Ports of `models/test_generate_videos.py` plain functions (Developer API):
//! start a long-running operation with `generate_videos`, then poll it with
//! `operations().get(...)` until `done`.

use gemini_genai::types::{
    GenerateVideosConfig, GenerateVideosOperation, GenerateVideosSource, Image, Video,
    VideoGenerationReferenceImage, VideoGenerationReferenceType,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::common::test_client;
use super::support::{TestResult, received};

const VEO_MODEL_LATEST: &str = "veo-3.1-generate-preview";
const OPERATION_NAME: &str = "models/veo-3.1-generate-preview/operations/abc123";
const VIDEO_URI: &str =
    "https://generativelanguage.googleapis.com/v1beta/files/video1:download?alt=media";
const PNG: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// A server that accepts `predictLongRunning`, reports "not done" on the
/// first poll and the finished video on the second.
async fn video_server() -> Result<MockServer, Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v1beta/models/{VEO_MODEL_LATEST}:predictLongRunning"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"name": OPERATION_NAME})),
        )
        .mount(&server)
        .await;
    let operation_path = format!("/v1beta/{OPERATION_NAME}");
    Mock::given(method("GET"))
        .and(path(operation_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": OPERATION_NAME,
            "done": true,
            "response": {"generateVideoResponse": {"generatedSamples": [
                {"video": {"uri": VIDEO_URI}}
            ]}}
        })))
        .mount(&server)
        .await;
    Ok(server)
}

/// Polls like the upstream `while not operation.done` loop.
async fn poll_until_done(
    client: &gemini_genai::Client,
    mut operation: GenerateVideosOperation,
) -> Result<GenerateVideosOperation, gemini_genai::Error> {
    while operation.done != Some(true) {
        operation = client.operations().get(&operation).await?;
    }
    Ok(operation)
}

fn first_video_uri(operation: &GenerateVideosOperation) -> Option<&str> {
    operation
        .response
        .as_ref()?
        .generated_videos
        .as_ref()?
        .first()?
        .video
        .as_ref()?
        .uri
        .as_deref()
}

/// Starts and polls one generation, returning the request body that was sent.
async fn run(
    source: GenerateVideosSource,
    config: Option<GenerateVideosConfig>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let server = video_server().await?;
    let client = test_client(server.uri());
    let operation = client
        .models()
        .generate_videos(VEO_MODEL_LATEST, source, config)
        .await?;
    assert_eq!(operation.name.as_deref(), Some(OPERATION_NAME));
    let operation = poll_until_done(&client, operation).await?;
    assert_eq!(first_video_uri(&operation), Some(VIDEO_URI));
    let requests = received(&server).await?;
    let post = requests
        .iter()
        .find(|r| r.method.as_str() == "POST")
        .ok_or("no POST was sent")?;
    Ok(post.body_json()?)
}

fn prompt_source(prompt: &str) -> GenerateVideosSource {
    GenerateVideosSource {
        prompt: Some(prompt.to_owned()),
        ..Default::default()
    }
}

fn png_image() -> Image {
    Image {
        image_bytes: Some(PNG.to_vec()),
        mime_type: Some("image/png".to_owned()),
        ..Default::default()
    }
}

// upstream-test: models/test_generate_videos.py::test_text_to_video_poll
#[tokio::test]
async fn test_text_to_video_poll() -> TestResult {
    let body = run(
        prompt_source("A neon hologram of a cat driving at top speed"),
        None,
    )
    .await?;
    assert_eq!(
        body["instances"][0]["prompt"], "A neon hologram of a cat driving at top speed",
        "{body}"
    );
    Ok(())
}

// upstream-test: models/test_generate_videos.py::test_image_to_video_poll
#[tokio::test]
async fn test_image_to_video_poll() -> TestResult {
    let body = run(
        GenerateVideosSource {
            image: Some(png_image()),
            ..Default::default()
        },
        None,
    )
    .await?;
    assert_eq!(
        body["instances"][0]["image"]["mimeType"], "image/png",
        "{body}"
    );
    assert!(body["instances"][0]["image"]["bytesBase64Encoded"].is_string());
    Ok(())
}

// upstream-test: models/test_generate_videos.py::test_text_and_image_to_video_poll
#[tokio::test]
async fn test_text_and_image_to_video_poll() -> TestResult {
    let body = run(
        GenerateVideosSource {
            prompt: Some("Lightning storm".to_owned()),
            image: Some(png_image()),
            ..Default::default()
        },
        None,
    )
    .await?;
    assert_eq!(body["instances"][0]["prompt"], "Lightning storm", "{body}");
    assert_eq!(body["instances"][0]["image"]["mimeType"], "image/png");
    Ok(())
}

async fn extension_flow() -> TestResult {
    let server = video_server().await?;
    let client = test_client(server.uri());
    let config = GenerateVideosConfig {
        number_of_videos: Some(1),
        ..Default::default()
    };
    let first = client
        .models()
        .generate_videos(
            VEO_MODEL_LATEST,
            prompt_source("Rain"),
            Some(config.clone()),
        )
        .await?;
    let first = poll_until_done(&client, first).await?;
    let video1: Video = first
        .response
        .and_then(|r| r.generated_videos)
        .and_then(|v| v.into_iter().next())
        .and_then(|g| g.video)
        .ok_or("no generated video")?;
    assert_eq!(video1.uri.as_deref(), Some(VIDEO_URI));

    // extend the generated video with a new prompt
    let second = client
        .models()
        .generate_videos(
            VEO_MODEL_LATEST,
            GenerateVideosSource {
                prompt: Some("Sun".to_owned()),
                video: Some(video1),
                ..Default::default()
            },
            Some(config),
        )
        .await?;
    let second = poll_until_done(&client, second).await?;
    assert_eq!(first_video_uri(&second), Some(VIDEO_URI));
    let requests = received(&server).await?;
    let posts: Vec<serde_json::Value> = requests
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .map(wiremock::Request::body_json)
        .collect::<Result<_, _>>()?;
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[1]["instances"][0]["prompt"], "Sun", "{}", posts[1]);
    assert_eq!(
        posts[1]["instances"][0]["video"]["uri"], VIDEO_URI,
        "{}",
        posts[1]
    );
    Ok(())
}

// upstream-test: models/test_generate_videos.py::test_generated_video_extension_poll
#[tokio::test]
async fn test_generated_video_extension_poll() -> TestResult {
    // upstream's `video=` shorthand is a `GenerateVideosSource.video` here.
    extension_flow().await
}

// upstream-test: models/test_generate_videos.py::test_generated_video_extension_from_source_poll
#[tokio::test]
async fn test_generated_video_extension_from_source_poll() -> TestResult {
    extension_flow().await
}

// upstream-test: models/test_generate_videos.py::test_generated_video_extension_from_source_poll_async
#[tokio::test]
async fn test_generated_video_extension_from_source_poll_async() -> TestResult {
    extension_flow().await
}

// upstream-test: models/test_generate_videos.py::test_image_to_video_frame_interpolation_poll
#[tokio::test]
async fn test_image_to_video_frame_interpolation_poll() -> TestResult {
    let body = run(
        GenerateVideosSource {
            prompt: Some("Rain".to_owned()),
            image: Some(png_image()),
            ..Default::default()
        },
        Some(GenerateVideosConfig {
            last_frame: Some(Image {
                image_bytes: Some(vec![1, 2, 3]),
                mime_type: Some("image/jpeg".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }),
    )
    .await?;
    assert_eq!(
        body["instances"][0]["lastFrame"]["mimeType"], "image/jpeg",
        "{body}"
    );
    Ok(())
}

// upstream-test: models/test_generate_videos.py::test_reference_images_to_video_poll
#[tokio::test]
async fn test_reference_images_to_video_poll() -> TestResult {
    let body = run(
        prompt_source("Chirping birds in a colorful forest"),
        Some(GenerateVideosConfig {
            reference_images: Some(vec![VideoGenerationReferenceImage {
                image: Some(png_image()),
                reference_type: Some(VideoGenerationReferenceType::Asset),
            }]),
            ..Default::default()
        }),
    )
    .await?;
    let reference = &body["instances"][0]["referenceImages"][0];
    assert_eq!(reference["referenceType"], "ASSET", "{body}");
    assert_eq!(reference["image"]["mimeType"], "image/png");
    Ok(())
}

// upstream-test: models/test_generate_videos.py::test_create_operation_to_poll
#[tokio::test]
async fn test_create_operation_to_poll() -> TestResult {
    let server = video_server().await?;
    let client = test_client(server.uri());
    // a bare operation carrying only its name, as upstream constructs it
    let operation = GenerateVideosOperation {
        name: Some(OPERATION_NAME.to_owned()),
        ..Default::default()
    };
    let operation = client.operations().get(&operation).await?;
    let operation = poll_until_done(&client, operation).await?;
    assert_eq!(first_video_uri(&operation), Some(VIDEO_URI));
    Ok(())
}

// upstream-test: models/test_generate_videos.py::test_text_to_video_poll_async
#[tokio::test]
async fn test_text_to_video_poll_async() -> TestResult {
    let body = run(
        prompt_source("A neon hologram of a cat driving at top speed"),
        None,
    )
    .await?;
    assert_eq!(
        body["instances"][0]["prompt"],
        "A neon hologram of a cat driving at top speed"
    );
    Ok(())
}
