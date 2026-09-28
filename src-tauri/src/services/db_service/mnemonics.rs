use super::connection::DbService;
use super::queries;
use crate::errors::{AppError, AppResult};
use crate::materials::flashcard_mnemonic::{CardMnemonic, LanguageMnemonic};
use chrono::Utc;
use rusqlite::{params, Row};

#[derive(Debug)]
struct MnemonicRow {
    id: String,
    card_id: i64,
    is_suitable: bool,
    unsuitable_reason: Option<String>,
    mnemonic_text: String,
    language: Option<LanguageMnemonic>,
    image_prompt: Option<String>,
    image_path: Option<String>,
    created_at: String,
    updated_at: String,
}

impl MnemonicRow {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(MnemonicRow {
            id: row.get("id")?,
            card_id: row.get("card_id")?,
            is_suitable: row.get("is_suitable")?,
            unsuitable_reason: row.get("unsuitable_reason")?,
            mnemonic_text: row.get("mnemonic_text")?,
            language: {
                let target: Option<String> = row.get("language_target_expression")?;
                let meaning: Option<String> = row.get("language_meaning")?;
                let transliteration: Option<String> = row.get("language_transliteration")?;
                match (target, meaning, transliteration) {
                    (Some(t), Some(m), Some(tr)) => {
                        Some(LanguageMnemonic {
                            target_expression: t,
                            meaning: m,
                            transliteration: tr,
                        })
                    }
                    _ => None,
                }
            },
            image_prompt: row.get("image_prompt")?,
            image_path: row.get("image_path")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    fn into_mnemonic(self) -> CardMnemonic {
        CardMnemonic {
            id: self.id,
            card_id: self.card_id,
            is_suitable: self.is_suitable,
            unsuitable_reason: self.unsuitable_reason,
            mnemonic_text: self.mnemonic_text,
            language: self.language,
            image_prompt: self.image_prompt,
            image_path: self.image_path,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub trait CardMnemonicOperations {
    /// Upsert the mnemonic for a card (text regeneration). Returns the stored
    /// row. The `image_path` passed in is kept as-is so text regeneration never
    /// touches the attached image.
    fn upsert_card_mnemonic(
        &self,
        card_id: i64,
        is_suitable: bool,
        unsuitable_reason: Option<String>,
        mnemonic_text: String,
        language: Option<LanguageMnemonic>,
        image_prompt: Option<String>,
        image_path: Option<String>,
    ) -> AppResult<CardMnemonic>;

    /// Point a card's mnemonic at a (new) image file. Used by the regeneration
    /// flow after the new file has been written and validated.
    fn update_card_mnemonic_image(&self, card_id: i64, image_path: String) -> AppResult<()>;

    /// Fetch the mnemonic for a card, if any.
    fn get_card_mnemonic(&self, card_id: i64) -> AppResult<Option<CardMnemonic>>;

    /// Fetch mnemonics for all cards in a deck (used to load the editor in
    /// one call).
    fn get_card_mnemonics_for_deck(&self, deck_id: &str) -> AppResult<Vec<CardMnemonic>>;

    /// Fetch the image file paths for mnemonics attached to the cards of a
    /// deck. Used by deletion flows to clean up orphaned image files after the
    /// DB cascade has removed the rows.
    fn get_mnemonic_image_paths_for_deck(&self, deck_id: &str) -> AppResult<Vec<String>>;

    /// Remove a card's mnemonic row (the image file is removed by the caller).
    fn delete_card_mnemonic(&self, card_id: i64) -> AppResult<Option<CardMnemonic>>;
}

impl CardMnemonicOperations for DbService {
    fn upsert_card_mnemonic(
        &self,
        card_id: i64,
        is_suitable: bool,
        unsuitable_reason: Option<String>,
        mnemonic_text: String,
        language: Option<LanguageMnemonic>,
        image_prompt: Option<String>,
        image_path: Option<String>,
    ) -> AppResult<CardMnemonic> {
        let mut conn = self.conn.borrow_mut();

        // The card must exist (FK would reject it anyway; this gives a clearer error).
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM flashcards WHERE id = ?1",
                params![card_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DbError(format!("Failed to check card: {}", e)))?;
        if exists == 0 {
            return Err(AppError::DbError(format!(
                "Flashcard {} does not exist",
                card_id
            )));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let (lt, lm, ltr) = match &language {
            Some(l) => (Some(l.target_expression.clone()), Some(l.meaning.clone()), Some(l.transliteration.clone())),
            None => (None, None, None),
        };

        conn.execute(
            queries::UPSERT_CARD_MNEMONIC,
            params![
                id,
                card_id,
                is_suitable,
                unsuitable_reason,
                mnemonic_text,
                lt,
                lm,
                ltr,
                image_prompt,
                image_path,
                now,
                now
            ],
        )
        .map_err(|e| AppError::DbError(format!("Failed to upsert card mnemonic: {}", e)))?;

        self.get_card_mnemonic(card_id)?
            .ok_or_else(|| AppError::DbError("Mnemonic row missing after upsert".to_string()))
    }

    fn update_card_mnemonic_image(&self, card_id: i64, image_path: String) -> AppResult<()> {
        let conn = self.conn.borrow();
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        conn.execute(
            queries::UPDATE_CARD_MNEMONIC_IMAGE,
            params![image_path, now, card_id],
        )
        .map_err(|e| AppError::DbError(format!("Failed to update mnemonic image: {}", e)))?;
        Ok(())
    }

    fn get_card_mnemonic(&self, card_id: i64) -> AppResult<Option<CardMnemonic>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_CARD_MNEMONIC)
            .map_err(|e| AppError::DbError(format!("Failed to prepare mnemonic query: {}", e)))?;
        let rows = stmt
            .query_map(params![card_id], MnemonicRow::from_row)
            .map_err(|e| AppError::DbError(format!("Failed to query mnemonic: {}", e)))?;
        for row in rows {
            let row = row.map_err(|e| AppError::DbError(format!("Failed to read mnemonic row: {}", e)))?;
            return Ok(Some(row.into_mnemonic()));
        }
        Ok(None)
    }

    fn get_card_mnemonics_for_deck(&self, deck_id: &str) -> AppResult<Vec<CardMnemonic>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_CARD_MNEMONICS_FOR_DECK)
            .map_err(|e| AppError::DbError(format!("Failed to prepare mnemonic query: {}", e)))?;
        let rows = stmt
            .query_map(params![deck_id], MnemonicRow::from_row)
            .map_err(|e| AppError::DbError(format!("Failed to query mnemonics: {}", e)))?;
        let mut result = Vec::new();
        for row in rows {
            let row = row.map_err(|e| AppError::DbError(format!("Failed to read mnemonic row: {}", e)))?;
            result.push(row.into_mnemonic());
        }
        Ok(result)
    }

    fn get_mnemonic_image_paths_for_deck(&self, deck_id: &str) -> AppResult<Vec<String>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_MNEMONIC_IMAGE_PATHS_FOR_DECK)
            .map_err(|e| AppError::DbError(format!("Failed to prepare query: {}", e)))?;
        let rows = stmt
            .query_map(params![deck_id], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::DbError(format!("Failed to query paths: {}", e)))?;
        let mut result = Vec::new();
        for row in rows {
            let path: String =
                row.map_err(|e| AppError::DbError(format!("Failed to read row: {}", e)))?;
            result.push(path);
        }
        Ok(result)
    }

    fn delete_card_mnemonic(&self, card_id: i64) -> AppResult<Option<CardMnemonic>> {
        let existing = self.get_card_mnemonic(card_id)?;
        let conn = self.conn.borrow();
        conn.execute(
            queries::DELETE_CARD_MNEMONIC,
            params![card_id],
        )
        .map_err(|e| AppError::DbError(format!("Failed to delete mnemonic: {}", e)))?;
        Ok(existing)
    }
}
