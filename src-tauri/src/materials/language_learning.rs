use serde::{Deserialize, Serialize};

/// Learning direction of a language-learning flashcard.
///
/// This is an explicitly persisted domain state. The direction must never be
/// re-inferred from the string ordering of front/back when reading a card: it
/// is stored with the card's language metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LearningDirection {
    /// Recognition: the card prompts with the target expression (in the
    /// learning language) and requires its meaning.
    /// Example: `كتاب` → `book`.
    Recognition,
    /// Production: the card prompts with the meaning (in the learner's
    /// language) and requires the target expression.
    /// Example: `book` → `كتاب`.
    Production,
}

impl Default for LearningDirection {
    fn default() -> Self {
        LearningDirection::Recognition
    }
}

impl LearningDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            LearningDirection::Recognition => "recognition",
            LearningDirection::Production => "production",
        }
    }

    /// Parse a direction value as produced by the LLM. Case-insensitive.
    /// Unknown values are rejected so a card can never be stored with an
    /// ambiguous direction.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "recognition" => Some(LearningDirection::Recognition),
            "production" => Some(LearningDirection::Production),
            _ => None,
        }
    }
}

/// Pedagogical task of a language-learning flashcard.
///
/// Translation tests the lexical mapping of the expression.
/// UnderstandingInContext tests whether the expression is understood inside
/// a sentence, not reproduced from a dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardTask {
    Translation,
    UnderstandingInContext,
}

impl Default for CardTask {
    fn default() -> Self {
        CardTask::Translation
    }
}

impl CardTask {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardTask::Translation => "translation",
            CardTask::UnderstandingInContext => "understanding_in_context",
        }
    }

    /// Parse a task value as produced by the LLM. Case-insensitive.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "translation" => Some(CardTask::Translation),
            "understanding_in_context" => Some(CardTask::UnderstandingInContext),
            _ => None,
        }
    }
}

/// Creation-time choice of which directions of a vocabulary item to produce
/// cards for. Each produced card itself stores a concrete
/// [`LearningDirection`]; "both" is only a generation option, never a
/// per-card state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardDirectionChoice {
    Recognition,
    Production,
    Both,
}

impl CardDirectionChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardDirectionChoice::Recognition => "recognition",
            CardDirectionChoice::Production => "production",
            CardDirectionChoice::Both => "both",
        }
    }

    /// Expand the choice into the concrete per-card directions it generates.
    pub fn expand(self) -> Vec<LearningDirection> {
        match self {
            CardDirectionChoice::Recognition => vec![LearningDirection::Recognition],
            CardDirectionChoice::Production => vec![LearningDirection::Production],
            CardDirectionChoice::Both => vec![
                LearningDirection::Recognition,
                LearningDirection::Production,
            ],
        }
    }
}

/// Typed vocabulary item for language learning.
///
/// The required fields are the ones every language-learning card needs to
/// function; the rest are genuinely optional and must be `Option`, not empty
/// strings. All text is stored verbatim: arbitrary Unicode and right-to-left
/// scripts are preserved exactly as produced by the model, with no
/// normalization (in particular Arabic diacritics are neither added nor
/// removed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VocabularyItem {
    /// The target word/expression in the learning language (verbatim).
    pub target_expression: String,
    /// The meaning in the learner's (base) language.
    pub meaning: String,
    /// Pronunciation aid: transliteration, IPA, or phonetic spelling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pronunciation: Option<String>,
    /// A natural example sentence using the target expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// Translation of the example sentence into the base language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_translation: Option<String>,
    /// Usage context notes (register, typical collocations, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Free-form grammatical information appropriate to the language
    /// (part of speech, gender, plural, case, root, aspect, ...).
    /// Deliberately unstructured: different languages expose different
    /// properties, and no language is required to have any of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grammatical_information: Option<String>,
}

impl VocabularyItem {
    /// True when the example sentence contains the target expression
    /// verbatim, as required for a valid contextual example.
    pub fn example_uses_target_expression(&self) -> bool {
        self.example
            .as_ref()
            .is_some_and(|example| example.contains(&self.target_expression))
    }
}

/// Per-card language-learning metadata.
///
/// Lives in a 1:1 relation with a flashcard (see the `flashcard_language`
/// table). Vocabulary is the canonical language data of the card; the
/// mnemonic (if one is ever generated) is an optional aid attached to the
/// same card, never the other way around. A language card must work with no
/// mnemonic at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashcardLanguage {
    /// Id of the flashcard this metadata belongs to.
    pub card_id: i64,
    pub direction: LearningDirection,
    pub task: CardTask,
    pub item: VocabularyItem,
}

/// Deck-level language-learning identification.
///
/// Persisted on the review material so a language-learning deck stays one
/// after save, restart, reopen, edit and review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageLearningSettings {
    /// The language the deck teaches (target language).
    pub learning_language: String,
    /// The learner's native language; used for meanings and prompts.
    pub base_language: String,
}

/// Deterministic reverse-direction transformation: build the canonical
/// translation front/back pair of a vocabulary item for a given direction.
///
/// - Recognition: front = target expression, back = meaning.
/// - Production:  front = meaning, back = target expression.
///
/// Used as the fallback text source when the LLM returns a vocabulary item
/// without question/answer text, and as the canonical definition of the
/// reverse-direction card.
pub fn render_translation_card(
    item: &VocabularyItem,
    direction: LearningDirection,
) -> (String, String) {
    match direction {
        LearningDirection::Recognition => (item.target_expression.clone(), item.meaning.clone()),
        LearningDirection::Production => (item.meaning.clone(), item.target_expression.clone()),
    }
}

/// Semantic direction hint for a *named* language (e.g. "Arabic", "ar").
///
/// Used to present language labels in their natural writing direction.
/// Card and vocabulary *content* is rendered with `dir="auto"` in the UI,
/// which follows the actual characters and needs no language list.
pub fn is_rtl_language(language: &str) -> bool {
    matches!(
        language.trim().to_lowercase().as_str(),
        "arabic"
            | "ar"
            | "hebrew"
            | "he"
            | "persian"
            | "farsi"
            | "fa"
            | "urdu"
            | "ur"
            | "sindhi"
            | "sd"
            | "pashto"
            | "ps"
            | "yiddish"
            | "yi"
            | "divani"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recognition_enum_serde_roundtrip() {
        let direction = LearningDirection::Recognition;
        let json = serde_json::to_string(&direction).unwrap();
        assert_eq!(json, "\"recognition\"");
        let back: LearningDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LearningDirection::Recognition);
    }

    #[test]
    fn test_production_enum_serde_roundtrip() {
        let direction = LearningDirection::Production;
        let json = serde_json::to_string(&direction).unwrap();
        assert_eq!(json, "\"production\"");
        let back: LearningDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LearningDirection::Production);
    }

    #[test]
    fn test_card_task_serde_roundtrip() {
        let translation: CardTask = serde_json::from_str("\"translation\"").unwrap();
        assert_eq!(translation, CardTask::Translation);
        let in_context: CardTask = serde_json::from_str("\"understanding_in_context\"").unwrap();
        assert_eq!(in_context, CardTask::UnderstandingInContext);

        let json = serde_json::to_string(&CardTask::UnderstandingInContext).unwrap();
        assert_eq!(json, "\"understanding_in_context\"");
    }

    #[test]
    fn test_direction_choice_expand() {
        assert_eq!(
            CardDirectionChoice::Recognition.expand(),
            vec![LearningDirection::Recognition]
        );
        assert_eq!(
            CardDirectionChoice::Production.expand(),
            vec![LearningDirection::Production]
        );
        assert_eq!(
            CardDirectionChoice::Both.expand(),
            vec![
                LearningDirection::Recognition,
                LearningDirection::Production
            ]
        );
    }

    #[test]
    fn test_recognition_only_generation() {
        let item = VocabularyItem {
            target_expression: "كتاب".into(),
            meaning: "book".into(),
            pronunciation: None,
            example: None,
            example_translation: None,
            context: None,
            grammatical_information: None,
        };
        let cards: Vec<(String, String)> = CardDirectionChoice::Recognition
            .expand()
            .iter()
            .map(|d| render_translation_card(&item, *d))
            .collect();
        assert_eq!(cards, vec![("كتاب".to_string(), "book".to_string())]);
    }

    #[test]
    fn test_production_only_generation() {
        let item = VocabularyItem {
            target_expression: "كتاب".into(),
            meaning: "book".into(),
            pronunciation: None,
            example: None,
            example_translation: None,
            context: None,
            grammatical_information: None,
        };
        let cards: Vec<(String, String)> = CardDirectionChoice::Production
            .expand()
            .iter()
            .map(|d| render_translation_card(&item, *d))
            .collect();
        assert_eq!(cards, vec![("book".to_string(), "كتاب".to_string())]);
    }

    #[test]
    fn test_both_direction_generation() {
        let item = VocabularyItem {
            target_expression: "كتاب".into(),
            meaning: "book".into(),
            pronunciation: None,
            example: None,
            example_translation: None,
            context: None,
            grammatical_information: None,
        };
        let cards: Vec<(String, String)> = CardDirectionChoice::Both
            .expand()
            .iter()
            .map(|d| render_translation_card(&item, *d))
            .collect();
        assert_eq!(
            cards,
            vec![
                ("كتاب".to_string(), "book".to_string()),
                ("book".to_string(), "كتاب".to_string())
            ]
        );
    }

    #[test]
    fn test_reverse_card_transformation() {
        let item = VocabularyItem {
            target_expression: "قطة".into(),
            meaning: "cat".into(),
            pronunciation: Some("qiṭṭa".into()),
            example: Some("هذه قطة صغيرة.".into()),
            example_translation: Some("This is a small cat.".into()),
            context: None,
            grammatical_information: Some("noun, feminine".into()),
        };
        let (recognition_front, recognition_back) =
            render_translation_card(&item, LearningDirection::Recognition);
        let (production_front, production_back) =
            render_translation_card(&item, LearningDirection::Production);
        // Production is the exact reverse of recognition and vice versa.
        assert_eq!(production_front, recognition_back);
        assert_eq!(production_back, recognition_front);
    }

    #[test]
    fn test_optional_vocabulary_fields() {
        let minimal = VocabularyItem {
            target_expression: "كتاب".into(),
            meaning: "book".into(),
            pronunciation: None,
            example: None,
            example_translation: None,
            context: None,
            grammatical_information: None,
        };
        let json = serde_json::to_string(&minimal).unwrap();
        // Optional fields must not be serialized as empty strings.
        assert!(!json.contains("example"));
        assert!(!json.contains("pronunciation"));
        assert!(!json.contains("grammatical_information"));

        let back: VocabularyItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, minimal);
        assert_eq!(back.pronunciation, None);
        assert_eq!(back.example, None);

        let full = VocabularyItem {
            target_expression: "مدرسة".into(),
            meaning: "school".into(),
            pronunciation: Some("madraسة".into()),
            example: Some("المدرسة قريبة.".into()),
            example_translation: Some("The school is close.".into()),
            context: Some("formal register".into()),
            grammatical_information: Some("noun, feminine; root م-د-ر".into()),
        };
        let full_json = serde_json::to_string(&full).unwrap();
        let back_full: VocabularyItem = serde_json::from_str(&full_json).unwrap();
        assert_eq!(back_full, full);
    }

    #[test]
    fn test_unicode_vocabulary_serde_roundtrip() {
        let item = VocabularyItem {
            target_expression: "كِتَاب".into(),
            meaning: "book".into(),
            pronunciation: Some("kitāb".into()),
            example: Some("هذا كتاب جديد.".into()),
            example_translation: Some("This is a new book.".into()),
            context: Some("used in everyday conversation".into()),
            grammatical_information: Some("noun, masculine; root ك-ت-ب".into()),
        };
        let json = serde_json::to_string(&item).unwrap();
        let back: VocabularyItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, item);
        assert_eq!(back.target_expression, "كِتَاب");
        assert_eq!(
            back.grammatical_information.as_deref(),
            Some("noun, masculine; root ك-ت-ب")
        );
    }

    #[test]
    fn test_translation_and_in_context_are_distinct_tasks() {
        let raw = r#"{
  "flashcards": [
    {
      "question": "What does 'كتاب' mean?",
      "answer": "book",
      "vocabulary": {
        "target_expression": "كتاب",
        "meaning": "book"
      },
      "direction": "recognition",
      "task": "translation"
    },
    {
      "question": "What does 'كتاب' mean in: 'هذا كتاب جديد.'?",
      "answer": "The sentence means: 'This is a new book.'",
      "vocabulary": {
        "target_expression": "كتاب",
        "meaning": "book",
        "example": "هذا كتاب جديد.",
        "example_translation": "This is a new book."
      },
      "direction": "recognition",
      "task": "understanding_in_context"
    }
  ]
}"#;
        #[derive(Deserialize)]
        struct Batch {
            flashcards: Vec<serde_json::Value>,
        }
        let batch: Batch = serde_json::from_str(raw).unwrap();
        let first: CardTask = serde_json::from_value(batch.flashcards[0]["task"].clone()).unwrap();
        let second: CardTask = serde_json::from_value(batch.flashcards[1]["task"].clone()).unwrap();
        assert_eq!(first, CardTask::Translation);
        assert_eq!(second, CardTask::UnderstandingInContext);
        assert_ne!(first, second);
        // The in-context task must carry the sentence it is about.
        assert_eq!(
            batch.flashcards[1]["vocabulary"]["example"]
                .as_str()
                .unwrap(),
            "هذا كتاب جديد."
        );
        assert!(batch.flashcards[1]["vocabulary"]
            .get("example_translation")
            .is_some());
    }

    #[test]
    fn test_direction_and_task_parse() {
        assert_eq!(
            LearningDirection::parse("Recognition"),
            Some(LearningDirection::Recognition)
        );
        assert_eq!(
            LearningDirection::parse("  PRODUCTION "),
            Some(LearningDirection::Production)
        );
        assert_eq!(LearningDirection::parse("recog"), None);
        assert_eq!(LearningDirection::parse(""), None);

        assert_eq!(CardTask::parse("translation"), Some(CardTask::Translation));
        assert_eq!(
            CardTask::parse("UNDERSTANDING_IN_CONTEXT"),
            Some(CardTask::UnderstandingInContext)
        );
        assert_eq!(CardTask::parse("understandingincontext"), None);
        assert_eq!(CardTask::parse("translation-ish"), None);
    }

    #[test]
    fn test_example_uses_target_expression() {
        let good = VocabularyItem {
            target_expression: "كتاب".into(),
            meaning: "book".into(),
            pronunciation: None,
            example: Some("هذا كتاب جديد.".into()),
            example_translation: None,
            context: None,
            grammatical_information: None,
        };
        assert!(good.example_uses_target_expression());

        let mut bad = VocabularyItem {
            example: Some("جديد.".into()),
            ..VocabularyItem::default()
        };
        bad.target_expression = "كتاب".into();
        bad.meaning = "book".into();
        assert!(!bad.example_uses_target_expression());

        let none: VocabularyItem = Default::default();
        assert!(!none.example_uses_target_expression());
    }

    #[test]
    fn test_rtl_language_detection() {
        assert!(is_rtl_language("Arabic"));
        assert!(is_rtl_language("ar"));
        assert!(is_rtl_language("Hebrew"));
        assert!(is_rtl_language("Persian"));
        assert!(is_rtl_language("Urdu"));
        assert!(!is_rtl_language("English"));
        assert!(!is_rtl_language("Japanese"));
        assert!(!is_rtl_language("Chinese"));
        assert!(!is_rtl_language(""));
    }
}
