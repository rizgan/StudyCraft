use crate::materials::concept_mastery::{ConceptMastery, WeakConcept};
use crate::services::db_service::concept_mastery::{
    get_mastery_for_concept, get_mastery_for_material, recalculate_all_mastery, weak_concepts,
    MaterialMastery,
};
use crate::state::AppState;
use tauri::State;

/// Mastery of one concept from every source of evidence. The label is
/// normalized to its canonical identity before lookup, so "Precision vs
/// Recall" and "precision vs recall" address the same concept.
#[tauri::command]
pub async fn get_concept_mastery(
    app_state: State<'_, AppState>,
    concept_label: String,
) -> Result<Option<ConceptMastery>, String> {
    let db_service = app_state.db.lock().await;

    get_mastery_for_concept(&db_service, &concept_label)
        .map_err(|e| format!("Failed to compute concept mastery: {}", e))
}

/// Mastery of the concepts one material has evidence for. For an assessment
/// this is the set of concepts its answered questions touched; for a
/// flashcard deck this is the set of concepts its reviewed cards cover.
#[tauri::command]
pub async fn get_material_mastery(
    app_state: State<'_, AppState>,
    material_id: String,
) -> Result<MaterialMastery, String> {
    let db_service = app_state.db.lock().await;

    get_mastery_for_material(&db_service, &material_id)
        .map_err(|e| format!("Failed to compute material mastery: {}", e))
}

/// The weak concepts across all evidence, weakest first. A concept is weak
/// when it has evidence and is limited by a weak, developing or learning
/// dimension. This is the deterministic list of concepts to prioritize for
/// review; no LLM is involved in selecting it.
#[tauri::command]
pub async fn get_weak_concepts(app_state: State<'_, AppState>) -> Result<Vec<WeakConcept>, String> {
    let db_service = app_state.db.lock().await;

    weak_concepts(&db_service).map_err(|e| format!("Failed to compute weak concepts: {}", e))
}

/// Full rebuild of every concept's mastery from its raw evidence. This is the
/// canonical recalculation entry point: mastery is always recomputed from the
/// stored raw evidence and never mutated in place, so a rebuild is idempotent
/// and lossless.
#[tauri::command]
pub async fn recalculate_mastery(
    app_state: State<'_, AppState>,
) -> Result<Vec<ConceptMastery>, String> {
    let db_service = app_state.db.lock().await;

    recalculate_all_mastery(&db_service)
        .map_err(|e| format!("Failed to recalculate mastery: {}", e))
}
