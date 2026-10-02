use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Difficulty level of an assessment question, following the recall →
/// understanding → application progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentDifficulty {
    Recall,
    Understanding,
    Application,
}

impl AssessmentDifficulty {
    /// Parses a difficulty string exactly. Unknown or missing values are
    /// rejected instead of being silently coerced to a default.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "recall" => Some(AssessmentDifficulty::Recall),
            "understanding" => Some(AssessmentDifficulty::Understanding),
            "application" => Some(AssessmentDifficulty::Application),
            _ => None,
        }
    }
}

impl From<AssessmentDifficulty> for String {
    fn from(difficulty: AssessmentDifficulty) -> Self {
        let s = match difficulty {
            AssessmentDifficulty::Recall => "recall",
            AssessmentDifficulty::Understanding => "understanding",
            AssessmentDifficulty::Application => "application",
        };
        s.to_string()
    }
}

/// A single free-text assessment question targeting a specific concept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentQuestion {
    pub id: Option<i64>,
    pub assessment_id: String,
    /// Question text in a new formulation, never a repeat of stored cards/questions.
    pub prompt: String,
    /// The concept this question targets.
    pub concept: String,
    pub difficulty: AssessmentDifficulty,
    /// Source-grounded reference content used to grade free-text answers.
    pub expected_content: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// New assessment question for creation (used when storing generated questions).
///
/// `expected_content` carries the source-grounded reference material the answer
/// is graded against, so evaluation is judged against what the student studied
/// rather than the LLM's own knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAssessmentQuestion {
    pub prompt: String,
    pub concept: String,
    pub difficulty: AssessmentDifficulty,
    pub expected_content: String,
    pub position: i32,
}

/// How the student answered an assessment question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentGrade {
    Correct,
    PartiallyCorrect,
    Incorrect,
    Insufficient,
}

impl AssessmentGrade {
    /// Parses a grade string exactly. Unknown values are rejected (an unknown
    /// grade must never silently become "correct"), so callers can surface a
    /// structured evaluation error instead of a fabricated grade.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "correct" => Some(AssessmentGrade::Correct),
            "partially_correct" | "partially correct" | "partial" => {
                Some(AssessmentGrade::PartiallyCorrect)
            }
            "incorrect" | "wrong" => Some(AssessmentGrade::Incorrect),
            "insufficient" => Some(AssessmentGrade::Insufficient),
            _ => None,
        }
    }
}

impl From<AssessmentGrade> for String {
    fn from(grade: AssessmentGrade) -> Self {
        let s = match grade {
            AssessmentGrade::Correct => "correct",
            AssessmentGrade::PartiallyCorrect => "partially_correct",
            AssessmentGrade::Incorrect => "incorrect",
            AssessmentGrade::Insufficient => "insufficient",
        };
        s.to_string()
    }
}

/// Structured evaluation of one free-text answer produced by the evaluator agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentEvaluation {
    pub grade: AssessmentGrade,
    /// One- to three-sentence reason for the grade, stored with the session.
    pub rationale: String,
    /// The specific part of the concept the student got wrong (or `None`
    /// when the answer shows full or no understanding).
    pub misconception: Option<String>,
}

/// A recorded answer to one question of one assessment session.
///
/// `question_id` references the immutable session-question snapshot row the
/// answer belongs to (never the mutable canonical question).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentAnswer {
    pub id: Option<i64>,
    pub session_id: String,
    pub question_id: i64,
    pub question_prompt: String,
    pub concept: String,
    pub difficulty: AssessmentDifficulty,
    pub student_answer: String,
    pub evaluation: Option<AssessmentEvaluation>,
}

/// An immutable snapshot of one question as it was presented in a specific
/// assessment session. Captured at session start and never mutated, so a
/// completed attempt's questions (and their reference content) survive any
/// later reassessment of the assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentSessionQuestion {
    pub id: Option<i64>,
    pub session_id: String,
    /// The canonical question id this snapshot was copied from, when available.
    /// Never relied on historically: it can become NULL if the canonical set
    /// is later replaced.
    #[serde(default)]
    pub source_question_id: Option<i64>,
    pub prompt: String,
    pub concept: String,
    pub difficulty: AssessmentDifficulty,
    /// The reference content the answer is graded against, frozen at session
    /// start so older attempts are never evaluated against newer content.
    pub expected_content: String,
    pub position: i32,
}

/// Summary of one assessment attempt (session) used to expose attempt history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentAttempt {
    pub id: String,
    pub assessment_id: String,
    /// `active` (pending) or `completed`.
    pub status: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub answer_count: i64,
}

/// Concept-level result aggregated from the answers of a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentConceptResult {
    pub concept: String,
    pub status: AssessmentConceptStatus,
    /// Human-readable summary of the concept performance, including the main
    /// misconception when one was identified.
    pub rationale: String,
    /// Distinct misconceptions identified for this concept, if any.
    pub misconceptions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentConceptStatus {
    Mastered,
    PartiallyUnderstood,
    Weak,
}

/// The assessment report: per-concept status plus suggested targets for
/// remediation. Deliberately has no single aggregate score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentReport {
    pub session_id: String,
    pub total_questions: u32,
    pub mastered: Vec<AssessmentConceptResult>,
    pub partially_understood: Vec<AssessmentConceptResult>,
    pub weak: Vec<AssessmentConceptResult>,
    /// Suggested focus for the next study/review pass, derived from weak and
    /// partially understood concepts (weak first, then partial ones that have
    /// misconceptions).
    pub remediation_targets: Vec<RemediationTarget>,
}

/// Concept-level target used to focus remediation decks on weak concepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationTarget {
    pub concept: String,
    pub status: AssessmentConceptStatus,
    /// Why remediation is suggested (misconceptions when known).
    pub reason: String,
    pub misconceptions: Vec<String>,
    /// Where the focused remediation set should start: weak concepts go back
    /// to recall, partially understood ones to understanding.
    pub suggested_difficulty: AssessmentDifficulty,
}

/// Request payload for the LLM evaluation of one answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateAnswerRequest {
    pub question_prompt: String,
    pub concept: String,
    pub difficulty: AssessmentDifficulty,
    /// What the source material says, so the evaluator can judge the answer
    /// against it instead of its own knowledge.
    pub expected_content: String,
    pub student_answer: String,
    pub language: String,
}
