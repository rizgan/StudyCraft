//! Pronunciation/audio provider abstraction (language-learning session 7A).
//!
//! StudyCraft does not bundle a TTS engine: speech synthesis is an optional
//! provider capability consumed through this abstraction. The current
//! provider is the OpenAI speech API, reusing the OpenAI API key the app
//! already manages. When no provider is configured (no OpenAI API key), the
//! service reports a clear `TtsNotConfigured` error instead of failing
//! silently; the UI hook then simply does not offer audio.

use crate::config::api_config::LlmUserConfig;
use crate::errors::{AppError, AppResult};
use flyllm::ProviderType;

/// A synthesized pronunciation sample.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TtsAudio {
    /// Raw audio bytes (MP3 for the current provider).
    pub data: Vec<u8>,
    /// MIME type of `data`.
    pub content_type: String,
}

/// A text-to-speech provider. Adding a new engine means adding a new
/// implementation here, not touching call sites.
///
/// `synthesize` returns a pinned, boxed future so the trait stays
/// object-safe (`Box<dyn TtsProvider>`); no async-trait dependency is
/// needed.
pub trait TtsProvider: Send + Sync {
    /// Stable identifier of the provider (for logging/diagnostics).
    fn id(&self) -> &str;
    /// Synthesize `text` into an audio sample.
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<TtsAudio>> + Send + 'a>>;
}

/// OpenAI speech endpoint (`POST /audio/speech`), authenticated with the
/// OpenAI API key already stored for the LLM provider.
pub struct OpenAiTtsProvider {
    api_key: String,
    base_url: String,
    model: String,
    voice: String,
}

impl OpenAiTtsProvider {
    pub fn new(api_key: String, base_url: String, model: String, voice: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            voice,
        }
    }
}

impl TtsProvider for OpenAiTtsProvider {
    fn id(&self) -> &str {
        "openai-tts"
    }

    fn synthesize<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<TtsAudio>> + Send + 'a>> {
        Box::pin(Self::synthesize_inner(self, text))
    }
}

impl OpenAiTtsProvider {
    async fn synthesize_inner(&self, text: &str) -> AppResult<TtsAudio> {
        if text.trim().is_empty() {
            return Err(AppError::ValidationError(
                "No text to synthesize".to_string(),
            ));
        }

        let url = format!("{}/audio/speech", self.base_url.trim_end_matches('/'));
        let client = shared_client();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "voice": self.voice,
                "input": text,
            }))
            .send()
            .await
            .map_err(|e| AppError::TtsProviderError(format!("TTS request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.map_err(|e| {
                AppError::TtsProviderError(format!("TTS response read failed: {}", e))
            })?;
            return Err(AppError::TtsProviderError(format!(
                "TTS provider returned {} : {}",
                status, body
            )));
        }

        let data = response
            .bytes()
            .await
            .map_err(|e| AppError::TtsProviderError(format!("TTS response read failed: {}", e)))?;

        Ok(TtsAudio {
            data: data.to_vec(),
            content_type: "audio/mpeg".to_string(),
        })
    }
}

/// Builds the shared reqwest client, reusing the same pattern as the vision
/// and image services.
/// Shared HTTP client, following the same OnceLock pattern as the vision
/// and image services.
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// The pronunciation/audio service. Holds at most one configured provider.
pub struct TtsService {
    provider: Option<Box<dyn TtsProvider>>,
}

impl TtsService {
    /// Build the service from the application configuration. A provider is
    /// available exactly when the OpenAI API key is configured; otherwise the
    /// service holds no provider and reports [`AppError::TtsNotConfigured`].
    pub fn from_config() -> AppResult<Self> {
        let config = LlmUserConfig::load()?;
        let provider = match config.get_api_key("OpenAI")? {
            Some(api_key) => {
                let base_url = config
                    .provider_configs
                    .get(&ProviderType::OpenAI)
                    .and_then(|c| c.endpoint_url.clone())
                    .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
                Some(Box::new(OpenAiTtsProvider::new(
                    api_key,
                    base_url,
                    "tts-1".to_string(),
                    "alloy".to_string(),
                )) as Box<dyn TtsProvider>)
            }
            None => None,
        };
        Ok(TtsService { provider })
    }

    /// True when a provider can synthesize speech.
    pub fn is_configured(&self) -> bool {
        self.provider.is_some()
    }

    /// Synthesize `text` through the configured provider.
    pub async fn synthesize(&self, text: &str) -> AppResult<TtsAudio> {
        match self.provider.as_ref() {
            Some(provider) => provider.synthesize(text).await,
            None => Err(AppError::TtsNotConfigured),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_service_without_provider_reports_not_configured() {
        // A service with no provider must report the clear not-configured
        // error, never succeed with empty audio.
        let service = TtsService { provider: None };
        assert!(!service.is_configured());
        let result = block_future(service.synthesize("hello"));
        match result {
            Err(AppError::TtsNotConfigured) => {}
            other => panic!(
                "expected TtsNotConfigured, got {:?}",
                other.map(|a| a.content_type)
            ),
        }
    }

    /// Runs a future in a fresh runtime for a unit test.
    fn block_future<F: std::future::Future>(future: F) -> F::Output {
        let runtime = tokio::runtime::Runtime::new().expect("failed to create test runtime");
        runtime.block_on(future)
    }
}
