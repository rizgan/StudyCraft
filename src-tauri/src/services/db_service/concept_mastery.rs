use super::connection::DbService;
use super::queries;
use crate::errors::{AppError, AppResult};
use crate::materials::assessment::{AssessmentDifficulty, AssessmentGrade};
use crate::materials::concept_mastery::{
    calculate_concept_mastery, concept_id_for_label, flashcard_rating_outcome, grade_outcome,
    MasteryDimension, MasteryObservation, MasteryState, WeakConcept,
};
use crate::materials::language_learning::LearningDirection;
use chrono::{DateTime, Utc};
use rusqlite::{params, Row};
use serde::Serialize;
use std::collections::BTreeMap;

/// The raw evidence records the mastery engine reads.
///
/// Assessment answers are the raw assessment evidence: they already exist
/// per answer with concept, difficulty, grade, misconception and timestamp in
/// `assessment_answers` joined to the immutable session snapshot. Flashcard
/// reviews have no raw per-review history in the schema, so every review is
/// appended to `flashcard_reviews` at the moment it happens.

/// One graded assessment answer across all assessments (raw evidence).
#[derive(Debug, Clone)]
pub struct AssessmentEvidenceRecord {
    pub answer_id: i64,
    pub session_id: String,
    #[allow(dead_code)]
    pub assessment_id: String,
    pub concept: String,
    pub difficulty: AssessmentDifficulty,
    pub grade: AssessmentGrade,
    pub misconception: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One recorded flashcard review (raw evidence).
#[derive(Debug, Clone)]
pub struct FlashcardReviewRecord {
    pub review_id: i64,
    pub card_id: i64,
    pub deck_id: Option<String>,
    pub rating: String,
    pub reviewed_at: DateTime<Utc>,
    /// The card's concept label (its language target expression when the card
    /// carries language metadata, otherwise its front text).
    pub concept_label: String,
    /// The card's learning direction when language metadata is present.
    pub direction: Option<LearningDirection>,
}

/// Raw fields of one flashcard-review evidence row, as read from the
/// joined query. Kept separate from the parsed record so the `query_map`
/// closure only performs `rusqlite`-level extraction.
#[derive(Debug, Clone)]
struct ReviewRowFields {
    review_id: i64,
    card_id: i64,
    deck_id: Option<String>,
    rating: String,
    reviewed_at: String,
    front: String,
    direction: Option<String>,
    target_expression: Option<String>,
}

/// Raw fields of one assessment-answer evidence row, as read from the
/// joined query.
#[derive(Debug, Clone)]
struct EvidenceRowFields {
    answer_id: i64,
    session_id: String,
    assessment_id: String,
    concept: String,
    difficulty: String,
    grade: String,
    misconception: Option<String>,
    created_at: String,
}

/// Scope of a mastery computation: everything or one assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MasteryScope {
    All,
    Assessment(String),
    Deck(String),
}

/// Low-level concept-mastery storage: the canonical concept registry, the
/// flashcard review log and the raw evidence reads.
///
/// `ensure_concept`, `record_flashcard_review`, `get_flashcard_reviews` and
/// `get_all_concepts` are exercised by the integration tests and form the
/// public storage API; the transactional write path in
/// `update_flashcard_after_review` issues the raw SQL directly so it can
/// share a transaction with the scheduling update.
#[allow(dead_code)]
pub trait ConceptMasteryOperations {
    /// Ensures the canonical concept row for a label's normalized identity
    /// exists and returns its id (the normalized label itself).
    fn ensure_concept(&self, label: &str) -> AppResult<String>;

    /// Lists the canonical concept registry.
    fn get_all_concepts(&self) -> AppResult<Vec<(String, String)>>;

    /// Appends one raw flashcard review observation for a card. Returns the
    /// new review row id.
    fn record_flashcard_review(
        &self,
        card_id: i64,
        rating: &str,
        reviewed_at: DateTime<Utc>,
    ) -> AppResult<i64>;

    /// Raw flashcard review evidence for one card, oldest first.
    fn get_flashcard_reviews(&self, card_id: i64) -> AppResult<Vec<FlashcardReviewRecord>>;

    /// All raw evidence of one scope, ready to be turned into
    /// [`MasteryObservation`]s grouped by canonical concept id.
    fn gather_mastery_evidence(
        &self,
        scope: &MasteryScope,
    ) -> AppResult<BTreeMap<String, (String, Vec<MasteryObservation>)>>;
}

/// Reads the mastery scope's raw evidence rows and folds them into per-concept
/// [`MasteryObservation`] lists keyed by canonical concept id.
///
/// Deterministic: the same database contents always produce the same
/// observations. No LLM is involved.
fn fold_evidence(
    db: &DbService,
    scope: &MasteryScope,
) -> AppResult<BTreeMap<String, (String, Vec<MasteryObservation>)>> {
    let mut gathered: BTreeMap<String, (String, Vec<MasteryObservation>)> = BTreeMap::new();

    let assessment_evidence = match scope {
        MasteryScope::All => db.get_all_assessment_evidence()?,
        MasteryScope::Assessment(assessment_id) => db.get_assessment_evidence(assessment_id)?,
        MasteryScope::Deck(_) => Vec::new(),
    };
    for record in assessment_evidence {
        let Some(concept_id) = concept_id_for_label(&record.concept) else {
            continue;
        };
        let observation = MasteryObservation {
            source_key: format!("assessment_answer:{}", record.answer_id),
            group_key: Some(record.session_id.clone()),
            dimension: dimension_from_difficulty(record.difficulty),
            outcome: grade_outcome(record.grade),
            occurred_at: record.created_at,
            misconception: record.misconception.clone(),
            description: format!(
                "Assessment session {}: {} answer graded {}",
                record.session_id,
                String::from(record.difficulty),
                String::from(record.grade)
            ),
            evidence_id: record.answer_id,
        };
        gathered
            .entry(concept_id)
            .or_insert_with(|| (record.concept.clone(), Vec::new()))
            .1
            .push(observation);
    }

    let flashcard_reviews = match scope {
        MasteryScope::All => db.get_all_flashcard_reviews()?,
        MasteryScope::Assessment(_) => Vec::new(),
        MasteryScope::Deck(deck_id) => db.get_deck_flashcard_reviews(deck_id)?,
    };
    for record in flashcard_reviews {
        let Some(concept_id) = concept_id_for_label(&record.concept_label) else {
            continue;
        };
        let Some(outcome) = flashcard_rating_outcome(&record.rating) else {
            continue;
        };
        let dimension = match record.direction {
            Some(LearningDirection::Recognition) => MasteryDimension::Recognition,
            Some(LearningDirection::Production) => MasteryDimension::Production,
            None => MasteryDimension::Recall,
        };
        let observation = MasteryObservation {
            source_key: format!("flashcard_review:{}", record.review_id),
            group_key: None,
            dimension,
            outcome,
            occurred_at: record.reviewed_at,
            misconception: None,
            description: format!("Flashcard {} rated '{}'", record.card_id, record.rating),
            evidence_id: record.review_id,
        };
        gathered
            .entry(concept_id)
            .or_insert_with(|| (record.concept_label.clone(), Vec::new()))
            .1
            .push(observation);
    }

    // Chronological order is part of the evidence contract: mastery is
    // rebuilt from raw rows that may arrive grouped by source, but the
    // observation lists handed to the aggregator must be time-ordered so
    // that "later" (e.g. a resolving correct answer) is well defined.
    for entry in gathered.values_mut() {
        entry.1.sort_by(|a, b| {
            a.occurred_at
                .cmp(&b.occurred_at)
                .then_with(|| a.evidence_id.cmp(&b.evidence_id))
        });
    }

    Ok(gathered)
}

/// Maps an assessment difficulty onto the mastery dimension of the same name.
pub fn dimension_from_difficulty(difficulty: AssessmentDifficulty) -> MasteryDimension {
    match difficulty {
        AssessmentDifficulty::Recall => MasteryDimension::Recall,
        AssessmentDifficulty::Understanding => MasteryDimension::Understanding,
        AssessmentDifficulty::Application => MasteryDimension::Application,
    }
}

/// Computes the mastery record of one concept from its gathered observations.
pub fn compute_concept_mastery(
    now: DateTime<Utc>,
    concept_id: &str,
    label: &str,
    observations: &[MasteryObservation],
) -> crate::materials::concept_mastery::ConceptMastery {
    calculate_concept_mastery(now, concept_id, label, observations)
}

/// Computes the mastery of every concept that has evidence in the given
/// scope. This is the single recalculation entry point: mastery is always
/// rebuilt from raw evidence, never mutated in place.
pub fn recalculate_mastery_for_scope(
    db: &DbService,
    scope: &MasteryScope,
) -> AppResult<Vec<crate::materials::concept_mastery::ConceptMastery>> {
    let gathered = fold_evidence(db, scope)?;
    let now = Utc::now();
    let mut mastery = gathered
        .into_iter()
        .map(|(concept_id, (label, observations))| {
            compute_concept_mastery(now, &concept_id, &label, &observations)
        })
        .collect::<Vec<_>>();
    mastery.sort_by(|a, b| a.concept_id.cmp(&b.concept_id));
    Ok(mastery)
}

/// A material-scoped mastery view: the concepts this material has evidence
/// for, with their full mastery records.
#[derive(Debug, Clone, Serialize)]
pub struct MaterialMastery {
    pub material_id: String,
    pub material_type: String,
    pub concepts: Vec<crate::materials::concept_mastery::ConceptMastery>,
}

/// The weak concepts of a scope: every concept that is not unseen and whose
/// overall state is limited by a weak, developing or learning dimension.
/// Sorted weakest-first, then by label, so consumers always see the most
/// urgent gaps first.
pub fn weak_concepts_for_scope(
    db: &DbService,
    scope: &MasteryScope,
) -> AppResult<Vec<WeakConcept>> {
    let mastery = recalculate_mastery_for_scope(db, scope)?;
    let mut weak: Vec<WeakConcept> = Vec::new();
    for m in mastery {
        if m.state == MasteryState::Unseen {
            continue;
        }
        let weakest = m
            .dimensions
            .iter()
            .map(|d| d.state)
            .min()
            .unwrap_or(MasteryState::Unseen);
        if matches!(
            weakest,
            MasteryState::Weak | MasteryState::Developing | MasteryState::Learning
        ) {
            weak.push(WeakConcept {
                concept_id: m.concept_id.clone(),
                label: m.label.clone(),
                weakest,
                mastery: m,
            });
        }
    }
    weak.sort_by(|a, b| {
        a.weakest
            .cmp(&b.weakest)
            .then_with(|| a.label.cmp(&b.label))
    });
    Ok(weak)
}

impl ConceptMasteryOperations for DbService {
    fn ensure_concept(&self, label: &str) -> AppResult<String> {
        let Some(concept_id) = concept_id_for_label(label) else {
            return Err(AppError::InvalidInput(
                "Concept label must not be blank".to_string(),
            ));
        };
        let conn = self.conn.borrow();
        conn.execute(
            "INSERT OR IGNORE INTO concepts (id, label) VALUES (?1, ?1)",
            params![concept_id],
        )
        .map_err(|e| AppError::DbError(format!("Failed to register concept: {}", e)))?;
        Ok(concept_id)
    }

    fn get_all_concepts(&self) -> AppResult<Vec<(String, String)>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT id, label FROM concepts ORDER BY id")
            .map_err(|e| AppError::DbError(format!("Failed to prepare concepts query: {}", e)))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| AppError::DbError(format!("Failed to list concepts: {}", e)))?;
        let mut concepts = Vec::new();
        for row in rows {
            let entry =
                row.map_err(|e| AppError::DbError(format!("Failed to read concept row: {}", e)))?;
            concepts.push(entry);
        }
        Ok(concepts)
    }

    fn record_flashcard_review(
        &self,
        card_id: i64,
        rating: &str,
        reviewed_at: DateTime<Utc>,
    ) -> AppResult<i64> {
        let conn = self.conn.borrow();
        conn.execute(
            queries::INSERT_FLASHCARD_REVIEW,
            params![
                card_id,
                rating,
                reviewed_at.format("%Y-%m-%d %H:%M:%S").to_string()
            ],
        )
        .map_err(|e| AppError::DbError(format!("Failed to record flashcard review: {}", e)))?;
        Ok(conn.last_insert_rowid())
    }

    fn get_flashcard_reviews(&self, card_id: i64) -> AppResult<Vec<FlashcardReviewRecord>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_FLASHCARD_REVIEWS_BY_CARD)
            .map_err(|e| AppError::DbError(format!("Failed to prepare review query: {}", e)))?;
        let rows = stmt
            .query_map(params![card_id], |row| review_row_fields(row))
            .map_err(|e| AppError::DbError(format!("Failed to query flashcard reviews: {}", e)))?;
        let mut reviews = Vec::new();
        for row in rows {
            let fields = row.map_err(|e| {
                AppError::DbError(format!("Failed to read flashcard review row: {}", e))
            })?;
            let record = review_fields_to_record(fields)?;
            reviews.push(record);
        }
        Ok(reviews)
    }

    fn gather_mastery_evidence(
        &self,
        scope: &MasteryScope,
    ) -> AppResult<BTreeMap<String, (String, Vec<MasteryObservation>)>> {
        fold_evidence(self, scope)
    }
}

/// Raw fields of one flashcard-review evidence row, extracted with only
/// `rusqlite::Error` so it can be used directly as a `query_map` closure.
fn review_row_fields(row: &Row) -> Result<ReviewRowFields, rusqlite::Error> {
    Ok(ReviewRowFields {
        review_id: row.get("id")?,
        card_id: row.get("card_id")?,
        deck_id: row.get("deck_id")?,
        rating: row.get("rating")?,
        reviewed_at: row.get("reviewed_at")?,
        front: row.get("front")?,
        direction: row.get("direction")?,
        target_expression: row.get("target_expression")?,
    })
}

/// Builds a flashcard-review evidence record from its raw row fields. The
/// concept identity is the card's language target expression when present
/// (so a recognition card and its production counterpart share the same
/// vocabulary concept), otherwise the card's front text.
fn review_fields_to_record(fields: ReviewRowFields) -> AppResult<FlashcardReviewRecord> {
    let direction = fields.direction.and_then(|d| LearningDirection::parse(&d));
    let raw_label = match fields.target_expression.as_deref().map(|t| t.trim()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => fields.front.clone(),
    };
    // The concept identity is the normalized label; fall back to the raw
    // label only when normalization yields nothing (blank text).
    let concept_label = concept_id_for_label(&raw_label).unwrap_or(raw_label);
    Ok(FlashcardReviewRecord {
        review_id: fields.review_id,
        card_id: fields.card_id,
        deck_id: fields.deck_id,
        rating: fields.rating,
        reviewed_at: parse_evidence_timestamp(&fields.reviewed_at)?,
        concept_label,
        direction,
    })
}

fn parse_evidence_timestamp(s: &str) -> AppResult<DateTime<Utc>> {
    let trimmed = s.trim();
    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f"))
        .map(|v| v.and_utc())
        .map_err(|e| AppError::DbError(format!("Failed to parse evidence timestamp {}: {}", s, e)))
}

impl DbService {
    /// All graded assessment answers across all assessments (raw evidence).
    pub fn get_all_assessment_evidence(&self) -> AppResult<Vec<AssessmentEvidenceRecord>> {
        self.get_assessment_evidence_inner(None)
    }

    /// All graded assessment answers of one assessment (raw evidence).
    pub fn get_assessment_evidence(
        &self,
        assessment_id: &str,
    ) -> AppResult<Vec<AssessmentEvidenceRecord>> {
        self.get_assessment_evidence_inner(Some(assessment_id))
    }

    fn get_assessment_evidence_inner(
        &self,
        assessment_id: Option<&str>,
    ) -> AppResult<Vec<AssessmentEvidenceRecord>> {
        let conn = self.conn.borrow();
        let sql = match assessment_id {
            Some(_) => queries::GET_ASSESSMENT_EVIDENCE_BY_ASSESSMENT,
            None => queries::GET_ALL_ASSESSMENT_EVIDENCE,
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| AppError::DbError(format!("Failed to prepare evidence query: {}", e)))?;
        let mut records = Vec::new();
        if let Some(id) = assessment_id {
            let rows = stmt
                .query_map(params![id], |row| evidence_row_fields(row))
                .map_err(|e| {
                    AppError::DbError(format!("Failed to query assessment evidence: {}", e))
                })?;
            for row in rows {
                let fields = row.map_err(|e| {
                    AppError::DbError(format!("Failed to read evidence row: {}", e))
                })?;
                if let Some(record) = evidence_fields_to_record(fields)? {
                    records.push(record);
                }
            }
        } else {
            let rows = stmt
                .query_map([], |row| evidence_row_fields(row))
                .map_err(|e| {
                    AppError::DbError(format!("Failed to query assessment evidence: {}", e))
                })?;
            for row in rows {
                let fields = row.map_err(|e| {
                    AppError::DbError(format!("Failed to read evidence row: {}", e))
                })?;
                if let Some(record) = evidence_fields_to_record(fields)? {
                    records.push(record);
                }
            }
        }
        Ok(records)
    }

    /// All raw flashcard review evidence across all decks, joined to each
    /// card's concept identity in a single query.
    pub fn get_all_flashcard_reviews(&self) -> AppResult<Vec<FlashcardReviewRecord>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_ALL_FLASHCARD_REVIEWS)
            .map_err(|e| AppError::DbError(format!("Failed to prepare review query: {}", e)))?;
        let rows = stmt
            .query_map([], |row| review_row_fields(row))
            .map_err(|e| AppError::DbError(format!("Failed to query flashcard reviews: {}", e)))?;
        let mut reviews = Vec::new();
        for row in rows {
            let fields = row.map_err(|e| {
                AppError::DbError(format!("Failed to read flashcard review row: {}", e))
            })?;
            let record = review_fields_to_record(fields)?;
            reviews.push(record);
        }
        Ok(reviews)
    }

    /// Raw flashcard review evidence scoped to one deck (deck-scoped read). A
    /// small, safe performance improvement over loading every review row and
    /// filtering in Rust: the deck filter is pushed down into SQL.
    pub fn get_deck_flashcard_reviews(
        &self,
        deck_id: &str,
    ) -> AppResult<Vec<FlashcardReviewRecord>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(queries::GET_FLASHCARD_REVIEWS_BY_DECK)
            .map_err(|e| AppError::DbError(format!("Failed to prepare review query: {}", e)))?;
        let rows = stmt
            .query_map(params![deck_id], |row| review_row_fields(row))
            .map_err(|e| AppError::DbError(format!("Failed to query flashcard reviews: {}", e)))?;
        let mut reviews = Vec::new();
        for row in rows {
            let fields = row.map_err(|e| {
                AppError::DbError(format!("Failed to read flashcard review row: {}", e))
            })?;
            let record = review_fields_to_record(fields)?;
            reviews.push(record);
        }
        Ok(reviews)
    }
}

/// Raw fields of one assessment-evidence row, extracted with only
/// `rusqlite::Error` so it can be used directly as a `query_map` closure.
fn evidence_row_fields(row: &Row) -> Result<EvidenceRowFields, rusqlite::Error> {
    Ok(EvidenceRowFields {
        answer_id: row.get("id")?,
        session_id: row.get("session_id")?,
        assessment_id: row.get("assessment_id")?,
        concept: row.get("concept")?,
        difficulty: row.get("difficulty")?,
        grade: row.get("grade")?,
        misconception: row.get("misconception")?,
        created_at: row.get("created_at")?,
    })
}

/// Builds an assessment-evidence record from its raw row fields. Rows with an
/// unknown difficulty fall back to recall (mirroring how the rest of the code
/// base reads legacy difficulty values); rows with an unknown grade are
/// dropped, since an unparseable grade must never be treated as evidence.
fn evidence_fields_to_record(
    fields: EvidenceRowFields,
) -> AppResult<Option<AssessmentEvidenceRecord>> {
    let difficulty = match AssessmentDifficulty::parse(&fields.difficulty) {
        Some(d) => d,
        None => {
            eprintln!(
                "Assessment answer {} stored with unknown difficulty '{}'; treating as recall",
                fields.answer_id, fields.difficulty
            );
            AssessmentDifficulty::Recall
        }
    };
    let grade = match AssessmentGrade::parse(&fields.grade) {
        Some(g) => g,
        None => {
            eprintln!(
                "Assessment answer {} stored with unknown grade '{}'; dropping from evidence",
                fields.answer_id, fields.grade
            );
            return Ok(None);
        }
    };
    Ok(Some(AssessmentEvidenceRecord {
        answer_id: fields.answer_id,
        session_id: fields.session_id,
        assessment_id: fields.assessment_id,
        concept: fields.concept,
        difficulty,
        grade,
        misconception: fields.misconception,
        created_at: parse_evidence_timestamp(&fields.created_at)?,
    }))
}

/// High-level mastery reads. Each is a thin, deterministic pipeline over
/// `recalculate_mastery_for_scope`; none of them caches or mutates anything.

/// Mastery of one concept from every source of evidence.
pub fn get_mastery_for_concept(
    db: &DbService,
    label: &str,
) -> AppResult<Option<crate::materials::concept_mastery::ConceptMastery>> {
    let Some(concept_id) = concept_id_for_label(label) else {
        return Ok(None);
    };
    let gathered = db.gather_mastery_evidence(&MasteryScope::All)?;
    Ok(gathered.get(&concept_id).map(|(label, observations)| {
        compute_concept_mastery(Utc::now(), &concept_id, label, observations)
    }))
}

/// Mastery of the concepts one material has evidence for.
pub fn get_mastery_for_material(db: &DbService, material_id: &str) -> AppResult<MaterialMastery> {
    let material_type: Option<String> = db
        .conn
        .borrow()
        .query_row(
            "SELECT type FROM review_materials WHERE id = ?1",
            params![material_id],
            |row| row.get(0),
        )
        .ok();
    let material_type = material_type
        .ok_or_else(|| AppError::NotFound(format!("Material {} not found", material_id)))?;
    // Assessments have concept-tagged answers; flashcard decks (and tests)
    // have flashcard review evidence scoped to the deck. A material's
    // mastery view only ever includes evidence produced by that material.
    let now = Utc::now();
    let mut concepts: BTreeMap<String, (String, Vec<MasteryObservation>)> = BTreeMap::new();
    if material_type == "assessment" {
        concepts.extend(
            db.gather_mastery_evidence(&MasteryScope::Assessment(material_id.to_string()))?,
        );
    } else {
        // Deck-scoped read: the deck filter is pushed down into SQL instead of
        // loading every review row and filtering in Rust.
        let reviews = db.get_deck_flashcard_reviews(material_id)?;
        for review in reviews.into_iter() {
            let Some(concept_id) = concept_id_for_label(&review.concept_label) else {
                continue;
            };
            let Some(outcome) = flashcard_rating_outcome(&review.rating) else {
                continue;
            };
            let dimension = match review.direction {
                Some(LearningDirection::Recognition) => MasteryDimension::Recognition,
                Some(LearningDirection::Production) => MasteryDimension::Production,
                None => MasteryDimension::Recall,
            };
            let observation = MasteryObservation {
                source_key: format!("flashcard_review:{}", review.review_id),
                group_key: None,
                dimension,
                outcome,
                occurred_at: review.reviewed_at,
                misconception: None,
                description: format!("Flashcard {} rated '{}'", review.card_id, review.rating),
                evidence_id: review.review_id,
            };
            concepts
                .entry(concept_id)
                .or_insert_with(|| (review.concept_label.clone(), Vec::new()))
                .1
                .push(observation);
        }
        // Chronological order is part of the evidence contract: mastery is
        // rebuilt from raw rows that may arrive grouped by source, but the
        // observation lists handed to the aggregator must be time-ordered so
        // that "later" (e.g. a resolving correct answer) is well defined.
        for entry in concepts.values_mut() {
            entry.1.sort_by(|a, b| {
                a.occurred_at
                    .cmp(&b.occurred_at)
                    .then_with(|| a.evidence_id.cmp(&b.evidence_id))
            });
        }
    }
    let mut list = concepts
        .into_iter()
        .map(|(concept_id, (label, observations))| {
            compute_concept_mastery(now, &concept_id, &label, &observations)
        })
        .collect::<Vec<_>>();
    list.sort_by(|a, b| a.concept_id.cmp(&b.concept_id));
    Ok(MaterialMastery {
        material_id: material_id.to_string(),
        material_type,
        concepts: list,
    })
}

/// The weak concepts across all evidence, weakest first.
pub fn weak_concepts(db: &DbService) -> AppResult<Vec<WeakConcept>> {
    weak_concepts_for_scope(db, &MasteryScope::All)
}

/// Full mastery rebuild for every concept with evidence.
pub fn recalculate_all_mastery(
    db: &DbService,
) -> AppResult<Vec<crate::materials::concept_mastery::ConceptMastery>> {
    recalculate_mastery_for_scope(db, &MasteryScope::All)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::assessment::NewAssessmentQuestion;
    use crate::materials::concept_mastery::MasteryConfidence;
    use crate::services::db_service::flashcard_decks::FlashcardDeckOperations;
    use crate::services::db_service::AssessmentOperations;

    fn new_db() -> (DbService, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp.path().join("studycraft_mastery.db");
        let service = DbService::new_for_tests(&db_path).expect("failed to open test database");
        (service, temp)
    }

    fn concept_mastery(
        db: &DbService,
        label: &str,
    ) -> crate::materials::concept_mastery::ConceptMastery {
        get_mastery_for_concept(db, label)
            .expect("mastery failed")
            .expect("concept present")
    }

    fn create_flashcard_deck_with_front(db: &DbService, deck_id: &str, front: &str) -> i64 {
        db.create_flashcard_deck(
            deck_id,
            "Deck",
            &[] as &[String],
            &[crate::materials::flashcard::NewFlashcard {
                front: front.to_string(),
                back: "back".to_string(),
                position: 0,
                deck_id: None,
            }],
            None,
            &[None],
        )
        .expect("failed to create deck");
        let cards = db.get_flashcards(deck_id).expect("failed to load cards");
        cards[0].id.expect("card id")
    }

    fn create_assessment_with_answers(
        db: &DbService,
        assessment_id: &str,
        session_id: &str,
        answers: &[(String, AssessmentDifficulty, AssessmentGrade, Option<&str>)],
    ) {
        let questions: Vec<NewAssessmentQuestion> = answers
            .iter()
            .enumerate()
            .map(|(i, (concept, difficulty, _, _))| NewAssessmentQuestion {
                prompt: format!("q{}", i),
                concept: concept.clone(),
                difficulty: *difficulty,
                expected_content: "expected".to_string(),
                position: i as i32,
            })
            .collect();
        db.create_assessment(assessment_id, "Mastery Test", &[] as &[String], &questions)
            .expect("failed to create assessment");
        let stored = db
            .get_assessment_questions(assessment_id)
            .expect("failed to load questions");
        db.create_assessment_session_with_questions(session_id, assessment_id, &stored)
            .expect("failed to create session");
        let snapshots = db
            .get_assessment_session_questions(session_id)
            .expect("failed to load snapshots");
        for (i, snapshot) in snapshots.iter().enumerate() {
            let (_, _, grade, misconception) = &answers[i];
            let snapshot_id = snapshot.id.expect("snapshot id");
            db.insert_assessment_answer(
                session_id,
                snapshot_id,
                "answer",
                Some(&crate::materials::assessment::AssessmentEvaluation {
                    grade: *grade,
                    rationale: "r".to_string(),
                    misconception: misconception.map(String::from),
                }),
            )
            .expect("failed to insert answer");
        }
    }

    #[test]
    fn concept_registry_is_deterministic_and_deduplicated() {
        let (db, _temp) = new_db();
        let id1 = db
            .ensure_concept("Precision vs Recall")
            .expect("ensure failed");
        let id2 = db
            .ensure_concept("  PRECISION   vs   recall ")
            .expect("ensure failed");
        assert_eq!(id1, id2, "normalized labels must map to the same concept");
        let concepts = db.get_all_concepts().expect("list concepts failed");
        assert_eq!(concepts.len(), 1, "no duplicate concept row");
        // Unrelated labels stay distinct: no fuzzy merging.
        let other = db
            .ensure_concept("Precision and Recall")
            .expect("ensure failed");
        assert_ne!(id1, other);
        let concepts = db.get_all_concepts().expect("list concepts failed");
        assert_eq!(concepts.len(), 2);
    }

    #[test]
    fn assessment_answers_are_raw_evidence_and_rebuildable() {
        let (db, _temp) = new_db();
        create_assessment_with_answers(
            &db,
            "a1",
            "s1",
            &[(
                "alpha".into(),
                AssessmentDifficulty::Recall,
                AssessmentGrade::Correct,
                None,
            )],
        );
        let evidence = db.get_all_assessment_evidence().expect("evidence failed");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].concept, "alpha");
        assert_eq!(evidence[0].grade, AssessmentGrade::Correct);

        let all = recalculate_all_mastery(&db).expect("recalc failed");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].label, "alpha");
        assert_eq!(all[0].state, MasteryState::Learning);
        // The single observation is explainable.
        assert_eq!(all[0].observation_count, 1);
    }

    #[test]
    fn flashcard_reviews_are_recorded_and_drive_mastery() {
        let (db, _temp) = new_db();
        let deck_id = "deck-mastery-1";
        db.create_flashcard_deck(
            deck_id,
            "Deck",
            &[] as &[String],
            &[crate::materials::flashcard::NewFlashcard {
                front: "What is 2+2?".to_string(),
                back: "4".to_string(),
                position: 0,
                deck_id: None,
            }],
            None,
            &[None],
        )
        .expect("failed to create deck");
        let cards = db.get_flashcards(deck_id).expect("failed to load cards");
        let card_id = cards[0].id.expect("card id");

        db.update_flashcard_after_review(card_id, "good", "evt-flashcard-1")
            .expect("review 1 failed");
        db.update_flashcard_after_review(card_id, "easy", "evt-flashcard-2")
            .expect("review 2 failed");

        let reviews = db.get_flashcard_reviews(card_id).expect("reviews failed");
        assert_eq!(reviews.len(), 2, "each review is a raw row");
        assert_eq!(reviews[0].rating, "good");
        assert_eq!(reviews[1].rating, "easy");
        assert_eq!(
            reviews[0].concept_label, "what is 2+2?",
            "a card's concept is its normalized front text"
        );

        let all = recalculate_all_mastery(&db).expect("recalc failed");
        assert_eq!(all.len(), 1);
        let m = &all[0];
        assert_eq!(
            m.dimensions[0].dimension,
            MasteryDimension::Recall,
            "non-language cards evidence recall"
        );
        // good (0.7) + easy (1.0), fresh => score 0.85, two observations =>
        // Strong by the deterministic rule (not Mastered: only two reviews).
        assert_eq!(m.state, MasteryState::Strong);
        assert!(
            m.confidence == MasteryConfidence::Low,
            "two observations => low"
        );
    }

    #[test]
    fn language_cards_share_one_concept_across_directions() {
        let (db, _temp) = new_db();
        let deck_id = "deck-lang-1";
        use crate::materials::language_learning::{CardTask, FlashcardLanguage, VocabularyItem};
        let item = VocabularyItem {
            target_expression: "كتاب".to_string(),
            meaning: "book".to_string(),
            ..VocabularyItem::default()
        };
        db.create_flashcard_deck(
            deck_id,
            "Lang",
            &[] as &[String],
            &[
                crate::materials::flashcard::NewFlashcard {
                    front: "كتاب".to_string(),
                    back: "book".to_string(),
                    position: 0,
                    deck_id: None,
                },
                crate::materials::flashcard::NewFlashcard {
                    front: "book".to_string(),
                    back: "كتاب".to_string(),
                    position: 1,
                    deck_id: None,
                },
            ],
            None,
            &[
                Some(FlashcardLanguage {
                    card_id: 0,
                    direction: LearningDirection::Recognition,
                    task: CardTask::Translation,
                    item: item.clone(),
                }),
                Some(FlashcardLanguage {
                    card_id: 0,
                    direction: LearningDirection::Production,
                    task: CardTask::Translation,
                    item,
                }),
            ],
        )
        .expect("failed to create deck");
        let cards = db.get_flashcards(deck_id).expect("failed to load cards");
        let rec_id = cards[0].id.expect("card id");
        let prod_id = cards[1].id.expect("card id");

        // Recognition: rated easy. Production: rated twice again.
        db.update_flashcard_after_review(rec_id, "easy", "evt-lang-1")
            .expect("review failed");
        db.update_flashcard_after_review(prod_id, "again", "evt-lang-2")
            .expect("review failed");
        db.update_flashcard_after_review(prod_id, "again", "evt-lang-3")
            .expect("review failed");

        let material = get_mastery_for_material(&db, deck_id).expect("material mastery failed");
        assert_eq!(
            material.concepts.len(),
            1,
            "both cards must evidence the same vocabulary concept"
        );
        let m = &material.concepts[0];
        let rec = m
            .dimensions
            .iter()
            .find(|d| d.dimension == MasteryDimension::Recognition)
            .expect("recognition dimension");
        let prod = m
            .dimensions
            .iter()
            .find(|d| d.dimension == MasteryDimension::Production)
            .expect("production dimension");
        assert_eq!(
            rec.state,
            MasteryState::Learning,
            "one easy rating is learning"
        );
        assert_eq!(
            prod.state,
            MasteryState::Weak,
            "two failed ratings are weak"
        );
        assert_eq!(
            m.state,
            MasteryState::Weak,
            "overall limited by weakest dimension"
        );
    }

    #[test]
    fn misconception_evidence_flows_into_mastery() {
        let (db, _temp) = new_db();
        create_assessment_with_answers(
            &db,
            "a2",
            "s2",
            &[
                (
                    "beta".into(),
                    AssessmentDifficulty::Application,
                    AssessmentGrade::Incorrect,
                    Some("thinks A implies B"),
                ),
                (
                    "beta".into(),
                    AssessmentDifficulty::Application,
                    AssessmentGrade::Correct,
                    None,
                ),
            ],
        );
        let all = recalculate_all_mastery(&db).expect("recalc failed");
        assert_eq!(all.len(), 1);
        let m = &all[0];
        assert_eq!(m.misconceptions.len(), 1);
        assert!(
            m.misconceptions[0].resolved,
            "a later correct answer must mark the misconception resolved"
        );
        assert_eq!(m.misconceptions[0].text, "thinks A implies B");
    }

    #[test]
    fn deleting_flashcard_removes_only_its_own_review_evidence() {
        let (db, _temp) = new_db();
        let deck_id = "deck-del-1";
        db.create_flashcard_deck(
            deck_id,
            "Deck",
            &[] as &[String],
            &[
                crate::materials::flashcard::NewFlashcard {
                    front: "card one".to_string(),
                    back: "one".to_string(),
                    position: 0,
                    deck_id: None,
                },
                crate::materials::flashcard::NewFlashcard {
                    front: "card two".to_string(),
                    back: "two".to_string(),
                    position: 1,
                    deck_id: None,
                },
            ],
            None,
            &[None, None],
        )
        .expect("failed to create deck");
        let cards = db.get_flashcards(deck_id).expect("failed to load cards");
        let first_id = cards[0].id.expect("card id");
        let second_id = cards[1].id.expect("card id");
        db.update_flashcard_after_review(first_id, "easy", "evt-del-1")
            .expect("review failed");
        db.update_flashcard_after_review(second_id, "easy", "evt-del-2")
            .expect("review failed");

        // Hard-delete the first flashcard (its FK ownership rule).
        db.conn
            .borrow()
            .execute("DELETE FROM flashcards WHERE id = ?1", params![first_id])
            .expect("delete card");

        let all = recalculate_all_mastery(&db).expect("recalc failed");
        assert_eq!(
            all.len(),
            1,
            "only the surviving card's concept must remain"
        );
        assert_eq!(all[0].label, "card two");
        assert_eq!(
            all[0].observation_count, 1,
            "the deleted card's review evidence went with its card"
        );
    }

    #[test]
    fn deleting_assessment_cascades_its_evidence_but_not_others() {
        let (db, _temp) = new_db();
        create_assessment_with_answers(
            &db,
            "a3",
            "s3",
            &[(
                "gamma".into(),
                AssessmentDifficulty::Recall,
                AssessmentGrade::Correct,
                None,
            )],
        );
        create_assessment_with_answers(
            &db,
            "a4",
            "s4",
            &[(
                "delta".into(),
                AssessmentDifficulty::Recall,
                AssessmentGrade::Correct,
                None,
            )],
        );

        // Hard-delete the first assessment (explicit ownership/FK rule).
        db.conn
            .borrow()
            .execute("DELETE FROM review_materials WHERE id = ?1", params!["a3"])
            .expect("delete material");

        let all = recalculate_all_mastery(&db).expect("recalc failed");
        assert_eq!(all.len(), 1, "the other assessment's evidence must survive");
        assert_eq!(all[0].label, "delta");
    }

    #[test]
    fn unobserved_dimension_is_not_penalized_and_caps_mastered() {
        let (db, _temp) = new_db();
        create_assessment_with_answers(
            &db,
            "cap-a",
            "cap-s",
            &[
                (
                    "capsule".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "capsule".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "capsule".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
            ],
        );
        let m = concept_mastery(&db, "capsule");
        // The single observed dimension is Mastered by its own rule.
        let recall = m
            .dimensions
            .iter()
            .find(|d| d.dimension == MasteryDimension::Recall)
            .expect("recall dimension must be present");
        assert_eq!(
            recall.state,
            MasteryState::Mastered,
            "a strong recall-only dimension must not be penalized by unobserved breadth"
        );
        // Overall is capped below Mastered by breadth, not by a penalty.
        assert_eq!(
            m.state,
            MasteryState::Strong,
            "recall-only evidence must not claim overall Mastered"
        );
        // Unobserved dimensions are not fabricated as scores.
        assert!(!m
            .dimensions
            .iter()
            .any(|d| d.dimension == MasteryDimension::Understanding));
        assert!(!m
            .dimensions
            .iter()
            .any(|d| d.dimension == MasteryDimension::Application));
        // Required breadth is expressed explicitly.
        assert!(m
            .required_dimensions
            .contains(&MasteryDimension::Understanding));
        assert!(m
            .required_dimensions
            .contains(&MasteryDimension::Application));
        assert_eq!(
            m.coverage,
            1.0 / 3.0,
            "one of the three required dimensions was observed"
        );
    }

    #[test]
    fn mastered_requires_full_breadth() {
        let (db, _temp) = new_db();
        // Each required dimension needs enough evidence to reach Mastered on
        // its own (3 correct observations, perfect score). Breadth alone does
        // not make a concept Mastered; the weakest dimension bounds it.
        create_assessment_with_answers(
            &db,
            "full-a",
            "full-s",
            &[
                (
                    "full".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Understanding,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Understanding,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Understanding,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Application,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Application,
                    AssessmentGrade::Correct,
                    None,
                ),
                (
                    "full".into(),
                    AssessmentDifficulty::Application,
                    AssessmentGrade::Correct,
                    None,
                ),
            ],
        );
        let m = concept_mastery(&db, "full");
        assert_eq!(
            m.state,
            MasteryState::Mastered,
            "full breadth at Mastered quality must reach overall Mastered"
        );
        assert_eq!(m.coverage, 1.0);
        assert!(m.observed_dimensions.contains(&MasteryDimension::Recall));
        assert!(m
            .observed_dimensions
            .contains(&MasteryDimension::Understanding));
        assert!(m
            .observed_dimensions
            .contains(&MasteryDimension::Application));
    }

    #[test]
    fn flashcard_review_is_idempotent_on_retry() {
        let (db, _temp) = new_db();
        let card_id = create_flashcard_deck_with_front(&db, "idem-deck", "idem");

        // First review of this card.
        db.update_flashcard_after_review(card_id, "good", "evt-idem-1")
            .expect("review 1 failed");
        let m1 = concept_mastery(&db, "idem");
        assert_eq!(m1.observation_count, 1, "one review = one observation");

        // Retry of the SAME review action (same event id) is a no-op.
        db.update_flashcard_after_review(card_id, "good", "evt-idem-1")
            .expect("review retry failed");
        let m2 = concept_mastery(&db, "idem");
        assert_eq!(m2.observation_count, 1, "retry must not double-count");

        // A new legitimate review of the same card creates a new row.
        db.update_flashcard_after_review(card_id, "good", "evt-idem-2")
            .expect("review 2 failed");
        let m3 = concept_mastery(&db, "idem");
        assert_eq!(
            m3.observation_count, 2,
            "new review event = new evidence row"
        );

        let reviews = db.get_flashcard_reviews(card_id).expect("reviews");
        assert_eq!(reviews.len(), 2, "exactly two raw review rows exist");
    }

    #[test]
    fn distinct_labels_are_not_merged_across_materials() {
        let (db, _temp) = new_db();
        // Two different materials that happen to use the same normalized label.
        let deck_a = "state-a";
        let deck_b = "state-b";
        let card_a = create_flashcard_deck_with_front(&db, deck_a, "state");
        let card_b = create_flashcard_deck_with_front(&db, deck_b, "state");
        db.update_flashcard_after_review(card_a, "easy", "evt-state-a")
            .expect("review a");
        db.update_flashcard_after_review(card_b, "easy", "evt-state-b")
            .expect("review b");

        // Per-material views are distinct even when labels collide.
        let mat_a = get_mastery_for_material(&db, deck_a).expect("mat a");
        let mat_b = get_mastery_for_material(&db, deck_b).expect("mat b");
        assert_eq!(mat_a.material_id, deck_a);
        assert_eq!(mat_b.material_id, deck_b);
        assert_eq!(mat_a.concepts.len(), 1);
        assert_eq!(mat_b.concepts.len(), 1);

        // Provenance records that the same concept id appears in two materials.
        let conn = db.conn.borrow();
        let occurrences: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM concept_material_occurrences \
                 WHERE concept_id = 'state'",
                [],
                |row| row.get(0),
            )
            .expect("count occurrences");
        assert_eq!(
            occurrences, 2,
            "the shared concept is provenance-linked to both materials"
        );
    }

    #[test]
    fn same_timestamp_misconception_resolves_by_numeric_id() {
        let (db, _temp) = new_db();
        // Three answers with an identical created_at: a wrong (misconception),
        // then a correct (resolving), in increasing numeric-id order. Under a
        // lexicographic source_key tiebreak the "resolving" correct answer
        // would sort before the wrong one and never resolve it.
        create_assessment_with_answers(
            &db,
            "ts-a",
            "ts-s",
            &[
                (
                    "ts-concept".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Incorrect,
                    Some("wrong reasoning"),
                ),
                (
                    "ts-concept".into(),
                    AssessmentDifficulty::Recall,
                    AssessmentGrade::Correct,
                    None,
                ),
            ],
        );
        let conn = db.conn.borrow();
        conn.execute(
            "UPDATE assessment_answers SET created_at = '2024-01-01 00:00:00' WHERE session_id = 'ts-s'",
            [],
        )
        .expect("set timestamps");
        drop(conn);

        let m = concept_mastery(&db, "ts-concept");
        // Historical misconceptions are never deleted; they are marked
        // resolved. The later correct answer (higher numeric id at the same
        // timestamp) must resolve the misconception.
        let entry = m
            .misconceptions
            .iter()
            .find(|c| c.text == "wrong reasoning");
        assert!(
            entry.is_some_and(|c| c.resolved),
            "the later correct answer must resolve the misconception"
        );
    }

    #[test]
    fn soft_deleted_assessment_evidence_is_excluded() {
        let (db, _temp) = new_db();
        create_assessment_with_answers(
            &db,
            "soft-a",
            "soft-s",
            &[(
                "soft-concept".into(),
                AssessmentDifficulty::Recall,
                AssessmentGrade::Correct,
                None,
            )],
        );
        let m0 = concept_mastery(&db, "soft-concept");
        assert!(
            m0.observation_count >= 1,
            "pre-deletion the evidence must be present"
        );

        // Soft-delete the assessment (production path sets deleted = 1).
        db.conn
            .borrow()
            .execute(
                "UPDATE review_materials SET deleted = 1 WHERE id = 'soft-a'",
                [],
            )
            .expect("soft delete");

        // All evidence for the concept is now soft-deleted, so the concept
        // has no observations and resolves to None (a concept with no
        // evidence is not present in the gathered map).
        let m = get_mastery_for_concept(&db, "soft-concept").expect("mastery failed");
        assert!(
            m.is_none(),
            "soft-deleted assessment evidence must not count"
        );
    }

    #[test]
    fn soft_deleted_flashcard_deck_reviews_are_excluded() {
        let (db, _temp) = new_db();
        let deck_id = "softfc-deck";
        let card_id = create_flashcard_deck_with_front(&db, deck_id, "softfc");
        db.update_flashcard_after_review(card_id, "easy", "evt-softfc")
            .expect("review failed");
        let m0 = concept_mastery(&db, "softfc");
        assert!(
            m0.observation_count >= 1,
            "pre-deletion the evidence must be present"
        );

        db.conn
            .borrow()
            .execute(
                "UPDATE review_materials SET deleted = 1 WHERE id = ?1",
                params![deck_id],
            )
            .expect("soft delete deck");

        let m = get_mastery_for_concept(&db, "softfc").expect("mastery failed");
        assert!(
            m.is_none(),
            "soft-deleted deck review evidence must not count"
        );
        // The raw rows still exist (soft delete keeps them); only the mastery
        // view excludes them.
        let reviews = db.get_flashcard_reviews(card_id).expect("reviews");
        assert_eq!(reviews.len(), 1, "raw review rows are preserved");
    }

    #[test]
    fn deck_scoped_review_read_matches_unscoped() {
        let (db, _temp) = new_db();
        let deck_a = "scop-a-deck";
        let deck_b = "scop-b-deck";
        let card_a = create_flashcard_deck_with_front(&db, deck_a, "scop-a");
        let card_b = create_flashcard_deck_with_front(&db, deck_b, "scop-b");
        db.update_flashcard_after_review(card_a, "easy", "evt-scope-a")
            .expect("review a");
        db.update_flashcard_after_review(card_b, "easy", "evt-scope-b")
            .expect("review b");

        let scoped = db.get_deck_flashcard_reviews(deck_a).expect("scoped");
        let unscoped = db.get_all_flashcard_reviews().expect("unscoped");
        let expected: Vec<_> = unscoped
            .iter()
            .filter(|r| r.deck_id.as_deref() == Some(deck_a))
            .collect();
        assert_eq!(scoped.len(), expected.len(), "scoped count must match");
        assert_eq!(scoped.len(), 1, "only deck-a's review is in scope");
        for (scoped, expected) in scoped.iter().zip(expected.iter()) {
            assert_eq!(scoped.review_id, expected.review_id);
            assert_eq!(scoped.reviewed_at, expected.reviewed_at);
        }
    }

    #[test]
    fn concept_provenance_tracks_materials() {
        let (db, _temp) = new_db();
        let deck_a = "prov-a";
        let deck_b = "prov-b";
        let card_a = create_flashcard_deck_with_front(&db, deck_a, "prov");
        let card_b = create_flashcard_deck_with_front(&db, deck_b, "prov");
        // The review hook registers the concept's provenance in the same tx.
        db.update_flashcard_after_review(card_a, "easy", "evt-prov-a")
            .expect("review a");
        db.update_flashcard_after_review(card_b, "easy", "evt-prov-b")
            .expect("review b");

        // One concept row (same label), but two material occurrences.
        let conn = db.conn.borrow();
        let concept_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM concepts WHERE id = 'prov'",
                [],
                |row| row.get(0),
            )
            .expect("count concepts");
        let material_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM concept_material_occurrences WHERE concept_id = 'prov'",
                [],
                |row| row.get(0),
            )
            .expect("count occurrences");
        assert_eq!(concept_rows, 1, "one canonical concept row for the label");
        assert_eq!(
            material_rows, 2,
            "both materials are provenance-linked to the concept"
        );
    }

    #[test]
    fn assessment_answer_upsert_preserves_created_at() {
        let (db, _temp) = new_db();
        create_assessment_with_answers(
            &db,
            "tsup-a",
            "tsup-s",
            &[(
                "tsup-concept".into(),
                AssessmentDifficulty::Recall,
                AssessmentGrade::Correct,
                None,
            )],
        );
        let snapshots = db
            .get_assessment_session_questions("tsup-s")
            .expect("snapshots");
        let snapshot_id = snapshots[0].id.expect("snapshot id");

        let conn = db.conn.borrow();
        conn.execute(
            "UPDATE assessment_answers SET created_at = '2023-05-05 12:00:00' WHERE session_id = 'tsup-s'",
            [],
        )
        .expect("set original timestamp");
        drop(conn);

        // Re-upsert the same answer (idempotency path). The historical
        // created_at must survive, not be rewritten to CURRENT_TIMESTAMP.
        db.insert_assessment_answer(
            "tsup-s",
            snapshot_id,
            "answer",
            Some(&crate::materials::assessment::AssessmentEvaluation {
                grade: AssessmentGrade::Correct,
                rationale: "r".to_string(),
                misconception: None,
            }),
        )
        .expect("re-upsert failed");

        let conn = db.conn.borrow();
        let created_at: String = conn
            .query_row(
                "SELECT created_at FROM assessment_answers WHERE session_id = 'tsup-s'",
                [],
                |row| row.get(0),
            )
            .expect("read created_at");
        assert!(
            created_at.starts_with("2023-05-05"),
            "upsert must preserve the historical created_at, got {}",
            created_at
        );
    }
}
