//! Uploads a file through the resumable upload protocol, uses it in a
//! prompt, then deletes it.
//!
//! Requires `GEMINI_API_KEY` (or `GOOGLE_API_KEY`) in the environment. Run
//! with:
//!
//! ```sh
//! GEMINI_API_KEY=... cargo run --example files_upload
//! ```

use gemini_genai::Client;
use gemini_genai::files::UploadSource;
use gemini_genai::types::{Content, Part};

#[tokio::main]
async fn main() -> gemini_genai::Result<()> {
    let client = Client::new()?;

    let uploaded = client
        .files()
        .upload(
            UploadSource::Bytes {
                data: b"Ada Lovelace wrote the first published algorithm intended \
                        for a machine, in 1843."
                    .to_vec(),
                mime_type: "text/plain".to_owned(),
            },
            None,
        )
        .await?;

    let name = uploaded.name.clone().unwrap_or_default();
    let uri = uploaded.uri.clone().unwrap_or_default();
    println!(
        "uploaded {name} ({} bytes)",
        uploaded.size_bytes.unwrap_or(0)
    );

    // Reference the uploaded file by URI rather than re-sending its bytes.
    let response = client
        .models()
        .generate_content(
            "gemini-flash-latest",
            vec![Content {
                role: Some("user".to_owned()),
                parts: Some(vec![
                    Part::from_text("In one sentence, what does this file say?"),
                    Part::from_uri(uri, "text/plain"),
                ]),
            }],
            None,
        )
        .await?;
    println!("{}", response.text().unwrap_or_default());

    client.files().delete(&name, None).await?;
    println!("deleted {name}");
    Ok(())
}
