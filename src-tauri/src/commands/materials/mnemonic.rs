//! Tauri commands for flashcard mnemonics (features 31–36).
//!
//! Two independent capabilities:
//! - **Text mnemonics** (`generate_card_mnemonic`): LLM-generated, always
//!   available when any LLM provider is configured.
//! - **Mnemonic images** (`generate_card_mnemonic_image`): opt-in, requires
//!   explicit configuration in `AgentSettings.image_generation`. Fails with a
//!   clear capability error when disabled/unconfigured — never falls back to
//!   a mock.
//!
//! Image regeneration is logically transactional: the new image file is written
//! first, the DB row is updated and committed, and only then is the old file
//! removed. Any failure before commit leaves the previously attached image
//! intact.

use crate::config::app_settings::AgentSettings;
use crate::config::keystore::ApiKeystore;
use crate::config::paths::APP_PATHS;
use crate::errors::{AppError, AppResult};
use crate::materials::flashcard_mnemonic::{read_mnemonic_image, CardMnemonic, LanguageMnemonic};
use crate::services::db_service::{CardMnemonicOperations, FlashcardDeckOperations};
use crate::services::image_generation::{build_provider, prompt::build_image_prompt};
use crate::services::llm_service::manager::MnemonicGenerationOutput;
use crate::state::AppState;
use std::fs;
use std::path::PathBuf;
use tauri::State;
use uuid::Uuid;

#[derive(serde::Serialize, Clone)]
pub struct CardMnemonicResponse {
    pub card_id: i64,
    pub is_suitable: bool,
    pub unsuitable_reason: Option<String>,
    pub mnemonic_text: String,
    pub language: Option<LanguageMnemonic>,
    pub image_prompt: Option<String>,
    pub image_path: Option<String>,
}

fn to_response(m: &CardMnemonic) -> CardMnemonicResponse {
    CardMnemonicResponse {
        card_id: m.card_id,
        is_suitable: m.is_suitable,
        unsuitable_reason: m.unsuitable_reason.clone(),
        mnemonic_text: m.mnemonic_text.clone(),
        language: m.language.clone(),
        image_prompt: m.image_prompt.clone(),
        image_path: m.image_path.clone(),
    }
}

/// Generate (or regenerate) the text mnemonic for a flashcard.
///
/// `language_mode` is `"on"` for language-pair cards (target expression +
/// meaning), `"off"` otherwise. Existing image attachment is preserved.
#[tauri::command]
pub async fn generate_card_mnemonic_cmd2(
    state: State<'_, AppState>,
    card_id: i64,
    fact: String,
    kind: Option<String>,
    language_mode: Option<String>,
    learner_profile: Option<String>,
) -> AppResult<CardMnemonicResponse> {
    let kind = kind.unwrap_or_else(|| "auto".to_string());
    let language_mode = language_mode.unwrap_or_else(|| "off".to_string());

    // Verify the card exists and capture its deck (also validates the id).
    let db = state.db.lock().await;
    let card = db
        .get_flashcard(card_id)
        .map_err(|e| e)?
        .ok_or_else(|| AppError::NotFound(format!("Flashcard {} not found", card_id)))?;

    // Preserve the existing image path — text regeneration never touches it.
    let existing_mnemonic = db.get_card_mnemonic(card_id)?;
    let image_path = existing_mnemonic
        .as_ref()
        .and_then(|m| m.image_path.clone());

    drop(db); // release before the (potentially slow) LLM call

    let llm = state.llm.lock().await;
    let agent_settings = state.agent_settings.lock().await;

    let out: MnemonicGenerationOutput = llm
        .generate_card_mnemonic(
            &fact,
            &kind,
            &language_mode,
            learner_profile.as_deref(),
            &agent_settings,
        )
        .await?;
    let is_suitable = out.is_suitable;
    let reason = out.reason.clone();
    let mnemonic = out.mnemonic.clone();
    let image_prompt = out.image_prompt.clone();
    let language = out.language.clone();

    drop(llm);
    drop(agent_settings);

    let db = state.db.lock().await;
    let stored = db.upsert_card_mnemonic(
        card_id,
        is_suitable,
        reason,
        if is_suitable { mnemonic } else { String::new() },
        if is_suitable { language } else { None },
        if is_suitable { image_prompt } else { None },
        image_path,
    )?;

    let _ = &card; // deck captured for future use (e.g. context)
    Ok(to_response(&stored))
}

/// Generate the mnemonic image for a flashcard.
///
/// Opt-in: fails with a clear capability/configuration error when image
/// generation is disabled or unconfigured (no mock provider exists).
#[tauri::command]
pub async fn generate_card_mnemonic_image_cmd2(
    state: State<'_, AppState>,
    card_id: i64,
) -> AppResult<CardMnemonicResponse> {
    let db = state.db.lock().await;
    let mnemonic = db.get_card_mnemonic(card_id)?.ok_or_else(|| {
        AppError::InvalidInput(
            "No mnemonic exists for this card yet. Generate the text mnemonic first.".to_string(),
        )
    })?;

    if !mnemonic.is_suitable {
        return Err(AppError::InvalidInput(
            "The model judged this fact unsuitable for a mnemonic; no image can be generated for it."
                .to_string(),
        ));
    }

    let scene_prompt = mnemonic.image_prompt.clone();
    let language = mnemonic.language.clone();
    let previous_image_path = mnemonic.image_path.clone();
    drop(db);

    // Resolve provider from configuration (clear error when unavailable).
    let agent_settings = state.agent_settings.lock().await;
    let image_config = agent_settings.image_generation.clone();
    drop(agent_settings);

    let keystore = ApiKeystore::new();
    let provider = build_provider(&image_config, &keystore)?;

    // Write the new image to a fresh temp path BEFORE touching the DB.
    let temp_path =
        APP_PATHS
            .mnemonic_images_dir
            .join(format!("{}_{}.png", card_id, Uuid::new_v4()));
    let final_path = APP_PATHS
        .mnemonic_images_dir
        .join(format!("{}.png", card_id));

    let prompt = build_image_prompt(scene_prompt.as_deref(), language.as_ref());
    // The trait returns a Pin<Box<dyn Future + Send>>, which `.await`
    // auto-derefs correctly.
    provider.generate(&prompt, &temp_path).await?;

    // Validate the file is non-empty before committing anything.
    let meta = fs::metadata(&temp_path).map_err(|e| AppError::IoError(e))?;
    if meta.len() == 0 {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::ApiError(
            "Image provider produced an empty file".to_string(),
        ));
    }

    // Promote temp -> final path (replaces any old final file).
    if let Err(e) = fs::rename(&temp_path, &final_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::IoError(e));
    }

    // Commit the DB change; only then delete any old file (transactional-ish).
    let db = state.db.lock().await;
    db.update_card_mnemonic_image(card_id, final_path.to_string_lossy().to_string())?;
    drop(db);

    // Best-effort cleanup of the previous file (if it was elsewhere).
    if let Some(old) = previous_image_path {
        if PathBuf::from(&old) != final_path {
            let _ = fs::remove_file(&old);
        }
    }

    let db = state.db.lock().await;
    let stored = db
        .get_card_mnemonic(card_id)?
        .ok_or_else(|| AppError::DbError("Mnemonic row missing after image update".to_string()))?;
    Ok(to_response(&stored))
}

/// Fetch the mnemonic for a card (None if none exists).
#[tauri::command]
pub async fn get_card_mnemonic_cmd2(
    state: State<'_, AppState>,
    card_id: i64,
) -> AppResult<Option<CardMnemonicResponse>> {
    let db = state.db.lock().await;
    Ok(db.get_card_mnemonic(card_id)?.map(|m| to_response(&m)))
}

/// Fetch mnemonics for all cards in a deck.
#[tauri::command]
pub async fn get_deck_mnemonics_cmd2(
    state: State<'_, AppState>,
    deck_id: String,
) -> AppResult<Vec<CardMnemonicResponse>> {
    let db = state.db.lock().await;
    Ok(db
        .get_card_mnemonics_for_deck(&deck_id)?
        .iter()
        .map(to_response)
        .collect())
}

/// Delete a card's mnemonic row and its image file.
#[tauri::command]
pub async fn delete_card_mnemonic_cmd2(state: State<'_, AppState>, card_id: i64) -> AppResult<()> {
    let db = state.db.lock().await;
    let removed = db.delete_card_mnemonic(card_id)?;
    drop(db);

    if let Some(m) = removed {
        if let Some(path) = m.image_path {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

/// Remove the image file attached to a card's mnemonic (keeps the text).
#[tauri::command]
pub async fn delete_card_mnemonic_image_cmd2(
    state: State<'_, AppState>,
    card_id: i64,
) -> AppResult<()> {
    let db = state.db.lock().await;
    let m = db.get_card_mnemonic(card_id)?;
    drop(db);

    if let Some(m) = m {
        if let Some(path) = m.image_path {
            fs::remove_file(&path).map_err(|e| AppError::IoError(e))?;
        }
        let db = state.db.lock().await;
        db.update_card_mnemonic_image(card_id, String::new())?;
    }
    Ok(())
}

/// Serve the raw bytes of a mnemonic image (mirrors `get_cover_image_raw`).
#[tauri::command]
pub async fn get_mnemonic_image_raw_cmd2(
    state: State<'_, AppState>,
    card_id: i64,
) -> AppResult<Vec<u8>> {
    let db = state.db.lock().await;
    let m = db.get_card_mnemonic(card_id)?;
    drop(db);

    let m = m.ok_or_else(|| AppError::NotFound(format!("No mnemonic for card {}", card_id)))?;
    let path = m
        .image_path
        .ok_or_else(|| AppError::NotFound(format!("No image for mnemonic of card {}", card_id)))?;
    Ok(read_mnemonic_image(&path)?)
}

/// Expose the current image-generation availability to the UI (feature 35):
/// the button is enabled only when `available` is true.
#[tauri::command]
pub async fn get_image_generation_status_cmd2(
    state: State<'_, AppState>,
) -> AppResult<ImageGenerationStatus> {
    let agent_settings = state.agent_settings.lock().await;
    let config = agent_settings.image_generation.clone();
    drop(agent_settings);

    let keystore = ApiKeystore::new();
    let available = match build_provider(&config, &keystore) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Image generation unavailable: {}", e);
            false
        }
    };

    Ok(ImageGenerationStatus {
        enabled: config.enabled,
        available,
        model: config.model.clone(),
        size: config.size.clone(),
    })
}

#[derive(serde::Serialize, Clone)]
pub struct ImageGenerationStatus {
    pub enabled: bool,
    pub available: bool,
    pub model: String,
    pub size: String,
}
