use super::connection::DbService;
use super::queries;
use crate::errors::{AppError, AppResult};
use crate::materials::language_learning::{
    CardTask, FlashcardLanguage, LearningDirection, VocabularyItem,
};
use rusqlite::{params, Row, Transaction};

#[derive(Debug)]
struct FlashcardLanguageRow {
    card_id: i64,
    direction: String,
    task: String,
    target_expression: String,
    meaning: String,
    pronunciation: Option<String>,
    example: Option<String>,
    example_translation: Option<String>,
    context: Option<String>,
    grammatical_information: Option<String>,
}

impl FlashcardLanguageRow {
    fn from_row(row: &Row) -> Result<Self, rusqlite::Error> {
        Ok(FlashcardLanguageRow {
            card_id: row.get("card_id")?,
            direction: row.get("direction")?,
            task: row.get("task")?,
            target_expression: row.get("target_expression")?,
            meaning: row.get("meaning")?,
            pronunciation: row.get("pronunciation")?,
            example: row.get("example")?,
            example_translation: row.get("example_translation")?,
            context: row.get("context")?,
            grammatical_information: row.get("grammatical_information")?,
        })
    }

    fn into_flashcard_language(self) -> AppResult<FlashcardLanguage> {
        Ok(FlashcardLanguage {
            card_id: self.card_id,
            direction: LearningDirection::parse(&self.direction).ok_or_else(|| {
                AppError::DbError(format!(
                    "Unknown card direction '{}' persisted in flashcard_language",
                    self.direction
                ))
            })?,
            task: CardTask::parse(&self.task).ok_or_else(|| {
                AppError::DbError(format!(
                    "Unknown card task '{}' persisted in flashcard_language",
                    self.task
                ))
            })?,
            item: VocabularyItem {
                target_expression: self.target_expression,
                meaning: self.meaning,
                pronunciation: self.pronunciation,
                example: self.example,
                example_translation: self.example_translation,
                context: self.context,
                grammatical_information: self.grammatical_information,
            },
        })
    }
}

pub trait FlashcardLanguageOperations {
    /// Insert the language metadata for a card. The caller runs this inside
    /// the same transaction that inserted the flashcard.
    fn insert_flashcard_language_tx(
        &self,
        tx: &Transaction,
        metadata: &FlashcardLanguage,
    ) -> AppResult<()>;
    /// Fetch the language metadata for a single card, if any.
    fn get_flashcard_language(&self, card_id: i64) -> AppResult<Option<FlashcardLanguage>>;
    /// Fetch the language metadata of every card in a deck (cards without
    /// metadata are absent; a card must work with no metadata).
    fn get_flashcard_language_for_deck(&self, deck_id: &str) -> AppResult<Vec<FlashcardLanguage>>;
}

impl FlashcardLanguageOperations for DbService {
    fn insert_flashcard_language_tx(
        &self,
        tx: &Transaction,
        metadata: &FlashcardLanguage,
    ) -> AppResult<()> {
        tx.execute(
            queries::INSERT_FLASHCARD_LANGUAGE,
            params![
                metadata.card_id,
                metadata.direction.as_str(),
                metadata.task.as_str(),
                metadata.item.target_expression,
                metadata.item.meaning,
                metadata.item.pronunciation,
                metadata.item.example,
                metadata.item.example_translation,
                metadata.item.context,
                metadata.item.grammatical_information,
            ],
        )
        .map_err(|e| {
            AppError::DbError(format!("Failed to insert flashcard_language row: {}", e))
        })?;
        Ok(())
    }

    fn get_flashcard_language(&self, card_id: i64) -> AppResult<Option<FlashcardLanguage>> {
        let conn = self.conn.borrow();
        let result = conn.query_row(
            queries::GET_FLASHCARD_LANGUAGE_BY_CARD,
            params![card_id],
            FlashcardLanguageRow::from_row,
        );

        match result {
            Ok(row) => Ok(Some(row.into_flashcard_language()?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::DbError(format!(
                "Failed to query flashcard_language row: {}",
                e
            ))),
        }
    }

    fn get_flashcard_language_for_deck(&self, deck_id: &str) -> AppResult<Vec<FlashcardLanguage>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_FLASHCARD_LANGUAGE_FOR_DECK)
            .map_err(|e| {
                AppError::DbError(format!("Failed to prepare flashcard_language query: {}", e))
            })?;

        let rows = stmt
            .query_map(params![deck_id], FlashcardLanguageRow::from_row)
            .map_err(|e| {
                AppError::DbError(format!("Failed to query flashcard_language rows: {}", e))
            })?;

        let mut result = Vec::new();
        for row in rows {
            let row = row.map_err(|e| {
                AppError::DbError(format!("Failed to read flashcard_language row: {}", e))
            })?;
            result.push(row.into_flashcard_language()?);
        }
        Ok(result)
    }
}
