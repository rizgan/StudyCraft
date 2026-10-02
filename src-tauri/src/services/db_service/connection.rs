use super::queries::*;
use crate::config::APP_PATHS;
use crate::errors::{AppError, AppResult};
use rusqlite::Connection;
use std::cell::RefCell;

pub use super::assessments::AssessmentOperations;
pub use super::flashcard_decks::FlashcardDeckOperations;
pub use super::language_metadata::FlashcardLanguageOperations;
pub use super::mnemonics::CardMnemonicOperations;
pub use super::review_materials::ReviewMaterialOperations;
pub use super::review_sessions::ReviewSessionOperations;
pub use super::study_materials::StudyMaterialOperations;
pub use super::tags::TagOperations;
pub use super::tests::TestOperations;

pub struct DbService {
    pub conn: RefCell<Connection>,
}

impl DbService {
    pub fn new() -> AppResult<Self> {
        APP_PATHS
            .ensure_dirs_exist()
            .map_err(|e| AppError::IoError(e))?;
        let db_path = APP_PATHS.config_dir.join("studycraft.db");
        Self::open_at(&db_path)
    }

    /// Opens (and migrates) a database file at an explicit path. Split out so
    /// the platform path selection stays in [Self::new].
    fn open_at(db_path: &std::path::Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::IoError)?;
        }
        let conn = Connection::open(db_path)
            .map_err(|e| AppError::DbError(format!("Failed to open database: {}", e)))?;
        // SQLite disables foreign key enforcement by default per-connection. The
        // schema declares ON DELETE CASCADE on child rows, so we must turn the
        // pragma on here or cascades are silently ignored.
        conn.execute_batch("PRAGMA foreign_keys = ON")
            .map_err(|e| AppError::DbError(format!("Failed to enable foreign keys: {}", e)))?;
        let service = Self {
            conn: RefCell::new(conn),
        };
        service.setup_tables()?;
        Ok(service)
    }

    /// Opens a private database at an explicit path. Intended for integration
    /// tests; harmless in release (dead-code-eliminated when unused).
    #[cfg(any(test, feature = "__integration_tests"))]
    pub fn new_for_tests(db_path: &std::path::Path) -> AppResult<Self> {
        Self::open_at(db_path)
    }

    fn setup_tables(&self) -> AppResult<()> {
        // Create study materials table
        self.conn
            .borrow()
            .execute(CREATE_STUDY_MATERIALS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create study_materials table: {}", e))
            })?;

        // Create review materials table
        self.conn
            .borrow()
            .execute(CREATE_REVIEW_MATERIALS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create review_materials table: {}", e))
            })?;

        // Create flashcards table
        self.conn
            .borrow()
            .execute(CREATE_FLASHCARDS_TABLE, [])
            .map_err(|e| AppError::DbError(format!("Failed to create flashcards table: {}", e)))?;

        // Create flashcard language-learning metadata table (1:1 with flashcards,
        // after flashcards so the FK resolves)
        self.conn
            .borrow()
            .execute(CREATE_FLASHCARD_LANGUAGE_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create flashcard_language table: {}", e))
            })?;

        // Create flashcard mnemonics table (after flashcards so the FK resolves)
        self.conn
            .borrow()
            .execute(CREATE_FLASHCARD_MNEMONICS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create flashcard_mnemonics table: {}", e))
            })?;

        // Create test questions table
        self.conn
            .borrow()
            .execute(CREATE_TEST_QUESTIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create test_questions table: {}", e))
            })?;

        // Create test answers table
        self.conn
            .borrow()
            .execute(CREATE_TEST_ANSWERS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create test_answers table: {}", e))
            })?;

        // Create assessment questions table
        self.conn
            .borrow()
            .execute(CREATE_ASSESSMENT_QUESTIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to create assessment_questions table: {}",
                    e
                ))
            })?;

        // Create assessment sessions table
        self.conn
            .borrow()
            .execute(CREATE_ASSESSMENT_SESSIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create assessment_sessions table: {}", e))
            })?;

        // Create the immutable per-session question snapshot table. Answers
        // reference these snapshot rows rather than the mutable canonical
        // assessment_questions, so reassessment never destroys prior attempts.
        self.conn
            .borrow()
            .execute(CREATE_ASSESSMENT_SESSION_QUESTIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to create assessment_session_questions table: {}",
                    e
                ))
            })?;

        // Create assessment answers table
        self.conn
            .borrow()
            .execute(CREATE_ASSESSMENT_ANSWERS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create assessment_answers table: {}", e))
            })?;

        // Create the canonical concept registry (id is the normalized label).
        self.conn
            .borrow()
            .execute(CREATE_CONCEPTS_TABLE, [])
            .map_err(|e| AppError::DbError(format!("Failed to create concepts table: {}", e)))?;

        // Create the raw per-review flashcard log. Placed after the
        // flashcards table so its FK resolves; a deleted card cascades to its
        // own review rows and nothing else.
        self.conn
            .borrow()
            .execute(CREATE_FLASHCARD_REVIEWS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create flashcard_reviews table: {}", e))
            })?;

        // Create the concept provenance table. Links a shared concept id back
        // to every material that produced it (false separation is preferred
        // over false merging): the concepts table above is the canonical label
        // registry, and this records which materials contributed to it.
        self.conn
            .borrow()
            .execute(CREATE_CONCEPT_MATERIAL_OCCURRENCES_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to create concept_material_occurrences table: {}",
                    e
                ))
            })?;

        // Create tags table
        self.conn
            .borrow()
            .execute(CREATE_TAGS_TABLE, [])
            .map_err(|e| AppError::DbError(format!("Failed to create tags table: {}", e)))?;

        // Create material tags table
        self.conn
            .borrow()
            .execute(CREATE_MATERIAL_TAGS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create material_tags table: {}", e))
            })?;

        // Create general review sessions table
        self.conn
            .borrow()
            .execute(CREATE_REVIEW_SESSIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!("Failed to create review_sessions table: {}", e))
            })?;

        // Create flashcard review sessions table
        self.conn
            .borrow()
            .execute(CREATE_FLASHCARD_REVIEW_SESSIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to create flashcard_review_sessions table: {}",
                    e
                ))
            })?;

        // Create test review sessions table
        self.conn
            .borrow()
            .execute(CREATE_TEST_REVIEW_SESSIONS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to create test_review_sessions table: {}",
                    e
                ))
            })?;

        self.migrate_assessment_schema()?;
        self.migrate_language_learning_schema()?;
        self.migrate_flashcard_reviews_schema()?;

        Ok(())
    }

    /// Migrates databases created before the language-learning feature:
    /// adds the language-learning columns to review_materials. Existing rows
    /// are not language-learning content (no canonical metadata proves
    /// otherwise), so the `is_language_learning` column defaults to 0 and the
    /// language columns stay NULL. New databases already have the full schema
    /// from CREATE TABLE above, making this a no-op there.
    fn migrate_language_learning_schema(&self) -> AppResult<()> {
        let mut conn = self.conn.borrow_mut();
        let mut cols: Vec<String> = conn
            .prepare("PRAGMA table_info(review_materials)")
            .map_err(|e| AppError::DbError(format!("Failed to inspect review_materials: {}", e)))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| {
                AppError::DbError(format!("Failed to read review_materials columns: {}", e))
            })?
            .collect::<Result<_, _>>()
            .map_err(|e| {
                AppError::DbError(format!("Failed to read review_materials columns: {}", e))
            })?;
        if !cols.iter().any(|c| c == "is_language_learning") {
            conn.execute(
                "ALTER TABLE review_materials ADD COLUMN is_language_learning BOOLEAN NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add is_language_learning: {}", e)))?;
            conn.execute(
                "ALTER TABLE review_materials ADD COLUMN learning_language TEXT",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add learning_language: {}", e)))?;
            conn.execute(
                "ALTER TABLE review_materials ADD COLUMN base_language TEXT",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add base_language: {}", e)))?;
        }
        let _ = cols;
        Ok(())
    }

    /// Adds the stable review-event identity to the flashcard review log on
    /// databases created by an earlier build of the concept-mastery feature.
    /// New databases already carry the column from CREATE TABLE, so this is a
    /// no-op there. Legacy review rows (if any) keep a NULL event id; NULLs are
    /// distinct under the UNIQUE constraint, so they never collide.
    fn migrate_flashcard_reviews_schema(&self) -> AppResult<()> {
        let conn = self.conn.borrow();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(flashcard_reviews)")
            .map_err(|e| AppError::DbError(format!("Failed to inspect flashcard_reviews: {}", e)))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| {
                AppError::DbError(format!("Failed to read flashcard_reviews columns: {}", e))
            })?
            .collect::<Result<_, _>>()
            .map_err(|e| {
                AppError::DbError(format!("Failed to read flashcard_reviews columns: {}", e))
            })?;
        let has_event_id = cols.iter().any(|c| c == "review_event_id");
        if !has_event_id {
            drop(conn);
            let mut conn = self.conn.borrow_mut();
            conn.execute(
                "ALTER TABLE flashcard_reviews ADD COLUMN review_event_id TEXT UNIQUE",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add review_event_id: {}", e)))?;
        }
        Ok(())
    }

    /// Backfills columns added to the assessment tables after a database already
    /// existed. New databases already get the full schema from the CREATE TABLE
    /// statements above, so these are no-ops there.
    fn migrate_assessment_schema(&self) -> AppResult<()> {
        let mut conn = self.conn.borrow_mut();
        let mut cols: Vec<String> = conn
            .prepare("PRAGMA table_info(assessment_questions)")
            .map_err(|e| {
                AppError::DbError(format!("Failed to inspect assessment_questions: {}", e))
            })?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to read assessment_questions columns: {}",
                    e
                ))
            })?
            .collect::<Result<_, _>>()
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to read assessment_questions columns: {}",
                    e
                ))
            })?;
        if !cols.iter().any(|c| c == "expected_content") {
            conn.execute(
                "ALTER TABLE assessment_questions ADD COLUMN expected_content TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add expected_content: {}", e)))?;
        }
        let _ = cols;

        let mut session_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(assessment_sessions)")
            .map_err(|e| {
                AppError::DbError(format!("Failed to inspect assessment_sessions: {}", e))
            })?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| {
                AppError::DbError(format!("Failed to read assessment_sessions columns: {}", e))
            })?
            .collect::<Result<_, _>>()
            .map_err(|e| {
                AppError::DbError(format!("Failed to read assessment_sessions columns: {}", e))
            })?;
        if !session_cols.iter().any(|c| c == "status") {
            conn.execute(
                "ALTER TABLE assessment_sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add status: {}", e)))?;
        }
        if !session_cols.iter().any(|c| c == "started_at") {
            // SQLite forbids a non-constant default (CURRENT_TIMESTAMP) on
            // ALTER TABLE ADD COLUMN, so add the column NULL-able and then
            // backfill existing rows from their created_at timestamp.
            conn.execute(
                "ALTER TABLE assessment_sessions ADD COLUMN started_at TIMESTAMP",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add started_at: {}", e)))?;
            conn.execute(
                "UPDATE assessment_sessions SET started_at = created_at WHERE started_at IS NULL",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to backfill started_at: {}", e)))?;
        }
        if !session_cols.iter().any(|c| c == "finished_at") {
            conn.execute(
                "ALTER TABLE assessment_sessions ADD COLUMN finished_at TIMESTAMP",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add finished_at: {}", e)))?;
        }
        if !session_cols.iter().any(|c| c == "completed_at") {
            conn.execute(
                "ALTER TABLE assessment_sessions ADD COLUMN completed_at TIMESTAMP",
                [],
            )
            .map_err(|e| AppError::DbError(format!("Failed to add completed_at: {}", e)))?;
        }

        self.migrate_legacy_assessment_answers(&mut conn)?;

        Ok(())
    }

    /// Repoints the `assessment_answers.question_id` foreign key from the
    /// mutable `assessment_questions` table to the immutable
    /// `assessment_session_questions` snapshot table on existing databases.
    /// New databases already carry the correct FK from the CREATE TABLE
    /// statement, so this is a no-op there.
    fn migrate_legacy_assessment_answers(&self, conn: &mut rusqlite::Connection) -> AppResult<()> {
        // Inspect the current FK target of assessment_answers.question_id.
        let fk_targets: Vec<String> = conn
            .prepare("PRAGMA foreign_key_list(assessment_answers)")
            .map_err(|e| {
                AppError::DbError(format!("Failed to inspect assessment_answers FKs: {}", e))
            })?
            .query_map([], |row| row.get::<_, String>(2))
            .map_err(|e| {
                AppError::DbError(format!("Failed to read assessment_answers FKs: {}", e))
            })?
            .collect::<Result<_, _>>()
            .map_err(|e| {
                AppError::DbError(format!("Failed to read assessment_answers FKs: {}", e))
            })?;

        // Nothing to do if answers already point at the snapshot table.
        if fk_targets
            .iter()
            .any(|t| t == "assessment_session_questions")
        {
            return Ok(());
        }

        // Recreate the answers table against the snapshot table, carrying over
        // any existing rows. Because legacy answers referenced canonical
        // questions, we reconstruct a snapshot row per legacy answer. The
        // expected_content is best-effort: taken from the canonical question
        // only if it still exists, and left empty otherwise (we never
        // synthesize content that was not persisted).
        let mut tx = conn
            .transaction()
            .map_err(|e| AppError::DbError(format!("Failed to start answers migration: {}", e)))?;

        tx.execute(
            "ALTER TABLE assessment_answers RENAME TO assessment_answers_legacy",
            [],
        )
        .map_err(|e| AppError::DbError(format!("Failed to rename legacy answers table: {}", e)))?;

        tx.execute(CREATE_ASSESSMENT_ANSWERS_TABLE, [])
            .map_err(|e| {
                AppError::DbError(format!(
                    "Failed to recreate assessment_answers table: {}",
                    e
                ))
            })?;

        // Build a snapshot row for each distinct (session_id, question_id) the
        // legacy answers referenced, resolving the canonical question fields.
        tx.execute(
            "INSERT OR IGNORE INTO assessment_session_questions \
             (session_id, source_question_id, prompt, concept, difficulty, expected_content, position) \
             SELECT a.session_id, a.question_id, \
                    COALESCE(q.prompt, ''), \
                    COALESCE(q.concept, 'General'), \
                    COALESCE(q.difficulty, 'recall'), \
                    COALESCE(q.expected_content, ''), \
                    COALESCE(q.position, 0) \
             FROM assessment_answers_legacy a \
             LEFT JOIN assessment_questions q ON a.question_id = q.id",
            [],
        )
        .map_err(|e| {
            AppError::DbError(format!("Failed to rebuild session question snapshots: {}", e))
        })?;

        // Carry the answers over, repointing each answer's question_id at the
        // snapshot row that now stands in for the question it answered.
        tx.execute(
            "INSERT OR IGNORE INTO assessment_answers \
             (session_id, question_id, student_answer, grade, rationale, misconception, created_at) \
             SELECT a.session_id, sq.id, a.student_answer, a.grade, a.rationale, a.misconception, \
                    COALESCE(a.created_at, CURRENT_TIMESTAMP) \
             FROM assessment_answers_legacy a \
             JOIN assessment_session_questions sq \
               ON sq.session_id = a.session_id AND sq.source_question_id = a.question_id",
            [],
        )
        .map_err(|e| {
            AppError::DbError(format!("Failed to migrate answers to snapshots: {}", e))
        })?;

        tx.execute("DROP TABLE assessment_answers_legacy", [])
            .map_err(|e| AppError::DbError(format!("Failed to drop legacy answers: {}", e)))?;

        tx.commit()
            .map_err(|e| AppError::DbError(format!("Failed to commit answers migration: {}", e)))?;

        Ok(())
    }
}
