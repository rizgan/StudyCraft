//! OpenAI-compatible `images/generations` provider (b64_json response format).
//!
//! This is the single concrete provider for mnemonic image generation. It is
//! deliberately generic (base endpoint + model + size) so it works with any
//! OpenAI-compatible images endpoint, and it reuses the shared reqwest client
//! and the keystore-based key management from the rest of the app.

use crate::errors::{AppError, AppResult};
use crate::services::image_generation::{shared_client, ImageGenerationProvider, PinnedBoxFuture};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone)]
pub struct OpenAiCompatibleImageProvider {
    endpoint: String,
    api_key: String,
    model: String,
    size: String,
}

#[derive(Deserialize)]
struct ImagesResponse {
    data: Vec<ImageData>,
}

#[derive(Deserialize)]
struct ImageData {
    b64_json: Option<String>,
    url: Option<String>,
}

impl OpenAiCompatibleImageProvider {
    pub fn new(endpoint: String, api_key: String, model: String, size: String) -> Self {
        Self {
            endpoint,
            api_key,
            model,
            size,
        }
    }

    async fn generate_inner(&self, prompt: &str, output_path: &Path) -> AppResult<()> {
        let url = format!("{}/images/generations", self.endpoint);
        let client = shared_client();

        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "n": 1,
            "size": self.size,
            "response_format": "b64_json",
        });

        let response = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::ApiError(format!("Image generation request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::ApiError(format!(
                "Image generation failed (HTTP {}): {}",
                status.as_u16(),
                text
            )));
        }

        let parsed: ImagesResponse = response
            .json()
            .await
            .map_err(|e| AppError::ApiError(format!("Failed to parse image response: {}", e)))?;

        let image = parsed
            .data
            .into_iter()
            .next()
            .ok_or_else(|| AppError::ApiError("Image provider returned no image".to_string()))?;

        let bytes = if let Some(b64) = image.b64_json {
            STANDARD
                .decode(b64)
                .map_err(|e| AppError::ApiError(format!("Failed to decode image: {}", e)))?
        } else if let Some(url) = image.url {
            // Some compatible endpoints return a URL instead of base64.
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| AppError::ApiError(format!("Failed to fetch image: {}", e)))?;
            let ok = resp.status().is_success();
            let body = resp
                .bytes()
                .await
                .map_err(|e| AppError::ApiError(format!("Failed to read image: {}", e)))?;
            if !ok {
                return Err(AppError::ApiError(
                    "Failed to fetch image from provider URL".to_string(),
                ));
            }
            body.to_vec()
        } else {
            return Err(AppError::ApiError(
                "Image provider returned neither b64_json nor url".to_string(),
            ));
        };

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::IoError(e))?;
        }
        std::fs::write(output_path, &bytes).map_err(|e| AppError::IoError(e))?;

        Ok(())
    }
}

impl ImageGenerationProvider for OpenAiCompatibleImageProvider {
    fn generate<'a>(&'a self, prompt: &'a str, output_path: &'a Path) -> PinnedBoxFuture<'a, AppResult<()>> {
        let this = self.clone();
        Box::pin(async move { this.generate_inner(prompt, output_path).await })
    }
}
