//! Structured image-prompt construction for mnemonic images (feature 33).
//!
//! The LLM already produces a scene-level `image_prompt` for the mnemonic.
//! This module wraps that in a strict, deterministic template that:
//! - keeps the mnemonic's visual scene intact,
//! - explicitly instructs the model to render NO text/words/letters in the
//!   image (image models render text badly and stray text hurts usability),
//! - keeps language-learning cards visually consistent (target expression and
//!   meaning are distinct concepts, but neither is requested as in-image text).

use crate::materials::flashcard_mnemonic::LanguageMnemonic;

/// Build the final image-generation prompt from the mnemonic's scene prompt.
///
/// `scene_prompt` is the LLM-generated illustration prompt; it must not be
/// empty or the result falls back to a generic but safe prompt.
pub fn build_image_prompt(
    scene_prompt: Option<&str>,
    language: Option<&LanguageMnemonic>,
) -> String {
    let scene = scene_prompt
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("a simple, vivid symbolic scene that aids memory");

    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "Create a clean, vivid, memorable illustration of the following scene: {}",
        scene
    ));

    if let Some(lang) = language {
        // Keep the linguistic concepts present in the *scene* (as objects/ideas)
        // without asking the model to print the words.
        parts.push(
            "This scene supports remembering a word in a language being learned. Convey the meaning through objects, actions and symbols rather than through any written words."
                .to_string(),
        );
        let _ = lang; // target_expression/meaning are semantic context, not rendered
    }

    parts.push(
        "STRICTLY DO NOT render any text, letters, words, labels, captions or writing anywhere in the image. The image must be purely pictorial."
            .to_string(),
    );
    parts.push(
        "Style: clear, bright, slightly stylized memory-palace illustration, single focused subject, uncluttered composition."
            .to_string(),
    );

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_includes_scene() {
        let prompt = build_image_prompt(Some("a golden river flowing through a desert"), None);
        assert!(prompt.contains("golden river flowing through a desert"));
    }

    #[test]
    fn test_prompt_never_requests_text_in_image() {
        let prompt = build_image_prompt(
            Some("a cat reading a book"),
            Some(&LanguageMnemonic {
                target_expression: "قطة".into(),
                meaning: "cat".into(),
                transliteration: "qiṭṭa".into(),
            }),
        );
        // The instruction to avoid text is always present.
        assert!(prompt.to_lowercase().contains("do not render any text"));
        // The raw target expression must NOT be asked to be drawn as text.
        assert!(!prompt.contains("قطة"));
    }

    #[test]
    fn test_prompt_fallback_for_empty_scene() {
        let prompt = build_image_prompt(Some("   "), None);
        assert!(prompt.contains("symbolic scene"));
    }

    #[test]
    fn test_prompt_handles_none_scene() {
        let prompt = build_image_prompt(None, None);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("STRICTLY DO NOT"));
    }
}
