//! Integration tests for the assessment attempt-history integrity model.
//!
//! Each test runs against a private, freshly created database file (never the
//! real application database) and verifies the invariant that re-running an
//! assessment never destroys a prior attempt: its immutable question snapshot
//! and its recorded answers must survive a canonical question-set replacement.

use app_lib::materials::assessment::NewAssessmentQuestion;
use app_lib::services::db_service::{queries, AssessmentOperations, DbService};
use rusqlite::params;

/// Opens a brand-new database for a single test. The file is created under the
/// OS temp dir and removed when the service is dropped.
fn new_db_service() -> (DbService, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp.path().join("studycraft_integrity.db");
    let service = DbService::new_for_tests(&db_path).expect("failed to open test database");
    (service, temp)
}

fn new_question(prompt: &str, concept: &str, expected: &str) -> NewAssessmentQuestion {
    NewAssessmentQuestion {
        prompt: prompt.to_string(),
        concept: concept.to_string(),
        difficulty: app_lib::materials::assessment::AssessmentDifficulty::Recall,
        expected_content: expected.to_string(),
        position: 0,
    }
}

/// A full attempt: start a session from the current question set, record
/// answers to every question in it, and complete the session.
fn take_attempt(db: &DbService, assessment_id: &str, label: &str) -> String {
    let questions = db
        .get_assessment_questions(assessment_id)
        .expect("failed to load questions");
    let session_id = format!("{}-{}", assessment_id, label);
    db.create_assessment_session_with_questions(&session_id, assessment_id, &questions)
        .expect("failed to create session");

    let snapshots = db
        .get_assessment_session_questions(&session_id)
        .expect("failed to load snapshot");
    assert!(
        !snapshots.is_empty(),
        "attempt {} has an empty snapshot",
        label
    );

    for snapshot in &snapshots {
        let snapshot_id = snapshot.id.expect("snapshot row has an id");
        db.insert_assessment_answer(
            &session_id,
            snapshot_id,
            &format!("student answer for {}", snapshot.prompt),
            Some(&app_lib::materials::assessment::AssessmentEvaluation {
                grade: app_lib::materials::assessment::AssessmentGrade::Correct,
                rationale: "ok".to_string(),
                misconception: None,
            }),
        )
        .expect("failed to record answer");
    }

    db.complete_assessment_session(&session_id)
        .expect("failed to complete session");

    session_id
}

#[test]
fn reassessment_preserves_previous_attempt_history() {
    let (db, _temp) = new_db_service();

    // Seed an assessment with a first question set.
    let assessment_id = "assessment-integrity-1";
    db.create_assessment(
        assessment_id,
        "Integrity Test",
        &[],
        &[
            new_question("Q1", "alpha", "expected alpha"),
            new_question("Q2", "beta", "expected beta"),
        ],
    )
    .expect("failed to create assessment");

    // Attempt #1: take and complete the assessment with the first question set.
    let first_attempt = take_attempt(&db, assessment_id, "attempt1");

    // Record the evaluation state attempt #1 was graded with: its report is
    // a deterministic function of these rows and must not change later.
    let report_before: Vec<(String, String, String)> = db
        .get_assessment_answers(&first_attempt)
        .expect("failed to load answers before replacement")
        .into_iter()
        .map(|a| {
            (
                a.question_prompt.clone(),
                a.concept.clone(),
                a.evaluation
                    .as_ref()
                    .map(|e| String::from(e.grade))
                    .unwrap_or_default(),
            )
        })
        .collect();

    // Replace the canonical question set (what an explicit reassessment does to
    // the canonical store). This must not cascade into the prior attempt.
    db.replace_assessment_questions(
        assessment_id,
        &[
            new_question("Q3", "alpha", "expected alpha v2"),
            new_question("Q4", "gamma", "expected gamma"),
        ],
    )
    .expect("failed to replace questions");

    // The previous attempt's immutable snapshot must still be intact and
    // reflect the ORIGINAL questions, not the replaced ones.
    let first_snapshot = db
        .get_assessment_session_questions(&first_attempt)
        .expect("failed to load attempt #1 snapshot");
    assert_eq!(first_snapshot.len(), 2, "attempt #1 snapshot size changed");
    let prompts: Vec<String> = first_snapshot.iter().map(|q| q.prompt.clone()).collect();
    assert!(
        prompts.contains(&"Q1".to_string()) && prompts.contains(&"Q2".to_string()),
        "attempt #1 snapshot no longer holds the original questions: {:?}",
        prompts
    );
    // The reference content used to grade attempt #1 must still be frozen.
    let alpha = first_snapshot
        .iter()
        .find(|q| q.prompt == "Q1")
        .expect("Q1 snapshot missing");
    assert_eq!(
        alpha.expected_content, "expected alpha",
        "expected_content was mutated"
    );

    // The previous attempt's answers must survive, joined to the snapshot.
    let answers = db
        .get_assessment_answers(&first_attempt)
        .expect("failed to load attempt #1 answers");
    assert_eq!(
        answers.len(),
        2,
        "attempt #1 answers were destroyed by the question replacement"
    );

    // Report #1 is a deterministic function of these rows: the graded state
    // must be byte-identical after the canonical replacement, so the report
    // reconstructed for attempt #1 cannot have changed.
    let report_after: Vec<(String, String, String)> = answers
        .iter()
        .map(|a| {
            (
                a.question_prompt.clone(),
                a.concept.clone(),
                a.evaluation
                    .as_ref()
                    .map(|e| String::from(e.grade))
                    .unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        report_before, report_after,
        "attempt #1 report values changed"
    );

    // Attempt #2 can now be taken against the new question set without
    // disturbing attempt #1.
    let second_attempt = take_attempt(&db, assessment_id, "attempt2");
    let second_answers = db
        .get_assessment_answers(&second_attempt)
        .expect("failed to load attempt #2 answers");
    assert_eq!(second_answers.len(), 2, "attempt #2 not recorded");

    // Attempt history exposes both completed attempts, most recent first.
    let attempts = db
        .get_assessment_attempts(assessment_id)
        .expect("failed to load attempts");
    assert_eq!(attempts.len(), 2, "expected two attempts in history");
    assert_eq!(attempts[0].id, second_attempt);
    assert_eq!(attempts[1].id, first_attempt);
    assert!(attempts.iter().all(|a| a.status == "completed"));
}

/// Deleting a single canonical question must not destroy a historical
/// session: the snapshot row survives (its source reference is SET NULL) and
/// the answer stays joined to the snapshot, never to the canonical row.
#[test]
fn deleting_canonical_question_preserves_session_history() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-fk-q";
    db.create_assessment(
        assessment_id,
        "FK Question Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    let attempt = take_attempt(&db, assessment_id, "attempt1");

    let canonical_id: i64 = db
        .conn
        .borrow()
        .query_row(
            "SELECT id FROM assessment_questions WHERE assessment_id = ?1",
            [assessment_id],
            |row| row.get(0),
        )
        .expect("canonical question id");
    db.conn
        .borrow()
        .execute(
            queries::DELETE_ASSESSMENT_QUESTION,
            params![canonical_id, assessment_id],
        )
        .expect("delete canonical question");

    // The historical snapshot must survive, with its source reference cleared.
    let snapshots = db
        .get_assessment_session_questions(&attempt)
        .expect("failed to load snapshot");
    assert_eq!(
        snapshots.len(),
        1,
        "snapshot destroyed by canonical question delete"
    );
    assert_eq!(snapshots[0].prompt, "Q1");
    assert_eq!(snapshots[0].expected_content, "expected alpha");
    assert!(
        snapshots[0].source_question_id.is_none(),
        "dangling source reference must be NULL, not a stale id"
    );

    // The answer must still be intact and joined through the snapshot.
    let answers = db
        .get_assessment_answers(&attempt)
        .expect("failed to load answers");
    assert_eq!(
        answers.len(),
        1,
        "answer destroyed by canonical question delete"
    );
    assert_eq!(answers[0].question_prompt, "Q1");
}

/// Deleting a session must cascade to its snapshot and answers only.
#[test]
fn deleting_session_cascades_to_its_snapshot_and_answers() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-fk-s";
    db.create_assessment(
        assessment_id,
        "FK Session Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    let attempt = take_attempt(&db, assessment_id, "attempt1");

    db.conn
        .borrow()
        .execute(
            "DELETE FROM assessment_sessions WHERE id = ?1",
            [attempt.clone()],
        )
        .expect("delete session");

    let snapshots = db
        .get_assessment_session_questions(&attempt)
        .expect("failed to load snapshot");
    assert!(
        snapshots.is_empty(),
        "session snapshot must cascade with the session"
    );

    let answers = db
        .get_assessment_answers(&attempt)
        .expect("failed to load answers");
    assert!(answers.is_empty(), "answers must cascade with the session");
}

/// Hard-deleting the assessment material row must cascade through everything:
/// canonical questions, sessions, session snapshots and answers.
#[test]
fn deleting_assessment_cascades_to_all_history() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-fk-m";
    db.create_assessment(
        assessment_id,
        "FK Material Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    let attempt = take_attempt(&db, assessment_id, "attempt1");

    // Bypass the soft-delete API and remove the material row directly to
    // verify the foreign-key cascade chain itself.
    db.conn
        .borrow()
        .execute(
            "DELETE FROM review_materials WHERE id = ?1",
            [assessment_id],
        )
        .expect("delete review material row");

    let conn = db.conn.borrow();
    let count = |sql: &str| {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .expect("count query")
    };
    assert_eq!(count("SELECT COUNT(*) FROM assessment_questions"), 0);
    assert_eq!(count("SELECT COUNT(*) FROM assessment_sessions"), 0);
    assert_eq!(
        count("SELECT COUNT(*) FROM assessment_session_questions"),
        0
    );
    assert_eq!(
        count("SELECT COUNT(*) FROM assessment_answers"),
        0,
        "answers of {attempt}"
    );
}

/// The assessment delete API is a soft delete of the material: it must never
/// destroy stored attempt history, which stays intact and consistent.
#[test]
fn soft_deleting_assessment_preserves_attempt_history() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-soft-del";
    db.create_assessment(
        assessment_id,
        "Soft Delete Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    let attempt = take_attempt(&db, assessment_id, "attempt1");

    db.delete_assessment(assessment_id)
        .expect("failed to soft-delete assessment");

    let deleted: i64 = db
        .conn
        .borrow()
        .query_row(
            "SELECT COALESCE(MAX(deleted), 0) FROM review_materials WHERE id = ?1",
            [assessment_id],
            |row| row.get(0),
        )
        .expect("deleted flag");
    assert_eq!(deleted, 1, "assessment should be soft-deleted");

    // History must remain fully intact and internally consistent.
    let snapshots = db
        .get_assessment_session_questions(&attempt)
        .expect("failed to load snapshot");
    assert_eq!(snapshots.len(), 1, "snapshot lost by soft delete");
    let answers = db
        .get_assessment_answers(&attempt)
        .expect("failed to load answers");
    assert_eq!(answers.len(), 1, "answer lost by soft delete");
    assert_eq!(answers[0].question_prompt, "Q1");
}

/// Reopening an assessment whose latest attempt is completed must resolve to
/// that completed session for read-only review: no new session is created,
/// nothing is regenerated, and the resolved session is stable across opens.
#[test]
fn reopening_completed_assessment_resolves_to_review_not_new_session() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-reopen";
    db.create_assessment(
        assessment_id,
        "Reopen Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    let attempt = take_attempt(&db, assessment_id, "attempt1");

    // First open after completion resolves to the completed session.
    let resolved = db
        .resolve_assessment_session_to_open(assessment_id)
        .expect("failed to resolve session");
    assert_eq!(
        resolved.as_ref(),
        Some(&(attempt.clone(), "completed".to_string())),
        "reopen of a completed attempt must attach to it, not start a new session"
    );

    // The resolution is read-only: repeating the open must not create
    // another session or touch the canonical question set.
    let resolved_again = db
        .resolve_assessment_session_to_open(assessment_id)
        .expect("failed to resolve session again");
    assert_eq!(resolved_again, resolved, "re-open must be stable");

    let sessions: i64 = db
        .conn
        .borrow()
        .query_row("SELECT COUNT(*) FROM assessment_sessions", [], |row| {
            row.get(0)
        })
        .expect("session count");
    assert_eq!(sessions, 1, "reopening must not create a new session");

    let questions: i64 = db
        .conn
        .borrow()
        .query_row("SELECT COUNT(*) FROM assessment_questions", [], |row| {
            row.get(0)
        })
        .expect("question count");
    assert_eq!(questions, 1, "reopening must not regenerate questions");
}

/// A pending session always wins over completed ones, and re-resolving it
/// yields the same immutable snapshot ids (a resume, not a restart).
#[test]
fn resolving_pending_session_is_stable_and_returns_same_snapshots() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-resume";
    db.create_assessment(
        assessment_id,
        "Resume Test",
        &[],
        &[
            new_question("Q1", "alpha", "expected alpha"),
            new_question("Q2", "beta", "expected beta"),
        ],
    )
    .expect("failed to create assessment");

    // A completed attempt exists, but a pending one must win the resolution.
    let completed = take_attempt(&db, assessment_id, "attempt1");
    let questions = db
        .get_assessment_questions(assessment_id)
        .expect("failed to load questions");
    db.create_assessment_session_with_questions("pending-1", assessment_id, &questions)
        .expect("failed to create pending session");

    let resolved = db
        .resolve_assessment_session_to_open(assessment_id)
        .expect("failed to resolve");
    assert_eq!(
        resolved.as_ref(),
        Some(&("pending-1".to_string(), "active".to_string())),
        "pending session must be resumed in preference to the completed one"
    );

    let first_ids: Vec<i64> = db
        .get_assessment_session_questions("pending-1")
        .expect("failed to load snapshots")
        .iter()
        .map(|q| q.id.expect("snapshot id"))
        .collect();
    assert_eq!(first_ids.len(), 2);

    // Re-resolving and re-loading must surface the same snapshot ids.
    let resolved_again = db
        .resolve_assessment_session_to_open(assessment_id)
        .expect("failed to resolve again");
    assert_eq!(resolved_again, resolved);
    let second_ids: Vec<i64> = db
        .get_assessment_session_questions("pending-1")
        .expect("failed to reload snapshots")
        .iter()
        .map(|q| q.id.expect("snapshot id"))
        .collect();
    assert_eq!(
        first_ids, second_ids,
        "resume must return the same session question ids"
    );

    assert_ne!(completed, "pending-1");
}

#[test]
fn resuming_a_pending_session_does_not_create_a_new_attempt() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-2";
    db.create_assessment(
        assessment_id,
        "Resume Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    // Start (do not complete) a session.
    let questions = db
        .get_assessment_questions(assessment_id)
        .expect("failed to load questions");
    db.create_assessment_session_with_questions("sess-a", assessment_id, &questions)
        .expect("failed to create session");

    // The pending-session lookup must surface the same session on re-entry.
    let pending = db
        .get_pending_assessment_session(assessment_id)
        .expect("failed to look up pending session");
    assert_eq!(
        pending.as_deref(),
        Some("sess-a"),
        "pending session not found for resume"
    );

    // Re-opening the same pending session and answering again must not create
    // a second session: there is still exactly one attempt.
    db.insert_assessment_answer(
        "sess-a",
        db.get_assessment_session_questions("sess-a")
            .expect("failed to load snapshot")
            .first()
            .expect("snapshot present")
            .id
            .expect("snapshot id present"),
        "resumed answer",
        None,
    )
    .expect("failed to record resumed answer");

    let attempts = db
        .get_assessment_attempts(assessment_id)
        .expect("failed to load attempts");
    assert_eq!(attempts.len(), 1, "resume created a duplicate attempt");
    assert_eq!(attempts[0].id, "sess-a");
}

#[test]
fn completed_report_is_reconstructible_without_canonical_questions() {
    let (db, _temp) = new_db_service();

    let assessment_id = "assessment-integrity-3";
    db.create_assessment(
        assessment_id,
        "Report Test",
        &[],
        &[new_question("Q1", "alpha", "expected alpha")],
    )
    .expect("failed to create assessment");

    let first_attempt = take_attempt(&db, assessment_id, "attempt1");

    // Wipe the canonical question set entirely (as deleting/replacing would).
    db.replace_assessment_questions(assessment_id, &[])
        .expect("failed to replace questions");

    // The completed attempt's answers and report data must still be fully
    // reconstructable from its own snapshot, independent of the canonical set.
    let answers = db
        .get_assessment_answers(&first_attempt)
        .expect("failed to load answers");
    assert_eq!(
        answers.len(),
        1,
        "completed attempt answers lost when canonical questions were removed"
    );
    assert_eq!(answers[0].question_prompt, "Q1");
    assert_eq!(answers[0].concept, "alpha");
    assert!(
        answers[0].evaluation.is_some(),
        "completed attempt evaluation lost"
    );
}

#[test]
fn legacy_schema_migration_repoints_answers_to_snapshots() {
    // Build a database file with the OLD assessment schema, where
    // assessment_answers.question_id referenced the mutable canonical
    // assessment_questions table. This is the state an existing user's
    // database is in before the snapshot migration.
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp.path().join("studycraft_legacy.db");
    let conn = rusqlite::Connection::open(&db_path).expect("failed to open legacy db");

    conn.execute_batch(queries::CREATE_REVIEW_MATERIALS_TABLE)
        .expect("create review materials");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS assessment_questions (\
         id INTEGER PRIMARY KEY AUTOINCREMENT,\
         assessment_id TEXT NOT NULL,\
         prompt TEXT NOT NULL,\
         concept TEXT NOT NULL,\
         difficulty TEXT NOT NULL,\
         expected_content TEXT NOT NULL DEFAULT '',\
         position INTEGER NOT NULL,\
         created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,\
         FOREIGN KEY (assessment_id) REFERENCES review_materials (id) ON DELETE CASCADE)",
    )
    .expect("create assessment questions");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS assessment_sessions (\
         id TEXT PRIMARY KEY,\
         assessment_id TEXT NOT NULL,\
         created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,\
         FOREIGN KEY (assessment_id) REFERENCES review_materials (id) ON DELETE CASCADE)",
    )
    .expect("create assessment sessions");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS assessment_answers (\
         id INTEGER PRIMARY KEY AUTOINCREMENT,\
         session_id TEXT NOT NULL,\
         question_id INTEGER NOT NULL,\
         student_answer TEXT NOT NULL,\
         grade TEXT,\
         rationale TEXT,\
         misconception TEXT,\
         created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,\
         FOREIGN KEY (session_id) REFERENCES assessment_sessions (id) ON DELETE CASCADE,\
         FOREIGN KEY (question_id) REFERENCES assessment_questions (id) ON DELETE CASCADE)",
    )
    .expect("create assessment answers");

    conn.execute(
        "INSERT INTO review_materials (id, name, type, last_review, created_at, updated_at)\
         VALUES ('legacy-assessment', 'Legacy', 'assessment', NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    )
    .expect("insert legacy review material");
    conn.execute(
        "INSERT INTO assessment_questions (assessment_id, prompt, concept, difficulty, expected_content, position)\
         VALUES ('legacy-assessment', 'Old question', 'legacy-concept', 'recall', 'legacy expected', 0)",
        [],
    )
    .expect("insert legacy question");
    conn.execute(
        "INSERT INTO assessment_sessions (id, assessment_id) VALUES ('legacy-session', 'legacy-assessment')",
        [],
    )
    .expect("insert legacy session");
    let question_id: i64 = conn
        .query_row("SELECT id FROM assessment_questions LIMIT 1", [], |r| {
            r.get(0)
        })
        .expect("legacy question id");
    conn.execute(
        "INSERT INTO assessment_answers (session_id, question_id, student_answer, grade)\
         VALUES ('legacy-session', ?1, 'legacy answer', 'correct')",
        [question_id],
    )
    .expect("insert legacy answer");
    conn.close().expect("close legacy db");

    // Opening the file through DbService must run the migration and repoint
    // the legacy answer to a freshly materialised session-question snapshot.
    let service = DbService::new_for_tests(&db_path).expect("failed to migrate legacy db");
    let answers = service
        .get_assessment_answers("legacy-session")
        .expect("failed to load migrated answers");
    assert_eq!(answers.len(), 1, "legacy answer lost during migration");
    assert_eq!(
        answers[0].question_prompt, "Old question",
        "legacy answer should carry its original question prompt"
    );
    assert_eq!(
        answers[0].concept, "legacy-concept",
        "legacy answer should carry its original concept"
    );
    assert!(
        matches!(
            answers[0].evaluation.as_ref().map(|e| &e.grade),
            Some(app_lib::materials::assessment::AssessmentGrade::Correct)
        ),
        "legacy answer should retain its grade after migration"
    );

    // The snapshot the migrated answer now points to must exist and be
    // independent of the canonical question set.
    let snapshots = service
        .get_assessment_session_questions("legacy-session")
        .expect("failed to load migrated snapshots");
    assert_eq!(
        snapshots.len(),
        1,
        "migrated session must have exactly one snapshot"
    );
    assert_eq!(snapshots[0].prompt, "Old question");
    assert_eq!(snapshots[0].expected_content, "legacy expected");
}
