<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { fade, slide } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";

  // Icons
  import { X as XIcon } from "lucide-svelte";

  // Components
  import Button from "../Shared/Button.svelte";
  import MasteryPanel from "./MasteryPanel.svelte";

  // Logic
  import {
    AssessmentSessionLogic,
    difficultyLabel,
    gradeLabel,
    normalizeAssessmentAnswer,
    normalizeSessionQuestion,
    normalizeAttempt,
    type AssessmentAnswer,
    type AssessmentReport,
    type AssessmentAttempt,
  } from "../../logic/ReviewMaterial/assessment";

  // Stores
  import { toastStore } from "../../stores/toastStore";

  // Props
  export let assessment = null;
  export let isOpen = false;

  // Callback Props
  export let onClose: () => void = () => {};
  export let onReviewCompleted: (data: any) => void = () => {};

  // State
  let logic: AssessmentSessionLogic | null = null;
  let isLoading = true;
  let isComplete = false;
  let error = "";
  let answerText = "";
  let lastSubmitted: AssessmentAnswer | null = null;
  let report: AssessmentReport | null = null;
  let isReassessing = false;
  let isRemediating = false;
  let isResumed = false;
  let isCompletedReview = false;
  let attempts: AssessmentAttempt[] = [];
  let showAttemptHistory = false;

  $: assessmentName = assessment && assessment.name ? assessment.name : "Assessment";

  $: if (assessment && isOpen && !logic && !error) {
    startAssessment();
  }

  async function startAssessment(forceNew = false) {
    isLoading = true;
    error = "";
    try {
      // start_assessment_session attaches to the right attempt without ever
      // mutating the canonical question set or a completed attempt:
      // pending session → resumed, completed session → reviewed read-only,
      // nothing taken yet (or forceNew) → fresh session from the snapshot of
      // the current canonical set.
      const session: any = await invoke("start_assessment_session", {
        assessmentId: assessment.id,
        forceNew,
      });

      const rawQuestions: any[] = session.questions ?? [];
      if (rawQuestions.length === 0) {
        error = "This assessment has no questions.";
        return;
      }

      const questions = rawQuestions.map(normalizeSessionQuestion);
      logic = new AssessmentSessionLogic(questions, session.session_id);
      isResumed = session.is_resumed ?? false;
      isCompletedReview = session.is_completed ?? false;
      await loadAttempts();

      if (isCompletedReview) {
        // Read-only review of an already completed attempt: no new session
        // was created, nothing is re-run.
        await openCompletedReport(session.session_id);
      } else {
        if (isResumed) {
          await preloadStoredAnswers();
        }
        updateDisplay();
      }
    } catch (err) {
      console.error("Failed to start assessment:", err);
      error = `Failed to start assessment: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      isLoading = false;
    }
  }

  async function openCompletedReport(sessionId: string) {
    report = await invoke("get_assessment_report", { sessionId });
    isComplete = true;
  }

  async function preloadStoredAnswers() {
    if (!logic) return;
    const raw: any[] = await invoke("get_assessment_answers", {
      sessionId: logic.sessionId,
    });
    logic.preloadAnswers((raw ?? []).map(normalizeAssessmentAnswer));
    syncAnswerInput();
  }

  function syncAnswerInput() {
    const stored = currentAnswer();
    answerText = stored?.student_answer ?? "";
    lastSubmitted = stored?.evaluation ? stored : null;
  }

  function updateDisplay() {
    if (logic) {
      if (logic.current === null && logic.total > 0) {
        finishSession();
      }
    }
  }

  function currentQuestion() {
    return logic ? logic.current : null;
  }

  function currentAnswer(): AssessmentAnswer | undefined {
    const q = currentQuestion();
    if (!q || !logic) return undefined;
    return logic.getAnswer(q.id);
  }

  async function handleSubmit() {
    if (!logic || !answerText.trim() || logic.isSubmitting) return;

    try {
      lastSubmitted = await logic.submitAnswer(answerText.trim());
      toastStore.success("Answer evaluated");
      // Auto-advance after a short delay so the student sees the evaluation.
      setTimeout(() => {
        if (!logic) return;
        if (logic.canGoNext()) {
          logic.moveNext();
          syncAnswerInput();
          updateDisplay();
        } else {
          finishSession();
        }
      }, 1200);
    } catch (err) {
      console.error("Failed to submit answer:", err);
      toastStore.error(`Failed to submit answer: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  function handlePrevious() {
    if (logic && logic.canGoPrevious()) {
      logic.movePrevious();
      syncAnswerInput();
      updateDisplay();
    }
  }

  async function finishSession() {
    if (!logic || !assessment) return;
    try {
      report = await invoke("get_assessment_report", {
        sessionId: logic.sessionId,
      });

      // Mark the session completed (idempotent) so a second take is a genuine
      // reassessment and the session has an explicit finished state.
      try {
        await invoke("complete_assessment_session", { sessionId: logic.sessionId });
      } catch (e) {
        console.warn("Could not mark session complete:", e);
      }

      await invoke("update_last_review", { materialId: assessment.id });

      onReviewCompleted({
        assessmentId: assessment.id,
        sessionId: logic.sessionId,
        report,
      });
      isComplete = true;
    } catch (err) {
      console.error("Failed to load assessment report:", err);
      toastStore.error(`Failed to load report: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  function handleClose() {
    logic = null;
    report = null;
    isComplete = false;
    isCompletedReview = false;
    answerText = "";
    lastSubmitted = null;
    onClose();
  }

  async function handleReassess() {
    if (!assessment || isReassessing) return;
    isReassessing = true;
    try {
      // Regenerate fresh question wording for the same concepts and open a new
      // pending session snapshotted from the new set. The current attempt's
      // history is preserved and can be reviewed below.
      const newSessionId: string = await invoke("reassess_assessment", {
        assessmentId: assessment.id,
      });
      toastStore.success("New reassessment attempt ready");
      logic = null;
      report = null;
      isComplete = false;
      isCompletedReview = false;
      answerText = "";
      lastSubmitted = null;
      isLoading = true;
      // Resume the pending session the backend just created (no regeneration).
      await startAssessment();
      void newSessionId;
    } catch (err) {
      console.error("Failed to start reassessment:", err);
      toastStore.error(`Reassessment failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      isReassessing = false;
    }
  }

  async function handleRemediation() {
    if (!report || !logic || isRemediating) return;
    const targets = report.remediation_targets;
    if (targets.length === 0) return;

    isRemediating = true;
    try {
      // Ask the backend to build a focused remediation assessment from the
      // weak / partially understood concepts of this session.
      const remediation: any = await invoke("start_remediation", {
        request: {
          sessionId: logic.sessionId,
        },
      });
      toastStore.success(
        `Created remediation assessment with ${remediation.questions_count} questions`,
      );
      onReviewCompleted({
        assessmentId: assessment.id,
        report,
        remediation: true,
        remediationMaterial: remediation,
      });
    } catch (err) {
      console.error("Failed to start remediation:", err);
      toastStore.error(`Remediation failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      isRemediating = false;
    }
  }

  onMount(() => {
    // Keyboard shortcuts
    const handler = (e: KeyboardEvent) => {
      if (!logic || isComplete || !logic.current) return;
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        handleSubmit();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });

  function statusIcon(status: string): string {
    switch (status) {
      case "mastered": return "✓";
      case "partially_understood": return "◐";
      default: return "✗";
    }
  }

  async function loadAttempts() {
    if (!assessment) return;
    try {
      const raw: any[] = await invoke("get_assessment_attempts", { assessmentId: assessment.id });
      attempts = (raw ?? []).map(normalizeAttempt);
    } catch (e) {
      console.warn("Failed to load attempt history:", e);
    }
  }

  /**
   * Explicitly re-takes the same assessment as a new attempt (same canonical
   * questions). Each attempt keeps its own immutable question snapshot and
   * report, so the previous attempt's results stay intact while a fresh
   * session is opened from the current canonical question set.
   */
  async function handleRetake() {
    if (!assessment || isReassessing) return;
    isReassessing = true;
    try {
      logic = null;
      report = null;
      isComplete = false;
      isCompletedReview = false;
      answerText = "";
      lastSubmitted = null;
      isResumed = false;
      isLoading = true;
      await startAssessment(true);
      toastStore.success("Started a new attempt");
    } catch (err) {
      console.error("Failed to start retake:", err);
      toastStore.error(`Retake failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      isReassessing = false;
    }
  }
</script>

<div class="ar-modal" class:open={isOpen}>
  {#if isOpen}
    <div class="ar-overlay" on:click={handleClose}></div>
    <div class="ar-container" transition:fade={{ duration: 200 }}>
      <div class="ar-header">
        <h3>{assessmentName}</h3>
        <button class="ar-close" on:click={handleClose}>
          <XIcon size={20} />
        </button>
      </div>

      {#if isLoading}
        <div class="ar-loading">
          <div class="loading-spinner"></div>
          <p>Loading assessment...</p>
        </div>
      {:else if error}
        <div class="ar-error">
          <p>{error}</p>
          <Button variant="secondary" text="Close" onClick={handleClose} />
        </div>
      {:else if !isComplete && logic && currentQuestion()}
        {#if logic}
          {@const q = currentQuestion()}
          {@const progress = logic.progress}
          <div class="ar-progress">
            <div class="ar-progress-bar">
              <div class="ar-progress-fill" style="width: {progress.percentage}%"></div>
            </div>
            <span class="ar-progress-text">{progress.current} / {progress.total}</span>
          </div>

          <div class="ar-question-card">
            <div class="ar-question-meta">
              <span class="ar-difficulty" class:recall={q.difficulty === 'recall'} class:understanding={q.difficulty === 'understanding'} class:application={q.difficulty === 'application'}>
                {difficultyLabel(q.difficulty)}
              </span>
              <span class="ar-concept">{q.concept}</span>
            </div>
            <p class="ar-question-prompt">{q.prompt}</p>

            <textarea
              class="ar-answer-input"
              bind:value={answerText}
              placeholder="Type your answer here..."
              rows="5"
              disabled={logic.isSubmitting}
            ></textarea>

            {#if lastSubmitted && lastSubmitted.evaluation}
              <div class="ar-evaluation" transition:slide>
                <div class="ar-grade" class:correct={lastSubmitted.evaluation.grade === 'correct'}
                     class:partially_correct={lastSubmitted.evaluation.grade === 'partially_correct'}
                     class:incorrect={lastSubmitted.evaluation.grade === 'incorrect'}
                     class:insufficient={lastSubmitted.evaluation.grade === 'insufficient'}>
                  {gradeLabel(lastSubmitted.evaluation.grade)}
                </div>
                <p class="ar-rationale">{lastSubmitted.evaluation.rationale}</p>
                {#if lastSubmitted.evaluation.misconception}
                  <p class="ar-misconception">Misconception: {lastSubmitted.evaluation.misconception}</p>
                {/if}
              </div>
            {/if}
          </div>

          <div class="ar-actions">
            <Button
              variant="secondary"
              text="Previous"
              disabled={!logic.canGoPrevious() || logic.isSubmitting}
              onClick={handlePrevious}
            />
            <Button
              variant="primary"
              text={logic.isSubmitting ? "Evaluating..." : "Submit Answer"}
              changed={logic.isSubmitting}
              disabled={logic.isSubmitting || !answerText.trim()}
              onClick={handleSubmit}
            />
          </div>
        {/if}
      {:else if isComplete && report}
        <div class="ar-report" transition:fade>
          <h4>Assessment Report</h4>
          <p class="ar-report-subtitle">No single score — here is what the results show concept by concept.</p>

          <div class="ar-report-section">
            <h5 class="ar-section-mastered">✓ Mastered</h5>
            {#if report.mastered.length === 0}
              <p class="ar-empty">None yet</p>
            {:else}
              {#each report.mastered as result (result.concept)}
                <div class="ar-concept-row">
                  <span class="ar-status mastered">✓</span>
                  <span class="ar-concept-name">{result.concept}</span>
                </div>
              {/each}
            {/if}
          </div>

          <div class="ar-report-section">
            <h5 class="ar-section-partial">◐ Partially Understood</h5>
            {#if report.partially_understood.length === 0}
              <p class="ar-empty">None</p>
            {:else}
              {#each report.partially_understood as result (result.concept)}
                <div class="ar-concept-row">
                  <span class="ar-status partial">◐</span>
                  <div class="ar-concept-detail">
                    <span class="ar-concept-name">{result.concept}</span>
                    {#if result.misconceptions.length > 0}
                      <p class="ar-misconception">{result.misconceptions.join(' · ')}</p>
                    {/if}
                    <p class="ar-rationale-small">{result.rationale}</p>
                  </div>
                </div>
              {/each}
            {/if}
          </div>

          <div class="ar-report-section">
            <h5 class="ar-section-weak">✗ Weak</h5>
            {#if report.weak.length === 0}
              <p class="ar-empty">None</p>
            {:else}
              {#each report.weak as result (result.concept)}
                <div class="ar-concept-row">
                  <span class="ar-status weak">✗</span>
                  <div class="ar-concept-detail">
                    <span class="ar-concept-name">{result.concept}</span>
                    {#if result.misconceptions.length > 0}
                      <p class="ar-misconception">{result.misconceptions.join(' · ')}</p>
                    {/if}
                    <p class="ar-rationale-small">{result.rationale}</p>
                  </div>
                </div>
              {/each}
            {/if}
          </div>

          {#if report.remediation_targets.length > 0}
            <div class="ar-remediation">
              <h5>Suggested Targets</h5>
              {#each report.remediation_targets as target (target.concept)}
                <div class="ar-target">
                  <span class="ar-target-concept">{target.concept}</span>
                  <p class="ar-target-reason">{target.reason}</p>
                </div>
              {/each}
            </div>
          {/if}

          {#if assessment && assessment.id}
            <div class="ar-mastery">
              <h5>Concept Mastery</h5>
              <MasteryPanel materialId={assessment.id} />
            </div>
          {/if}

          {#if attempts.length > 1}
            <div class="ar-attempts">
              <button class="ar-attempts-toggle" on:click={() => (showAttemptHistory = !showAttemptHistory)}>
                Attempt history ({attempts.length})
              </button>
              {#if showAttemptHistory}
                {#each attempts as attempt (attempt.id)}
                  {@const isCurrent = attempt.id === (logic?.sessionId ?? null)}
                  <div class="ar-attempt-row" class:current={isCurrent}>
                    <span class="ar-attempt-status" class:completed={attempt.status === 'completed'} class:active={attempt.status === 'active'}>
                      {attempt.status === 'completed' ? 'Completed' : 'In progress'}
                    </span>
                    <span class="ar-attempt-date">
                      {attempt.completed_at ?? attempt.created_at}
                    </span>
                    <span class="ar-attempt-count">{attempt.answer_count} answers</span>
                  </div>
                {/each}
              {/if}
            </div>
          {/if}

          <div class="ar-report-actions">
            <Button
              variant="primary"
              text={isReassessing ? "Starting..." : "Reassess (New Questions)"}
              changed={isReassessing}
              disabled={isReassessing}
              onClick={handleReassess}
            />
            <Button
              variant="secondary"
              text="Take Again (Same Questions)"
              disabled={isReassessing}
              onClick={handleRetake}
            />
            <Button
              variant="secondary"
              text={isRemediating ? "Preparing..." : "Focused Remediation"}
              changed={isRemediating}
              disabled={isRemediating || report.remediation_targets.length === 0}
              onClick={handleRemediation}
            />
            <Button variant="secondary" text="Close" onClick={handleClose} />
          </div>
        </div>
      {:else}
        <div class="ar-complete">
          <h4>Assessment Complete</h4>
          <p>Your answers have been evaluated.</p>
          <div class="ar-report-actions">
            <Button variant="primary" text="View Report" onClick={finishSession} />
            <Button variant="secondary" text="Close" onClick={handleClose} />
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .ar-modal {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .ar-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
  }
  .ar-container {
    position: relative;
    background: var(--background, #fff);
    border-radius: 16px;
    width: min(680px, 92vw);
    max-height: 85vh;
    overflow-y: auto;
    padding: 1.5rem;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.3);
  }
  .ar-attempts {
    margin-top: 1.25rem;
    border-top: 1px solid var(--border, #e5e5e5);
    padding-top: 0.75rem;
  }
  .ar-attempts-toggle {
    background: none;
    border: none;
    color: var(--muted-foreground, #888);
    font-size: 0.8rem;
    cursor: pointer;
    text-decoration: underline;
  }
  .ar-attempt-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.5rem;
    font-size: 0.8rem;
  }
  .ar-attempt-row.current {
    font-weight: 600;
  }
  .ar-attempt-status {
    font-size: 0.72rem;
    padding: 0.1rem 0.5rem;
    border-radius: 999px;
  }
  .ar-attempt-status.completed { background: rgba(34, 197, 94, 0.15); color: #166534; }
  .ar-attempt-status.active { background: rgba(59, 130, 246, 0.15); color: #1e40af; }
  .ar-attempt-date {
    color: var(--muted-foreground, #888);
    flex: 1;
  }
  .ar-attempt-count { color: var(--muted-foreground, #888); }
  .ar-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }
  .ar-header h3 {
    margin: 0;
    font-size: 1.25rem;
  }
  .ar-close {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-secondary, #666);
    padding: 0.25rem;
  }
  .ar-loading, .ar-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
    padding: 3rem 0;
  }
  .ar-progress {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 1rem;
  }
  .ar-progress-bar {
    flex: 1;
    height: 6px;
    background: var(--border-color, #e1e5e9);
    border-radius: 3px;
    overflow: hidden;
  }
  .ar-progress-fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  .ar-progress-text {
    font-size: 0.8rem;
    color: var(--text-secondary, #666);
    min-width: 50px;
    text-align: right;
  }
  .ar-question-card {
    background: var(--background-secondary, #f8f9fa);
    border-radius: 12px;
    padding: 1.25rem;
  }
  .ar-question-meta {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    margin-bottom: 0.75rem;
  }
  .ar-difficulty {
    display: inline-block;
    padding: 0.15rem 0.6rem;
    border-radius: 999px;
    font-size: 0.7rem;
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }
  .ar-difficulty.application {
    background: color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .ar-concept {
    font-size: 0.8rem;
    color: var(--text-secondary, #666);
  }
  .ar-question-prompt {
    margin: 0 0 1rem;
    font-size: 1rem;
    line-height: 1.5;
    white-space: pre-wrap;
  }
  .ar-answer-input {
    width: 100%;
    border: 1px solid var(--border-color, #e1e5e9);
    border-radius: 8px;
    padding: 0.75rem;
    font-size: 0.95rem;
    resize: vertical;
    font-family: inherit;
  }
  .ar-evaluation {
    margin-top: 1rem;
    padding: 0.75rem 1rem;
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-left: 3px solid var(--accent);
  }
  .ar-grade {
    display: inline-block;
    font-weight: 700;
    margin-bottom: 0.25rem;
  }
  .ar-grade.correct { color: #16a34a; }
  .ar-grade.partially_correct { color: #ca8a04; }
  .ar-grade.incorrect { color: #dc2626; }
  .ar-grade.insufficient { color: #6b7280; }
  .ar-rationale {
    margin: 0;
    font-size: 0.9rem;
    line-height: 1.4;
  }
  .ar-misconception {
    margin: 0.5rem 0 0;
    font-size: 0.85rem;
    font-style: italic;
    color: #b91c1c;
  }
  .ar-actions {
    display: flex;
    justify-content: space-between;
    margin-top: 1.25rem;
  }
  .ar-report {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .ar-report-subtitle {
    margin: 0;
    font-size: 0.9rem;
    color: var(--text-secondary, #666);
  }
  .ar-report-section {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .ar-section-mastered { color: #16a34a; margin: 0; font-size: 1rem; }
  .ar-section-partial { color: #ca8a04; margin: 0; font-size: 1rem; }
  .ar-section-weak { color: #dc2626; margin: 0; font-size: 1rem; }
  .ar-empty {
    font-size: 0.85rem;
    color: var(--text-secondary, #999);
    margin: 0;
  }
  .ar-concept-row {
    display: flex;
    gap: 0.75rem;
    align-items: flex-start;
  }
  .ar-status {
    font-size: 1.1rem;
    line-height: 1.4;
  }
  .ar-status.mastered { color: #16a34a; }
  .ar-status.partial { color: #ca8a04; }
  .ar-status.weak { color: #dc2626; }
  .ar-concept-detail {
    flex: 1;
  }
  .ar-concept-name {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .ar-rationale-small {
    margin: 0.25rem 0 0;
    font-size: 0.8rem;
    color: var(--text-secondary, #666);
    line-height: 1.4;
  }
  .ar-remediation {
    padding: 0.75rem 1rem;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .ar-remediation h5 {
    margin: 0;
    font-size: 0.9rem;
  }
  .ar-mastery {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .ar-mastery h5 {
    margin: 0;
    font-size: 0.9rem;
  }
  .ar-target {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .ar-target-concept {
    font-weight: 600;
    font-size: 0.85rem;
  }
  .ar-target-reason {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-secondary, #666);
  }
  .ar-report-actions {
    display: flex;
    gap: 0.75rem;
    flex-wrap: wrap;
    margin-top: 0.5rem;
  }
  .ar-complete {
    text-align: center;
    padding: 2rem 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
  }
  .loading-spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--border-color, #e1e5e9);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
