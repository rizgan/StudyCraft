use crate::config::api_config::LlmUserConfig;
use crate::config::app_settings::AgentSettings;
use crate::config::keystore::ApiKeystore;
use crate::constants;
use crate::errors::{AppError, AppResult};
use crate::materials::assessment::{AssessmentDifficulty, AssessmentGrade, NewAssessmentQuestion};
use crate::materials::flashcard_mnemonic::LanguageMnemonic;
use crate::materials::language_learning::{
    render_translation_card, CardDirectionChoice, CardTask, LearningDirection, VocabularyItem,
};
use crate::services::db_service::{
    FlashcardDeckOperations, StudyMaterialOperations, TestOperations,
};
use crate::services::llm_service::agents::AgentType;
use crate::services::vector_service::vector_service::RelevantChunk;
use crate::state::AppState;
use flyllm::{GenerationRequest, LlmManager, ProviderType, TaskDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConceptExtraction {
    pub concept: String,
    pub context: String,
    pub chunk_id: usize, // which chunk this came from
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConceptList {
    pub concepts: Vec<ConceptExtraction>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FlashcardBatch {
    pub flashcards: Vec<GeneratedCard>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeneratedCard {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MaterialGenerationRequest {
    pub material_id: String,
    pub cards_to_generate: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FlashcardGenerationRequest {
    pub materials: Vec<MaterialGenerationRequest>,
    pub language: Option<String>,
    pub concept: Option<String>,
}

/// Request for language-learning flashcard generation (language-learning
/// mode). The general [`FlashcardGenerationRequest`] is left unchanged for
/// general decks.
#[derive(Debug, Serialize, Deserialize)]
pub struct LanguageFlashcardGenerationRequest {
    pub materials: Vec<MaterialGenerationRequest>,
    pub concept: Option<String>,
    /// The language the deck teaches (target language).
    pub learning_language: String,
    /// The learner's native language.
    pub base_language: String,
    /// Which card direction(s) to generate for each vocabulary item.
    pub direction: CardDirectionChoice,
}

/// A single language-learning flashcard: the standard question/answer plus
/// structured language data. The structured fields are authoritative for the
/// card's language semantics; front/back text must never be parsed to recover
/// them.
///
/// `direction`/`task` are `Option` on the wire so a single missing key does
/// not reject the whole batch; missing values default to Recognition /
/// Translation. `vocabulary` is required for a card to be a language card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageFlashcard {
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub vocabulary: Option<VocabularyItem>,
    #[serde(default)]
    pub direction: Option<LearningDirection>,
    #[serde(default)]
    pub task: Option<CardTask>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LanguageFlashcardBatch {
    pub flashcards: Vec<LanguageFlashcard>,
}

/// Parsed, structured output of the mnemonic generator agent.
#[derive(Debug, Clone, Default)]
pub struct MnemonicGenerationOutput {
    pub is_suitable: bool,
    pub reason: Option<String>,
    pub mnemonic: String,
    pub image_prompt: Option<String>,
    pub language: Option<LanguageMnemonic>,
}

impl Serialize for MnemonicGenerationOutput {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("MnemonicGenerationOutput", 5)?;
        st.serialize_field("is_suitable", &self.is_suitable)?;
        st.serialize_field("reason", &self.reason)?;
        st.serialize_field("mnemonic", &self.mnemonic)?;
        st.serialize_field("image_prompt", &self.image_prompt)?;
        st.serialize_field("language", &self.language)?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for MnemonicGenerationOutput {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            #[serde(default)]
            is_suitable: bool,
            #[serde(default)]
            reason: Option<String>,
            #[serde(default)]
            mnemonic: String,
            #[serde(default)]
            image_prompt: Option<String>,
            #[serde(default)]
            language: Option<LanguageMnemonic>,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(MnemonicGenerationOutput {
            is_suitable: h.is_suitable,
            reason: h.reason,
            mnemonic: h.mnemonic,
            image_prompt: h.image_prompt,
            language: h.language,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub total_concepts_extracted: usize,
    pub total_flashcards_created: usize,
    pub concepts_used: Vec<ConceptExtraction>,
    pub flashcards: Vec<GeneratedCard>,
    pub workflow_stats: WorkflowStats,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowStats {
    pub chunks_processed: usize,
    pub concept_extraction_requests: usize,
    pub flashcard_creation_requests: usize,
    pub concepts_per_chunk_avg: f32,
    pub flashcards_per_request_avg: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestQuestionBatch {
    pub test_questions: Vec<GeneratedTestQuestion>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestMaterialRequest {
    pub material_id: String,
    pub questions_to_generate: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestGenerationRequest {
    pub materials: Vec<TestMaterialRequest>,
    pub language: Option<String>,
    pub concept: Option<String>,
}

/// A single transfer question generated for a knowledge assessment.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeneratedAssessmentQuestion {
    pub prompt: String,
    pub concept: String,
    pub difficulty: String,
    /// Source-grounded reference content for grading the question's answer.
    pub expected_content: Option<String>,
    #[serde(default)]
    pub source_chunk: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssessmentQuestionBatch {
    pub questions: Vec<GeneratedAssessmentQuestion>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssessmentGenerationRequest {
    pub materials: Vec<MaterialGenerationRequest>,
    pub language: Option<String>,
    /// Describes WHY this generation run happens: initial assessment,
    /// reassessment of the same concepts, or remediation focused on weak
    /// concepts. The model uses it to tailor the questions.
    pub run_context: String,
}

/// Structured evaluation of one free-text assessment answer.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnswerEvaluationOutput {
    pub grade: String,
    pub rationale: String,
    #[serde(default)]
    pub misconception: Option<String>,
}

impl AnswerEvaluationOutput {
    /// Parses the grade, rejecting any value the evaluator is not allowed to
    /// produce. An unknown grade is an evaluation error, never a default.
    pub fn parsed_grade(&self) -> AppResult<AssessmentGrade> {
        AssessmentGrade::parse(&self.grade).ok_or_else(|| {
            AppError::LlmError(format!(
                "Answer evaluator returned unknown grade '{}'",
                self.grade
            ))
        })
    }
}

/// A deterministic, case-insensitive duplicate key for an assessment question
/// prompt. Whitespace is normalized and surrounding punctuation removed so that
/// cosmetic differences between an old and a regenerated prompt are still
/// detected.
pub fn normalize_question_prompt(prompt: &str) -> String {
    prompt
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::assessment::{AssessmentDifficulty, AssessmentGrade};

    #[test]
    fn normalize_question_prompt_is_case_and_punctuation_insensitive() {
        assert_eq!(
            normalize_question_prompt("What is the derivative of x^2?"),
            normalize_question_prompt("what is the derivative of x2")
        );
        assert_eq!(
            normalize_question_prompt("  A  B  "),
            normalize_question_prompt("ab")
        );
    }

    #[test]
    fn both_direction_choice_yields_two_distinct_normalized_cards() {
        let item = VocabularyItem {
            target_expression: "كتاب".to_string(),
            meaning: "book".to_string(),
            pronunciation: None,
            example: None,
            example_translation: None,
            context: None,
            grammatical_information: None,
        };

        let mut normalized = Vec::new();
        for direction in CardDirectionChoice::Both.expand() {
            let raw = LanguageFlashcard {
                question: String::new(),
                answer: String::new(),
                vocabulary: Some(item.clone()),
                direction: Some(direction),
                task: Some(CardTask::Translation),
            };
            normalized.push(
                LlmService::normalize_language_flashcard(raw)
                    .expect("direction card must normalize"),
            );
        }

        // Exactly two logical cards, one per direction.
        assert_eq!(normalized.len(), 2, "Both must yield exactly two cards");
        let recognition = normalized
            .iter()
            .find(|c| c.2 == LearningDirection::Recognition)
            .expect("recognition card missing");
        let production = normalized
            .iter()
            .find(|c| c.2 == LearningDirection::Production)
            .expect("production card missing");

        // The two cards carry different directions but the same vocabulary.
        assert_ne!(recognition.2, production.2);
        assert_eq!(recognition.4, item);
        assert_eq!(production.4, item);

        // Correct front/back per direction (deterministic rendering).
        assert_eq!(recognition.0, "كتاب");
        assert_eq!(recognition.1, "book");
        assert_eq!(production.0, "book");
        assert_eq!(production.1, "كتاب");

        // The batch dedup key includes the direction, so dedup can never
        // drop one direction of the same vocabulary item.
        let key = |card: &(String, String, LearningDirection, CardTask, VocabularyItem)| {
            format!("{}\u{0}{}", card.2.as_str(), card.4.target_expression)
        };
        assert_ne!(key(recognition), key(production));
    }

    #[test]
    fn understanding_in_context_fallback_is_context_bearing() {
        let item = VocabularyItem {
            target_expression: "كتاب".to_string(),
            meaning: "book".to_string(),
            pronunciation: None,
            example: Some("هذا كتاب جديد.".to_string()),
            example_translation: Some("This is a new book.".to_string()),
            context: None,
            grammatical_information: None,
        };

        // Same vocabulary, no LLM text: the in-context card must use the
        // example sentence and differ from the translation card.
        let in_context = LlmService::normalize_language_flashcard(LanguageFlashcard {
            question: String::new(),
            answer: String::new(),
            vocabulary: Some(item.clone()),
            direction: Some(LearningDirection::Recognition),
            task: Some(CardTask::UnderstandingInContext),
        })
        .expect("in-context card must normalize");
        let translation = LlmService::normalize_language_flashcard(LanguageFlashcard {
            question: String::new(),
            answer: String::new(),
            vocabulary: Some(item.clone()),
            direction: Some(LearningDirection::Recognition),
            task: Some(CardTask::Translation),
        })
        .expect("translation card must normalize");

        assert_eq!(in_context.3, CardTask::UnderstandingInContext);
        assert_eq!(translation.3, CardTask::Translation);
        // The in-context question embeds the example and the target.
        assert!(in_context.0.contains("هذا كتاب جديد."));
        assert!(in_context.0.contains("كتاب"));
        // The translation question is the bare target and must not.
        assert_eq!(translation.0, "كتاب");
        assert!(!translation.0.contains("هذا كتاب جديد."));
        // The answers are legitimately distinct here (translation falls
        // back to the meaning; in-context uses the example translation).
        assert_eq!(in_context.1, "This is a new book.");
        assert_eq!(translation.1, "book");

        // Without an example, the fallback still embeds the target and
        // stays distinct from the translation question.
        let bare = VocabularyItem {
            example: None,
            example_translation: None,
            ..item.clone()
        };
        let in_context_bare = LlmService::normalize_language_flashcard(LanguageFlashcard {
            question: String::new(),
            answer: String::new(),
            vocabulary: Some(bare),
            direction: Some(LearningDirection::Recognition),
            task: Some(CardTask::UnderstandingInContext),
        })
        .expect("in-context card without example must normalize");
        assert_ne!(in_context_bare.0, translation.0);
    }

    #[test]
    fn clean_json_preserves_content_characters() {
        let raw = r#"{"rationale":"uses * and / and \"quotes\""}"#;
        let cleaned = clean_json_content(raw);
        assert!(cleaned.contains('*'), "asterisk was destroyed: {}", cleaned);
        assert!(cleaned.contains('/'), "slash was destroyed: {}", cleaned);
        let parsed: serde_json::Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(
            parsed["rationale"].as_str().unwrap(),
            "uses * and / and \"quotes\""
        );
    }

    #[test]
    fn clean_json_strips_code_fences() {
        let fenced = "```json\n{\"grade\":\"correct\"}\n```";
        let cleaned = clean_json_content(fenced);
        assert!(cleaned.starts_with('{'));
        let parsed: serde_json::Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(parsed["grade"].as_str(), Some("correct"));
    }

    #[test]
    fn grade_parse_rejects_unknown() {
        assert_eq!(
            AssessmentGrade::parse("correct").unwrap(),
            AssessmentGrade::Correct
        );
        assert_eq!(
            AssessmentGrade::parse("partially_correct").unwrap(),
            AssessmentGrade::PartiallyCorrect
        );
        assert_eq!(
            AssessmentGrade::parse("incorrect").unwrap(),
            AssessmentGrade::Incorrect
        );
        assert_eq!(
            AssessmentGrade::parse("insufficient").unwrap(),
            AssessmentGrade::Insufficient
        );
        assert_eq!(AssessmentGrade::parse("maybe"), None);
        assert_eq!(AssessmentGrade::parse(""), None);
    }

    #[test]
    fn parsed_grade_errors_on_unknown() {
        let out = AnswerEvaluationOutput {
            grade: "weird".to_string(),
            rationale: "r".to_string(),
            misconception: None,
        };
        assert!(out.parsed_grade().is_err());
        let ok = AnswerEvaluationOutput {
            grade: "correct".to_string(),
            rationale: "r".to_string(),
            misconception: None,
        };
        assert_eq!(ok.parsed_grade().unwrap(), AssessmentGrade::Correct);
    }

    #[test]
    fn difficulty_parse_rejects_unknown() {
        assert_eq!(
            AssessmentDifficulty::parse("recall").unwrap(),
            AssessmentDifficulty::Recall
        );
        assert_eq!(
            AssessmentDifficulty::parse("understanding").unwrap(),
            AssessmentDifficulty::Understanding
        );
        assert_eq!(
            AssessmentDifficulty::parse("application").unwrap(),
            AssessmentDifficulty::Application
        );
        assert_eq!(AssessmentDifficulty::parse("easy"), None);
    }

    #[test]
    fn resolve_expected_content_prefers_cited_chunk() {
        let batch = vec![ConceptExtraction {
            concept: "A".to_string(),
            context: "context A".to_string(),
            chunk_id: 1,
        }];
        let out = resolve_expected_content(Some("cited chunk"), "A", &batch);
        assert_eq!(out, "cited chunk");
        let out2 = resolve_expected_content(None, "A", &batch);
        assert_eq!(out2, "context A");
        let out3 = resolve_expected_content(Some(""), "B", &batch);
        assert_eq!(out3, "context A");
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeneratedTestQuestion {
    pub question: String,
    pub answers: Vec<TestAnswerGenerated>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TestAnswerGenerated {
    pub answer_text: String,
    pub is_correct: bool,
    #[serde(default)]
    pub position: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestWorkflowResult {
    pub total_concepts_extracted: usize,
    pub total_questions_created: usize,
    pub concepts_used: Vec<ConceptExtraction>,
    pub test_questions: Vec<GeneratedTestQuestion>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchAgentResponse {
    pub answer: String,
    pub sources: Vec<RelevantChunk>,
}

pub struct LlmService {
    manager: LlmManager,
}
impl LlmService {
    pub async fn new(config: &LlmUserConfig, agent_settings: &AgentSettings) -> Self {
        let manager = build_llm_manager_from_config(config, agent_settings).await;
        Self { manager }
    }

    pub async fn rebuild(&mut self, config: &LlmUserConfig, agent_settings: &AgentSettings) {
        self.manager = build_llm_manager_from_config(config, agent_settings).await;
    }

    pub async fn generate_flashcards_from_materials(
        &self,
        state: &State<'_, AppState>,
        request: FlashcardGenerationRequest,
    ) -> AppResult<Vec<GeneratedCard>> {
        if request.materials.is_empty() {
            return Ok(Vec::new());
        }
        println!(
            "Starting simplified flashcard generation for {} materials",
            request.materials.len()
        );

        let concept = request
            .concept
            .as_ref()
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| {
                AppError::ValidationError(
                    "Concept is required for flashcard generation".to_string(),
                )
            })?;

        let language = request
            .language
            .clone()
            .unwrap_or_else(|| "English".to_string());

        let material_ids: Vec<String> = {
            let db_service = state.db.lock().await;
            let mut ids = Vec::new();

            for material_request in &request.materials {
                match db_service.get_study_material_by_id(&material_request.material_id) {
                    Ok(Some(_)) => ids.push(material_request.material_id.clone()),
                    Ok(None) => eprintln!("Material not found: {}", material_request.material_id),
                    Err(e) => eprintln!(
                        "Error looking up material {}: {}",
                        material_request.material_id, e
                    ),
                }
            }
            ids
        };

        if material_ids.is_empty() {
            return Err(AppError::ValidationError(
                "No valid materials found".to_string(),
            ));
        }

        // Perform vector search
        let relevant_chunks = {
            let vector_service = state.vector.lock().await;
            let total_cards_requested: u32 =
                request.materials.iter().map(|m| m.cards_to_generate).sum();
            let chunks_to_retrieve = std::cmp::max(5, total_cards_requested / 2);

            vector_service
                .search_material_specific(concept, &material_ids, chunks_to_retrieve)
                .await
                .map_err(|e| AppError::VectorError(format!("Vector search failed: {}", e)))?
        };

        if relevant_chunks.is_empty() {
            return Err(AppError::ValidationError(format!(
                "No relevant content found for concept '{}'",
                concept
            )));
        }

        // Extract chunk texts
        let chunk_texts: Vec<String> = relevant_chunks
            .iter()
            .map(|chunk| format!("From {}: {}", chunk.metadata.file_display_name, chunk.text))
            .collect();

        let total_flashcards_needed: u32 =
            request.materials.iter().map(|m| m.cards_to_generate).sum();

        // Get agent settings from state
        let agent_settings = state.agent_settings.lock().await;

        let workflow_result = self
            .generate_flashcards_from_material_chunks(
                chunk_texts,
                total_flashcards_needed,
                concept.clone(),
                Some(language),
                &agent_settings,
            )
            .await?;

        let generated_cards: Vec<GeneratedCard> = workflow_result.flashcards;

        println!(
            "Successfully generated {} flashcards using simplified workflow",
            generated_cards.len()
        );

        Ok(generated_cards)
    }

    pub async fn generate_flashcards_from_material_chunks(
        &self,
        chunks: Vec<String>,
        total_flashcards_needed: u32,
        focus_concept: String,
        language: Option<String>,
        agent_settings: &AgentSettings,
    ) -> AppResult<WorkflowResult> {
        let language = language.unwrap_or_else(|| "English".to_string());

        println!("=== CREATING FLASHCARDS ===");
        println!(
            "Chunks: {}, Target flashcards: {}, Focus: '{}', Language: '{}'",
            chunks.len(),
            total_flashcards_needed,
            focus_concept,
            language
        );

        // Extract concepts from all chunks in parallel
        let all_concepts = self
            .extract_concepts_from_chunks_parallel(
                chunks.clone(),
                focus_concept.clone(),
                language.clone(),
                agent_settings,
            )
            .await?;

        let total_concepts = all_concepts.len();
        println!(
            "Extracted {} total concepts from {} chunks",
            total_concepts,
            chunks.len()
        );

        // Create flashcards directly from concepts
        let flashcards = self
            .create_flashcards_from_concepts(
                all_concepts.clone(),
                total_flashcards_needed,
                language.clone(),
                agent_settings,
            )
            .await?;

        println!(
            "Created {} flashcards from {} concepts",
            flashcards.len(),
            total_concepts
        );

        let workflow_stats = WorkflowStats {
            chunks_processed: chunks.len(),
            concept_extraction_requests: chunks.len(),
            flashcard_creation_requests: self.calculate_flashcard_requests(total_flashcards_needed),
            concepts_per_chunk_avg: total_concepts as f32 / chunks.len() as f32,
            flashcards_per_request_avg: if flashcards.is_empty() {
                0.0
            } else {
                total_flashcards_needed as f32
                    / self.calculate_flashcard_requests(total_flashcards_needed) as f32
            },
        };

        let result = WorkflowResult {
            total_concepts_extracted: total_concepts,
            total_flashcards_created: flashcards.len(),
            concepts_used: all_concepts,
            flashcards,
            workflow_stats,
        };

        println!(
            "Success: {} concepts → {} flashcards",
            total_concepts, result.total_flashcards_created
        );

        Ok(result)
    }

    /// Extract concepts from chunks in parallel
    async fn extract_concepts_from_chunks_parallel(
        &self,
        chunks: Vec<String>,
        focus_concept: String,
        language: String,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<ConceptExtraction>> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        let task_definition =
            self.get_task_definition_from_settings(&AgentType::ConceptExtractor, agent_settings);
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::ConceptExtractor)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::ConceptExtractor.get_prompt_template());

        let requests: Vec<GenerationRequest> = chunks
            .iter()
            .enumerate()
            .map(|(chunk_id, chunk)| {
                let formatted_prompt = prompt_template
                    .replace("{content}", chunk)
                    .replace("{focus_concept}", &focus_concept)
                    .replace("{language}", &language)
                    .replace("{chunk_id}", chunk_id.to_string().as_str());

                GenerationRequest::builder(formatted_prompt)
                    .task(&task_definition.name)
                    .build()
            })
            .collect();

        println!(
            "Executing {} concept extraction requests in parallel",
            requests.len()
        );

        let responses = self.manager.batch_generate(requests).await;
        let mut all_concepts = Vec::new();

        // Process responses
        for (chunk_id, response) in responses.iter().enumerate() {
            if response.success {
                let cleaned_content = &clean_json_content(&response.content);
                match serde_json::from_str::<ConceptList>(cleaned_content) {
                    Ok(concept_list) => {
                        for mut concept in concept_list.concepts {
                            concept.chunk_id = chunk_id;
                            all_concepts.push(concept);
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to parse concepts for chunk {}: {} | {:#?}",
                            chunk_id, e, response
                        );
                        all_concepts.push(ConceptExtraction {
                            concept: format!(
                                "Error extracting concept {} from chunk {}",
                                focus_concept,
                                chunk_id + 1
                            ),
                            context: "Failed to automatically extract specific concepts"
                                .to_string(),
                            chunk_id,
                        });
                    }
                }
            } else {
                eprintln!(
                    "Concept extraction failed for chunk {}: {}",
                    chunk_id,
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                );
                all_concepts.push(ConceptExtraction {
                    concept: format!(
                        "Error extracting concept {} from chunk {}",
                        focus_concept,
                        chunk_id + 1
                    ),
                    context: "Extraction failed, manual review needed".to_string(),
                    chunk_id,
                });
            }
        }

        Ok(all_concepts)
    }

    /// Create flashcards from concepts using smart batching to avoid duplicates
    async fn create_flashcards_from_concepts(
        &self,
        all_concepts: Vec<ConceptExtraction>,
        target_flashcards: u32,
        language: String,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<GeneratedCard>> {
        if all_concepts.is_empty() || target_flashcards == 0 {
            return Ok(Vec::new());
        }

        // Creates balanced batches that distribute concepts across requests
        let flashcards_per_batch =
            std::cmp::min(constants::MAX_FLASHCARDS_PER_REQUEST, target_flashcards); // Max 5 flashcards per request

        // Request needed calculation
        let total_requests = (target_flashcards + flashcards_per_batch - 1) / flashcards_per_batch;

        println!(
            "Smart batching: {} concepts → {} requests ({} flashcards per request)",
            all_concepts.len(),
            total_requests,
            flashcards_per_batch
        );

        // Distributing concepts across requests to avoid overlap
        let concept_batches =
            self.distribute_concepts_evenly(all_concepts, total_requests as usize);
        let task_definition = self
            .get_task_definition_from_settings(&AgentType::FlashcardContentCreator, agent_settings);
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::FlashcardContentCreator)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::FlashcardContentCreator.get_prompt_template());

        let mut requests = Vec::new();
        let mut expected_flashcards_per_request = Vec::new();

        for (batch_id, batch_concepts) in concept_batches.iter().enumerate() {
            if batch_concepts.is_empty() {
                continue;
            }

            let remaining_flashcards = target_flashcards - (batch_id as u32 * flashcards_per_batch);
            let flashcards_for_this_batch =
                std::cmp::min(flashcards_per_batch, remaining_flashcards);

            if flashcards_for_this_batch == 0 {
                break;
            }

            let concepts_json = serde_json::to_string(&ConceptList {
                concepts: batch_concepts.clone(),
            })
            .unwrap_or_else(|_| "[]".to_string());

            let formatted_prompt = prompt_template
                .replace("{count}", &flashcards_for_this_batch.to_string())
                .replace("{concepts}", &concepts_json)
                .replace("{language}", &language)
                .replace("{batch_id}", &(batch_id + 1).to_string());

            let request = GenerationRequest::builder(formatted_prompt)
                .task(&task_definition.name)
                .build();

            requests.push(request);
            expected_flashcards_per_request.push(flashcards_for_this_batch);
        }

        println!("Executing {} flashcard creation requests", requests.len());

        let responses = self.manager.batch_generate(requests).await;
        let mut all_flashcards = Vec::new();
        let mut used_questions = HashSet::new();
        for (batch_id, response) in responses.iter().enumerate() {
            let expected_count = expected_flashcards_per_request.get(batch_id).unwrap_or(&0);

            if response.success {
                let cleaned_content = &clean_json_content(&response.content);
                match serde_json::from_str::<FlashcardBatch>(cleaned_content) {
                    Ok(batch_result) => {
                        let mut added_count = 0;

                        for flashcard in batch_result.flashcards {
                            // duplicate checking
                            let question_key = flashcard.question.to_lowercase();
                            if !used_questions.contains(&question_key)
                                && all_flashcards.len() < target_flashcards as usize
                            {
                                used_questions.insert(question_key);
                                all_flashcards.push(flashcard);
                                added_count += 1;
                            }
                        }

                        println!(
                            "Batch {}: Created {} flashcards (expected {})",
                            batch_id + 1,
                            added_count,
                            expected_count
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to parse flashcards for batch {}: {}", batch_id, e);
                        if let Some(batch_concepts) = concept_batches.get(batch_id) {
                            for (i, concept) in batch_concepts.iter().enumerate() {
                                if all_flashcards.len() >= target_flashcards as usize {
                                    break;
                                }
                                if i >= *expected_count as usize {
                                    break;
                                }

                                all_flashcards.push(GeneratedCard {
                                    question: format!(
                                        "Flashcard creation error while generating for: {}?",
                                        concept.concept
                                    ),
                                    answer: format!("Context: {}", concept.context),
                                });
                            }
                        }
                    }
                }
            } else {
                eprintln!(
                    "Flashcard creation failed for batch {}: {}",
                    batch_id,
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                );
            }
        }

        // Trim to exact target if we somehow got more
        all_flashcards.truncate(target_flashcards as usize);

        println!(
            "Final result: {} unique flashcards created (target: {})",
            all_flashcards.len(),
            target_flashcards
        );

        Ok(all_flashcards)
    }

    /// Deterministic contextual rendering used when the model returned no
    /// question/answer text for an in-context card. The result is guaranteed
    /// to be context-bearing: it embeds the example sentence (or, failing
    /// that, the bare target expression) and the target expression itself,
    /// so an UnderstandingInContext card can never silently collapse into a
    /// plain Translation card.
    fn render_in_context_card(
        item: &VocabularyItem,
        direction: LearningDirection,
    ) -> (String, String) {
        let example = item
            .example
            .clone()
            .unwrap_or_else(|| item.target_expression.clone());
        match direction {
            LearningDirection::Recognition => (
                format!(
                    "What does '{}' mean in: \"{}\"?",
                    item.target_expression, example
                ),
                item.example_translation
                    .clone()
                    .unwrap_or_else(|| item.meaning.clone()),
            ),
            LearningDirection::Production => (
                format!(
                    "How do you say '{}' in this context: \"{}\"?",
                    item.meaning, example
                ),
                item.target_expression.clone(),
            ),
        }
    }

    /// Validate one parsed LLM card into a canonical language card.
    ///
    /// Returns `None` when the card is unusable as a language card: a
    /// language card must carry its structured vocabulary, and the direction
    /// and task must be explicit values (missing values default; unknown
    /// values fail the whole batch). When the model returned no usable
    /// question/answer text, a deterministic rendering of the vocabulary
    /// item is used so the card still works; the rendering depends on the
    /// task so an in-context card is never identical to a translation card.
    pub fn normalize_language_flashcard(
        raw: LanguageFlashcard,
    ) -> Option<(String, String, LearningDirection, CardTask, VocabularyItem)> {
        let item = raw.vocabulary?;
        if item.target_expression.trim().is_empty() || item.meaning.trim().is_empty() {
            return None;
        }
        let direction = raw.direction.unwrap_or_default();
        let task = raw.task.unwrap_or_default();
        let question = raw.question.trim().to_string();
        let answer = raw.answer.trim().to_string();
        let (question, answer) = if question.is_empty() || answer.is_empty() {
            match task {
                CardTask::Translation => render_translation_card(&item, direction),
                CardTask::UnderstandingInContext => Self::render_in_context_card(&item, direction),
            }
        } else {
            (question, answer)
        };
        Some((question, answer, direction, task, item))
    }

    /// Language-learning flashcard generation: reuses the general concept
    /// extraction step, then creates cards with structured language data.
    /// The general flashcard pipeline above is untouched and is used when
    /// language-learning mode is off.
    pub async fn generate_language_flashcards_from_materials(
        &self,
        state: &State<'_, AppState>,
        request: LanguageFlashcardGenerationRequest,
    ) -> AppResult<Vec<LanguageFlashcard>> {
        if request.materials.is_empty() {
            return Ok(Vec::new());
        }

        let concept = request
            .concept
            .as_ref()
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| {
                AppError::ValidationError(
                    "Concept is required for language flashcard generation".to_string(),
                )
            })?;

        let learning_language = request.learning_language.trim().to_string();
        let base_language = request.base_language.trim().to_string();
        if learning_language.is_empty() || base_language.is_empty() {
            return Err(AppError::ValidationError(
                "Learning language and base language are required for language flashcard generation".to_string(),
            ));
        }

        let material_ids: Vec<String> = {
            let db_service = state.db.lock().await;
            let mut ids = Vec::new();

            for material_request in &request.materials {
                match db_service.get_study_material_by_id(&material_request.material_id) {
                    Ok(Some(_)) => ids.push(material_request.material_id.clone()),
                    Ok(None) => eprintln!("Material not found: {}", material_request.material_id),
                    Err(e) => eprintln!(
                        "Error looking up material {}: {}",
                        material_request.material_id, e
                    ),
                }
            }
            ids
        };

        if material_ids.is_empty() {
            return Err(AppError::ValidationError(
                "No valid materials found".to_string(),
            ));
        }

        let relevant_chunks = {
            let vector_service = state.vector.lock().await;
            let total_cards_requested: u32 =
                request.materials.iter().map(|m| m.cards_to_generate).sum();
            let chunks_to_retrieve = std::cmp::max(5, total_cards_requested / 2);

            vector_service
                .search_material_specific(concept, &material_ids, chunks_to_retrieve)
                .await
                .map_err(|e| AppError::VectorError(format!("Vector search failed: {}", e)))?
        };

        if relevant_chunks.is_empty() {
            return Err(AppError::ValidationError(format!(
                "No relevant content found for concept '{}'",
                concept
            )));
        }

        let chunk_texts: Vec<String> = relevant_chunks
            .iter()
            .map(|chunk| format!("From {}: {}", chunk.metadata.file_display_name, chunk.text))
            .collect();

        let total_flashcards_needed: u32 =
            request.materials.iter().map(|m| m.cards_to_generate).sum();

        let agent_settings = state.agent_settings.lock().await;

        let flashcards = self
            .create_language_flashcards_from_concepts(
                &chunk_texts,
                total_flashcards_needed,
                concept.clone(),
                &learning_language,
                &base_language,
                request.direction,
                &agent_settings,
            )
            .await?;

        println!(
            "Successfully generated {} language flashcards using language pipeline",
            flashcards.len()
        );

        Ok(flashcards)
    }

    /// Concept extraction (reused from the general pipeline, targeting the
    /// learning language) followed by language flashcard creation.
    pub async fn create_language_flashcards_from_concepts(
        &self,
        chunks: &[String],
        total_flashcards_needed: u32,
        focus_concept: String,
        learning_language: &str,
        base_language: &str,
        direction_choice: CardDirectionChoice,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<LanguageFlashcard>> {
        if chunks.is_empty() || total_flashcards_needed == 0 {
            return Ok(Vec::new());
        }

        let all_concepts = self
            .extract_concepts_from_chunks_parallel(
                chunks.to_vec(),
                focus_concept.clone(),
                learning_language.to_string(),
                agent_settings,
            )
            .await?;

        self.create_language_flashcards_from_concepts_batched(
            &all_concepts,
            total_flashcards_needed,
            learning_language,
            base_language,
            direction_choice,
            agent_settings,
        )
        .await
    }

    /// Create language flashcards from extracted concepts using the same
    /// smart batching as the general pipeline.
    async fn create_language_flashcards_from_concepts_batched(
        &self,
        all_concepts: &[ConceptExtraction],
        target_flashcards: u32,
        learning_language: &str,
        base_language: &str,
        direction_choice: CardDirectionChoice,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<LanguageFlashcard>> {
        if all_concepts.is_empty() || target_flashcards == 0 {
            return Ok(Vec::new());
        }

        let flashcards_per_batch =
            std::cmp::min(constants::MAX_FLASHCARDS_PER_REQUEST, target_flashcards);

        let total_requests = (target_flashcards + flashcards_per_batch - 1) / flashcards_per_batch;

        let concept_batches =
            self.distribute_concepts_evenly(all_concepts.to_vec(), total_requests as usize);
        let task_definition = self.get_task_definition_from_settings(
            &AgentType::LanguageFlashcardContentCreator,
            agent_settings,
        );
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::LanguageFlashcardContentCreator)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::LanguageFlashcardContentCreator.get_prompt_template());

        let mut requests = Vec::new();
        let mut expected_flashcards_per_request = Vec::new();

        for (batch_id, batch_concepts) in concept_batches.iter().enumerate() {
            if batch_concepts.is_empty() {
                continue;
            }

            let remaining_flashcards = target_flashcards - (batch_id as u32 * flashcards_per_batch);
            let flashcards_for_this_batch =
                std::cmp::min(flashcards_per_batch, remaining_flashcards);

            if flashcards_for_this_batch == 0 {
                break;
            }

            let concepts_json = serde_json::to_string(&ConceptList {
                concepts: batch_concepts.clone(),
            })
            .unwrap_or_else(|_| "[]".to_string());

            let formatted_prompt = prompt_template
                .replace("{count}", &flashcards_for_this_batch.to_string())
                .replace("{concepts}", &concepts_json)
                .replace("{learning_language}", learning_language)
                .replace("{base_language}", base_language)
                .replace("{direction}", direction_choice.as_str())
                .replace("{batch_id}", &(batch_id + 1).to_string());

            let request = GenerationRequest::builder(formatted_prompt)
                .task(&task_definition.name)
                .build();

            requests.push(request);
            expected_flashcards_per_request.push(flashcards_for_this_batch);
        }

        let responses = self.manager.batch_generate(requests).await;
        let mut all_flashcards = Vec::new();
        // A language card is identified by (direction, target expression); the
        // same item may legitimately appear once per direction.
        let mut used_cards = HashSet::new();
        for (batch_id, response) in responses.iter().enumerate() {
            if response.success {
                let cleaned_content = &clean_json_content(&response.content);
                match serde_json::from_str::<LanguageFlashcardBatch>(cleaned_content) {
                    Ok(batch_result) => {
                        let mut added_count = 0;

                        for flashcard in batch_result.flashcards {
                            let Some((question, answer, direction, task, item)) =
                                Self::normalize_language_flashcard(flashcard)
                            else {
                                eprintln!(
                                    "Batch {}: dropping language card without usable vocabulary",
                                    batch_id + 1
                                );
                                continue;
                            };

                            let card_key =
                                format!("{}\u{0}{}", direction.as_str(), item.target_expression);
                            if !used_cards.contains(&card_key)
                                && all_flashcards.len() < target_flashcards as usize
                            {
                                used_cards.insert(card_key);
                                all_flashcards.push(LanguageFlashcard {
                                    question,
                                    answer,
                                    vocabulary: Some(item),
                                    direction: Some(direction),
                                    task: Some(task),
                                });
                                added_count += 1;
                            }
                        }

                        println!(
                            "Batch {}: Created {} language flashcards (expected {})",
                            batch_id + 1,
                            added_count,
                            expected_flashcards_per_request.get(batch_id).unwrap_or(&0)
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to parse language flashcards for batch {}: {}",
                            batch_id + 1,
                            e
                        );
                    }
                }
            } else {
                eprintln!(
                    "Language flashcard creation failed for batch {}: {}",
                    batch_id + 1,
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                );
            }
        }

        all_flashcards.truncate(target_flashcards as usize);

        println!(
            "Final result: {} unique language flashcards created (target: {})",
            all_flashcards.len(),
            target_flashcards
        );

        Ok(all_flashcards)
    }

    /// Generate a text mnemonic for a single flashcard fact (feature 31).
    ///
    /// `kind` is the desired mnemonic style ("auto", "association", "acronym",
    /// "story", "visual", "chunking"). `language_mode` is "on" when the card is
    /// a language pair and the model must fill the structured language fields.
    ///
    /// This capability is fully independent of image generation: a failure here
    /// must not affect image state and vice versa.
    pub async fn generate_card_mnemonic(
        &self,
        fact: &str,
        kind: &str,
        language_mode: &str,
        learner_profile: Option<&str>,
        agent_settings: &AgentSettings,
    ) -> AppResult<MnemonicGenerationOutput> {
        let task_definition =
            self.get_task_definition_from_settings(&AgentType::MnemonicGenerator, agent_settings);
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::MnemonicGenerator)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::MnemonicGenerator.get_prompt_template());

        let formatted_prompt = prompt_template
            .replace("{fact}", fact)
            .replace("{kind}", kind)
            .replace("{language_mode}", language_mode)
            .replace(
                "{learner_profile}",
                learner_profile.unwrap_or("General knowledge learner"),
            );

        let request = GenerationRequest::builder(formatted_prompt)
            .task(&task_definition.name)
            .build();

        let responses = self.manager.batch_generate(vec![request]).await;

        let first = responses.first().ok_or_else(|| {
            AppError::LlmError("No response from LLM for mnemonic generation".into())
        })?;
        if !first.success {
            return Err(AppError::LlmError(format!(
                "Mnemonic generation failed: {}",
                first
                    .error
                    .clone()
                    .unwrap_or_else(|| "unknown error".to_string())
            )));
        }
        let raw = first.content.clone();

        let parsed = crate::materials::flashcard_mnemonic::parse_mnemonic_response(&raw)
            .map_err(|e| AppError::LlmError(format!("Failed to parse mnemonic response: {}", e)))?;

        // Build the output explicitly (the derive has a known trait-bound
        // issue with the nested Option<LanguageMnemonic> field in this
        // context, so we construct it by hand).
        let out = MnemonicGenerationOutput {
            is_suitable: parsed.is_suitable,
            reason: parsed.reason.clone(),
            mnemonic: parsed.mnemonic.clone(),
            image_prompt: parsed.image_prompt.clone(),
            language: parsed.language.clone(),
        };
        Ok(out)
    }

    /// Generate flashcard explanation
    pub async fn generate_flashcard_explanation(
        &self,
        question: String,
        answer: String,
        agent_settings: &AgentSettings,
    ) -> AppResult<String> {
        match self
            .execute_flashcard_explanation(&question, &answer, agent_settings)
            .await
        {
            Ok(explanation_response) => {
                let cleaned_content = &clean_json_content(&explanation_response);
                match serde_json::from_str::<serde_json::Value>(cleaned_content) {
                    Ok(explanation_json) => {
                        if let Some(explanation_text) =
                            explanation_json.get("explanation").and_then(|e| e.as_str())
                        {
                            Ok(explanation_text.to_string())
                        } else {
                            Ok(explanation_response)
                        }
                    }
                    Err(e) => Err(AppError::JsonError(format!(
                        "Failed to parse explanation JSON: {}",
                        e
                    ))),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Search study materials
    pub async fn search_study_materials(
        &self,
        state: &State<'_, AppState>,
        query: String,
    ) -> AppResult<SearchAgentResponse> {
        let study_settings = state.study_settings.lock().await;
        if query.trim().is_empty() {
            return Err(AppError::ValidationError(
                "Search query cannot be empty".to_string(),
            ));
        }
        let has_materials = {
            let db_service = state.db.lock().await;
            match db_service.get_study_materials() {
                Ok(materials) => !materials.is_empty(),
                Err(_) => false,
            }
        };
        if !has_materials {
            return Err(AppError::ValidationError(
                "No study materials found. Please upload some PDF files first.".to_string(),
            ));
        }
        // Perform vector search
        let relevant_chunks = {
            let vector_service = state.vector.lock().await;
            let num_chunks = std::cmp::max(
                study_settings.max_chunks_to_recover_from_search,
                constants::VECDB_MIN_CHUNKS_RECOVERED_AI_SEARCH,
            );
            let num_chunks =
                std::cmp::min(num_chunks, constants::VECDB_MAX_CHUNKS_RECOVERED_AI_SEARCH);
            vector_service
                .search_global(&query, num_chunks)
                .await
                .map_err(|e| {
                    AppError::VectorError(format!(
                        "Failed to search through your study materials {e}"
                    ))
                })?
        };
        if relevant_chunks.is_empty() {
            return Err(AppError::ValidationError(
                "No relevant content found in your study materials for this query.".to_string(),
            ));
        }
        // Prepare chunk data for SearchAgent
        let document_chunks_with_metadata: Vec<(String, Option<String>, Option<f32>)> =
            relevant_chunks
                .iter()
                .map(|chunk| {
                    (
                        chunk.text.clone(),
                        Some(chunk.metadata.file_display_name.clone()),
                        Some(chunk.score),
                    )
                })
                .collect();
        // Get agent settings from state
        let agent_settings = state.agent_settings.lock().await;
        // Use SearchAgent
        match self
            .execute_knowledge_search_with_metadata(
                &query,
                &document_chunks_with_metadata,
                &agent_settings,
            )
            .await
        {
            Ok(search_response) => {
                let cleaned_content = &clean_json_content(&search_response);
                match serde_json::from_str::<serde_json::Value>(cleaned_content) {
                    Ok(search_result) => {
                        let answer = search_result
                            .get("answer")
                            .and_then(|a| a.as_str())
                            .unwrap_or("No answer provided")
                            .to_string();

                        let sources = search_result
                            .get("sources")
                            .and_then(|s| s.as_array())
                            .map(|arr| {
                                let mut seen_files = HashSet::new();
                                let mut unique_sources = Vec::new();

                                for source_value in arr.iter() {
                                    if let Some(source_text) = source_value.as_str() {
                                        if let Some(chunk) = relevant_chunks.iter().find(|chunk| {
                                            source_text.contains(&chunk.metadata.file_display_name)
                                        }) {
                                            // Only add if we haven't seen this file before
                                            if seen_files
                                                .insert(chunk.metadata.file_display_name.clone())
                                            {
                                                unique_sources.push(chunk.clone());
                                            }
                                        }
                                    }
                                }
                                unique_sources
                            })
                            .unwrap_or_else(Vec::new);

                        Ok(SearchAgentResponse { answer, sources })
                    }
                    Err(e) => Err(AppError::JsonError(format!(
                        "Failed to parse SearchAgent response: {}",
                        e
                    ))),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn generate_test_questions_from_materials(
        &self,
        state: &State<'_, AppState>,
        request: TestGenerationRequest,
    ) -> AppResult<Vec<GeneratedTestQuestion>> {
        if request.materials.is_empty() {
            return Ok(Vec::new());
        }

        println!(
            "Starting test question generation for {} materials",
            request.materials.len()
        );

        let concept = request
            .concept
            .as_ref()
            .filter(|c| !c.trim().is_empty())
            .ok_or_else(|| {
                AppError::ValidationError(
                    "Concept is required for test question generation".to_string(),
                )
            })?;

        let language = request
            .language
            .clone()
            .unwrap_or_else(|| "English".to_string());

        let material_ids: Vec<String> = {
            let db_service = state.db.lock().await;
            let mut ids = Vec::new();

            for material_request in &request.materials {
                match db_service.get_study_material_by_id(&material_request.material_id) {
                    Ok(Some(_)) => ids.push(material_request.material_id.clone()),
                    Ok(None) => eprintln!("Material not found: {}", material_request.material_id),
                    Err(e) => eprintln!(
                        "Error looking up material {}: {}",
                        material_request.material_id, e
                    ),
                }
            }
            ids
        };

        if material_ids.is_empty() {
            return Err(AppError::ValidationError(
                "No valid materials found".to_string(),
            ));
        }

        // Perform vector search
        let relevant_chunks = {
            let vector_service = state.vector.lock().await;
            let total_questions_requested: u32 = request
                .materials
                .iter()
                .map(|m| m.questions_to_generate)
                .sum();
            let chunks_to_retrieve = std::cmp::max(5, total_questions_requested / 2);

            vector_service
                .search_material_specific(concept, &material_ids, chunks_to_retrieve)
                .await
                .map_err(|e| AppError::VectorError(format!("Vector search failed: {}", e)))?
        };

        if relevant_chunks.is_empty() {
            return Err(AppError::ValidationError(format!(
                "No relevant content found for concept '{}'",
                concept
            )));
        }

        // Extract chunk texts
        let chunk_texts: Vec<String> = relevant_chunks
            .iter()
            .map(|chunk| format!("From {}: {}", chunk.metadata.file_display_name, chunk.text))
            .collect();

        let total_questions_needed: u32 = request
            .materials
            .iter()
            .map(|m| m.questions_to_generate)
            .sum();

        // Get agent settings from state
        let agent_settings = state.agent_settings.lock().await;

        let workflow_result = self
            .generate_test_questions_from_material_chunks(
                chunk_texts,
                total_questions_needed,
                concept.clone(),
                Some(language),
                &agent_settings,
            )
            .await?;

        let generated_questions: Vec<GeneratedTestQuestion> = workflow_result.test_questions;

        println!(
            "Successfully generated {} test questions",
            generated_questions.len()
        );

        Ok(generated_questions)
    }

    pub async fn generate_test_questions_from_material_chunks(
        &self,
        chunks: Vec<String>,
        total_questions_needed: u32,
        focus_concept: String,
        language: Option<String>,
        agent_settings: &AgentSettings,
    ) -> AppResult<TestWorkflowResult> {
        let language = language.unwrap_or_else(|| "English".to_string());

        println!("=== CREATING TEST QUESTIONS ===");
        println!(
            "Chunks: {}, Target questions: {}, Focus: '{}', Language: '{}'",
            chunks.len(),
            total_questions_needed,
            focus_concept,
            language
        );

        // Extract concepts from all chunks in parallel (reuse existing method)
        let all_concepts = self
            .extract_concepts_from_chunks_parallel(
                chunks.clone(),
                focus_concept.clone(),
                language.clone(),
                agent_settings,
            )
            .await?;

        let total_concepts = all_concepts.len();
        println!(
            "Extracted {} total concepts from {} chunks",
            total_concepts,
            chunks.len()
        );

        // Create test questions directly from concepts
        let test_questions = self
            .create_test_questions_from_concepts(
                all_concepts.clone(),
                total_questions_needed,
                language.clone(),
                agent_settings,
            )
            .await?;

        println!(
            "Created {} test questions from {} concepts",
            test_questions.len(),
            total_concepts
        );

        let result = TestWorkflowResult {
            total_concepts_extracted: total_concepts,
            total_questions_created: test_questions.len(),
            concepts_used: all_concepts,
            test_questions,
        };

        println!(
            "Success: {} concepts → {} test questions",
            total_concepts, result.total_questions_created
        );

        Ok(result)
    }

    /// Generates transfer questions for a knowledge assessment from the
    /// studied material (feature 38-40).
    pub async fn generate_assessment_questions_from_materials(
        &self,
        state: &State<'_, AppState>,
        request: AssessmentGenerationRequest,
    ) -> AppResult<Vec<NewAssessmentQuestion>> {
        if request.materials.is_empty() {
            return Ok(Vec::new());
        }
        if request.run_context.trim().is_empty() {
            return Err(AppError::ValidationError(
                "Run context is required for assessment question generation".to_string(),
            ));
        }

        let language = request
            .language
            .clone()
            .unwrap_or_else(|| "English".to_string());

        let material_ids: Vec<String> = {
            let db_service = state.db.lock().await;
            let mut ids = Vec::new();

            for material_request in &request.materials {
                match db_service.get_study_material_by_id(&material_request.material_id) {
                    Ok(Some(_)) => ids.push(material_request.material_id.clone()),
                    Ok(None) => eprintln!("Material not found: {}", material_request.material_id),
                    Err(e) => eprintln!(
                        "Error looking up material {}: {}",
                        material_request.material_id, e
                    ),
                }
            }
            ids
        };

        if material_ids.is_empty() {
            return Err(AppError::ValidationError(
                "No valid materials found".to_string(),
            ));
        }

        // The vector search is guided by the run context, which is a natural
        // language description of what to focus on (e.g. the concept name for
        // an initial assessment, or the weak concepts for remediation).
        let relevant_chunks = {
            let vector_service = state.vector.lock().await;
            let total_questions_requested: u32 =
                request.materials.iter().map(|m| m.cards_to_generate).sum();
            let chunks_to_retrieve = std::cmp::max(5, total_questions_requested / 2);

            vector_service
                .search_material_specific(&request.run_context, &material_ids, chunks_to_retrieve)
                .await
                .map_err(|e| AppError::VectorError(format!("Vector search failed: {}", e)))?
        };

        if relevant_chunks.is_empty() {
            return Err(AppError::ValidationError(format!(
                "No relevant content found for run context '{}'",
                request.run_context
            )));
        }

        let chunk_texts: Vec<String> = relevant_chunks
            .iter()
            .map(|chunk| format!("From {}: {}", chunk.metadata.file_display_name, chunk.text))
            .collect();

        let total_questions_needed: u32 =
            request.materials.iter().map(|m| m.cards_to_generate).sum();

        let agent_settings = state.agent_settings.lock().await;

        let all_concepts = self
            .extract_concepts_from_chunks_parallel(
                chunk_texts.clone(),
                request.run_context.clone(),
                language.clone(),
                &agent_settings,
            )
            .await?;

        if all_concepts.is_empty() {
            return Err(AppError::ValidationError(
                "No concepts could be extracted from the studied material".to_string(),
            ));
        }

        let questions = self
            .create_assessment_questions_from_concepts(
                all_concepts,
                total_questions_needed,
                language.clone(),
                request.run_context.clone(),
                &agent_settings,
            )
            .await?;

        println!(
            "Successfully generated {} assessment questions",
            questions.len()
        );

        Ok(questions)
    }

    /// Create transfer assessment questions from concepts (initial assessment).
    ///
    /// Unlike multiple-choice tests, assessment questions must test the
    /// concepts in NEW formulations/scenarios, so the prompt explicitly
    /// forbids paraphrasing stored content and asks for a recall →
    /// understanding → application progression where the material supports it.
    async fn create_assessment_questions_from_concepts(
        &self,
        all_concepts: Vec<ConceptExtraction>,
        target_questions: u32,
        language: String,
        run_context: String,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<NewAssessmentQuestion>> {
        self.create_assessment_questions_from_concepts_internal(
            all_concepts,
            target_questions,
            language,
            run_context,
            agent_settings,
            Vec::new(),
        )
        .await
    }

    async fn create_assessment_questions_from_concepts_internal(
        &self,
        all_concepts: Vec<ConceptExtraction>,
        target_questions: u32,
        language: String,
        run_context: String,
        agent_settings: &AgentSettings,
        exclude_prompts: Vec<String>,
    ) -> AppResult<Vec<NewAssessmentQuestion>> {
        if all_concepts.is_empty() || target_questions == 0 {
            return Ok(Vec::new());
        }

        // Collect the known concept names before moving `all_concepts` into
        // `distribute_concepts_evenly`, so generated questions can be checked
        // against them later.
        let known_concepts: Vec<String> = all_concepts.iter().map(|c| c.concept.clone()).collect();

        // Normalized prompts that must NOT be re-generated (e.g. the existing
        // questions of an assessment being re-assessed).
        let mut exclude_keys: HashSet<String> = exclude_prompts
            .iter()
            .map(|p| normalize_question_prompt(p))
            .collect();

        // Assessment questions are more complex than flashcards; keep batches small.
        let questions_per_batch = std::cmp::min(3, target_questions);
        let total_requests = (target_questions + questions_per_batch - 1) / questions_per_batch;

        let concept_batches =
            self.distribute_concepts_evenly(all_concepts.clone(), total_requests as usize);
        let task_definition = self.get_task_definition_from_settings(
            &AgentType::AssessmentContentCreator,
            agent_settings,
        );
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::AssessmentContentCreator)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::AssessmentContentCreator.get_prompt_template());

        let mut requests = Vec::new();
        let mut request_concepts: Vec<Vec<ConceptExtraction>> = Vec::new();

        for (batch_id, batch_concepts) in concept_batches.iter().enumerate() {
            if batch_concepts.is_empty() {
                continue;
            }

            let remaining_questions = target_questions - (batch_id as u32 * questions_per_batch);
            let questions_for_this_batch = std::cmp::min(questions_per_batch, remaining_questions);

            if questions_for_this_batch == 0 {
                break;
            }

            let concepts_json = serde_json::to_string(&ConceptList {
                concepts: batch_concepts.clone(),
            })
            .unwrap_or_else(|_| "[]".to_string());

            // On a re-run, tell the model which phrasings it must NOT repeat so
            // the retry yields genuinely new questions rather than the same ones.
            let avoid_clause = if exclude_keys.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\nIMPORTANT: Do NOT reuse these already-used question phrasings (write genuinely new ones): {}",
                    exclude_prompts
                        .iter()
                        .take(8)
                        .map(|p| format!("- {}", p.trim()))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };

            let formatted_prompt = prompt_template
                .replace("{count}", &questions_for_this_batch.to_string())
                .replace("{concepts}", &concepts_json)
                .replace("{language}", &language)
                .replace("{run_context}", &run_context)
                + &avoid_clause;

            let request = GenerationRequest::builder(formatted_prompt)
                .task(&task_definition.name)
                .build();

            requests.push(request);
            request_concepts.push(batch_concepts.clone());
        }

        println!(
            "Executing {} assessment question creation requests",
            requests.len()
        );

        let responses = self.manager.batch_generate(requests).await;
        let mut all_questions: Vec<NewAssessmentQuestion> = Vec::new();
        let mut used_prompts = HashSet::new();
        let mut failed_request_indices: Vec<usize> = Vec::new();

        for (response_index, response) in responses.iter().enumerate() {
            if !response.success {
                eprintln!(
                    "Assessment question creation failed: {}",
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                );
                failed_request_indices.push(response_index);
                continue;
            }

            let cleaned_content = clean_json_content(&response.content);
            let batch_result: AssessmentQuestionBatch = match serde_json::from_str(&cleaned_content)
            {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Failed to parse assessment questions: {}", e);
                    failed_request_indices.push(response_index);
                    continue;
                }
            };

            let batch_concepts = request_concepts
                .get(response_index)
                .map(|c| c.as_slice())
                .unwrap_or(&[]);

            for question in batch_result.questions {
                if all_questions.len() >= target_questions as usize {
                    break;
                }

                let prompt_key = normalize_question_prompt(&question.prompt);
                if prompt_key.is_empty()
                    || used_prompts.contains(&prompt_key)
                    || exclude_keys.contains(&prompt_key)
                {
                    continue;
                }

                // Reject questions that target a concept not in the studied set.
                if !question.concept.trim().is_empty()
                    && !known_concepts
                        .iter()
                        .any(|c| c.trim().eq_ignore_ascii_case(question.concept.trim()))
                {
                    eprintln!(
                        "Dropping assessment question with unknown concept '{}'",
                        question.concept
                    );
                    continue;
                }

                // Unknown or missing difficulty is rejected rather than coerced.
                let difficulty = match AssessmentDifficulty::parse(&question.difficulty) {
                    Some(d) => d,
                    None => {
                        eprintln!(
                            "Dropping assessment question with invalid difficulty '{}'",
                            question.difficulty
                        );
                        continue;
                    }
                };

                let concept = if question.concept.trim().is_empty() {
                    known_concepts[0].clone()
                } else {
                    question.concept
                };

                // Resolve the source-grounded reference content: the chunk the
                // model cited, else the chunk for the targeted concept, else the
                // batch's first chunk. Guarantees evaluation is grounded in the
                // studied material, not the model's own knowledge.
                let expected_content = resolve_expected_content(
                    question.source_chunk.as_deref(),
                    &concept,
                    batch_concepts,
                );

                used_prompts.insert(prompt_key);
                all_questions.push(NewAssessmentQuestion {
                    prompt: question.prompt,
                    concept,
                    difficulty,
                    expected_content,
                    position: all_questions.len() as i32,
                });
            }
        }

        // Bounded retry: re-issue the failed / under-filled requests once, telling
        // the model to avoid the phrasings already produced. Without this, a single
        // dropped batch permanently shortens the assessment with no recourse.
        if !failed_request_indices.is_empty() && all_questions.len() < target_questions as usize {
            let deficit = target_questions - all_questions.len() as u32;
            let existing: Vec<String> = all_questions.iter().map(|q| q.prompt.clone()).collect();

            let mut retry_requests = Vec::new();
            let mut retry_concepts: Vec<Vec<ConceptExtraction>> = Vec::new();
            for idx in failed_request_indices {
                if retry_requests.len() as u32 >= deficit {
                    break;
                }
                let concepts = all_concepts
                    .get(idx % all_concepts.len())
                    .map(|c| vec![c.clone()])
                    .unwrap_or_default();
                if concepts.is_empty() {
                    continue;
                }
                let concepts_json = serde_json::to_string(&ConceptList {
                    concepts: concepts.clone(),
                })
                .unwrap_or_else(|_| "[]".to_string());
                let avoid = if existing.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n\nIMPORTANT: Do NOT reuse these question phrasings (write new ones): {}",
                        existing
                            .iter()
                            .map(|p| format!("- {}", p.trim()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };
                let formatted_prompt = prompt_template
                    .replace("{count}", "1")
                    .replace("{concepts}", &concepts_json)
                    .replace("{language}", &language)
                    .replace("{run_context}", &run_context)
                    + &avoid;
                retry_requests.push(
                    GenerationRequest::builder(formatted_prompt)
                        .task(&task_definition.name)
                        .build(),
                );
                retry_concepts.push(concepts);
            }

            if !retry_requests.is_empty() {
                let retry_responses = self.manager.batch_generate(retry_requests).await;
                for (response_index, response) in retry_responses.iter().enumerate() {
                    if all_questions.len() >= target_questions as usize {
                        break;
                    }
                    if !response.success {
                        continue;
                    }
                    let cleaned_content = clean_json_content(&response.content);
                    let batch_result: AssessmentQuestionBatch =
                        match serde_json::from_str(&cleaned_content) {
                            Ok(result) => result,
                            Err(_) => continue,
                        };
                    let batch_concepts = retry_concepts
                        .get(response_index)
                        .map(|c| c.as_slice())
                        .unwrap_or(&[]);
                    for question in batch_result.questions {
                        if all_questions.len() >= target_questions as usize {
                            break;
                        }
                        let prompt_key = normalize_question_prompt(&question.prompt);
                        if prompt_key.is_empty()
                            || used_prompts.contains(&prompt_key)
                            || exclude_keys.contains(&prompt_key)
                        {
                            continue;
                        }
                        if !question.concept.trim().is_empty()
                            && !known_concepts
                                .iter()
                                .any(|c| c.trim().eq_ignore_ascii_case(question.concept.trim()))
                        {
                            continue;
                        }
                        let difficulty = match AssessmentDifficulty::parse(&question.difficulty) {
                            Some(d) => d,
                            None => continue,
                        };
                        let concept = if question.concept.trim().is_empty() {
                            known_concepts[0].clone()
                        } else {
                            question.concept
                        };
                        let expected_content = resolve_expected_content(
                            question.source_chunk.as_deref(),
                            &concept,
                            batch_concepts,
                        );
                        used_prompts.insert(prompt_key);
                        all_questions.push(NewAssessmentQuestion {
                            prompt: question.prompt,
                            concept,
                            difficulty,
                            expected_content,
                            position: all_questions.len() as i32,
                        });
                    }
                }
            }
        }

        // Re-number positions so the final list is contiguous.
        for (index, question) in all_questions.iter_mut().enumerate() {
            question.position = index as i32;
        }

        println!(
            "Final result: {} unique assessment questions created (target: {})",
            all_questions.len(),
            target_questions
        );

        Ok(all_questions)
    }

    /// Evaluates one free-text assessment answer against the expected content
    /// and returns the structured evaluation (feature 41-43).
    pub async fn evaluate_assessment_answer(
        &self,
        question: &str,
        concept: &str,
        difficulty: &str,
        expected_content: &str,
        student_answer: &str,
        language: &str,
        agent_settings: &AgentSettings,
    ) -> AppResult<crate::materials::assessment::AssessmentEvaluation> {
        if student_answer.trim().is_empty() {
            return Ok(crate::materials::assessment::AssessmentEvaluation {
                grade: AssessmentGrade::Insufficient,
                rationale: "The answer was empty.".to_string(),
                misconception: None,
            });
        }

        // Evaluation must be grounded in the source material. An empty expected
        // content means there is no reference to grade against, so we surface an
        // error rather than grading against nothing (which is what happens when
        // expected_content was never persisted).
        if expected_content.trim().is_empty() {
            return Err(AppError::LlmError(
                "Cannot evaluate answer: the question has no stored reference content.".to_string(),
            ));
        }

        let task_definition = self.get_task_definition_from_settings(
            &AgentType::AssessmentAnswerEvaluator,
            agent_settings,
        );
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::AssessmentAnswerEvaluator)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::AssessmentAnswerEvaluator.get_prompt_template());

        let formatted_prompt = prompt_template
            .replace("{question}", question)
            .replace("{concept}", concept)
            .replace("{difficulty}", difficulty)
            .replace("{expected_content}", expected_content)
            .replace("{student_answer}", student_answer)
            .replace("{language}", language);

        let request = GenerationRequest::builder(formatted_prompt)
            .task(&task_definition.name)
            .build();

        let responses = self.manager.batch_generate(vec![request]).await;

        let response = responses.into_iter().next().ok_or_else(|| {
            AppError::LlmError("No response received for answer evaluation".to_string())
        })?;

        if !response.success {
            return Err(AppError::LlmError(format!(
                "Answer evaluation failed: {}",
                response
                    .error
                    .as_ref()
                    .unwrap_or(&"Unknown error".to_string())
            )));
        }

        let cleaned_content = clean_json_content(&response.content);
        let parsed: AnswerEvaluationOutput = serde_json::from_str(&cleaned_content)
            .map_err(|e| AppError::JsonError(format!("Failed to parse evaluation JSON: {}", e)))?;

        // An unknown grade is a structured evaluation error, never a default.
        let grade = parsed.parsed_grade()?;

        let misconception = parsed
            .misconception
            .filter(|m| !m.trim().is_empty() && !m.trim().eq_ignore_ascii_case("null"));

        Ok(crate::materials::assessment::AssessmentEvaluation {
            grade,
            rationale: parsed.rationale,
            misconception,
        })
    }

    /// Regenerates fresh assessment questions for a set of already-studied
    /// concepts, without re-deriving the concepts themselves. Used for
    /// reassessments (same concepts, new question text) and for building
    /// focused remediation sets on weak concepts.
    /// Regenerates fresh assessment questions for a set of already-studied
    /// concepts, without re-deriving the concepts themselves. Used for
    /// reassessments (same concepts, new question text) and for building
    /// focused remediation sets on weak concepts.
    ///
    /// The model is shown real source material for the concepts (via a bounded
    /// vector search) and is told to AVOID the already-existing question
    /// phrasings, so the result is genuinely new wording over the same concepts.
    pub async fn reassess_concepts(
        &self,
        state: &AppState,
        source_material_ids: &[String],
        concepts: Vec<String>,
        exclude_prompts: Vec<String>,
        target_questions: u32,
        run_context: String,
        language: Option<String>,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<crate::materials::assessment::NewAssessmentQuestion>> {
        if concepts.is_empty() || target_questions == 0 {
            return Ok(Vec::new());
        }

        // Pull real source context for the targeted concepts so the regenerated
        // questions are grounded in what the student actually studied (and the
        // stored expected content is meaningful). Bounded to keep cost down.
        let concept_extractions = self
            .fetch_concept_contexts_for_reassessment(
                state,
                source_material_ids,
                &concepts,
                run_context.as_str(),
            )
            .await
            .unwrap_or_else(|err| {
                eprintln!(
                    "Reassessment context lookup failed, falling back to run-context only: {}",
                    err
                );
                Vec::new()
            });

        // Fall back to a run-context-only context if the search returned nothing,
        // so generation can still proceed (the model then relies on the concept
        // names plus the run context).
        let concept_extractions = if concept_extractions.is_empty() {
            concepts
                .iter()
                .enumerate()
                .map(|(i, name)| ConceptExtraction {
                    concept: name.clone(),
                    context: format!(
                        "Concept '{}' from the student's studied material. {}",
                        name, run_context
                    ),
                    chunk_id: i,
                })
                .collect()
        } else {
            concept_extractions
        };

        let language = language.unwrap_or_else(|| "English".to_string());

        self.create_assessment_questions_from_concepts_internal(
            concept_extractions,
            target_questions,
            language,
            run_context,
            agent_settings,
            exclude_prompts,
        )
        .await
    }

    /// Gathers real source material for a set of concept names using a bounded
    /// vector search scoped to the assessment's source materials. The query is
    /// built from the concept names so the results are on-topic for the
    /// reassessment/remediation target.
    async fn fetch_concept_contexts_for_reassessment(
        &self,
        state: &AppState,
        source_material_ids: &[String],
        concepts: &[String],
        run_context: &str,
    ) -> AppResult<Vec<ConceptExtraction>> {
        let query = format!("{} {}", concepts.join(" "), run_context)
            .trim()
            .to_string();
        if query.is_empty() || source_material_ids.is_empty() {
            return Ok(Vec::new());
        }

        let vector = state.vector.lock().await;
        let chunks = vector
            .search_material_specific(&query, source_material_ids, 8)
            .await
            .map_err(|e| AppError::VectorError(format!("Reassessment search failed: {}", e)))?;

        Ok(chunks
            .into_iter()
            .filter(|c| !c.text.trim().is_empty())
            .map(|c| ConceptExtraction {
                concept: concepts[0].clone(),
                context: c.text,
                chunk_id: 0,
            })
            .collect())
    }

    /// Create test questions from concepts using smart batching
    async fn create_test_questions_from_concepts(
        &self,
        all_concepts: Vec<ConceptExtraction>,
        target_questions: u32,
        language: String,
        agent_settings: &AgentSettings,
    ) -> AppResult<Vec<GeneratedTestQuestion>> {
        if all_concepts.is_empty() || target_questions == 0 {
            return Ok(Vec::new());
        }

        // Max 3 test questions per request (they're more complex than flashcards)
        let questions_per_batch = std::cmp::min(3, target_questions);
        let total_requests = (target_questions + questions_per_batch - 1) / questions_per_batch;

        println!(
            "Smart batching: {} concepts → {} requests ({} questions per request)",
            all_concepts.len(),
            total_requests,
            questions_per_batch
        );

        // Distribute concepts across requests
        let concept_batches =
            self.distribute_concepts_evenly(all_concepts, total_requests as usize);
        let task_definition =
            self.get_task_definition_from_settings(&AgentType::TestContentCreator, agent_settings);
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::TestContentCreator)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::TestContentCreator.get_prompt_template());

        let mut requests = Vec::new();
        let mut expected_questions_per_request = Vec::new();

        for (batch_id, batch_concepts) in concept_batches.iter().enumerate() {
            if batch_concepts.is_empty() {
                continue;
            }

            let remaining_questions = target_questions - (batch_id as u32 * questions_per_batch);
            let questions_for_this_batch = std::cmp::min(questions_per_batch, remaining_questions);

            if questions_for_this_batch == 0 {
                break;
            }

            let concepts_json = serde_json::to_string(&ConceptList {
                concepts: batch_concepts.clone(),
            })
            .unwrap_or_else(|_| "[]".to_string());

            let formatted_prompt = prompt_template
                .replace("{count}", &questions_for_this_batch.to_string())
                .replace("{concepts}", &concepts_json)
                .replace("{language}", &language);

            let request = GenerationRequest::builder(formatted_prompt)
                .task(&task_definition.name)
                .build();

            requests.push(request);
            expected_questions_per_request.push(questions_for_this_batch);
        }

        println!(
            "Executing {} test question creation requests",
            requests.len()
        );

        let responses = self.manager.batch_generate(requests).await;
        let mut all_questions = Vec::new();
        let mut used_questions = HashSet::new();

        for (batch_id, response) in responses.iter().enumerate() {
            let expected_count = expected_questions_per_request.get(batch_id).unwrap_or(&0);

            if response.success {
                let cleaned_content = &clean_json_content(&response.content);
                match serde_json::from_str::<TestQuestionBatch>(cleaned_content) {
                    Ok(batch_result) => {
                        let mut added_count = 0;

                        for mut question in batch_result.test_questions {
                            // Duplicate checking
                            let question_key = question.question.to_lowercase();
                            if !used_questions.contains(&question_key)
                                && all_questions.len() < target_questions as usize
                            {
                                used_questions.insert(question_key);

                                // Simply assign positions 0, 1, 2, 3... to all answers
                                for (idx, answer) in question.answers.iter_mut().enumerate() {
                                    answer.position = idx as u32;
                                }

                                all_questions.push(question);
                                added_count += 1;
                            }
                        }

                        println!(
                            "Batch {}: Created {} test questions (expected {})",
                            batch_id + 1,
                            added_count,
                            expected_count
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to parse test questions for batch {}: {}",
                            batch_id, e
                        );
                        // Create fallback questions
                        if let Some(batch_concepts) = concept_batches.get(batch_id) {
                            for (i, concept) in batch_concepts.iter().enumerate() {
                                if all_questions.len() >= target_questions as usize {
                                    break;
                                }
                                if i >= *expected_count as usize {
                                    break;
                                }

                                all_questions.push(GeneratedTestQuestion {
                                    question: format!(
                                        "Test question generation error for concept: {}?",
                                        concept.concept
                                    ),
                                    answers: vec![TestAnswerGenerated {
                                        answer_text: "Generation failed".to_string(),
                                        is_correct: true,
                                        position: 0,
                                    }],
                                });
                            }
                        }
                    }
                }
            } else {
                eprintln!(
                    "Test question creation failed for batch {}: {}",
                    batch_id,
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                );
            }
        }

        // Trim to exact target if we somehow got more
        all_questions.truncate(target_questions as usize);

        println!(
            "Final result: {} unique test questions created (target: {})",
            all_questions.len(),
            target_questions
        );

        Ok(all_questions)
    }

    async fn execute_flashcard_explanation(
        &self,
        question: &str,
        answer: &str,
        agent_settings: &AgentSettings,
    ) -> AppResult<String> {
        let task_definition =
            self.get_task_definition_from_settings(&AgentType::ExplanationAgent, agent_settings);
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::ExplanationAgent)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::ExplanationAgent.get_prompt_template());

        let formatted_prompt = prompt_template
            .replace("{question}", question)
            .replace("{answer}", answer);

        let request = GenerationRequest::builder(formatted_prompt)
            .task(&task_definition.name)
            .build();

        let responses = self.manager.batch_generate(vec![request]).await;

        if let Some(response) = responses.first() {
            if response.success {
                Ok(response.content.clone())
            } else {
                Err(AppError::LlmError(format!(
                    "Explanation generation failed: {}",
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                )))
            }
        } else {
            Err(AppError::LlmError(
                "No response received for explanation generation".to_string(),
            ))
        }
    }

    async fn execute_knowledge_search_with_metadata(
        &self,
        query: &str,
        document_chunks: &[(String, Option<String>, Option<f32>)],
        agent_settings: &AgentSettings,
    ) -> AppResult<String> {
        let task_definition =
            self.get_task_definition_from_settings(&AgentType::SearchAgent, agent_settings);
        let prompt_template = agent_settings
            .agent_prompts
            .get(&AgentType::SearchAgent)
            .map(|s| s.as_str())
            .unwrap_or(AgentType::SearchAgent.get_prompt_template());

        let formatted_chunks = document_chunks
            .iter()
            .enumerate()
            .map(|(i, (content, source, score))| {
                let mut chunk_header = String::new();
                if let Some(src) = source {
                    chunk_header.push_str(&format!(" (Source: {})", src));
                }
                format!("{}:\n{}", chunk_header, content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let formatted_prompt = prompt_template
            .replace("{query}", query)
            .replace("{document_chunks}", &formatted_chunks);

        let request = GenerationRequest::builder(formatted_prompt)
            .task(&task_definition.name)
            .build();

        let responses = self.manager.batch_generate(vec![request]).await;

        if let Some(response) = responses.first() {
            if response.success {
                Ok(response.content.clone())
            } else {
                Err(AppError::LlmError(format!(
                    "Knowledge search failed: {}",
                    response
                        .error
                        .as_ref()
                        .unwrap_or(&"Unknown error".to_string())
                )))
            }
        } else {
            Err(AppError::LlmError(
                "No response received for knowledge search".to_string(),
            ))
        }
    }

    /// Distribute concepts evenly across batches to minimize overlap
    fn distribute_concepts_evenly(
        &self,
        concepts: Vec<ConceptExtraction>,
        num_batches: usize,
    ) -> Vec<Vec<ConceptExtraction>> {
        if num_batches == 0 || concepts.is_empty() {
            return vec![];
        }

        let mut batches: Vec<Vec<ConceptExtraction>> = vec![Vec::new(); num_batches];

        for (i, concept) in concepts.into_iter().enumerate() {
            let batch_index = i % num_batches;
            batches[batch_index].push(concept);
        }

        batches
            .into_iter()
            .filter(|batch| !batch.is_empty())
            .collect()
    }

    /// Calculate the number of flashcard creation requests needed
    fn calculate_flashcard_requests(&self, target_flashcards: u32) -> usize {
        let flashcards_per_request = 5; // Max flashcards per request
        ((target_flashcards + flashcards_per_request - 1) / flashcards_per_request) as usize
    }

    /// Helper method to create TaskDefinition from agent settings
    fn get_task_definition_from_settings(
        &self,
        agent_type: &AgentType,
        agent_settings: &AgentSettings,
    ) -> TaskDefinition {
        let max_tokens = agent_settings
            .agent_max_tokens
            .get(agent_type)
            .copied()
            .unwrap_or_else(|| {
                agent_type
                    .get_task_definition()
                    .parameters
                    .get("max_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3000) as u32
            });

        let temperature = agent_settings
            .agent_temperature
            .get(agent_type)
            .copied()
            .unwrap_or_else(|| {
                agent_type
                    .get_task_definition()
                    .parameters
                    .get("temperature")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.3) as f32
            });

        TaskDefinition::new(&format!("{} Task", agent_type.as_str()))
            .with_max_tokens(max_tokens)
            .with_temperature(temperature)
    }
}

async fn build_llm_manager_from_config(
    config: &LlmUserConfig,
    agent_settings: &AgentSettings,
) -> LlmManager {
    let mut builder = LlmManager::builder();

    // Create keystore instance
    let keystore = ApiKeystore::new();

    // Define tasks using agent settings
    for agent_type in AgentType::all() {
        let max_tokens = agent_settings
            .agent_max_tokens
            .get(&agent_type)
            .copied()
            .unwrap_or_else(|| {
                agent_type
                    .get_task_definition()
                    .parameters
                    .get("max_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3000) as u32
            });

        let temperature = agent_settings
            .agent_temperature
            .get(&agent_type)
            .copied()
            .unwrap_or_else(|| {
                agent_type
                    .get_task_definition()
                    .parameters
                    .get("temperature")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.3) as f32
            });

        let task_def = TaskDefinition::new(&format!("{} Task", agent_type.as_str()))
            .with_max_tokens(max_tokens)
            .with_temperature(temperature);

        builder = builder.define_task(task_def);
    }

    let mut provider_count = 0;

    let ollama_endpoint = config
        .get_ollama_endpoint()
        .ok()
        .flatten()
        .unwrap_or_else(|| constants::OLLAMA_CUSTOM_ENDPOINT.to_string());

    // For each enabled provider, create instances for each agent type that has a model configured
    for (provider_type, provider_config) in &config.provider_configs {
        if !provider_config.enabled {
            continue;
        }

        // Get API key from keystore
        let api_key = if *provider_type == ProviderType::Ollama {
            String::new()
        } else {
            let provider_str = format!("{:?}", provider_type);
            match keystore.get_api_key(&provider_str) {
                Ok(Some(key)) => key,
                Ok(None) => {
                    eprintln!(
                        "Skipping provider {:?}: API key required but not found in keystore",
                        provider_type
                    );
                    continue;
                }
                Err(e) => {
                    eprintln!(
                        "Skipping provider {:?}: Failed to retrieve API key from keystore: {}",
                        provider_type, e
                    );
                    continue;
                }
            }
        };

        // Create a separate instance for each agent type that has a model configured for this provider
        for agent_type in AgentType::all() {
            if let Some(agent_models) = agent_settings.agent_models_per_provider.get(&agent_type) {
                if let Some(model_name) = agent_models.get(provider_type) {
                    let task_name = format!("{} Task", agent_type.as_str());

                    println!(
                        "Adding provider {:?} with model '{}' for task '{}'",
                        provider_type, model_name, task_name
                    );

                    builder = match provider_type {
                        ProviderType::Ollama => builder
                            .add_instance(*provider_type, model_name, "")
                            .supports(&task_name)
                            .custom_endpoint(&ollama_endpoint),
                        _ => builder
                            .add_instance(*provider_type, model_name, &api_key)
                            .supports(&task_name),
                    };

                    provider_count += 1;
                }
            }
        }
    }

    match builder.build().await {
        Ok(manager) => {
            println!(
                "LLM Manager successfully built with {} provider instances",
                provider_count
            );
            manager
        }
        Err(e) => {
            eprintln!(
                "Failed to build LLM Manager: {}. Creating empty manager.",
                e
            );
            LlmManager::new()
        }
    }
}

fn clean_json_content(content: &str) -> String {
    // Only the JSON code-fence wrapper is removed. Content inside string values
    // (rationale, misconception, expected content) is preserved verbatim, so
    // legitimate characters such as '*', '/', or 'x' are never destroyed.
    let fenced = strip_code_fence(content);
    sanitize_json_strings(&fenced)
}

/// Resolves the source-grounded reference content for a generated question.
///
/// Preference order: (1) the exact chunk the model cited via `source_chunk`,
/// (2) the chunk belonging to the question's targeted concept, (3) the first
/// chunk in the batch. This guarantees every stored question carries non-empty
/// expected content drawn from the studied material.
fn resolve_expected_content(
    source_chunk: Option<&str>,
    concept: &str,
    batch_concepts: &[ConceptExtraction],
) -> String {
    if let Some(chunk) = source_chunk {
        let trimmed = chunk.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(by_concept) = batch_concepts.iter().find(|c| {
        !c.concept.trim().is_empty()
            && !c.context.trim().is_empty()
            && c.concept.trim().eq_ignore_ascii_case(concept.trim())
    }) {
        return by_concept.context.trim().to_string();
    }
    batch_concepts
        .iter()
        .find(|c| !c.context.trim().is_empty())
        .map(|c| c.context.trim().to_string())
        .unwrap_or_default()
}

/// Removes a surrounding ``` / ```json code fence, if present. This is the only
/// text transformation applied before structured responses are parsed: the
/// model is instructed to emit bare JSON, and a stray fence is the sole
/// wrapper that needs to be stripped. Inner string content is left untouched.
fn strip_code_fence(content: &str) -> String {
    let trimmed = content.trim();
    let without_prefix = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    without_prefix
        .strip_suffix("```")
        .unwrap_or(without_prefix)
        .trim()
        .to_string()
}

fn sanitize_json_strings(json_str: &str) -> String {
    let mut result = String::new();
    let mut chars = json_str.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;

    while let Some(ch) = chars.next() {
        if escape_next {
            result.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '"' => {
                in_string = !in_string;
                result.push(ch);
            }
            '\\' if in_string => {
                if let Some(&next_ch) = chars.peek() {
                    if matches!(next_ch, 'n' | 'r' | 't' | '"' | '\\' | '/') {
                        result.push(ch);
                    } else {
                        result.push(ch);
                    }
                } else {
                    result.push(ch);
                }
            }
            '\n' if in_string => {
                result.push_str("\\n");
            }
            '\r' if in_string => {
                result.push_str("\\r");
            }
            '\t' if in_string => {
                result.push_str("\\t");
            }
            _ => {
                result.push(ch);
            }
        }
    }

    result
}
