import { invoke } from '@tauri-apps/api/core';

export type AssessmentDifficulty = 'recall' | 'understanding' | 'application';
export type AssessmentGrade = 'correct' | 'partially_correct' | 'incorrect' | 'insufficient';
export type AssessmentConceptStatus = 'mastered' | 'partially_understood' | 'weak';

export interface AssessmentQuestion {
  id?: number;
  assessment_id?: string;
  prompt: string;
  concept: string;
  difficulty: AssessmentDifficulty;
  position: number;
}

/**
 * An immutable snapshot of a question as presented in a specific session.
 * The `id` here is the session-question snapshot id; answers are submitted
 * against these ids so attempts are never disturbed by later reassessments.
 */
export interface AssessmentSessionQuestion {
  id: number;
  session_id: string;
  source_question_id?: number | null;
  prompt: string;
  concept: string;
  difficulty: AssessmentDifficulty;
  expected_content: string;
  position: number;
}

export interface AssessmentAttempt {
  id: string;
  assessment_id: string;
  status: 'active' | 'completed';
  created_at: string;
  started_at?: string | null;
  completed_at?: string | null;
  answer_count: number;
}

export interface AssessmentEvaluation {
  grade: AssessmentGrade;
  rationale: string;
  misconception?: string | null;
}

export interface AssessmentAnswer {
  id?: number;
  session_id: string;
  question_id: number;
  question_prompt: string;
  concept: string;
  difficulty: AssessmentDifficulty;
  student_answer: string;
  evaluation?: AssessmentEvaluation | null;
}

export interface AssessmentConceptResult {
  concept: string;
  status: AssessmentConceptStatus;
  rationale: string;
  misconceptions: string[];
}

export interface RemediationTarget {
  concept: string;
  reason: string;
}

export interface AssessmentReport {
  session_id: string;
  total_questions: number;
  mastered: AssessmentConceptResult[];
  partially_understood: AssessmentConceptResult[];
  weak: AssessmentConceptResult[];
  remediation_targets: RemediationTarget[];
}

export interface AssessmentSession {
  session_id: string;
  assessment_id: string;
  started_at: string;
  completed_at?: string | null;
}

export interface GeneratedAssessmentQuestion {
  prompt: string;
  concept: string;
  difficulty: string;
  expected_content: string;
}

export interface AssessmentCreationResponse {
  review_material_id: string;
  review_material_name: string;
  questions_count: number;
}

export interface MaterialGenerationRequest {
  material_id: string;
  cards_to_generate: number;
}

const DIFFICULTY_LABELS: Record<AssessmentDifficulty, string> = {
  recall: 'Recall',
  understanding: 'Understanding',
  application: 'Application',
};

const GRADE_LABELS: Record<AssessmentGrade, string> = {
  correct: 'Correct',
  partially_correct: 'Partially correct',
  incorrect: 'Incorrect',
  insufficient: 'Insufficient',
};

export function difficultyLabel(difficulty: AssessmentDifficulty): string {
  return DIFFICULTY_LABELS[difficulty] ?? difficulty;
}

export function gradeLabel(grade: AssessmentGrade): string {
  return GRADE_LABELS[grade] ?? grade;
}

function normalizeDifficulty(raw: string): AssessmentDifficulty {
  const value = raw.trim().toLowerCase();
  if (value === 'recall' || value === 'understanding' || value === 'application') {
    return value;
  }
  // Fallback for unexpected values keeps the UI functional.
  return 'recall';
}

export function normalizeAssessmentQuestion(raw: any): AssessmentQuestion {
  return {
    id: raw.id,
    assessment_id: raw.assessment_id,
    prompt: raw.prompt ?? raw.question ?? '',
    concept: raw.concept ?? 'General',
    difficulty: normalizeDifficulty(String(raw.difficulty ?? 'recall')),
    position: raw.position ?? 0,
  };
}

export function normalizeSessionQuestion(raw: any): AssessmentSessionQuestion {
  return {
    id: raw.id,
    session_id: raw.session_id ?? '',
    source_question_id: raw.source_question_id ?? null,
    prompt: raw.prompt ?? '',
    concept: raw.concept ?? 'General',
    difficulty: normalizeDifficulty(String(raw.difficulty ?? 'recall')),
    expected_content: raw.expected_content ?? '',
    position: raw.position ?? 0,
  };
}

export function normalizeAttempt(raw: any): AssessmentAttempt {
  return {
    id: raw.id,
    assessment_id: raw.assessment_id ?? '',
    status: raw.status === 'completed' ? 'completed' : 'active',
    created_at: raw.created_at ?? '',
    started_at: raw.started_at ?? null,
    completed_at: raw.completed_at ?? null,
    answer_count: raw.answer_count ?? 0,
  };
}

export function normalizeEvaluation(raw: any): AssessmentEvaluation | null {
  if (!raw || !raw.grade) return null;
  const grade = String(raw.grade);
  return {
    grade: (['correct', 'partially_correct', 'incorrect', 'insufficient'].includes(grade)
      ? grade
      : 'incorrect') as AssessmentGrade,
    rationale: raw.rationale ?? '',
    misconception: raw.misconception ?? null,
  };
}

export function normalizeAssessmentAnswer(raw: any): AssessmentAnswer {
  return {
    id: raw.id,
    session_id: raw.session_id,
    question_id: raw.question_id,
    question_prompt: raw.question_prompt ?? '',
    concept: raw.concept ?? 'General',
    difficulty: normalizeDifficulty(String(raw.difficulty ?? 'recall')),
    student_answer: raw.student_answer ?? '',
    evaluation: normalizeEvaluation(raw.evaluation),
  };
}

/**
 * Session logic for taking an assessment: walks through questions in order,
 * collects free-text answers, and submits each one to the backend for
 * structured evaluation as it is given.
 */
export class AssessmentSessionLogic {
  private questions: AssessmentSessionQuestion[];
  private currentIndex: number = 0;
  private answers: Map<number, AssessmentAnswer> = new Map();
  public sessionId: string = '';
  public isSubmitting: boolean = false;

  constructor(questions: AssessmentSessionQuestion[], sessionId: string) {
    this.questions = [...questions].sort((a, b) => a.position - b.position);
    this.sessionId = sessionId;
  }

  get total(): number {
    return this.questions.length;
  }

  get current(): AssessmentSessionQuestion | null {
    return this.questions[this.currentIndex] ?? null;
  }

  get progress(): { current: number; total: number; percentage: number } {
    return {
      current: this.currentIndex + 1,
      total: this.questions.length,
      percentage: this.questions.length > 0 ? ((this.currentIndex + 1) / this.questions.length) * 100 : 0,
    };
  }

  get answeredCount(): number {
    return this.answers.size;
  }

  isComplete(): boolean {
    return this.currentIndex >= this.questions.length - 1;
  }

  canGoNext(): boolean {
    return this.currentIndex < this.questions.length - 1;
  }

  canGoPrevious(): boolean {
    return this.currentIndex > 0;
  }

  moveNext(): void {
    if (this.canGoNext()) this.currentIndex++;
  }

  movePrevious(): void {
    if (this.canGoPrevious()) this.currentIndex--;
  }

  getAnswer(questionId: number): AssessmentAnswer | undefined {
    return this.answers.get(questionId);
  }

  /**
   * Loads previously stored answers of a resumed session, keyed by the
   * session-question snapshot id, so an in-progress attempt picks up where it
   * left off instead of re-asking answered questions.
   */
  preloadAnswers(answers: AssessmentAnswer[]): void {
    for (const answer of answers) {
      this.answers.set(answer.question_id, answer);
    }
  }

  /**
   * Submits the student's free-text answer for the current question and
   * returns the stored answer (with the backend's evaluation). The backend
   * grades against this question's immutable session snapshot.
   */
  async submitAnswer(studentAnswer: string): Promise<AssessmentAnswer> {
    const question = this.current;
    if (!question) {
      throw new Error('No current question to answer');
    }
    if (this.isSubmitting) throw new Error('A submission is already in progress');

    this.isSubmitting = true;
    try {
      const raw = await invoke<any>('submit_assessment_answer', {
        sessionId: this.sessionId,
        questionId: question.id,
        studentAnswer,
      });
      const answer = normalizeAssessmentAnswer(raw);
      this.answers.set(question.id, answer);
      return answer;
    } finally {
      this.isSubmitting = false;
    }
  }
}

export class AssessmentCreatorLogic {
  private questions: GeneratedAssessmentQuestion[] = [];
  private isGenerating: boolean = false;
  private isCreating: boolean = false;
  private error: string = '';
  private generationStatus: string = '';

  get getQuestions(): GeneratedAssessmentQuestion[] {
    return this.questions;
  }

  get getIsGenerating(): boolean {
    return this.isGenerating;
  }

  get getIsCreating(): boolean {
    return this.isCreating;
  }

  get getError(): string {
    return this.error;
  }

  get getGenerationStatus(): string {
    return this.generationStatus;
  }

  async generateQuestions(
    materials: MaterialGenerationRequest[],
    language: string,
    runContext: string,
  ): Promise<void> {
    if (materials.length === 0) {
      throw new Error('Select at least one studied material');
    }

    this.isGenerating = true;
    this.error = '';
    this.generationStatus = 'Sending request to AI model...';
    this.questions = [];

    try {
      const generated = await invoke<any[]>('generate_assessment_questions', {
        request: {
          materials,
          language,
          run_context: runContext,
        },
      });

      this.questions = (generated ?? []).map((q: any) => ({
        prompt: q.prompt ?? '',
        concept: q.concept ?? 'General',
        difficulty: String(q.difficulty ?? 'recall'),
        expected_content: q.expected_content ?? '',
      }));

      if (this.questions.length === 0) {
        this.error = 'No assessment questions were generated. Try a clearer topic focus or more material.';
      } else {
        this.generationStatus = `Generated ${this.questions.length} assessment questions`;
      }
    } catch (err) {
      this.error = `Failed to generate assessment questions: ${err}`;
      throw err;
    } finally {
      this.isGenerating = false;
    }
  }

  async createAssessment(
    id: string,
    name: string,
    language: string,
    tags: string[],
    questions: GeneratedAssessmentQuestion[],
    sourceMaterialIds: string[] = [],
  ): Promise<AssessmentCreationResponse> {
    if (!name.trim()) throw new Error('Name is required');
    if (questions.length === 0) throw new Error('No questions to save');

    this.isCreating = true;
    this.error = '';

    try {
      const result = await invoke<AssessmentCreationResponse>('create_assessment', {
        id,
        name,
        language,
        tags,
        sourceMaterialIds,
        questions: questions.map((q, index) => ({
          prompt: q.prompt,
          concept: q.concept,
          difficulty: q.difficulty,
          expected_content: q.expected_content,
          position: index,
        })),
      });
      return result;
    } catch (err) {
      this.error = `Failed to create assessment: ${err}`;
      throw err;
    } finally {
      this.isCreating = false;
    }
  }

  reset(): void {
    this.questions = [];
    this.error = '';
    this.generationStatus = '';
  }
}
