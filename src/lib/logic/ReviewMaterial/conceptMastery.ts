import { invoke } from '@tauri-apps/api/core';

/**
 * Concept Mastery Engine frontend bindings.
 *
 * These are thin wrappers over the deterministic backend. No scores are
 * computed or guessed here: every value comes from the Rust engine, which
 * rebuilds mastery from stored raw evidence. See
 * `src-tauri/src/materials/concept_mastery.rs` for the canonical model.
 */

export type MasteryDimension =
  | 'recall'
  | 'understanding'
  | 'application'
  | 'recognition'
  | 'production';

export type MasteryState =
  | 'unseen'
  | 'weak'
  | 'learning'
  | 'developing'
  | 'strong'
  | 'mastered';

export type MasteryConfidence = 'low' | 'moderate' | 'high';

export interface MisconceptionRecord {
  text: string;
  first_seen_at: string;
  last_seen_at: string;
  occurrences: number;
  resolved: boolean;
  resolved_at?: string | null;
}

export interface DimensionMastery {
  dimension: MasteryDimension;
  state: MasteryState;
  /** Deterministic recency-weighted score in 0..=1. */
  score: number;
  observation_count: number;
}

/**
 * The canonical "what does this learner currently know about this concept"
 * record. Every field is derived from stored evidence and is therefore
 * fully explainable.
 */
export interface ConceptMastery {
  concept_id: string;
  label: string;
  state: MasteryState;
  confidence: MasteryConfidence;
  /** Only dimensions that have evidence are present. */
  dimensions: DimensionMastery[];
  observation_count: number;
  misconceptions: MisconceptionRecord[];
  first_evidence_at?: string | null;
  last_evidence_at?: string | null;
}

export interface MaterialMastery {
  material_id: string;
  material_type: string;
  concepts: ConceptMastery[];
}

export interface WeakConcept {
  concept_id: string;
  label: string;
  weakest: MasteryState;
  mastery: ConceptMastery;
}

const DIMENSION_LABELS: Record<MasteryDimension, string> = {
  recall: 'Recall',
  understanding: 'Understanding',
  application: 'Application',
  recognition: 'Recognition',
  production: 'Production',
};

const STATE_LABELS: Record<MasteryState, string> = {
  unseen: 'Not seen',
  weak: 'Weak',
  learning: 'Learning',
  developing: 'Developing',
  strong: 'Strong',
  mastered: 'Mastered',
};

const CONFIDENCE_LABELS: Record<MasteryConfidence, string> = {
  low: 'Low',
  moderate: 'Moderate',
  high: 'High',
};

export function dimensionLabel(dimension: MasteryDimension): string {
  return DIMENSION_LABELS[dimension] ?? dimension;
}

export function stateLabel(state: MasteryState): string {
  return STATE_LABELS[state] ?? state;
}

export function confidenceLabel(confidence: MasteryConfidence): string {
  return CONFIDENCE_LABELS[confidence] ?? confidence;
}

/** CSS class for a state badge so per-state colors apply. */
export function stateBadgeClass(state: MasteryState): string {
  return `mastery-badge state-${state}`;
}

/** CSS class for a per-dimension state cell so per-state colors apply. */
export function dimensionStateClass(state: MasteryState): string {
  return `mastery-dimension-state state-${state}`;
}

function normalizeState(raw: string): MasteryState {
  const value = raw.trim().toLowerCase();
  if (
    value === 'unseen' ||
    value === 'weak' ||
    value === 'learning' ||
    value === 'developing' ||
    value === 'strong' ||
    value === 'mastered'
  ) {
    return value;
  }
  return 'unseen';
}

function normalizeConfidence(raw: string): MasteryConfidence {
  const value = raw.trim().toLowerCase();
  if (value === 'low' || value === 'moderate' || value === 'high') return value;
  return 'low';
}

function normalizeDimension(raw: string): MasteryDimension {
  const value = raw.trim().toLowerCase();
  if (
    value === 'recall' ||
    value === 'understanding' ||
    value === 'application' ||
    value === 'recognition' ||
    value === 'production'
  ) {
    return value;
  }
  return 'recall';
}

export function normalizeMisconception(raw: any): MisconceptionRecord {
  return {
    text: raw.text ?? '',
    first_seen_at: raw.first_seen_at ?? '',
    last_seen_at: raw.last_seen_at ?? '',
    occurrences: raw.occurrences ?? 0,
    resolved: raw.resolved ?? false,
    resolved_at: raw.resolved_at ?? null,
  };
}

export function normalizeDimensionMastery(raw: any): DimensionMastery {
  return {
    dimension: normalizeDimension(String(raw.dimension ?? 'recall')),
    state: normalizeState(String(raw.state ?? 'unseen')),
    score: typeof raw.score === 'number' ? raw.score : 0,
    observation_count: raw.observation_count ?? 0,
  };
}

export function normalizeConceptMastery(raw: any): ConceptMastery {
  return {
    concept_id: raw.concept_id ?? '',
    label: raw.label ?? '',
    state: normalizeState(String(raw.state ?? 'unseen')),
    confidence: normalizeConfidence(String(raw.confidence ?? 'low')),
    dimensions: (raw.dimensions ?? []).map(normalizeDimensionMastery),
    observation_count: raw.observation_count ?? 0,
    misconceptions: (raw.misconceptions ?? []).map(normalizeMisconception),
    first_evidence_at: raw.first_evidence_at ?? null,
    last_evidence_at: raw.last_evidence_at ?? null,
  };
}

/**
 * Mastery of one concept, from every source of evidence. The label is
 * normalized to its canonical identity before lookup, so casing and
 * whitespace variants address the same concept.
 */
export async function getConceptMastery(conceptLabel: string): Promise<ConceptMastery | null> {
  const raw = await invoke<any>('get_concept_mastery', { conceptLabel });
  if (raw === null || raw === undefined) return null;
  return normalizeConceptMastery(raw);
}

/**
 * Mastery of the concepts one material has evidence for. For an assessment
 * this is the set of concepts its answered questions touched; for a
 * flashcard deck this is the set of concepts its reviewed cards cover.
 */
export async function getMasteryForMaterial(materialId: string): Promise<MaterialMastery> {
  const raw = await invoke<any>('get_material_mastery', { materialId });
  return {
    material_id: raw.material_id ?? materialId,
    material_type: raw.material_type ?? '',
    concepts: (raw.concepts ?? []).map(normalizeConceptMastery),
  };
}

/**
 * The weak concepts across all evidence, weakest first. A concept is weak
 * when it has evidence and is limited by a weak, developing or learning
 * dimension. This is the deterministic list of concepts to prioritize for
 * review.
 */
export async function getWeakConcepts(): Promise<WeakConcept[]> {
  const raw = await invoke<any[]>('get_weak_concepts');
  return (raw ?? []).map((item: any) => ({
    concept_id: item.concept_id ?? '',
    label: item.label ?? '',
    weakest: normalizeState(String(item.weakest ?? 'unseen')),
    mastery: normalizeConceptMastery(item.mastery),
  }));
}

/**
 * Full rebuild of every concept's mastery from its raw evidence. Mastery is
 * always recomputed from stored raw evidence and never mutated in place, so
 * a rebuild is idempotent and lossless.
 */
export async function recalculateMastery(): Promise<ConceptMastery[]> {
  const raw = await invoke<any[]>('recalculate_mastery');
  return (raw ?? []).map(normalizeConceptMastery);
}
