use crate::config::api_config::LlmUserConfig;
use crate::config::keystore::ApiKeystore;
use crate::errors::{AppError, AppResult};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flyllm::ProviderType;
use serde::{Deserialize, Serialize};
use std::fs;

/// Structured output produced by the vision (image understanding) pipeline.
///
/// The model is asked to describe an image and extract the study-relevant
/// concepts, terms and facts it contains, so the result can be fed into the
/// RAG / flashcard generation flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAnalysis {
    /// A natural-language description of the image.
    pub description: String,
    /// Key terms or labels visible in the image.
    pub key_terms: Vec<String>,
    /// Study-relevant facts or relationships extracted from the image.
    pub extracted_facts: Vec<String>,
    /// Whether the image appears to be a diagram / figure rather than a photo.
    pub is_diagram: bool,
}

/// The default vision-capable model used per provider when the caller does not
/// supply an explicit model name.
pub fn default_vision_model(provider: &ProviderType) -> &'static str {
    match provider {
        ProviderType::OpenAI => "gpt-4o-mini",
        ProviderType::Anthropic => "claude-3-5-sonnet-latest",
        ProviderType::Mistral => "mistral-large-latest",
        ProviderType::Google => "gemini-2.0-flash",
        // Ollama has no guaranteed multimodal default; a caller-specified model
        // (e.g. llava) is required.
        ProviderType::Ollama => "llava",
    }
}

const VISION_PROMPT: &str = "You are an expert study assistant. Analyze this image and return \
ONLY a valid JSON object (no markdown, no code fences) with exactly these keys: \
\"description\" (a clear text description of the image), \
\"key_terms\" (an array of important terms or labels shown), \
\"extracted_facts\" (an array of study-relevant facts or relationships), and \
\"is_diagram\" (a boolean: true if this is a diagram/figure/illustration rather than a photo). \
Be precise and factual. If the image is not a diagram, set is_diagram to false.";

/// Analyze an image file using a vision-capable LLM.
///
/// Reads the image, base64-encodes it, sends it to the configured provider's
/// multimodal endpoint and parses the model's response into an
/// [`ImageAnalysis`].
pub async fn analyze_image(
    config: &LlmUserConfig,
    image_path: &str,
    prompt: Option<String>,
    model: Option<String>,
    provider_override: Option<&ProviderType>,
) -> AppResult<ImageAnalysis> {
    let image_bytes = fs::read(image_path)
        .map_err(|e| AppError::IoError(e))
        .and_then(|bytes| {
            if bytes.is_empty() {
                Err(AppError::InvalidInput(format!(
                    "Image '{}' is empty",
                    image_path
                )))
            } else {
                Ok(bytes)
            }
        })?;

    let (provider, model_name, api_key, endpoint) = resolve_provider(
        config,
        provider_override,
        model.as_deref(),
    )
    .map_err(|e| AppError::ApiError(e))?;

    let user_prompt = prompt.unwrap_or_else(|| VISION_PROMPT.to_string());
    let base64_image = STANDARD.encode(&image_bytes);
    let mime = mime_type_for_path(image_path);

    let raw = match provider {
        ProviderType::OpenAI => {
            call_openai_compatible(
                endpoint,
                &api_key,
                &model_name,
                &user_prompt,
                &base64_image,
                &mime,
            )
            .await
        }
        ProviderType::Mistral => {
            call_openai_compatible(
                endpoint,
                &api_key,
                &model_name,
                &user_prompt,
                &base64_image,
                &mime,
            )
            .await
        }
        ProviderType::Anthropic => {
            call_anthropic(
                endpoint,
                &api_key,
                &model_name,
                &user_prompt,
                &base64_image,
                &mime,
            )
            .await
        }
        ProviderType::Google => {
            call_google(
                endpoint,
                &api_key,
                &model_name,
                &user_prompt,
                &base64_image,
                &mime,
            )
            .await
        }
        ProviderType::Ollama => {
            call_ollama(&endpoint, &model_name, &user_prompt, &base64_image, &mime).await
        }
    }?;

    parse_image_analysis(&raw)
}

/// Determine the provider/model/endpoint/api key to use.
fn resolve_provider(
    config: &LlmUserConfig,
    provider_override: Option<&ProviderType>,
    model: Option<&str>,
) -> Result<(ProviderType, String, String, String), String> {
    let providers: Vec<ProviderType> = match provider_override {
        Some(p) => vec![*p],
        None => config
            .get_enabled_providers()
            .iter()
            .filter_map(|s| {
                match s.as_str() {
                    "OpenAI" => Some(ProviderType::OpenAI),
                    "Anthropic" => Some(ProviderType::Anthropic),
                    "Mistral" => Some(ProviderType::Mistral),
                    "Google" => Some(ProviderType::Google),
                    "Ollama" => Some(ProviderType::Ollama),
                    _ => None,
                }
            })
            .collect(),
    };

    if providers.is_empty() {
        return Err(
            "No enabled LLM provider available for vision. Enable at least one provider in \
             Settings."
                .to_string(),
        );
    }

    let keystore = ApiKeystore::new();

    for provider in providers {
        let provider_str = format!("{:?}", provider);

        // Pick the model: explicit override, else first available vision model,
        // else the provider default.
        let available = config
            .get_available_models(&provider_str)
            .unwrap_or_default();

        let model_name = model
            .map(|m| m.to_string())
            .or_else(|| {
                available
                    .iter()
                    .find(|m| is_vision_capable(m))
                    .cloned()
            })
            .unwrap_or_else(|| default_vision_model(&provider).to_string());

        // Resolve the endpoint.
        let endpoint = match provider {
            ProviderType::OpenAI => "https://api.openai.com/v1/chat/completions".to_string(),
            ProviderType::Mistral => "https://api.mistral.ai/v1/chat/completions".to_string(),
            ProviderType::Anthropic => "https://api.anthropic.com/v1/messages".to_string(),
            ProviderType::Google => "https://generativelanguage.googleapis.com/v1beta".to_string(),
            ProviderType::Ollama => {
                config
                    .get_ollama_endpoint()
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "http://localhost:11434".to_string())
            }
        };

        // Ollama requires no API key; others do.
        if provider == ProviderType::Ollama {
            return Ok((provider, model_name, String::new(), endpoint));
        }

        match keystore.get_api_key(&provider_str) {
            Ok(Some(key)) if !key.is_empty() => {
                return Ok((provider, model_name, key, endpoint));
            }
            Ok(_) => {
                continue; // No key for this provider — try the next.
            }
            Err(e) => {
                eprintln!(
                    "Failed to read API key for {}: {}",
                    provider_str, e
                );
                continue;
            }
        }
    }

    Err(
        "No configured (key + enabled) provider available for vision. Add an API key in \
         Settings or use a local Ollama vision model."
            .to_string(),
    )
}

/// Heuristic: does this model name look multimodal?
fn is_vision_capable(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("gpt-4o")
        || m.contains("gpt-4-turbo")
        || m.contains("gpt-4-vision")
        || m.contains("gpt-3.5-turbo")
        || m.contains("vision")
        || m.contains("claude")
        || m.contains("gemini")
        || m.contains("mistral-large")
        || m.contains("pixtral")
        || m.contains("llava")
        || m.contains("moondream")
        || m.contains("minicpm-v")
}

fn mime_type_for_path(path: &str) -> String {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "webp" => "image/webp".to_string(),
        "gif" => "image/gif".to_string(),
        "bmp" => "image/bmp".to_string(),
        other => format!("image/{}", other),
    }
}

// ---------------------------------------------------------------------------
// Provider calls
// ---------------------------------------------------------------------------

async fn call_openai_compatible(
    endpoint: String,
    api_key: &str,
    model: &str,
    prompt: &str,
    image_b64: &str,
    mime: &str,
) -> AppResult<String> {
    let client = reqwest::Client::new();
    let url = if endpoint.ends_with("chat/completions") {
        endpoint
    } else {
        format!("{}/chat/completions", endpoint.trim_end_matches('/'))
    };

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", mime, image_b64)
                        }
                    }
                ]
            }
        ]
    });

    let response = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::ApiError(format!("Vision request failed: {}", e)))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| AppError::ApiError(format!("Failed to read response: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::ApiError(format!(
            "Vision API error ({}): {}",
            status.as_u16(),
            text
        )));
    }

    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| AppError::ApiError(format!("Invalid vision response JSON: {}", e)))?;

    parsed
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::ApiError("No content in vision response".to_string()))
}

async fn call_anthropic(
    endpoint: String,
    api_key: &str,
    model: &str,
    prompt: &str,
    image_b64: &str,
    mime: &str,
) -> AppResult<String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": mime,
                            "data": image_b64
                        }
                    },
                    {
                        "type": "text",
                        "text": prompt
                    }
                ]
            }
        ]
    });

    let response = client
        .post(&endpoint)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::ApiError(format!("Vision request failed: {}", e)))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| AppError::ApiError(format!("Failed to read response: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::ApiError(format!(
            "Anthropic vision error ({}): {}",
            status.as_u16(),
            text
        )));
    }

    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| AppError::ApiError(format!("Invalid vision response JSON: {}", e)))?;

    let content = parsed
        .pointer("/content/0/text")
        .and_then(|c| c.as_str())
        .ok_or_else(|| AppError::ApiError("No content in Anthropic response".to_string()))?;

    Ok(content.to_string())
}

async fn call_google(
    endpoint: String,
    api_key: &str,
    model: &str,
    prompt: &str,
    image_b64: &str,
    mime: &str,
) -> AppResult<String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/models/{}:generateContent",
        endpoint.trim_end_matches('/'),
        model
    );

    let body = serde_json::json!({
        "contents": [
            {
                "parts": [
                    { "text": prompt },
                    { "inline_data": { "mime_type": mime, "data": image_b64 } }
                ]
            }
        ]
    });

    let response = client
        .post(&url)
        .query(&[("key", api_key)])
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::ApiError(format!("Vision request failed: {}", e)))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| AppError::ApiError(format!("Failed to read response: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::ApiError(format!(
            "Google vision error ({}): {}",
            status.as_u16(),
            text
        )));
    }

    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| AppError::ApiError(format!("Invalid vision response JSON: {}", e)))?;

    let candidate = parsed
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|c| c.as_str())
        .ok_or_else(|| AppError::ApiError("No content in Google response".to_string()))?;

    Ok(candidate.to_string())
}

async fn call_ollama(
    endpoint: &str,
    model: &str,
    prompt: &str,
    image_b64: &str,
    _mime: &str,
) -> AppResult<String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "messages": [
            {
                "role": "user",
                "content": prompt,
                "images": [image_b64]
            }
        ]
    });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::ApiError(format!("Ollama vision request failed: {}", e)))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| AppError::ApiError(format!("Failed to read response: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::ApiError(format!(
            "Ollama vision error ({}): {}",
            status.as_u16(),
            text
        )));
    }

    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| AppError::ApiError(format!("Invalid Ollama response JSON: {}", e)))?;

    parsed
        .pointer("/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::ApiError("No content in Ollama response".to_string()))
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Parse the model's free-text response into a structured [`ImageAnalysis`].
///
/// The model is instructed to return JSON, but we are defensive: we strip
/// markdown fences, locate the first JSON object, and fall back to a plain
/// description if structured parsing fails.
pub fn parse_image_analysis(raw: &str) -> AppResult<ImageAnalysis> {
    let cleaned = strip_markdown_fences(raw);

    let Some(json_str) = extract_json_object(&cleaned) else {
        // No JSON object found — treat the whole response as a description.
        return Ok(ImageAnalysis {
            description: cleaned.trim().to_string(),
            key_terms: Vec::new(),
            extracted_facts: Vec::new(),
            is_diagram: false,
        });
    };

    let value: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| AppError::ApiError(format!("Failed to parse vision JSON: {}", e)))?;

    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let key_terms: Vec<String> = value
        .get("key_terms")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let extracted_facts: Vec<String> = value
        .get("extracted_facts")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let is_diagram = value
        .get("is_diagram")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // If the model returned nothing useful, fall back to the raw text.
    if description.trim().is_empty() && key_terms.is_empty() && extracted_facts.is_empty() {
        return Ok(ImageAnalysis {
            description: cleaned.trim().to_string(),
            key_terms: Vec::new(),
            extracted_facts: Vec::new(),
            is_diagram: false,
        });
    }

    Ok(ImageAnalysis {
        description,
        key_terms,
        extracted_facts,
        is_diagram,
    })
}

/// Remove markdown code fences (```json ... ```) from a string.
fn strip_markdown_fences(s: &str) -> String {
    let mut out = s.to_string();
    for fence in ["```json", "```JSON", "```"] {
        while let Some(start) = out.find(fence) {
            let after = &out[start + fence.len()..];
            if let Some(end) = after.find("```") {
                let inner = &after[..end];
                out = format!("{}{}", &out[..start], inner);
            } else {
                break;
            }
        }
    }
    out
}

/// Find the first balanced JSON object in a string, or None.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    let bytes = s.as_bytes();

    for i in start..bytes.len() {
        let b = bytes[i];
        match b {
            b'\\' if in_string => escape = true,
            b'"' if in_string => {
                if escape {
                    escape = false;
                } else {
                    in_string = false;
                }
            }
            b'"' => in_string = true,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
        // Reset escape flag at end of each char unless it was a backslash
        if b != b'\\' {
            escape = false;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_image_analysis_valid_json() {
        let raw = r#"{"description": "A flowchart of the water cycle", "key_terms": ["evaporation", "condensation"], "extracted_facts": ["Evaporation moves water vapor upward"], "is_diagram": true}"#;
        let analysis = parse_image_analysis(raw).unwrap();
        assert_eq!(analysis.description, "A flowchart of the water cycle");
        assert_eq!(analysis.key_terms.len(), 2);
        assert!(analysis.is_diagram);
        assert_eq!(analysis.extracted_facts.len(), 1);
    }

    #[test]
    fn test_parse_image_analysis_markdown_fenced() {
        let raw = "Here is the analysis:\n```json\n{\"description\": \"A photo\", \"key_terms\": [], \"extracted_facts\": [], \"is_diagram\": false}\n```";
        let analysis = parse_image_analysis(raw).unwrap();
        assert_eq!(analysis.description, "A photo");
        assert!(!analysis.is_diagram);
    }

    #[test]
    fn test_parse_image_analysis_plain_text_fallback() {
        let raw = "This image shows a diagram of the solar system with planets orbiting the sun.";
        let analysis = parse_image_analysis(raw).unwrap();
        assert!(analysis.description.contains("solar system"));
        assert!(analysis.key_terms.is_empty());
    }

    #[test]
    fn test_parse_image_analysis_json_with_surrounding_text() {
        let raw = "Sure! {\"description\": \"X\", \"key_terms\": [\"a\"], \"extracted_facts\": [\"f\"], \"is_diagram\": true} hope that helps.";
        let analysis = parse_image_analysis(raw).unwrap();
        assert_eq!(analysis.description, "X");
        assert_eq!(analysis.key_terms, vec!["a".to_string()]);
        assert!(analysis.is_diagram);
    }

    #[test]
    fn test_strip_markdown_fences() {
        let input = "```json\n{\"a\":1}\n```";
        assert!(strip_markdown_fences(input).contains("{\"a\":1}"));
    }

    #[test]
    fn test_extract_json_object_nested() {
        let s = "text {\"a\": {\"b\": \"c\"}, \"d\": [1,2]} tail";
        let extracted = extract_json_object(s).unwrap();
        assert_eq!(extracted, "{\"a\": {\"b\": \"c\"}, \"d\": [1,2]}");
    }

    #[test]
    fn test_extract_json_object_with_braces_in_string() {
        let s = "{\"desc\": \"use { and } carefully\", \"n\": 1}";
        let extracted = extract_json_object(s).unwrap();
        assert_eq!(extracted, "{\"desc\": \"use { and } carefully\", \"n\": 1}");
    }
}
