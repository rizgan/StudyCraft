use crate::constants;
use flyllm::TaskDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AgentType {
    ConceptExtractor,
    FlashcardContentCreator,
    LanguageFlashcardContentCreator,
    TestContentCreator,
    AssessmentContentCreator,
    AssessmentAnswerEvaluator,
    ExplanationAgent,
    SearchAgent,
    MnemonicGenerator,
}

impl AgentType {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ConceptExtractor,
            Self::FlashcardContentCreator,
            Self::LanguageFlashcardContentCreator,
            Self::TestContentCreator,
            Self::AssessmentContentCreator,
            Self::AssessmentAnswerEvaluator,
            Self::ExplanationAgent,
            Self::SearchAgent,
            Self::MnemonicGenerator,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConceptExtractor => "ConceptExtractor",
            Self::FlashcardContentCreator => "FlashcardContentCreator",
            Self::LanguageFlashcardContentCreator => "LanguageFlashcardContentCreator",
            Self::TestContentCreator => "TestContentCreator",
            Self::AssessmentContentCreator => "AssessmentContentCreator",
            Self::AssessmentAnswerEvaluator => "AssessmentAnswerEvaluator",
            Self::ExplanationAgent => "ExplanationAgent",
            Self::SearchAgent => "SearchAgent",
            Self::MnemonicGenerator => "MnemonicGenerator",
        }
    }

    pub fn get_task_definition(&self) -> TaskDefinition {
        match self {
            Self::ConceptExtractor => TaskDefinition::new("Concept Extraction")
                .with_max_tokens(2000)
                .with_temperature(0.3),

            Self::FlashcardContentCreator => TaskDefinition::new("Flashcard Content Creation")
                .with_max_tokens(3500)
                .with_temperature(0.25),

            Self::LanguageFlashcardContentCreator => {
                TaskDefinition::new("Language Flashcard Content Creation")
                    .with_max_tokens(4000)
                    .with_temperature(0.25)
            }

            Self::TestContentCreator => TaskDefinition::new("Test Content Creation")
                .with_max_tokens(4000)
                .with_temperature(0.25),

            Self::AssessmentContentCreator => TaskDefinition::new("Assessment Content Creation")
                .with_max_tokens(4000)
                .with_temperature(0.3),

            Self::AssessmentAnswerEvaluator => TaskDefinition::new("Assessment Answer Evaluation")
                .with_max_tokens(1024)
                .with_temperature(0.1),

            Self::ExplanationAgent => TaskDefinition::new("Answer Explanation")
                .with_max_tokens(1500)
                .with_temperature(0.2),

            Self::SearchAgent => TaskDefinition::new("Knowledge Search")
                .with_max_tokens(2500)
                .with_temperature(0.3),

            Self::MnemonicGenerator => TaskDefinition::new("Mnemonic Generation")
                .with_max_tokens(1500)
                .with_temperature(0.7),
        }
    }

    pub fn get_prompt_template(&self) -> &'static str {
        match self {
            Self::ConceptExtractor => constants::CONCEPT_EXTRACTOR_PROMPT,
            Self::FlashcardContentCreator => constants::FLASHCARD_CONTENT_CREATOR_PROMPT,
            Self::LanguageFlashcardContentCreator => {
                constants::LANGUAGE_FLASHCARD_CONTENT_CREATOR_PROMPT
            }
            Self::TestContentCreator => constants::TEST_CONTENT_CREATOR_PROMPT,
            Self::AssessmentContentCreator => constants::ASSESSMENT_CONTENT_CREATOR_PROMPT,
            Self::AssessmentAnswerEvaluator => constants::ASSESSMENT_ANSWER_EVALUATOR_PROMPT,
            Self::ExplanationAgent => constants::EXPLANATION_AGENT_PROMPT,
            Self::SearchAgent => constants::SEARCH_AGENT_PROMPT,
            Self::MnemonicGenerator => constants::MNEMONIC_GENERATOR_PROMPT,
        }
    }
}

impl From<String> for AgentType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "ConceptExtractor" => Self::ConceptExtractor,
            "FlashcardContentCreator" => Self::FlashcardContentCreator,
            "LanguageFlashcardContentCreator" => Self::LanguageFlashcardContentCreator,
            "TestContentCreator" => Self::TestContentCreator,
            "AssessmentContentCreator" => Self::AssessmentContentCreator,
            "AssessmentAnswerEvaluator" => Self::AssessmentAnswerEvaluator,
            "ExplanationAgent" => Self::ExplanationAgent,
            "SearchAgent" => Self::SearchAgent,
            "MnemonicGenerator" => Self::MnemonicGenerator,
            _ => Self::ConceptExtractor, // will not be the case, but use concept one as failcase
        }
    }
}
