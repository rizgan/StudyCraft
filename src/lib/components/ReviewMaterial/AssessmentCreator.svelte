<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';
  import "../../styles/variables.css";
  import "../../styles/ReviewMaterial/testCreator.css";

  // Components
  import Button from '../Shared/Button.svelte';

  // Logic & Types
  import {
    AssessmentCreatorLogic,
    difficultyLabel,
    type GeneratedAssessmentQuestion,
  } from '../../logic/ReviewMaterial/assessment';
  import type { StudyMaterial } from '../../logic/ReviewMaterial/testCreator';
  const logic = new AssessmentCreatorLogic();

  // Stores
  import { llmStore } from '../../stores/llmStore';
  import { materialsStore } from '../../stores/materialsStore';
  import { toastStore } from '../../stores/toastStore';

  // Icons
  import { Bot as BotIcon, File as FileIcon } from 'lucide-svelte';

  // Constants
  import { LANGUAGE_OPTIONS } from '../../logic/Constants/creatorConstants';

  // Props
  export let initialSelectedMaterials: StudyMaterial[] = [];

  // Callbacks
  export let onClose = () => {};
  export let onCreated = (assessment: any) => {};
  export let onShowMaterialsSelector = (details: any) => {};

  // State
  let name = '';
  let language = 'English';
  let runContext = '';
  let selectedMaterials: StudyMaterial[] = [...initialSelectedMaterials];
  let questions: GeneratedAssessmentQuestion[] = [];
  let isGenerating = false;
  let isCreating = false;
  let error = '';
  let generationStatus = '';
  let updateInterval: number | null = null;

  // Reactivity
  $: llmStatus = $llmStore;
  $: canUseAI = llmStatus.hasConfiguredProvider && !llmStatus.isLoading;
  $: totalQuestions = selectedMaterials.reduce(
    (sum, m) => sum + (m.questionsToGenerate || 5),
    0,
  );
  $: canGenerate = canUseAI && selectedMaterials.length > 0 && runContext.trim() !== '' && !isGenerating;

  $: if (isGenerating) {
    if (!updateInterval) {
      updateInterval = setInterval(() => {
        questions = logic.getQuestions;
        error = logic.getError;
        generationStatus = logic.getGenerationStatus;
      }, 100);
    }
  } else if (updateInterval) {
    clearInterval(updateInterval);
    updateInterval = null;
  }

  function handleClose() {
    onClose();
  }

  function handleShowMaterialSelector() {
    onShowMaterialsSelector({
      fromAssessmentCreator: true,
      currentSelectedMaterials: selectedMaterials,
    });
  }

  function handleRemoveMaterial(material: StudyMaterial) {
    selectedMaterials = selectedMaterials.filter((m) => m.id !== material.id);
  }

  function handleMaterialQuestionCountChange(materialId: string, count: number) {
    selectedMaterials = selectedMaterials.map((m) =>
      m.id === materialId ? { ...m, questionsToGenerate: count } : m,
    );
  }

  async function handleGenerate() {
    if (!canGenerate) return;
    isGenerating = true;
    error = '';
    try {
      await logic.generateQuestions(
        selectedMaterials.map((m) => ({
          material_id: m.id,
          cards_to_generate: m.questionsToGenerate || 5,
        })),
        language,
        runContext.trim(),
      );
      questions = logic.getQuestions;
      error = logic.getError;
    } catch (err) {
      error = `Failed to generate questions: ${err instanceof Error ? err.message : String(err)}`;
      console.error('Assessment generation failed:', err);
    } finally {
      isGenerating = logic.getIsGenerating;
    }
  }

  function handleRemoveQuestion(index: number) {
    questions = questions.filter((_, i) => i !== index);
  }

  async function handleCreate() {
    if (isCreating) return;
    if (!name.trim()) {
      toastStore.warning('Please enter an assessment name');
      return;
    }
    if (questions.length === 0) {
      toastStore.warning('Generate or add at least one question');
      return;
    }
    isCreating = true;
    try {
      const assessmentId = await materialsStore.addReviewMaterial({
        name,
        type: 'assessment',
      });
      const result = await logic.createAssessment(
        assessmentId,
        name.trim(),
        language,
        [],
        questions,
        selectedMaterials.map((m) => m.id),
      );
      toastStore.success('Assessment created successfully');
      onCreated(result);
      onClose();
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : 'Unknown error';
      console.error('Failed to create assessment:', err);
      toastStore.error(`Failed to create assessment: ${message}`);
    } finally {
      isCreating = false;
    }
  }

  onDestroy(() => {
    if (updateInterval) clearInterval(updateInterval);
  });

  export function handleMaterialsSelected(materials: StudyMaterial[]) {
    if (!materials || materials.length === 0) return;
    selectedMaterials = materials.map((m) => ({
      ...m,
      questionsToGenerate: m.questionsToGenerate || 5,
    }));
  }
</script>

<div class="creator-container" transition:fade={{ duration: 200 }}>
  <div class="creator-header">
    <h3>Create Assessment</h3>
    <button class="close-button" on:click={handleClose}>×</button>
  </div>

  {#if error}
    <div class="error-message" transition:fade>{error}</div>
  {/if}

  {#if isGenerating}
    <div class="tc-generation-status" transition:fade>
      <div class="loading-spinner"></div>
      <div class="tc-status-text">
        <strong>Generating Assessment Questions...</strong>
        <p>{generationStatus || 'Building transfer questions from your studied material...'}</p>
      </div>
    </div>
  {/if}

  <div class="creator-content">
    <div class="tc-step-content">
      <div class="tc-test-info-section">
        <h4>Assessment Information</h4>
        <div class="tc-form-grid">
          <div class="tc-form-field">
            <label for="assessment-name">Assessment Name *</label>
            <input
              id="assessment-name"
              type="text"
              bind:value={name}
              placeholder="Enter assessment name..."
              disabled={isGenerating}
              class="input"
            />
          </div>

          <div class="tc-form-field">
            <label for="assessment-language">Language</label>
            <select id="assessment-language" bind:value={language} class="tc-language-select" disabled={isGenerating}>
              {#each LANGUAGE_OPTIONS as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
        </div>
      </div>

      <div class="tc-generation-section">
        <div class="tc-section-header">
          <h4>Generate from Studied Materials</h4>
          <p>Creates free-response transfer questions (recall → understanding → application) that are graded automatically.</p>
          {#if !canUseAI}
            <p class="tc-ai-warning">⚠️ No AI provider configured. Configure one in Settings to enable generation.</p>
          {/if}
        </div>

        <div class="tc-select-materials-wrapper">
          <Button
            variant="secondary"
            text="Select Study Materials"
            onClick={handleShowMaterialSelector}
            icon={BotIcon}
            disabled={isGenerating || !canUseAI}
          ></Button>
        </div>

        {#if selectedMaterials.length > 0}
          <div class="tc-selected-materials-compact">
            <div class="tc-materials-header">
              <span class="tc-materials-count">{selectedMaterials.length} materials selected</span>
              <span class="tc-total-questions">Total: {totalQuestions} questions</span>
            </div>

            <div class="tc-materials-grid">
              {#each selectedMaterials as material (material.id)}
                <div class="tc-material-chip">
                  <div class="tc-material-info">
                    <FileIcon size={14}></FileIcon>
                    <span class="tc-material-name">{material.name || material.display_name}</span>
                  </div>
                  <div class="tc-material-controls">
                    <input
                      type="number"
                      value={material.questionsToGenerate || 5}
                      on:input={(e) => handleMaterialQuestionCountChange(material.id, parseInt((e.target as HTMLInputElement).value))}
                      min="1"
                      max="20"
                      class="tc-question-count-input"
                      title="Questions to generate"
                      disabled={isGenerating}
                    />
                    <button
                      aria-label="Remove Material"
                      class="tc-remove-material-btn"
                      on:click={() => handleRemoveMaterial(material)}
                      title="Remove material"
                      disabled={isGenerating}
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <line x1="18" y1="6" x2="6" y2="18"></line>
                        <line x1="6" y1="6" x2="18" y2="18"></line>
                      </svg>
                    </button>
                  </div>
                </div>
              {/each}
            </div>

            <div class="tc-concept-field">
              <label for="assessment-context">What should this assessment focus on? *</label>
              <input
                id="assessment-context"
                type="text"
                bind:value={runContext}
                placeholder="e.g., the water cycle, binary search, photosynthesis..."
                disabled={isGenerating}
              />
              <p class="tc-concept-help">
                Questions will target this topic from the selected materials, using new wording and scenarios rather than repeating your flashcards.
              </p>
            </div>

            <Button
              variant="primary"
              text={isGenerating ? 'Generating...' : questions.length > 0 ? 'Regenerate Questions' : 'Generate Questions'}
              changed={isGenerating}
              disabled={!canGenerate}
              onClick={handleGenerate}
            />
          </div>
        {:else}
          <div class="tc-no-materials">
            <p>Select studied materials and describe the focus topic to generate questions.</p>
          </div>
        {/if}
      </div>

      {#if questions.length > 0}
        <div class="tc-step-content" style="margin-top: 1.5rem;">
          <div class="tc-questions-header">
            <h4>Generated Questions ({questions.length})</h4>
          </div>
          <div class="tc-questions-list">
            {#each questions as question, questionIndex (questionIndex)}
              <div class="tc-question-item">
                <div class="tc-question-header">
                  <span>
                    Question {questionIndex + 1}
                    <span class="tc-difficulty-badge" class:recall={question.difficulty === 'recall'} class:understanding={question.difficulty === 'understanding'} class:application={question.difficulty === 'application'}>
                      {difficultyLabel((question.difficulty as any))}
                    </span>
                    · {question.concept}
                  </span>
                  <button class="tc-remove-question" on:click={() => handleRemoveQuestion(questionIndex)}>
                    Remove Question
                  </button>
                </div>
                <p class="tc-question-prompt">{question.prompt}</p>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>

  <div class="creator-actions">
    <Button
      variant="secondary"
      text="Cancel"
      disabled={isCreating || isGenerating}
      onClick={handleClose}
    />
    <Button
      variant="primary"
      text={isCreating ? 'Creating...' : 'Create Assessment'}
      changed={isCreating}
      disabled={isCreating || isGenerating || !name.trim() || questions.length === 0}
      onClick={handleCreate}
    />
  </div>
</div>

<style>
  .tc-difficulty-badge {
    display: inline-block;
    margin-left: 0.5rem;
    padding: 0.1rem 0.5rem;
    border-radius: 999px;
    font-size: 0.7rem;
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }
  .tc-difficulty-badge.application {
    background: color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .tc-question-prompt {
    margin: 0.5rem 0 0;
    font-size: 0.9rem;
    white-space: pre-wrap;
  }
</style>
