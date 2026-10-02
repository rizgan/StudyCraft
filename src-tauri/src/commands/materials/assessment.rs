use crate::materials::assessment::{
    AssessmentAnswer, AssessmentAttempt, AssessmentConceptResult, AssessmentConceptStatus,
    AssessmentDifficulty, AssessmentEvaluation, AssessmentGrade, AssessmentQuestion,
    AssessmentReport, AssessmentSessionQuestion, NewAssessmentQuestion, RemediationTarget,
};
use crate::services::db_service::{AssessmentOperations, StudyMaterialOperations};
use crate::services::llm_service::manager::AssessmentGenerationRequest;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tauri::State;

/// Request payload for generating a new assessment from studied materials.
#[derive(Debug, Deserialize)]
pub struct AssessmentGenerationCommand {
    pub materials: Vec<crate::services::llm_service::manager::MaterialGenerationRequest>,
    pub language: Option<String>,
    /// What to assess: for an initial assessment this is the topic/concept
    /// focus; for remediation it describes the weak concepts to practice.
    pub run_context: String,
}

/// Request payload for creating (persisting) an assessment.
#[derive(Debug, Deserialize)]
pub struct CreateAssessmentCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub questions: Vec<NewAssessmentQuestion>,
    /// Study materials the assessment was generated from. Stored so that
    /// reassessment/remediation can re-ground new questions in the same
    /// source content.
    #[serde(default)]
    pub source_material_ids: Vec<String>,
}

/// Request payload for submitting one free-text answer.
#[derive(Debug, Deserialize)]
pub struct SubmitAssessmentAnswerCommand {
    pub session_id: String,
    pub question_id: i64,
    pub answer: String,
}

/// Request payload for starting a focused remediation session on weak concepts.
#[derive(Debug, Deserialize)]
pub struct StartRemediationCommand {
    pub session_id: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AssessmentCreationResponse {
    pub review_material_id: String,
    pub review_material_name: String,
    pub questions_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionStartResponse {
    pub session_id: String,
    /// The immutable session-question snapshot this attempt is taken from.
    /// The frontend submits answers against these snapshot ids.
    pub questions: Vec<AssessmentSessionQuestion>,
    /// `true` when an existing pending session was resumed rather than a new
    /// attempt started.
    pub is_resumed: bool,
    /// `true` when this attempt is a fresh reassessment (new question wording).
    pub is_reassessment: bool,
    /// `true` when the returned session is already completed: the attempt is
    /// opened read-only for review and no new session was created.
    pub is_completed: bool,
}

#[derive(Debug, Serialize)]
pub struct SubmitAnswerResponse {
    pub evaluation: AssessmentEvaluation,
}

/// Generates transfer questions for an assessment from the given materials.
/// The caller decides whether to persist them with [`create_assessment`].
#[tauri::command]
pub async fn generate_assessment_questions(
    app_state: State<'_, AppState>,
    request: AssessmentGenerationCommand,
) -> Result<Vec<NewAssessmentQuestion>, String> {
    let llm_service = app_state.llm.lock().await;

    let generation_request = AssessmentGenerationRequest {
        materials: request.materials,
        language: request.language,
        run_context: request.run_context,
    };

    llm_service
        .generate_assessment_questions_from_materials(&app_state, generation_request)
        .await
        .map_err(|e| format!("Failed to generate assessment questions: {}", e))
}

/// Persists a new assessment (review material + questions) in one transaction.
#[tauri::command]
pub async fn create_assessment(
    app_state: State<'_, AppState>,
    request: CreateAssessmentCommand,
) -> Result<AssessmentCreationResponse, String> {
    if request.name.trim().is_empty() {
        return Err("Assessment name is required".to_string());
    }
    if request.questions.is_empty() {
        return Err("Assessment must have at least one question".to_string());
    }
    for (i, q) in request.questions.iter().enumerate() {
        if q.prompt.trim().is_empty() {
            return Err(format!("Question {} has no text", i + 1));
        }
        // The reference content is what the answer is graded against, so it
        // must be present for the assessment to be usable.
        if q.expected_content.trim().is_empty() {
            return Err(format!(
                "Question {} is missing its reference content; generation did not ground it in the studied material",
                i + 1
            ));
        }
    }

    let db_service = app_state.db.lock().await;

    let material = db_service
        .create_assessment(
            &request.id,
            &request.name,
            &request.tags,
            &request.questions,
        )
        .map_err(|e| format!("Failed to create assessment: {}", e))?;

    Ok(AssessmentCreationResponse {
        review_material_id: material.id,
        review_material_name: material.display_name,
        questions_count: request.questions.len(),
    })
}

/// Lists the stored questions of an assessment, ordered by position.
#[tauri::command]
pub async fn get_assessment_questions(
    app_state: State<'_, AppState>,
    assessment_id: String,
) -> Result<Vec<AssessmentQuestion>, String> {
    let db_service = app_state.db.lock().await;

    db_service
        .get_assessment_questions(&assessment_id)
        .map_err(|e| format!("Failed to get assessment questions: {}", e))
}

/// Deletes an assessment and, through foreign-key cascades, its questions,
/// sessions and answers. This is the correct deletion path for assessment
/// materials (the flashcard/test delete commands must not be used).
#[tauri::command]
pub async fn delete_assessment(
    app_state: State<'_, AppState>,
    assessment_id: String,
) -> Result<(), String> {
    let db_service = app_state.db.lock().await;

    db_service
        .delete_assessment(&assessment_id)
        .map_err(|e| format!("Failed to delete assessment: {}", e))
}

/// Opens an assessment for taking, resuming or review. This command is
/// deliberately read-only with respect to the canonical question set and to
/// any existing completed attempt:
///
/// - If a **pending** (active) session exists, it is resumed using its own
///   immutable question snapshot (no regeneration, no mutation).
/// - Otherwise, if a **completed** session exists, the most recent completed
///   attempt is returned read-only for review. Opening a completed
///   assessment never creates a new session, never regenerates questions and
///   never mutates the completed attempt or its report.
/// - Only when the assessment has never been taken (or `force_new` is set
///   for an explicit retake) is a **new** session created, snapshotting the
///   assessment's current canonical questions.
///
/// Regenerating questions is a separate, explicit action
/// ([`reassess_assessment`]) and must never be triggered merely by reopening
/// an assessment.
#[tauri::command]
pub async fn start_assessment_session(
    app_state: State<'_, AppState>,
    assessment_id: String,
    // Explicit retake: start a fresh session from the current canonical
    // question set even when an existing completed attempt could be reviewed.
    force_new: Option<bool>,
) -> Result<SessionStartResponse, String> {
    let db_service = app_state.db.lock().await;

    // Normal open: attach to an existing attempt without creating anything.
    if !force_new.unwrap_or(false) {
        if let Some((session_id, status)) = db_service
            .resolve_assessment_session_to_open(&assessment_id)
            .map_err(|e| format!("Failed to resolve assessment session: {}", e))?
        {
            let questions = db_service
                .get_assessment_session_questions(&session_id)
                .map_err(|e| format!("Failed to load session questions: {}", e))?;
            return Ok(SessionStartResponse {
                is_resumed: status == "active",
                is_completed: status == "completed",
                is_reassessment: false,
                session_id,
                questions,
            });
        }
    }

    // No attachable attempt (or an explicit retake was requested): start a
    // fresh one from the current canonical set.
    let questions = db_service
        .get_assessment_questions(&assessment_id)
        .map_err(|e| format!("Failed to load assessment questions: {}", e))?;
    if questions.is_empty() {
        return Err("This assessment has no questions to take".to_string());
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    db_service
        .create_assessment_session_with_questions(&session_id, &assessment_id, &questions)
        .map_err(|e| format!("Failed to create assessment session: {}", e))?;

    let session_questions = db_service
        .get_assessment_session_questions(&session_id)
        .map_err(|e| format!("Failed to load session questions: {}", e))?;

    Ok(SessionStartResponse {
        session_id,
        questions: session_questions,
        is_resumed: false,
        is_reassessment: false,
        is_completed: false,
    })
}

/// Regenerates fresh question text for an assessment's existing concepts so a
/// reassessment does not replay the previous prompts, stores the new wording
/// as the canonical question set, and opens a new pending session snapshotted
/// from the new set.
///
/// Previous attempts are left completely untouched: their immutable
/// `assessment_session_questions` snapshot and answers remain intact and are
/// the only things that would have been destroyed by the old cascade-on-replace.
async fn regenerate_reassessment_questions(
    app_state: &AppState,
    assessment_id: &str,
) -> Result<String, String> {
    let db_service = app_state.db.lock().await;

    let previous_questions = db_service
        .get_assessment_questions(assessment_id)
        .map_err(|e| format!("Failed to get previous questions: {}", e))?;

    if previous_questions.is_empty() {
        return Err("Assessment has no questions to re-assess".to_string());
    }

    let concepts: Vec<String> = previous_questions
        .iter()
        .map(|q| q.concept.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let target = previous_questions.len().max(1) as u32;
    let exclude_prompts: Vec<String> = previous_questions
        .iter()
        .map(|q| q.prompt.clone())
        .collect();

    let run_context = "Reassessment: generate NEW question wording for the same concepts the student previously took. Do not reuse the provided question phrasings.".to_string();

    let source_material_ids = resolve_source_material_ids(app_state, assessment_id).await?;

    let llm_service = app_state.llm.lock().await;
    let agent_settings = app_state.agent_settings.lock().await.clone();

    let new_questions = llm_service
        .reassess_concepts(
            app_state,
            &source_material_ids,
            concepts,
            exclude_prompts,
            target,
            run_context,
            Some("English".to_string()),
            &agent_settings,
        )
        .await
        .map_err(|e| format!("Failed to regenerate reassessment questions: {}", e))?;

    if new_questions.is_empty() {
        return Err(
            "Could not generate any new reassessment questions; please try again or check the LLM provider."
                .to_string(),
        );
    }

    db_service
        .replace_assessment_questions(assessment_id, &new_questions)
        .map_err(|e| format!("Failed to store reassessment questions: {}", e))?;

    // Snapshot the freshly stored canonical set into a new pending session.
    let stored = db_service
        .get_assessment_questions(assessment_id)
        .map_err(|e| format!("Failed to load reassessment questions: {}", e))?;
    let session_id = uuid::Uuid::new_v4().to_string();
    db_service
        .create_assessment_session_with_questions(&session_id, assessment_id, &stored)
        .map_err(|e| format!("Failed to create reassessment session: {}", e))?;

    Ok(session_id)
}

/// Explicitly re-assesses an assessment: regenerates fresh question wording for
/// the same concepts, stores it as the canonical question set, and opens a new
/// pending session for the fresh attempt. Prior attempts are preserved.
/// Returns the new session id.
#[tauri::command]
pub async fn reassess_assessment(
    app_state: State<'_, AppState>,
    assessment_id: String,
) -> Result<String, String> {
    regenerate_reassessment_questions(&app_state, &assessment_id).await
}

/// Resolves the study materials to ground regenerated questions in. An
/// assessment does not persist its own source material list, so this falls
/// back to the materials the student has studied (the ones vector-searchable).
async fn resolve_source_material_ids(
    app_state: &AppState,
    assessment_id: &str,
) -> Result<Vec<String>, String> {
    let _ = assessment_id;
    let db_service = app_state.db.lock().await;
    db_service
        .get_study_materials()
        .map_err(|e| format!("Failed to load study materials: {}", e))
        .map(|materials| {
            materials
                .into_iter()
                .filter(|m| m.is_indexed)
                .map(|m| m.id)
                .collect()
        })
}

/// Submits a free-text answer, evaluates it with the structured LLM evaluator
/// and stores the answer plus its evaluation in the session.
///
/// Answers are idempotent per question: re-submitting overwrites the previous
/// answer (and its evaluation) rather than creating a duplicate.
#[tauri::command]
pub async fn submit_assessment_answer(
    app_state: State<'_, AppState>,
    request: SubmitAssessmentAnswerCommand,
) -> Result<SubmitAnswerResponse, String> {
    if request.answer.trim().is_empty() {
        return Err("Answer text is empty".to_string());
    }

    let db_service = app_state.db.lock().await;

    // Resolve the immutable session-question snapshot the answer belongs to.
    // Grading and history are both anchored to this snapshot, never to the
    // mutable canonical question set.
    let question = db_service
        .get_assessment_session_question(&request.session_id, request.question_id)
        .map_err(|e| format!("Failed to load session question: {}", e))?
        .ok_or_else(|| "Question not found for this session".to_string())?;

    let difficulty_str = String::from(question.difficulty);

    // Evaluate through the same LLM architecture as the other agents.
    let llm_service = app_state.llm.lock().await;
    let agent_settings = app_state.agent_settings.lock().await.clone();

    let evaluation = llm_service
        .evaluate_assessment_answer(
            &question.prompt,
            &question.concept,
            &difficulty_str,
            &question.expected_content,
            &request.answer,
            "English",
            &agent_settings,
        )
        .await
        .map_err(|e| format!("Failed to evaluate answer: {}", e))?;

    db_service
        .insert_assessment_answer(
            &request.session_id,
            request.question_id,
            &request.answer,
            Some(&evaluation),
        )
        .map_err(|e| format!("Failed to store answer: {}", e))?;

    Ok(SubmitAnswerResponse { evaluation })
}

/// Returns all recorded answers of a session.
#[tauri::command]
pub async fn get_assessment_answers(
    app_state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<AssessmentAnswer>, String> {
    let db_service = app_state.db.lock().await;

    db_service
        .get_assessment_answers(&session_id)
        .map_err(|e| format!("Failed to get assessment answers: {}", e))
}

/// Marks an assessment session completed. Idempotent: completing an already
/// completed session is a no-op, and an unknown session is an error.
#[tauri::command]
pub async fn complete_assessment_session(
    app_state: State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    let db_service = app_state.db.lock().await;

    let status = db_service
        .get_assessment_session_status(&session_id)
        .map_err(|e| format!("Failed to check session status: {}", e))?
        .ok_or_else(|| "Assessment session not found".to_string())?;

    if status == "completed" {
        return Ok(true);
    }

    let updated = db_service
        .complete_assessment_session(&session_id)
        .map_err(|e| format!("Failed to complete session: {}", e))?;

    if updated == 0 {
        return Err("Session is no longer active; it may have already been completed".to_string());
    }

    Ok(true)
}

/// Returns the assessment's attempt history (all sessions), most recent first.
/// Each attempt is an immutable unit with its own question snapshot and report.
#[tauri::command]
pub async fn get_assessment_attempts(
    app_state: State<'_, AppState>,
    assessment_id: String,
) -> Result<Vec<AssessmentAttempt>, String> {
    let db_service = app_state.db.lock().await;

    db_service
        .get_assessment_attempts(&assessment_id)
        .map_err(|e| format!("Failed to get assessment attempts: {}", e))
}

/// Aggregates a session's answers into a per-concept report without a single
/// aggregate score: mastered / partially understood / weak buckets plus
/// focused remediation targets.
#[tauri::command]
pub async fn get_assessment_report(
    app_state: State<'_, AppState>,
    session_id: String,
) -> Result<AssessmentReport, String> {
    let db_service = app_state.db.lock().await;

    let answers = db_service
        .get_assessment_answers(&session_id)
        .map_err(|e| format!("Failed to load answers for report: {}", e))?;

    let total_questions = answers.len() as u32;

    let mut by_concept: BTreeMap<String, Vec<AssessmentAnswer>> = BTreeMap::new();
    for answer in answers {
        by_concept
            .entry(answer.concept.clone())
            .or_default()
            .push(answer);
    }

    let mut mastered = Vec::new();
    let mut partially_understood = Vec::new();
    let mut weak = Vec::new();
    let mut remediation_targets = Vec::new();

    for (concept, answers) in by_concept {
        let (result, status) = assess_concept(&concept, &answers);
        match status {
            AssessmentConceptStatus::Mastered => mastered.push(result),
            AssessmentConceptStatus::PartiallyUnderstood => {
                partially_understood.push(result.clone());
                // Partials with identified misconceptions also become a
                // remediation target, but at a higher starting difficulty.
                if !result.misconceptions.is_empty() {
                    remediation_targets.push(RemediationTarget {
                        concept: concept.clone(),
                        status: AssessmentConceptStatus::PartiallyUnderstood,
                        reason: format!(
                            "Partially understood; misconception: {}",
                            result.misconceptions.join("; ")
                        ),
                        misconceptions: result.misconceptions.clone(),
                        suggested_difficulty: AssessmentDifficulty::Understanding,
                    });
                }
            }
            AssessmentConceptStatus::Weak => {
                weak.push(result.clone());
                remediation_targets.push(RemediationTarget {
                    concept: concept.clone(),
                    status: AssessmentConceptStatus::Weak,
                    reason: if result.misconceptions.is_empty() {
                        "Weak or incorrect understanding after the assessment.".to_string()
                    } else {
                        format!(
                            "Weak understanding. Misconception: {}",
                            result.misconceptions.join("; ")
                        )
                    },
                    misconceptions: result.misconceptions.clone(),
                    suggested_difficulty: AssessmentDifficulty::Recall,
                });
            }
        }
    }

    Ok(AssessmentReport {
        session_id,
        total_questions,
        mastered,
        partially_understood,
        weak,
        remediation_targets,
    })
}

/// Starts a focused remediation session from a completed assessment session.
/// Builds new transfer questions targeting only the weak / partially
/// understood concepts (with their reported misconceptions) so the student can
/// practice the exact gaps, then stores them as a new assessment material.
#[tauri::command]
pub async fn start_remediation(
    app_state: State<'_, AppState>,
    request: StartRemediationCommand,
) -> Result<AssessmentCreationResponse, String> {
    let db_service = app_state.db.lock().await;

    // Rebuild the report to identify the weak / partial concepts.
    let answers = db_service
        .get_assessment_answers(&request.session_id)
        .map_err(|e| format!("Failed to load answers: {}", e))?;
    if answers.is_empty() {
        return Err("Session has no answers to remediate".to_string());
    }

    let mut by_concept: BTreeMap<String, Vec<AssessmentAnswer>> = BTreeMap::new();
    for answer in answers {
        by_concept
            .entry(answer.concept.clone())
            .or_default()
            .push(answer);
    }

    let mut target_concepts: Vec<String> = Vec::new();
    let mut misconception_notes: Vec<String> = Vec::new();
    for (concept, answers) in by_concept {
        let (_, status) = assess_concept(&concept, &answers);
        if matches!(
            status,
            AssessmentConceptStatus::Weak | AssessmentConceptStatus::PartiallyUnderstood
        ) {
            target_concepts.push(concept.clone());
            let misconceptions: Vec<String> = answers
                .iter()
                .filter_map(|a| a.evaluation.as_ref().and_then(|e| e.misconception.clone()))
                .filter(|m| !m.trim().is_empty())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            if !misconceptions.is_empty() {
                misconception_notes.push(format!("{}: {}", concept, misconceptions.join("; ")));
            }
        }
    }

    if target_concepts.is_empty() {
        return Err("No weak or partially understood concepts to remediate".to_string());
    }

    let run_context = format!(
        "Focused remediation: rebuild understanding of these concepts: {}. {} Write NEW question wording (do not reuse phrasings the student already saw) that target the specific gaps.",
        target_concepts.join(", "),
        if misconception_notes.is_empty() {
            String::new()
        } else {
            format!("Reported misconceptions to correct: {}", misconception_notes.join(" | "))
        }
    );

    let source_material_ids = resolve_source_material_ids(&app_state, &request.session_id).await?;

    let llm_service = app_state.llm.lock().await;
    let agent_settings = app_state.agent_settings.lock().await.clone();

    let target_questions = std::cmp::min(8, target_concepts.len() as u32 * 2);
    let new_questions = llm_service
        .reassess_concepts(
            &app_state,
            &source_material_ids,
            target_concepts.clone(),
            Vec::new(),
            target_questions,
            run_context,
            request.language,
            &agent_settings,
        )
        .await
        .map_err(|e| format!("Failed to generate remediation questions: {}", e))?;

    if new_questions.is_empty() {
        return Err("Could not generate remediation questions; please try again".to_string());
    }

    let remediation_id = uuid::Uuid::new_v4().to_string();
    let remediation_name = format!("Remediation ({})", target_concepts.join(", "));

    let material = db_service
        .create_assessment(
            &remediation_id,
            &remediation_name,
            &Vec::new(),
            &new_questions,
        )
        .map_err(|e| format!("Failed to create remediation assessment: {}", e))?;

    Ok(AssessmentCreationResponse {
        review_material_id: material.id,
        review_material_name: material.display_name,
        questions_count: new_questions.len(),
    })
}

/// Computes the per-concept result from its answered questions.
fn assess_concept(
    concept: &str,
    answers: &[AssessmentAnswer],
) -> (AssessmentConceptResult, AssessmentConceptStatus) {
    let total = answers.len().max(1);
    let (correct, partial, other) = answers.iter().fold((0, 0, 0), |acc, a| {
        match a.evaluation.as_ref().map(|e| e.grade) {
            Some(AssessmentGrade::Correct) => (acc.0 + 1, acc.1, acc.2),
            Some(AssessmentGrade::PartiallyCorrect) => (acc.0, acc.1 + 1, acc.2),
            Some(_) | None => (acc.0, acc.1, acc.2 + 1),
        }
    });

    let status = if other == 0 && correct == total {
        AssessmentConceptStatus::Mastered
    } else if other == 0 && partial > 0 {
        AssessmentConceptStatus::PartiallyUnderstood
    } else if correct > 0 {
        AssessmentConceptStatus::PartiallyUnderstood
    } else {
        AssessmentConceptStatus::Weak
    };

    // Collect distinct misconceptions reported by the evaluator.
    let misconceptions: Vec<String> = answers
        .iter()
        .filter_map(|a| a.evaluation.as_ref().and_then(|e| e.misconception.clone()))
        .filter(|m| !m.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let rationale = if misconceptions.is_empty() {
        format!(
            "{} answered, {} correct, {} partially correct, {} incorrect/insufficient.",
            total, correct, partial, other
        )
    } else {
        format!(
            "{} answered, {} correct, {} partially correct, {} incorrect/insufficient. Main issue: {}",
            total,
            correct,
            partial,
            other,
            misconceptions.join("; ")
        )
    };

    let result = AssessmentConceptResult {
        concept: concept.to_string(),
        status,
        rationale,
        misconceptions,
    };

    (result, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::assessment::{AssessmentEvaluation, AssessmentGrade};

    fn answer(
        concept: &str,
        grade: AssessmentGrade,
        misconception: Option<&str>,
    ) -> AssessmentAnswer {
        AssessmentAnswer {
            id: None,
            session_id: "session".to_string(),
            question_id: 1,
            question_prompt: "q".to_string(),
            concept: concept.to_string(),
            difficulty: AssessmentDifficulty::Recall,
            student_answer: "a".to_string(),
            evaluation: Some(AssessmentEvaluation {
                grade,
                rationale: "r".to_string(),
                misconception: misconception.map(String::from),
            }),
        }
    }

    #[test]
    fn all_correct_is_mastered() {
        let answers = vec![
            answer("c", AssessmentGrade::Correct, None),
            answer("c", AssessmentGrade::Correct, None),
        ];
        let (_, status) = assess_concept("c", &answers);
        assert_eq!(status, AssessmentConceptStatus::Mastered);
    }

    #[test]
    fn only_partials_is_partially_understood() {
        let answers = vec![
            answer("c", AssessmentGrade::PartiallyCorrect, Some("m")),
            answer("c", AssessmentGrade::Correct, None),
        ];
        let (result, status) = assess_concept("c", &answers);
        assert_eq!(status, AssessmentConceptStatus::PartiallyUnderstood);
        assert!(result.misconceptions.contains(&"m".to_string()));
    }

    #[test]
    fn any_incorrect_is_not_mastered() {
        let answers = vec![
            answer("c", AssessmentGrade::Correct, None),
            answer("c", AssessmentGrade::Incorrect, Some("m")),
        ];
        let (result, status) = assess_concept("c", &answers);
        assert_ne!(status, AssessmentConceptStatus::Mastered);
        assert!(result.rationale.contains("Main issue: m"));
    }

    #[test]
    fn all_wrong_is_weak() {
        let answers = vec![answer("c", AssessmentGrade::Incorrect, None)];
        let (_, status) = assess_concept("c", &answers);
        assert_eq!(status, AssessmentConceptStatus::Weak);
    }
}
