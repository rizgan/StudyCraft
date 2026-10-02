//! Mnemonic image generation.
//!
//! Domain-neutral abstraction over image-generation providers. The domain layer
//! (this module and the commands that use it) only ever speak
//! [`ImageGenerationProvider`] — no provider SDK types leak out. Providers are
//! resolved from [`crate::config::app_settings::ImageGenerationConfig`], so the
//! capability is opt-in and configuration-driven.
//!
//! There is intentionally **no mock provider**: when the capability is not
//! configured, generation fails fast with a clear error so the UI can show the
//! feature as unavailable. Text mnemonics (a separate capability) are not
//! affected.

pub mod openai_compatible;
pub mod prompt;

use crate::config::app_settings::ImageGenerationConfig;
use crate::config::keystore::ApiKeystore;
use crate::errors::{AppError, AppResult};
use std::future::Future;
use std::path::Path;
use std::sync::OnceLock;

/// A vendor-agnostic image generator.
pub trait ImageGenerationProvider: Send + Sync {
    /// Generate an image for `prompt` and write the image bytes to `output_path`.
    ///
    /// Implementations must fail with a descriptive error when the request
    /// cannot be made (bad config, missing key, provider error). Callers treat
    /// any error as "no image produced"; they never assume partial output.
    ///
    /// The pinned, boxed future keeps the trait object-safe so providers can be
    /// resolved behind a `dyn ImageGenerationProvider` and the returned future
    /// can be `.await`ed directly by callers (Box::pin is required for the
    /// auto-deref `.await` on `Box<dyn Future>` to work).
    fn generate<'a>(
        &'a self,
        prompt: &'a str,
        output_path: &'a Path,
    ) -> PinnedBoxFuture<'a, AppResult<()>>;
}

/// A pinned, boxed, 'a-tied future used to keep [`ImageGenerationProvider`]
/// object-safe. `Pin<Box<...>>` is what `Box::pin` produces and is the type
/// that `.await` auto-derefs correctly.
pub type PinnedBoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Build the concrete provider from user configuration.
///
/// Returns an error when the capability is unavailable (disabled, no model,
/// no key) — the caller surfaces this to the UI instead of faking a result.
pub fn build_provider(
    config: &ImageGenerationConfig,
    keystore: &ApiKeystore,
) -> AppResult<Box<dyn ImageGenerationProvider>> {
    config.unavailability_error()?;

    let api_key = if config.use_keystore_key {
        keystore
            .get_api_key("OpenAI")?
            .ok_or_else(|| {
                AppError::ApiError(
                    "No OpenAI API key is stored. Add one in Settings → API Keys, or enter a key for mnemonic image generation."
                        .to_string(),
                )
            })?
    } else {
        return Err(AppError::ApiError(
            "Mnemonic image generation must use the keystore for API keys.".to_string(),
        ));
    };

    Ok(Box::new(
        openai_compatible::OpenAiCompatibleImageProvider::new(
            config.endpoint.trim_end_matches('/').to_string(),
            api_key,
            config.model.trim().to_string(),
            config.size.trim().to_string(),
        ),
    ))
}

/// Shared HTTP client for image-generation provider calls (mirrors the
/// `shared_client` pattern used by the vision service).
pub fn shared_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disabled_config() -> ImageGenerationConfig {
        ImageGenerationConfig::default()
    }

    #[test]
    fn test_build_provider_disabled_returns_clear_error() {
        let keystore = ApiKeystore::new();
        let result = build_provider(&disabled_config(), &keystore);
        assert!(result.is_err());
        let msg = match result {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected an error"),
        };
        assert!(
            msg.contains("disabled") || msg.contains("Disabled"),
            "expected a clear disabled-capability error, got: {}",
            msg
        );
    }

    #[test]
    fn test_build_provider_no_model_returns_clear_error() {
        let mut config = disabled_config();
        config.enabled = true;
        config.model = "  ".into();
        let keystore = ApiKeystore::new();
        let result = build_provider(&config, &keystore);
        assert!(result.is_err());
        let msg = match result {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected an error"),
        };
        assert!(msg.contains("model"), "got: {}", msg);
    }

    #[test]
    fn test_unavailability_error_default_is_disabled() {
        let config = ImageGenerationConfig::default();
        assert!(!config.enabled);
        let err = config.unavailability_error().unwrap_err();
        assert!(err.to_string().contains("disabled"));
    }
}
