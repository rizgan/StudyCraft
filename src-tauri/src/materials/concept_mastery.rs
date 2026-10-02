use super::assessment::AssessmentGrade;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The mastery dimensions a concept can be evidenced in.
///
/// `Recall`, `Understanding` and `Application` mirror the assessment
/// difficulty ladder (each assessment answer carries exactly one of these).
/// `Recognition` and `Production` are the generic extra dimensions used by
/// language-learning evidence; they are not special-cased anywhere — any
/// concept can carry evidence in any dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasteryDimension {
    Recall,
    Understanding,
    Application,
    Recognition,
    Production,
}

#[allow(dead_code)]
impl MasteryDimension {
    pub fn as_str(&self) -> &'static str {
        match self {
            MasteryDimension::Recall => "recall",
            MasteryDimension::Understanding => "understanding",
            MasteryDimension::Application => "application",
            MasteryDimension::Recognition => "recognition",
            MasteryDimension::Production => "production",
        }
    }

    /// Strict parse; unknown values are rejected rather than silently coerced.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "recall" => Some(MasteryDimension::Recall),
            "understanding" => Some(MasteryDimension::Understanding),
            "application" => Some(MasteryDimension::Application),
            "recognition" => Some(MasteryDimension::Recognition),
            "production" => Some(MasteryDimension::Production),
            _ => None,
        }
    }
}

/// The strong type for "what does this learner currently know about this
/// concept?". Deliberately not a percentage: a state is only ever produced by
/// the deterministic aggregation in [`calculate_concept_mastery`].
///
/// The declaration order is the *worsening* order: `Unseen` (no evidence) is
/// the weakest and `Mastered` the strongest. `Ord` is derived over that order
/// so `min()` over a set of dimension states yields the weakest demonstrated
/// dimension, which is what bounds a concept's overall state.
///
/// Note the non-obvious placement: a single unproven positive observation is
/// `Learning`, which is *better* than `Weak` (demonstrated failure); a
/// demonstrated partial understanding (`Developing`) is better than an
/// unproven single observation (`Learning`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasteryState {
    Unseen,
    Weak,
    Learning,
    Developing,
    Strong,
    Mastered,
}

#[allow(dead_code)]
impl MasteryState {
    pub fn as_str(&self) -> &'static str {
        match self {
            MasteryState::Unseen => "unseen",
            MasteryState::Learning => "learning",
            MasteryState::Weak => "weak",
            MasteryState::Developing => "developing",
            MasteryState::Strong => "strong",
            MasteryState::Mastered => "mastered",
        }
    }
}

/// How much independent evidence supports a mastery result. Kept separate
/// from the mastery state: a concept can be `Strong` with `Low` confidence
/// when only a handful of observations back it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasteryConfidence {
    Low,
    Moderate,
    High,
}

#[allow(dead_code)]
impl MasteryConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            MasteryConfidence::Low => "low",
            MasteryConfidence::Moderate => "moderate",
            MasteryConfidence::High => "high",
        }
    }
}

/// One unit of learner evidence about a concept.
///
/// This is the input of the deterministic mastery calculation. It never
/// carries a score that was guessed by an LLM: `outcome` is always derived
/// from a stored, graded observation (assessment answer or flashcard review
/// rating).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasteryObservation {
    /// Stable key of the originating evidence row, e.g.
    /// `assessment_answer:<id>` or `flashcard_review:<id>`.
    pub source_key: String,
    /// Independent-evidence group. Assessment answers group by session id
    /// (a whole attempt is one unit of independent evidence); flashcard
    /// reviews are each their own unit, so they carry `None`.
    pub group_key: Option<String>,
    pub dimension: MasteryDimension,
    /// Deterministic outcome score in 0.0..=1.0 derived from the stored grade
    /// or rating.
    pub outcome: f64,
    pub occurred_at: DateTime<Utc>,
    /// Misconception text reported by the evaluator for this observation
    /// (assessment answers only).
    pub misconception: Option<String>,
    /// Structured, human-readable description of this observation, used for
    /// explainability (e.g. "Attempt #2: incorrect application answer").
    pub description: String,
    /// Stable numeric ordering key for this evidence row: the primary-key id
    /// of the raw row in its source table. This is the deterministic
    /// tiebreaker when two pieces of evidence share a timestamp; unlike a
    /// lexicographic string comparison it orders 5 before 10.
    pub evidence_id: i64,
}

/// A tracked misconception for a concept, kept distinct from "answered
/// incorrectly". Historical misconceptions are never deleted; they carry an
/// explicit resolved status instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisconceptionRecord {
    pub text: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub occurrences: u32,
    /// True when a later positive observation (after `last_seen_at`) shows
    /// the learner no longer exhibits this misconception.
    pub resolved: bool,
    /// When the resolving positive observation occurred.
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Per-dimension mastery of one concept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionMastery {
    pub dimension: MasteryDimension,
    pub state: MasteryState,
    /// Deterministic weighted score in 0.0..=1.0.
    pub score: f64,
    pub observation_count: u32,
}

/// The canonical "what does this learner currently know about this concept"
/// record. Every field is derived from stored evidence by
/// [`calculate_concept_mastery`] and is therefore fully explainable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptMastery {
    pub concept_id: String,
    pub label: String,
    pub state: MasteryState,
    pub confidence: MasteryConfidence,
    /// Only dimensions that have evidence are present. Recall /
    /// Understanding / Application are kept separate and are never
    /// collapsed into one number.
    pub dimensions: Vec<DimensionMastery>,
    /// The dimensions that have at least one observation (breadth of evidence).
    pub observed_dimensions: Vec<MasteryDimension>,
    /// The dimensions that should be demonstrated for this concept. General
    /// concepts require {Recall, Understanding, Application}; language
    /// concepts require {Recognition, Production}. Unobserved required
    /// dimensions are never scored as failures; they only limit how high the
    /// overall state can reach (see `coverage`).
    pub required_dimensions: Vec<MasteryDimension>,
    /// Fraction of required dimensions that have been observed, in 0..=1.
    pub coverage: f64,
    pub observation_count: u32,
    pub misconceptions: Vec<MisconceptionRecord>,
    pub first_evidence_at: Option<DateTime<Utc>>,
    pub last_evidence_at: Option<DateTime<Utc>>,
}

/// A weak concept: not unseen, and limited by its weakest demonstrated
/// dimension.
#[derive(Debug, Clone, Serialize)]
pub struct WeakConcept {
    pub concept_id: String,
    pub label: String,
    /// Weakest dimension state across the demonstrated dimensions.
    pub weakest: MasteryState,
    pub mastery: ConceptMastery,
}

// Deterministic aggregation parameters.
pub const MASTERY_HALF_LIFE_DAYS: f64 = 30.0;
pub const MASTERY_MIN_RECENCY_WEIGHT: f64 = 0.05;
pub const MASTERY_DEVELOPING_THRESHOLD: f64 = 0.25;
pub const MASTERY_STRONG_THRESHOLD: f64 = 0.6;
pub const MASTERY_MASTERED_THRESHOLD: f64 = 0.85;
pub const MASTERY_MASTERED_MIN_OBSERVATIONS: u32 = 3;
pub const MASTERY_CONFIDENCE_MIN_OBSERVATIONS: u32 = 3;
pub const MASTERY_CONFIDENCE_MIN_UNITS: u32 = 2;
pub const MASTERY_HIGH_CONFIDENCE_OBSERVATIONS: u32 = 6;

/// Canonical concept identity for a concept label.
///
/// Concepts are identified by the normalized form of their label. The label
/// is the only existing concept identifier in the codebase (assessment
/// questions, session snapshots and answers all carry a `concept: String`);
/// no stronger identifier exists, so the normalized label IS the canonical
/// identity. Normalization is deliberately exact (casefold + whitespace
/// collapse): no fuzzy matching and no LLM-based merging are ever used to
/// decide that two concepts are the same.
pub fn concept_id_for_label(label: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut last_space = false;
    for ch in label.trim().to_lowercase().chars() {
        if ch.is_whitespace() {
            if !last_space {
                normalized.push(' ');
                last_space = true;
            }
        } else {
            normalized.push(ch);
            last_space = false;
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// The dimensions that should be demonstrated for a concept, inferred from
/// which dimension families the observed evidence touches. A concept that
/// shows any general-dimension evidence requires the general set; a concept
/// that shows any language-dimension evidence requires the language set.
/// Irrelevant dimensions are never forced: a pure language concept does not
/// require Recall / Understanding / Application, and vice versa.
pub fn required_dimensions_for(observed: &[MasteryDimension]) -> Vec<MasteryDimension> {
    let mut required: BTreeSet<MasteryDimension> = BTreeSet::new();
    let touches_general = observed.iter().any(|d| {
        matches!(
            d,
            MasteryDimension::Recall
                | MasteryDimension::Understanding
                | MasteryDimension::Application
        )
    });
    let touches_language = observed.iter().any(|d| {
        matches!(
            d,
            MasteryDimension::Recognition | MasteryDimension::Production
        )
    });
    if touches_general {
        required.insert(MasteryDimension::Recall);
        required.insert(MasteryDimension::Understanding);
        required.insert(MasteryDimension::Application);
    }
    if touches_language {
        required.insert(MasteryDimension::Recognition);
        required.insert(MasteryDimension::Production);
    }
    required.into_iter().collect()
}

/// Gate a dimension state by the breadth of evidence: a concept cannot claim
/// Mastered unless every required dimension has been observed (full coverage).
/// Below Mastered the state is purely a function of observed quality —
/// unobserved dimensions neither penalize nor constrain it.
pub fn gate_state_by_breadth(state: MasteryState, coverage: f64) -> MasteryState {
    if state == MasteryState::Mastered && coverage < 1.0 {
        MasteryState::Strong
    } else {
        state
    }
}

/// Deterministic outcome score for an assessment grade. Correct answers are
/// full evidence of the dimension; partial is half; incorrect and insufficient
/// contribute nothing but still count as observations.
pub fn grade_outcome(grade: AssessmentGrade) -> f64 {
    match grade {
        AssessmentGrade::Correct => 1.0,
        AssessmentGrade::PartiallyCorrect => 0.5,
        AssessmentGrade::Incorrect | AssessmentGrade::Insufficient => 0.0,
    }
}

/// Deterministic outcome score for a flashcard review rating. The rating is
/// the learner's self-report of how hard the recall was, mapped onto the same
/// 0..=1 outcome scale as assessment grades so both evidence kinds are
/// directly comparable. Unknown ratings are rejected rather than guessed.
pub fn flashcard_rating_outcome(rating: &str) -> Option<f64> {
    match rating.trim().to_lowercase().as_str() {
        "again" => Some(0.0),
        "hard" => Some(0.4),
        "good" => Some(0.7),
        "easy" => Some(1.0),
        _ => None,
    }
}

/// Recency weight of one observation: exponential decay with a 30-day half
/// life and a floor, so historical evidence is always preserved and never
/// vanishes. Deterministic: a pure function of the timestamps.
pub fn recency_weight(occurred_at: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    let age_days = (occurred_at - now).num_seconds().abs() as f64 / 86_400.0;
    let weight = 0.5_f64.powf(age_days / MASTERY_HALF_LIFE_DAYS);
    weight.max(MASTERY_MIN_RECENCY_WEIGHT)
}

/// Deterministic per-dimension state from the weighted score and the number
/// of observations. `Mastered` is never reachable from a single correct
/// answer: it requires both a high score and a minimum number of
/// observations.
pub fn dimension_state(count: u32, score: f64) -> MasteryState {
    match count {
        0 => MasteryState::Unseen,
        // A single observation is always "just started": even a perfect one
        // is not yet evidence of durable knowledge.
        1 => {
            if score <= 0.0 {
                MasteryState::Weak
            } else {
                MasteryState::Learning
            }
        }
        _ => {
            if score >= MASTERY_MASTERED_THRESHOLD && count >= MASTERY_MASTERED_MIN_OBSERVATIONS {
                MasteryState::Mastered
            } else if score >= MASTERY_STRONG_THRESHOLD {
                MasteryState::Strong
            } else if score >= MASTERY_DEVELOPING_THRESHOLD {
                MasteryState::Developing
            } else {
                MasteryState::Weak
            }
        }
    }
}

/// Deterministic confidence from the amount of independent evidence.
///
/// - `Low`: fewer than 3 observations in total.
/// - `Moderate`: enough observations, but all of them from a single
///   independent evidence unit (e.g. one assessment attempt), or 3–5
///   observations spread across units.
/// - `High`: 6+ observations with at least 2 independent evidence units.
pub fn mastery_confidence(observation_count: u32, independent_units: u32) -> MasteryConfidence {
    if observation_count < MASTERY_CONFIDENCE_MIN_OBSERVATIONS {
        MasteryConfidence::Low
    } else if independent_units < MASTERY_CONFIDENCE_MIN_UNITS
        || observation_count < MASTERY_HIGH_CONFIDENCE_OBSERVATIONS
    {
        MasteryConfidence::Moderate
    } else {
        MasteryConfidence::High
    }
}

/// Deterministic aggregation of a concept's observations into its mastery
/// record. Pure function of `(now, concept_id, label, observations)`; the
/// same inputs always produce the same output. No LLM is involved.
pub fn calculate_concept_mastery(
    now: DateTime<Utc>,
    concept_id: &str,
    label: &str,
    observations: &[MasteryObservation],
) -> ConceptMastery {
    // Work over a chronologically ordered copy so that "later" is well
    // defined regardless of how the caller ordered the input. Ties on
    // timestamp break deterministically on the source key.
    let mut ordered: Vec<&MasteryObservation> = observations.iter().collect();
    ordered.sort_by(|a, b| {
        a.occurred_at
            .cmp(&b.occurred_at)
            .then_with(|| a.evidence_id.cmp(&b.evidence_id))
    });

    let mut by_dimension: BTreeMap<MasteryDimension, Vec<&MasteryObservation>> = BTreeMap::new();
    for observation in &ordered {
        by_dimension
            .entry(observation.dimension)
            .or_default()
            .push(observation);
    }

    let mut dimensions = Vec::new();
    for (dimension, obs) in &by_dimension {
        let count = obs.len() as u32;
        let (weighted_sum, weight_total) = obs.iter().fold((0.0f64, 0.0f64), |acc, o| {
            let w = recency_weight(o.occurred_at, now);
            (acc.0 + w * o.outcome, acc.1 + w)
        });
        let score = if weight_total > 0.0 {
            (weighted_sum / weight_total).min(1.0)
        } else {
            0.0
        };
        dimensions.push(DimensionMastery {
            dimension: *dimension,
            state: dimension_state(count, score),
            score,
            observation_count: count,
        });
    }

    // Breadth of evidence: which dimensions were observed, which should be
    // demonstrated for this concept, and how much of that has been observed.
    // Unobserved dimensions are not scored as failures; they only limit how
    // high the overall state can reach.
    let observed_dimensions: Vec<MasteryDimension> =
        dimensions.iter().map(|d| d.dimension).collect();
    let required_dimensions = required_dimensions_for(&observed_dimensions);
    let coverage = if required_dimensions.is_empty() {
        0.0
    } else {
        observed_dimensions.len() as f64 / required_dimensions.len() as f64
    };

    // Overall state = the weakest demonstrated dimension, gated by breadth: a
    // concept cannot claim Mastered unless every required dimension has been
    // observed (sufficient breadth). Unobserved required dimensions are never
    // scored as failures; they only prevent the Mastered claim.
    let weakest = dimensions
        .iter()
        .map(|d| d.state)
        .min()
        .unwrap_or(MasteryState::Unseen);
    let overall = gate_state_by_breadth(weakest, coverage);

    // Misconceptions: kept per distinct text, with an explicit resolution
    // status. A misconception is resolved when a later observation (strictly
    // after its last occurrence) shows positive understanding of the concept
    // without carrying that misconception. Historical records are never
    // dropped, only marked resolved.
    let mut misconceptions: BTreeMap<String, MisconceptionRecord> = BTreeMap::new();
    for observation in &ordered {
        let Some(text) = observation
            .misconception
            .as_ref()
            .filter(|m| !m.trim().is_empty())
        else {
            continue;
        };
        let entry = misconceptions
            .entry(text.clone())
            .or_insert_with(|| MisconceptionRecord {
                text: text.clone(),
                first_seen_at: observation.occurred_at,
                last_seen_at: observation.occurred_at,
                occurrences: 0,
                resolved: false,
                resolved_at: None,
            });
        entry.first_seen_at = entry.first_seen_at.min(observation.occurred_at);
        entry.last_seen_at = entry.last_seen_at.max(observation.occurred_at);
        entry.occurrences += 1;
    }
    // The last position (in chronological order) of each misconception.
    // Resolution is decided by position, not by timestamp alone, because
    // raw evidence timestamps can share a second: a correct answer recorded
    // in the same second as a misconception does not count as "later".
    let last_pos: BTreeMap<String, usize> = ordered
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            let text = o.misconception.as_deref()?.trim();
            if text.is_empty() {
                None
            } else {
                Some((text.to_string(), i))
            }
        })
        .fold(BTreeMap::new(), |mut acc, (text, i)| {
            acc.entry(text)
                .and_modify(|last| *last = (*last).max(i))
                .or_insert(i);
            acc
        });
    for (text, entry) in misconceptions.iter_mut() {
        let last = last_pos[text];
        let resolving: Option<&MasteryObservation> = ordered
            .iter()
            .enumerate()
            .find(|(i, o)| {
                *i > last
                    && o.outcome >= MASTERY_STRONG_THRESHOLD
                    && o.misconception
                        .as_deref()
                        .map(|m| m.trim().is_empty())
                        .unwrap_or(true)
            })
            .map(|(_, o)| *o);
        if let Some(resolving) = resolving {
            entry.resolved = true;
            entry.resolved_at = Some(resolving.occurred_at);
        }
    }
    let misconceptions = misconceptions.into_values().collect::<Vec<_>>();

    // Independent evidence units: assessment answers group by session,
    // flashcard reviews stand alone.
    let mut units: BTreeMap<&str, ()> = BTreeMap::new();
    for observation in &ordered {
        let key = match &observation.group_key {
            Some(group) => group.as_str(),
            None => observation.source_key.as_str(),
        };
        units.entry(key).or_insert(());
    }
    let independent_units = units.len() as u32;
    let observation_count = observations.len() as u32;

    let first_evidence_at = observations.iter().map(|o| o.occurred_at).min();
    let last_evidence_at = observations.iter().map(|o| o.occurred_at).max();

    ConceptMastery {
        concept_id: concept_id.to_string(),
        label: label.to_string(),
        state: overall,
        confidence: mastery_confidence(observation_count, independent_units),
        dimensions,
        observed_dimensions,
        required_dimensions,
        coverage,
        observation_count,
        misconceptions,
        first_evidence_at,
        last_evidence_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_750_000_000, 0).unwrap()
    }

    fn obs(
        source: &str,
        dimension: MasteryDimension,
        outcome: f64,
        days_ago: i64,
        misconception: Option<&str>,
    ) -> MasteryObservation {
        let evidence_id: i64 = source.parse().unwrap_or(0);
        MasteryObservation {
            source_key: format!("assessment_answer:{}", source),
            group_key: Some(format!("session-{}", source)),
            dimension,
            outcome,
            occurred_at: now() - Duration::days(days_ago),
            misconception: misconception.map(String::from),
            description: format!("test obs {}", source),
            evidence_id,
        }
    }

    fn dim_state(m: &ConceptMastery, d: MasteryDimension) -> Option<MasteryState> {
        m.dimensions
            .iter()
            .find(|x| x.dimension == d)
            .map(|x| x.state)
    }

    #[test]
    fn no_evidence_is_unseen() {
        let m = calculate_concept_mastery(now(), "c1", "C1", &[]);
        assert_eq!(m.state, MasteryState::Unseen);
        assert!(m.dimensions.is_empty());
        assert!(m.confidence == MasteryConfidence::Low);
        assert_eq!(m.observation_count, 0);
    }

    #[test]
    fn one_correct_answer_is_learning_not_mastered() {
        let m = calculate_concept_mastery(
            now(),
            "c1",
            "C1",
            &[obs("1", MasteryDimension::Recall, 1.0, 0, None)],
        );
        assert_eq!(
            dim_state(&m, MasteryDimension::Recall),
            Some(MasteryState::Learning)
        );
        assert_eq!(m.state, MasteryState::Learning);
        assert!(m.confidence == MasteryConfidence::Low);
    }

    #[test]
    fn repeated_correct_recall_evidence_reaches_dimension_mastered_but_not_overall() {
        let observations: Vec<MasteryObservation> = (0..3)
            .map(|i| obs(&i.to_string(), MasteryDimension::Recall, 1.0, 0, None))
            .collect();
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        // The Recall dimension itself reaches Mastered...
        assert_eq!(
            dim_state(&m, MasteryDimension::Recall),
            Some(MasteryState::Mastered)
        );
        // ...but the overall state is capped at Strong because Understanding
        // and Application were never observed (insufficient breadth).
        // Unobserved dimensions are not penalized as failures, but they do
        // prevent the Mastered claim.
        assert_eq!(m.state, MasteryState::Strong);
        assert!(m.coverage < 1.0);
    }

    #[test]
    fn recall_strong_but_application_weak() {
        let observations = vec![
            obs("1", MasteryDimension::Recall, 1.0, 0, None),
            obs("2", MasteryDimension::Recall, 1.0, 1, None),
            obs("3", MasteryDimension::Recall, 1.0, 2, None),
            obs("4", MasteryDimension::Application, 0.0, 0, Some("m")),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            dim_state(&m, MasteryDimension::Recall),
            Some(MasteryState::Mastered)
        );
        assert_eq!(
            dim_state(&m, MasteryDimension::Application),
            Some(MasteryState::Weak)
        );
        // The overall state is limited by the weakest demonstrated
        // dimension, not averaged away.
        assert_eq!(m.state, MasteryState::Weak);
    }

    #[test]
    fn incorrect_answers_are_weak() {
        let observations = vec![
            obs("1", MasteryDimension::Recall, 0.0, 0, None),
            obs("2", MasteryDimension::Recall, 0.0, 1, None),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            dim_state(&m, MasteryDimension::Recall),
            Some(MasteryState::Weak)
        );
        assert_eq!(m.state, MasteryState::Weak);
    }

    #[test]
    fn partial_answers_land_in_developing() {
        let observations = vec![
            obs("1", MasteryDimension::Understanding, 0.5, 0, Some("m")),
            obs("2", MasteryDimension::Understanding, 0.5, 1, None),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            dim_state(&m, MasteryDimension::Understanding),
            Some(MasteryState::Developing)
        );
    }

    #[test]
    fn misconception_tracked_and_resolved() {
        let observations = vec![
            obs(
                "1",
                MasteryDimension::Application,
                0.0,
                10,
                Some("thinks recall means precision"),
            ),
            obs(
                "2",
                MasteryDimension::Application,
                0.0,
                8,
                Some("thinks recall means precision"),
            ),
            obs("3", MasteryDimension::Application, 1.0, 2, None),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(m.misconceptions.len(), 1);
        let mc = &m.misconceptions[0];
        assert_eq!(mc.occurrences, 2);
        assert!(
            mc.resolved,
            "later correct answer must resolve the misconception"
        );
        assert!(mc.resolved_at.is_some());
        assert_eq!(
            mc.resolved_at.unwrap(),
            now() - Duration::days(2),
            "resolved_at is the resolving observation's timestamp"
        );
        // Historical record survives with its full history.
        assert_eq!(
            mc.first_seen_at,
            now() - Duration::days(10),
            "first occurrence timestamp preserved"
        );
        assert_eq!(
            mc.last_seen_at,
            now() - Duration::days(8),
            "last occurrence timestamp preserved"
        );
    }

    #[test]
    fn unresolved_misconception_stays_open() {
        let observations = vec![
            obs("1", MasteryDimension::Application, 0.0, 10, Some("m")),
            obs("2", MasteryDimension::Application, 0.5, 5, Some("m")),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(m.misconceptions.len(), 1);
        assert!(!m.misconceptions[0].resolved);
        assert!(m.misconceptions[0].resolved_at.is_none());
        // The partial-correct observation still carries the misconception.
        assert_eq!(m.misconceptions[0].occurrences, 2);
    }

    #[test]
    fn old_and_recent_evidence_are_weighted_differently() {
        // Same grades, one very old and one fresh: the recency-weighted
        // score must be strictly higher than the one dominated by the old
        // failure.
        let fresh_fail_old_pass = vec![
            obs("1", MasteryDimension::Recall, 1.0, 365, None),
            obs("2", MasteryDimension::Recall, 0.0, 0, None),
        ];
        let recent_pass = vec![
            obs("1", MasteryDimension::Recall, 1.0, 1, None),
            obs("2", MasteryDimension::Recall, 1.0, 0, None),
        ];
        let m1 = calculate_concept_mastery(now(), "c1", "C1", &fresh_fail_old_pass);
        let m2 = calculate_concept_mastery(now(), "c1", "C1", &recent_pass);
        let s1 = m1.dimensions[0].score;
        let s2 = m2.dimensions[0].score;
        assert!(
            s2 > s1,
            "recent correct evidence must outweigh an old failure"
        );
        // The old failure still counts (never destroyed): one correct + one
        // incorrect is not the same as two correct.
        assert!(
            s1 < 1.0,
            "an old failure must still pull the score below perfect"
        );
    }

    #[test]
    fn insufficient_evidence_keeps_confidence_low() {
        let observations = vec![
            obs("1", MasteryDimension::Recall, 1.0, 0, None),
            obs("2", MasteryDimension::Recall, 1.0, 0, None),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert!(
            m.confidence == MasteryConfidence::Low,
            "2 observations => low confidence"
        );
    }

    #[test]
    fn cross_attempt_evidence_raises_confidence() {
        let observations = vec![
            // Two sessions, three answers in one and three in the other.
            obs("1", MasteryDimension::Recall, 1.0, 5, None),
            obs("2", MasteryDimension::Recall, 1.0, 4, None),
            obs("3", MasteryDimension::Recall, 1.0, 3, None),
            obs("4", MasteryDimension::Recall, 1.0, 2, None),
            obs("5", MasteryDimension::Recall, 1.0, 1, None),
            obs("6", MasteryDimension::Recall, 1.0, 0, None),
        ];
        // Re-point obs 4..6 to a different session to model a reassessment.
        let mut observations = observations;
        for o in observations.iter_mut().skip(3) {
            o.group_key = Some("session-b".to_string());
        }
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert!(m.confidence == MasteryConfidence::High);
    }

    #[test]
    fn single_source_many_observations_cap_at_moderate() {
        // Six answers, but all from a single assessment session: one
        // independent evidence unit, so confidence is capped at moderate.
        let observations: Vec<MasteryObservation> = (0..6)
            .map(|i| obs(&i.to_string(), MasteryDimension::Recall, 1.0, 0, None))
            .collect::<Vec<_>>();
        let observations: Vec<MasteryObservation> = observations
            .into_iter()
            .map(|mut o| {
                o.group_key = Some("one-session".to_string());
                o
            })
            .collect();
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert!(
            m.confidence == MasteryConfidence::Moderate,
            "six observations all from one session must not read as high confidence"
        );
    }

    #[test]
    fn reassessment_improvement_reflects_in_state_and_keeps_history() {
        // Attempt 1: application weak with a misconception, now well in the
        // past so recency weighting lets the fresh attempt dominate.
        // Attempt 2 (after remediation): application strong.
        let mut observations = vec![
            obs(
                "1",
                MasteryDimension::Application,
                0.0,
                90,
                Some("confuses precision and recall"),
            ),
            obs(
                "2",
                MasteryDimension::Application,
                0.0,
                90,
                Some("confuses precision and recall"),
            ),
            // Attempt 2: three fresh correct application answers in a new
            // session (a reassessment).
            obs("3", MasteryDimension::Application, 1.0, 1, None),
            obs("4", MasteryDimension::Application, 1.0, 1, None),
            obs("5", MasteryDimension::Application, 1.0, 0, None),
        ];
        for o in observations.iter_mut().skip(2) {
            o.group_key = Some("session-2".to_string());
        }
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            dim_state(&m, MasteryDimension::Application),
            Some(MasteryState::Mastered),
            "five observations dominated by the fresh correct attempt"
        );
        // The historical misconception is preserved and marked resolved, not
        // deleted.
        assert_eq!(m.misconceptions.len(), 1);
        assert!(m.misconceptions[0].resolved);
        // All five historical observations remain visible.
        assert_eq!(m.observation_count, 5);
    }

    #[test]
    fn recognition_strong_production_weak() {
        let observations = vec![
            obs("1", MasteryDimension::Recognition, 1.0, 2, None),
            obs("2", MasteryDimension::Recognition, 1.0, 1, None),
            obs("3", MasteryDimension::Recognition, 1.0, 0, None),
            obs("4", MasteryDimension::Production, 0.0, 1, None),
            obs("5", MasteryDimension::Production, 0.0, 0, None),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            dim_state(&m, MasteryDimension::Recognition),
            Some(MasteryState::Mastered)
        );
        assert_eq!(
            dim_state(&m, MasteryDimension::Production),
            Some(MasteryState::Weak)
        );
        assert_eq!(m.state, MasteryState::Weak);
    }

    #[test]
    fn deterministic_same_inputs_same_output() {
        let observations = vec![
            obs("1", MasteryDimension::Recall, 1.0, 3, None),
            obs("2", MasteryDimension::Application, 0.0, 2, Some("m")),
            obs("3", MasteryDimension::Understanding, 0.5, 1, None),
        ];
        let a = calculate_concept_mastery(now(), "c1", "C1", &observations);
        let b = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "same evidence must always produce byte-identical mastery"
        );
        // And a different `now` must not change the result in a way that is
        // not just recency weighting: states from identical relative
        // evidence are stable.
        let later = now() + Duration::hours(1);
        let c = calculate_concept_mastery(later, "c1", "C1", &observations);
        assert_eq!(
            a.state, c.state,
            "a one-hour shift must not flip the overall state"
        );
    }

    #[test]
    fn concept_id_normalization_is_exact_not_fuzzy() {
        assert_eq!(
            concept_id_for_label("Precision vs Recall"),
            Some("precision vs recall".to_string())
        );
        assert_eq!(
            concept_id_for_label("  Precision   Vs   RECALL  "),
            Some("precision vs recall".to_string()),
            "casefold + whitespace collapse"
        );
        // Near-miss labels are NOT merged: no fuzzy matching.
        assert_ne!(
            concept_id_for_label("Precision vs Recall"),
            concept_id_for_label("Precision and Recall")
        );
        assert_eq!(
            concept_id_for_label("   "),
            None,
            "blank labels have no concept"
        );
        assert_eq!(concept_id_for_label(""), None);
    }

    #[test]
    fn recency_weight_bounded_and_monotonic() {
        let t = now();
        let w0 = recency_weight(t, t);
        let w30 = recency_weight(t - Duration::days(30), t);
        let w365 = recency_weight(t - Duration::days(365), t);
        assert!((w0 - 1.0).abs() < f64::EPSILON);
        assert!(
            (w30 - 0.5).abs() < 0.01,
            "half life: weight after 30 days must be ~0.5"
        );
        assert!(w365 > MASTERY_MIN_RECENCY_WEIGHT - f64::EPSILON);
        assert!(w365 < w30, "older evidence weighs less");
    }

    #[test]
    fn recall_only_is_not_overall_mastered() {
        // Three perfect recall answers: the Recall dimension is Mastered, but
        // Understanding and Application were never observed, so the overall
        // state must not claim Mastered.
        let observations: Vec<MasteryObservation> = (0..3)
            .map(|i| obs(&i.to_string(), MasteryDimension::Recall, 1.0, 0, None))
            .collect();
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(m.state, MasteryState::Strong, "recall-only caps at Strong");
        assert!(m.coverage < 1.0);
        // The unobserved required dimensions are declared, but they are not
        // penalized: they do not appear as dimensions and do not lower the
        // state below what the observed evidence supports.
        assert!(m
            .required_dimensions
            .contains(&MasteryDimension::Understanding));
        assert!(m
            .required_dimensions
            .contains(&MasteryDimension::Application));
        assert!(!m
            .dimensions
            .iter()
            .any(|d| d.dimension == MasteryDimension::Understanding));
        assert!(!m
            .dimensions
            .iter()
            .any(|d| d.dimension == MasteryDimension::Application));
    }

    #[test]
    fn full_breadth_reaches_mastered() {
        // All three general dimensions observed with Mastered-quality
        // evidence: full coverage, so the overall state may be Mastered.
        let observations: Vec<MasteryObservation> = (0..3)
            .flat_map(|i| {
                [
                    obs(&format!("r{}", i), MasteryDimension::Recall, 1.0, 0, None),
                    obs(
                        &format!("u{}", i),
                        MasteryDimension::Understanding,
                        1.0,
                        0,
                        None,
                    ),
                    obs(
                        &format!("a{}", i),
                        MasteryDimension::Application,
                        1.0,
                        0,
                        None,
                    ),
                ]
            })
            .enumerate()
            .map(|(k, o)| {
                // Give each observation a distinct numeric evidence id.
                let mut o = o;
                o.evidence_id = k as i64;
                o
            })
            .collect();
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(m.state, MasteryState::Mastered);
        assert!((m.coverage - 1.0).abs() < f64::EPSILON);
        assert_eq!(m.observed_dimensions.len(), 3);
        assert_eq!(m.required_dimensions.len(), 3);
    }

    #[test]
    fn language_breadth_reaches_mastered() {
        // Both language dimensions observed with Mastered-quality evidence:
        // full coverage for a language concept, so overall Mastered.
        let observations: Vec<MasteryObservation> = (0..3)
            .flat_map(|i| {
                [
                    obs(
                        &format!("r{}", i),
                        MasteryDimension::Recognition,
                        1.0,
                        0,
                        None,
                    ),
                    obs(
                        &format!("p{}", i),
                        MasteryDimension::Production,
                        1.0,
                        0,
                        None,
                    ),
                ]
            })
            .enumerate()
            .map(|(k, o)| {
                let mut o = o;
                o.evidence_id = k as i64;
                o
            })
            .collect();
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(m.state, MasteryState::Mastered);
        assert!((m.coverage - 1.0).abs() < f64::EPSILON);
        // A language concept must not be forced to require general dimensions.
        assert!(
            !m.required_dimensions.iter().any(|d| matches!(
                d,
                MasteryDimension::Recall
                    | MasteryDimension::Understanding
                    | MasteryDimension::Application
            )),
            "language concept must not require general dimensions"
        );
    }

    #[test]
    fn required_dimensions_are_language_only_for_language_concepts() {
        // A recognition-only concept is a language concept: it requires
        // Recognition + Production, and nothing general.
        let observations = vec![obs("1", MasteryDimension::Recognition, 1.0, 0, None)];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert_eq!(
            m.required_dimensions,
            vec![MasteryDimension::Recognition, MasteryDimension::Production]
        );
        assert!(m.coverage < 1.0);
        assert_eq!(m.state, MasteryState::Learning);
    }

    #[test]
    fn dimension_state_threshold_boundaries() {
        // Developing boundary at 0.25.
        assert_eq!(
            dimension_state(2, 0.249_999),
            MasteryState::Weak,
            "just below the developing threshold is weak"
        );
        assert_eq!(
            dimension_state(2, 0.25),
            MasteryState::Developing,
            "at the developing threshold is developing"
        );
        // Strong boundary at 0.6.
        assert_eq!(
            dimension_state(2, 0.599_999),
            MasteryState::Developing,
            "just below the strong threshold is developing"
        );
        assert_eq!(
            dimension_state(2, 0.6),
            MasteryState::Strong,
            "at the strong threshold is strong"
        );
        // Mastered boundary at 0.85, and it still needs 3+ observations.
        assert_eq!(
            dimension_state(3, 0.849_999),
            MasteryState::Strong,
            "just below the mastered threshold is strong"
        );
        assert_eq!(
            dimension_state(3, 0.85),
            MasteryState::Mastered,
            "at the mastered threshold with enough observations is mastered"
        );
        // Below the mastered observation minimum, even a perfect score is not
        // mastered.
        assert_eq!(
            dimension_state(2, 1.0),
            MasteryState::Strong,
            "two perfect observations are not mastered"
        );
    }

    #[test]
    fn numeric_evidence_id_orders_same_timestamp_deterministically() {
        // Two observations in the same second. The misconception is on the
        // lower evidence id and the clean correct observation is on the higher
        // one, so in true numeric order the correct answer comes later and
        // resolves the misconception. (A lexicographic string comparison of
        // the source keys would order "10" before "5" and fail to resolve.)
        let observations = vec![
            obs("5", MasteryDimension::Application, 0.0, 0, Some("m")),
            obs("10", MasteryDimension::Application, 1.0, 0, None),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert!(
            m.misconceptions[0].resolved,
            "numeric ordering must place the later-id correct answer after the misconception"
        );

        // The mirror image: the correct answer has the lower id, the
        // misconception the higher one. In numeric order the correct answer
        // comes first, so the misconception is NOT resolved.
        let observations = vec![
            obs("5", MasteryDimension::Application, 1.0, 0, None),
            obs("10", MasteryDimension::Application, 0.0, 0, Some("m")),
        ];
        let m = calculate_concept_mastery(now(), "c1", "C1", &observations);
        assert!(
            !m.misconceptions[0].resolved,
            "numeric ordering must not resolve when the correct answer precedes the misconception"
        );
    }
}
