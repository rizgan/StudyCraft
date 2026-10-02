use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Typed language-learning payload for a mnemonic.
///
/// These fields are deliberately kept separate from the mnemonic text itself:
/// the target expression is source linguistic data (never rewritten by the
/// model), while the mnemonic is a recall aid built *around* it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LanguageMnemonic {
    /// The exact target word/expression in the target language (verbatim,
    /// Unicode/RTL preserved).
    pub target_expression: String,
    /// The meaning in the learner's language.
    pub meaning: String,
    /// A pronunciation aid (transliteration, IPA, or phonetic spelling).
    pub transliteration: String,
}

/// A stored mnemonic attached to a single flashcard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardMnemonic {
    /// Database id (uuid string).
    pub id: String,
    pub card_id: i64,
    /// Whether the model judged a mnemonic to actually help recall for this card.
    pub is_suitable: bool,
    /// Short explanation when `is_suitable` is false (shown in the UI).
    pub unsuitable_reason: Option<String>,
    /// The mnemonic text (empty when unsuitable).
    pub mnemonic_text: String,
    /// Structured language-learning fields, when the card is a language pair.
    pub language: Option<LanguageMnemonic>,
    /// The structured prompt used to generate the mnemonic image (if any).
    pub image_prompt: Option<String>,
    /// Path of the attached image file, if one has been generated.
    pub image_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Raw JSON shape returned by the mnemonic generator agent.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MnemonicGeneratorResponse {
    pub is_suitable: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub mnemonic: String,
    #[serde(default)]
    pub image_prompt: Option<String>,
    #[serde(default)]
    pub language: Option<LanguageMnemonic>,
}

/// Parse the raw agent response into a mnemonic.
///
/// Rules:
/// - `is_suitable == false` is a *valid* result: no mnemonic text is stored,
///   only the reason.
/// - `mnemonic` is required only when suitable.
/// - All Unicode/RTL text is preserved verbatim (serde handles UTF-8).
pub fn parse_mnemonic_response(raw: &str) -> Result<MnemonicGeneratorResponse, String> {
    let cleaned = clean_json(raw);
    let parsed: MnemonicGeneratorResponse = serde_json::from_str(&cleaned)
        .map_err(|e| format!("Failed to parse mnemonic JSON: {}", e))?;

    if parsed.is_suitable && parsed.mnemonic.trim().is_empty() {
        return Err("Mnemonic response marked suitable but contains no mnemonic text".to_string());
    }
    Ok(parsed)
}

/// Strip markdown fences and surrounding prose so `serde_json` can parse the
/// first JSON object in the response.
fn clean_json(raw: &str) -> String {
    let trimmed = raw.trim();
    let start = match trimmed.find('{') {
        Some(i) => i,
        None => return trimmed.to_string(),
    };
    let end = match trimmed.rfind('}') {
        Some(i) => i,
        None => return trimmed.to_string(),
    };
    trimmed[start..=end].to_string()
}

/// Read a mnemonic image from disk (mirrors the existing
/// `get_cover_image_raw` pattern used for study-material covers).
pub fn read_mnemonic_image(path: &str) -> Result<Vec<u8>, String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(format!("Mnemonic image file not found: {}", path));
    }
    fs::read(p).map_err(|e| format!("Failed to read mnemonic image: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_suitable() {
        let raw = r#"
{
  "is_suitable": true,
  "mnemonic": "A **river** of **gold** — the Nile feeds Egypt",
  "image_prompt": "A golden river flowing through a desert, vivid illustration",
  "language": null
}"#;
        let parsed = parse_mnemonic_response(raw).unwrap();
        assert!(parsed.is_suitable);
        assert!(parsed.mnemonic.contains("river"));
        assert!(parsed.image_prompt.is_some());
        assert!(parsed.language.is_none());
    }

    #[test]
    fn test_parse_is_suitable_false_is_valid() {
        let raw = r#"{
  "is_suitable": false,
  "reason": "The fact is a single numeric value; a mnemonic would add no recall value.",
  "mnemonic": ""
}"#;
        let parsed = parse_mnemonic_response(raw).unwrap();
        assert!(!parsed.is_suitable);
        assert!(parsed.mnemonic.is_empty());
        assert!(parsed.reason.as_deref().unwrap().contains("numeric"));
    }

    #[test]
    fn test_parse_malformed_is_error() {
        let raw = "I cannot generate a mnemonic.";
        assert!(parse_mnemonic_response(raw).is_err());

        let raw = r#"{"is_suitable": true, "mnemonic": "x"#; // truncated JSON
        assert!(parse_mnemonic_response(raw).is_err());
    }

    #[test]
    fn test_parse_suitable_but_empty_mnemonic_is_error() {
        let raw = r#"{"is_suitable": true, "mnemonic": "   "}"#;
        assert!(parse_mnemonic_response(raw).is_err());
    }

    #[test]
    fn test_parse_unicode_and_rtl_preserved() {
        let raw = r#"{
  "is_suitable": true,
  "mnemonic": "Imagine a cat (قطة) drinking tea while reciting «ما معنى كتاب؟»",
  "language": {
    "target_expression": "قطة",
    "meaning": "cat",
    "transliteration": "qiṭṭa"
  }
}"#;
        let parsed = parse_mnemonic_response(raw).unwrap();
        let lang = parsed.language.expect("language payload");
        assert_eq!(lang.target_expression, "قطة");
        assert_eq!(lang.meaning, "cat");
        assert_eq!(lang.transliteration, "qiṭṭa");
        assert!(parsed.mnemonic.contains("ما معنى كتاب؟"));
        assert!(parsed.mnemonic.contains("«"));
    }

    #[test]
    fn test_parse_language_fields_distinct() {
        let raw = r#"{
  "is_suitable": true,
  "mnemonic": "sunflower = soleil + flower",
  "language": {
    "target_expression": "tournesol",
    "meaning": "sunflower",
    "transliteration": "toor-no-sohl"
  }
}"#;
        let parsed = parse_mnemonic_response(raw).unwrap();
        let lang = parsed.language.unwrap();
        // The mnemonic must not overwrite the source linguistic data.
        assert_eq!(lang.target_expression, "tournesol");
        assert_eq!(lang.meaning, "sunflower");
        assert_ne!(lang.target_expression, lang.meaning);
    }

    #[test]
    fn test_parse_markdown_fenced_json() {
        let raw = "```json\n{\"is_suitable\": false, \"reason\": \"too trivial\", \"mnemonic\": \"\"}\n```";
        let parsed = parse_mnemonic_response(raw).unwrap();
        assert!(!parsed.is_suitable);
    }

    #[test]
    fn test_read_mnemonic_image_missing_file() {
        let err = read_mnemonic_image("/definitely/not/a/real/file.png");
        assert!(err.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_read_mnemonic_image_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mnemo_img_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("img.png");
        fs::write(&path, [0u8, 1, 2, 3]).unwrap();
        let bytes = read_mnemonic_image(path.to_str().unwrap()).unwrap();
        assert_eq!(bytes, vec![0, 1, 2, 3]);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn test_language_mnemonic_serde_roundtrip() {
        let lm = LanguageMnemonic {
            target_expression: "日本語".into(),
            meaning: "Japanese language".into(),
            transliteration: "にほんご".into(),
        };
        let json = serde_json::to_string(&lm).unwrap();
        let back: LanguageMnemonic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lm);
    }
}
