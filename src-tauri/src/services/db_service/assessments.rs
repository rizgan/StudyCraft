use super::connection::DbService;
use super::queries;
use super::review_materials::ReviewMaterialOperations;
use crate::errors::{AppError, AppResult};
use crate::materials::assessment::{
    AssessmentAnswer, AssessmentAttempt, AssessmentDifficulty, AssessmentEvaluation,
    AssessmentGrade, AssessmentQuestion, AssessmentSessionQuestion, NewAssessmentQuestion,
};
use crate::materials::review_material::{ReviewMaterial, ReviewMaterialType};
use chrono::{DateTime, Utc};
use rusqlite::{params, Row};

/// Parses a stored timestamp. SQLite's `CURRENT_TIMESTAMP` defaults to
/// second-precision (`YYYY-MM-DD HH:MM:SS`), while app-written values may carry
/// sub-seconds. Both are handled so reads never fail on a default value.
fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, rusqlite::Error> {
    let trimmed = s.trim();
    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f"))
        .map(|v| v.and_utc())
        .map_err(|e| rusqlite::Error::InvalidParameterName(format!("Bad date {}: {}", s, e)))
}

fn question_from_row(row: &Row) -> Result<AssessmentQuestion, rusqlite::Error> {
    let difficulty_str: String = row.get("difficulty")?;
    let id: i64 = row.get("id")?;
    let difficulty = match AssessmentDifficulty::parse(&difficulty_str) {
        Some(d) => d,
        None => {
            eprintln!(
                "Assessment question {} stored with unknown difficulty '{}'; treating as recall",
                id, difficulty_str
            );
            AssessmentDifficulty::Recall
        }
    };
    Ok(AssessmentQuestion {
        id: Some(row.get("id")?),
        assessment_id: row.get("assessment_id")?,
        prompt: row.get("prompt")?,
        concept: row.get("concept")?,
        difficulty,
        expected_content: row.get("expected_content")?,
        position: row.get("position")?,
        created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?,
        updated_at: parse_timestamp(&row.get::<_, String>("updated_at")?)?,
    })
}

fn session_question_from_row(row: &Row) -> Result<AssessmentSessionQuestion, rusqlite::Error> {
    let difficulty_str: String = row.get("difficulty")?;
    let id: i64 = row.get("id")?;
    let difficulty = match AssessmentDifficulty::parse(&difficulty_str) {
        Some(d) => d,
        None => {
            eprintln!(
                "Session question {} stored with unknown difficulty '{}'; treating as recall",
                id, difficulty_str
            );
            AssessmentDifficulty::Recall
        }
    };
    Ok(AssessmentSessionQuestion {
        id: Some(id),
        session_id: row.get("session_id")?,
        source_question_id: row.get("source_question_id")?,
        prompt: row.get("prompt")?,
        concept: row.get("concept")?,
        difficulty,
        expected_content: row.get("expected_content")?,
        position: row.get("position")?,
    })
}

/// Operations for assessment materials (questions, sessions and answers).
pub trait AssessmentOperations {
    /// Creates an assessment review material with its questions, in a single transaction.
    fn create_assessment(
        &self,
        id: &str,
        name: &str,
        tags: &[String],
        questions: &[NewAssessmentQuestion],
    ) -> AppResult<ReviewMaterial>;

    /// Fetches the assessment's questions ordered by position.
    fn get_assessment_questions(&self, assessment_id: &str) -> AppResult<Vec<AssessmentQuestion>>;

    /// Replaces all stored questions of an assessment with the new set.
    fn replace_assessment_questions(
        &self,
        assessment_id: &str,
        questions: &[NewAssessmentQuestion],
    ) -> AppResult<()>;

    fn count_assessment_questions(&self, assessment_id: &str) -> AppResult<i64>;

    /// Starts a new assessment session. Creates the session row and copies the
    /// given question set into an immutable per-session snapshot, so the
    /// attempt is self-contained and independent of the canonical question set.
    fn create_assessment_session_with_questions(
        &self,
        session_id: &str,
        assessment_id: &str,
        questions: &[AssessmentQuestion],
    ) -> AppResult<()>;

    /// Returns the id of a pending (status = 'active') session for the
    /// assessment, if one exists. Used to resume an attempt in progress.
    fn get_pending_assessment_session(&self, assessment_id: &str) -> AppResult<Option<String>>;

    /// Returns the immutable question snapshot of a session, ordered by
    /// position. This is the question set an attempt was actually taken from.
    fn get_assessment_session_questions(
        &self,
        session_id: &str,
    ) -> AppResult<Vec<AssessmentSessionQuestion>>;

    /// Returns a single session-question snapshot by id, scoped to a session.
    fn get_assessment_session_question(
        &self,
        session_id: &str,
        snapshot_id: i64,
    ) -> AppResult<Option<AssessmentSessionQuestion>>;

    /// Lists the assessment's attempts (sessions) with their status and answer
    /// counts, most recent first.
    fn get_assessment_attempts(&self, assessment_id: &str) -> AppResult<Vec<AssessmentAttempt>>;

    /// Records the student's answer and its evaluation for one question.
    fn insert_assessment_answer(
        &self,
        session_id: &str,
        question_id: i64,
        student_answer: &str,
        evaluation: Option<&AssessmentEvaluation>,
    ) -> AppResult<()>;

    /// Fetches all recorded answers of a session with their question data.
    fn get_assessment_answers(&self, session_id: &str) -> AppResult<Vec<AssessmentAnswer>>;

    /// Deletes the assessment material and, through cascades, its questions,
    /// sessions and answers.
    fn delete_assessment(&self, assessment_id: &str) -> AppResult<()>;

    /// Returns the id of the most recent completed session for an assessment,
    /// or `None` if the assessment has never been taken.
    fn get_last_assessment_session(&self, assessment_id: &str) -> AppResult<Option<String>>;

    /// Returns the id of the most recent *completed* session for an
    /// assessment, or `None` if none has been completed yet.
    fn get_last_completed_assessment_session(
        &self,
        assessment_id: &str,
    ) -> AppResult<Option<String>>;

    /// Resolves which session an open of the assessment should attach to,
    /// following the reopen/resume semantics:
    ///
    /// - a pending (active) session always wins, so an in-progress attempt is
    ///   resumed rather than replaced;
    /// - otherwise the most recent completed session is returned so a
    ///   completed attempt is reviewed, never mutated or re-created;
    /// - otherwise `None`: the assessment has never been taken and a fresh
    ///   session must be started.
    ///
    /// Returns `(session_id, status)` with status `"active"` or
    /// `"completed"`.
    fn resolve_assessment_session_to_open(
        &self,
        assessment_id: &str,
    ) -> AppResult<Option<(String, String)>>;

    /// Marks an active session as completed. Returns the number of rows updated
    /// (0 if the session was not active, i.e. already completed or unknown).
    fn complete_assessment_session(&self, session_id: &str) -> AppResult<i64>;

    /// Returns `true` if the session's status is `completed`.
    fn is_assessment_session_completed(&self, session_id: &str) -> AppResult<bool>;

    /// Returns the stored status string of a session, or `None` if unknown.
    fn get_assessment_session_status(&self, session_id: &str) -> AppResult<Option<String>>;
}

impl AssessmentOperations for DbService {
    fn create_assessment(
        &self,
        id: &str,
        name: &str,
        tags: &[String],
        questions: &[NewAssessmentQuestion],
    ) -> AppResult<ReviewMaterial> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DbError(format!("Failed to start transaction: {}", e)))?;

        let material = ReviewMaterial {
            id: id.to_string(),
            display_name: name.to_string(),
            rm_type: ReviewMaterialType::Assessment,
            last_review: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            language_learning: None,
        };
        self.add_review_material_tx(&tx, &material)?;

        for tag in tags {
            if tag.trim().is_empty() {
                continue;
            }

            tx.execute(queries::INSERT_TAG, params![tag.trim()])
                .map_err(|e| AppError::DbError(format!("Failed to insert tag: {}", e)))?;

            let tag_id: i64 = tx
                .query_row(queries::GET_TAG_ID, params![tag.trim()], |row| row.get(0))
                .map_err(|e| AppError::DbError(format!("Failed to get tag ID: {}", e)))?;

            tx.execute(queries::INSERT_MATERIAL_TAG, params![id, tag_id])
                .map_err(|e| {
                    AppError::DbError(format!("Failed to link assessment to tag: {}", e))
                })?;
        }

        for (index, question) in questions.iter().enumerate() {
            tx.execute(
                queries::INSERT_ASSESSMENT_QUESTION,
                params![
                    id,
                    question.prompt,
                    question.concept,
                    String::from(question.difficulty),
                    question.expected_content,
                    index as i32
                ],
            )
            .map_err(|e| {
                AppError::DbError(format!("Failed to insert assessment question: {}", e))
            })?;
        }

        tx.commit()
            .map_err(|e| AppError::DbError(format!("Failed to commit transaction: {}", e)))?;

        Ok(material)
    }

    fn get_assessment_questions(&self, assessment_id: &str) -> AppResult<Vec<AssessmentQuestion>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_ASSESSMENT_QUESTIONS)
            .map_err(|e| AppError::DbError(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map(params![assessment_id], |row| question_from_row(row))
            .map_err(|e| {
                AppError::DbError(format!("Failed to query assessment questions: {}", e))
            })?;

        let mut questions = Vec::new();
        for row in rows {
            let question =
                row.map_err(|e| AppError::DbError(format!("Failed to process row: {}", e)))?;
            questions.push(question);
        }

        Ok(questions)
    }

    fn replace_assessment_questions(
        &self,
        assessment_id: &str,
        questions: &[NewAssessmentQuestion],
    ) -> AppResult<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DbError(format!("Failed to start transaction: {}", e)))?;

        tx.execute(
            queries::DELETE_ALL_ASSESSMENT_QUESTIONS,
            params![assessment_id],
        )
        .map_err(|e| AppError::DbError(format!("Failed to delete old questions: {}", e)))?;

        for (index, question) in questions.iter().enumerate() {
            tx.execute(
                queries::INSERT_ASSESSMENT_QUESTION,
                params![
                    assessment_id,
                    question.prompt,
                    question.concept,
                    String::from(question.difficulty),
                    question.expected_content,
                    index as i32
                ],
            )
            .map_err(|e| {
                AppError::DbError(format!("Failed to insert assessment question: {}", e))
            })?;
        }

        tx.commit()
            .map_err(|e| AppError::DbError(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    fn count_assessment_questions(&self, assessment_id: &str) -> AppResult<i64> {
        let conn = self.conn.borrow();
        conn.query_row(
            queries::COUNT_ASSESSMENT_QUESTIONS,
            params![assessment_id],
            |row| row.get(0),
        )
        .map_err(|e| AppError::DbError(format!("Failed to count assessment questions: {}", e)))
    }

    fn create_assessment_session_with_questions(
        &self,
        session_id: &str,
        assessment_id: &str,
        questions: &[AssessmentQuestion],
    ) -> AppResult<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DbError(format!("Failed to start transaction: {}", e)))?;

        tx.execute(
            queries::INSERT_ASSESSMENT_SESSION,
            params![session_id, assessment_id],
        )
        .map_err(|e| AppError::DbError(format!("Failed to insert assessment session: {}", e)))?;

        // Copy the question set into the immutable snapshot for this session.
        for question in questions {
            tx.execute(
                queries::INSERT_ASSESSMENT_SESSION_QUESTION,
                params![
                    session_id,
                    question.id,
                    question.prompt,
                    question.concept,
                    String::from(question.difficulty),
                    question.expected_content,
                    question.position
                ],
            )
            .map_err(|e| {
                AppError::DbError(format!("Failed to snapshot assessment question: {}", e))
            })?;
        }

        tx.commit()
            .map_err(|e| AppError::DbError(format!("Failed to commit session: {}", e)))?;

        Ok(())
    }

    fn get_pending_assessment_session(&self, assessment_id: &str) -> AppResult<Option<String>> {
        let conn = self.conn.borrow();
        match conn.query_row(
            queries::GET_PENDING_ASSESSMENT_SESSION,
            params![assessment_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(session_id) => Ok(Some(session_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(other) => Err(AppError::DbError(format!(
                "Failed to query pending assessment session: {}",
                other
            ))),
        }
    }

    fn get_assessment_session_questions(
        &self,
        session_id: &str,
    ) -> AppResult<Vec<AssessmentSessionQuestion>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_ASSESSMENT_SESSION_QUESTIONS)
            .map_err(|e| AppError::DbError(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map(params![session_id], |row| session_question_from_row(row))
            .map_err(|e| AppError::DbError(format!("Failed to query session questions: {}", e)))?;

        let mut questions = Vec::new();
        for row in rows {
            let question =
                row.map_err(|e| AppError::DbError(format!("Failed to process row: {}", e)))?;
            questions.push(question);
        }

        Ok(questions)
    }

    fn get_assessment_session_question(
        &self,
        session_id: &str,
        snapshot_id: i64,
    ) -> AppResult<Option<AssessmentSessionQuestion>> {
        let conn = self.conn.borrow();
        match conn.query_row(
            queries::GET_ASSESSMENT_SESSION_QUESTION_BY_ID,
            params![snapshot_id, session_id],
            |row| session_question_from_row(row),
        ) {
            Ok(question) => Ok(Some(question)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(other) => Err(AppError::DbError(format!(
                "Failed to query session question: {}",
                other
            ))),
        }
    }

    fn get_assessment_attempts(&self, assessment_id: &str) -> AppResult<Vec<AssessmentAttempt>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_ASSESSMENT_SESSIONS_BY_ASSESSMENT)
            .map_err(|e| AppError::DbError(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map(params![assessment_id], |row| {
                Ok(AssessmentAttempt {
                    id: row.get("id")?,
                    assessment_id: row.get("assessment_id")?,
                    status: row.get("status")?,
                    created_at: parse_timestamp(&row.get::<_, String>("created_at")?)?.to_rfc3339(),
                    started_at: row
                        .get::<_, Option<String>>("started_at")?
                        .and_then(|v| parse_timestamp(&v).ok())
                        .map(|v| v.to_rfc3339()),
                    completed_at: row
                        .get::<_, Option<String>>("completed_at")?
                        .and_then(|v| parse_timestamp(&v).ok())
                        .map(|v| v.to_rfc3339()),
                    answer_count: row.get("answer_count")?,
                })
            })
            .map_err(|e| {
                AppError::DbError(format!("Failed to query assessment attempts: {}", e))
            })?;

        let mut attempts = Vec::new();
        for row in rows {
            let attempt =
                row.map_err(|e| AppError::DbError(format!("Failed to process row: {}", e)))?;
            attempts.push(attempt);
        }

        Ok(attempts)
    }

    fn insert_assessment_answer(
        &self,
        session_id: &str,
        question_id: i64,
        student_answer: &str,
        evaluation: Option<&AssessmentEvaluation>,
    ) -> AppResult<()> {
        let conn = self.conn.borrow();

        // The answer is the raw assessment evidence of record. Register the
        // question's concept identity in the canonical concept registry in
        // the same flow so evidence and registry stay consistent. The concept
        // is read from the immutable session-question snapshot the answer
        // belongs to, never the mutable canonical question.
        let concept: String = conn
            .query_row(
                "SELECT concept FROM assessment_session_questions \
                 WHERE session_id = ?1 AND id = ?2",
                params![session_id, question_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                AppError::DbError(format!("Failed to load assessment question concept: {}", e))
            })?;
        let assessment_id: String = conn
            .query_row(
                "SELECT assessment_id FROM assessment_sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                AppError::DbError(format!("Failed to load assessment material id: {}", e))
            })?;
        if let Some(concept_id) = crate::materials::concept_mastery::concept_id_for_label(&concept)
        {
            conn.execute(
                "INSERT OR IGNORE INTO concepts (id, label) VALUES (?1, ?1)",
                params![concept_id],
            )
            .map_err(|e| AppError::DbError(format!("Failed to register concept: {}", e)))?;
            // Record the material provenance of this concept occurrence so a
            // shared label can be traced back to every material that produced
            // it (false separation is preferred over false merging).
            conn.execute(
                "INSERT OR IGNORE INTO concept_material_occurrences \
                 (concept_id, material_id) VALUES (?1, ?2)",
                params![concept_id, assessment_id],
            )
            .map_err(|e| {
                AppError::DbError(format!("Failed to record concept provenance: {}", e))
            })?;
        }

        conn.execute(
            queries::INSERT_ASSESSMENT_ANSWER,
            params![
                session_id,
                question_id,
                student_answer,
                evaluation.map(|e| String::from(e.grade)),
                evaluation.map(|e| e.rationale.clone()),
                evaluation.and_then(|e| e.misconception.clone())
            ],
        )
        .map_err(|e| AppError::DbError(format!("Failed to insert assessment answer: {}", e)))?;

        Ok(())
    }

    fn get_assessment_answers(&self, session_id: &str) -> AppResult<Vec<AssessmentAnswer>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_ASSESSMENT_ANSWERS_BY_SESSION)
            .map_err(|e| AppError::DbError(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let grade_str: Option<String> = row.get("grade")?;
                let rationale: Option<String> = row.get("rationale")?;
                let misconception: Option<String> = row.get("misconception")?;

                let evaluation = grade_str.and_then(|grade_str| {
                    AssessmentGrade::parse(&grade_str).map(|grade| AssessmentEvaluation {
                        grade,
                        rationale: rationale.clone().unwrap_or_default(),
                        misconception: misconception.clone(),
                    })
                });

                let difficulty_str: String = row.get("difficulty")?;
                let difficulty = AssessmentDifficulty::parse(&difficulty_str)
                    .unwrap_or(AssessmentDifficulty::Recall);

                Ok(AssessmentAnswer {
                    id: Some(row.get("id")?),
                    session_id: row.get("session_id")?,
                    question_id: row.get("question_id")?,
                    question_prompt: row.get("prompt")?,
                    concept: row.get("concept")?,
                    difficulty,
                    student_answer: row.get("student_answer")?,
                    evaluation,
                })
            })
            .map_err(|e| AppError::DbError(format!("Failed to query assessment answers: {}", e)))?;

        let mut answers = Vec::new();
        for row in rows {
            let answer =
                row.map_err(|e| AppError::DbError(format!("Failed to process row: {}", e)))?;
            answers.push(answer);
        }

        Ok(answers)
    }

    fn delete_assessment(&self, assessment_id: &str) -> AppResult<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DbError(format!("Failed to start transaction: {}", e)))?;

        self.delete_review_material_tx(&tx, assessment_id)?;

        tx.commit()
            .map_err(|e| AppError::DbError(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    fn get_last_assessment_session(&self, assessment_id: &str) -> AppResult<Option<String>> {
        let conn = self.conn.borrow();

        match conn.query_row(
            queries::GET_LAST_ASSESSMENT_SESSION,
            params![assessment_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(session_id) => Ok(Some(session_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(other) => Err(AppError::DbError(format!(
                "Failed to query last assessment session: {}",
                other
            ))),
        }
    }

    fn get_last_completed_assessment_session(
        &self,
        assessment_id: &str,
    ) -> AppResult<Option<String>> {
        let conn = self.conn.borrow();

        match conn.query_row(
            queries::GET_LAST_COMPLETED_ASSESSMENT_SESSION,
            params![assessment_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(session_id) => Ok(Some(session_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(other) => Err(AppError::DbError(format!(
                "Failed to query last completed assessment session: {}",
                other
            ))),
        }
    }

    fn resolve_assessment_session_to_open(
        &self,
        assessment_id: &str,
    ) -> AppResult<Option<(String, String)>> {
        // A pending attempt is always resumed; reopening an assessment must
        // never start a new session while one is in progress.
        if let Some(pending) = self.get_pending_assessment_session(assessment_id)? {
            return Ok(Some((pending, "active".to_string())));
        }

        // No in-progress attempt: the most recent completed attempt is
        // reviewed as-is. Returning it (instead of `None`) is what keeps a
        // reopen from mutating or re-creating a completed attempt.
        if let Some(completed) = self.get_last_completed_assessment_session(assessment_id)? {
            return Ok(Some((completed, "completed".to_string())));
        }

        Ok(None)
    }

    fn complete_assessment_session(&self, session_id: &str) -> AppResult<i64> {
        let conn = self.conn.borrow();
        let rows = conn
            .execute(queries::COMPLETE_ASSESSMENT_SESSION, params![session_id])
            .map_err(|e| {
                AppError::DbError(format!("Failed to complete assessment session: {}", e))
            })?;
        Ok(rows as i64)
    }

    fn is_assessment_session_completed(&self, session_id: &str) -> AppResult<bool> {
        let conn = self.conn.borrow();
        match conn.query_row(
            "SELECT status FROM assessment_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(status) => Ok(status == "completed"),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(other) => Err(AppError::DbError(format!(
                "Failed to query assessment session status: {}",
                other
            ))),
        }
    }

    fn get_assessment_session_status(&self, session_id: &str) -> AppResult<Option<String>> {
        let conn = self.conn.borrow();
        match conn.query_row(
            "SELECT status FROM assessment_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(status) => Ok(Some(status)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(other) => Err(AppError::DbError(format!(
                "Failed to query assessment session status: {}",
                other
            ))),
        }
    }
}
