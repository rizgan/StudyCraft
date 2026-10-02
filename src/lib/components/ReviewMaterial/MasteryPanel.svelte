<script lang="ts">
  import "../../styles/variables.css";
  import { onMount } from 'svelte';

  import Button from '../Shared/Button.svelte';
  import {
    getConceptMastery,
    getMasteryForMaterial,
    dimensionLabel,
    stateLabel,
    confidenceLabel,
    stateBadgeClass,
    dimensionStateClass,
    type ConceptMastery,
    type MaterialMastery,
  } from '../../logic/ReviewMaterial/conceptMastery';

  // ------------------------------------------------------------------ Props
  /** Load a single concept by its (raw) label. */
  export let conceptLabel: string | null = null;
  /** Load every concept a material has evidence for (assessment or deck). */
  export let materialId: string | null = null;
  /** Fired whenever the panel finishes a successful load. */
  export let onLoaded: (m: { concept?: ConceptMastery | null; material?: MaterialMastery }) => void =
    () => {};

  // ------------------------------------------------------------------ State
  let concept: ConceptMastery | null = null;
  let material: MaterialMastery | null = null;
  let loading = false;
  let error = '';

  // --------------------------------------------------------------- Loading
  async function refresh() {
    if (!conceptLabel && !materialId) return;
    loading = true;
    error = '';
    try {
      if (materialId) {
        const m = await getMasteryForMaterial(materialId);
        material = m;
        onLoaded({ material: m });
      } else {
        const c = await getConceptMastery(conceptLabel as string);
        concept = c;
        onLoaded({ concept: c });
      }
    } catch (e) {
      console.error('Failed to load mastery:', e);
      error = 'Failed to load mastery.';
    } finally {
      loading = false;
    }
  }

  onMount(refresh);
</script>

{#if loading}
  <div class="mastery-panel">
    <span class="mastery-empty">Loading mastery…</span>
  </div>
{:else if error}
  <div class="mastery-panel">
    <span class="mastery-error">{error}</span>
  </div>
{:else}
  {#if material}
    <div class="mastery-panel">
      <div class="mastery-panel-header">
        <span class="mastery-panel-title">Concept mastery</span>
        <Button variant="secondary" text="Refresh" onClick={refresh} />
      </div>
      {#if material.concepts.length === 0}
        <div class="mastery-empty">
          No mastery evidence for this material yet. Take the assessment or review the cards to
          build mastery.
        </div>
      {:else}
        {#each material.concepts as m (m.concept_id)}
          {@const dims = m.dimensions}
          <section class="mastery-concept">
            <header class="mastery-concept-header">
              <span class="mastery-concept-label" dir="auto">{m.label}</span>
              <span class="mastery-badges">
                <span class={stateBadgeClass(m.state)}>{stateLabel(m.state)}</span>
                <span class="mastery-confidence">confidence: {confidenceLabel(m.confidence)}</span>
                <span class="mastery-evidence">{m.observation_count} evidence</span>
              </span>
            </header>

            <ul class="mastery-dimensions">
              {#each dims as d (d.dimension)}
                <li class="mastery-dimension">
                  <span class="mastery-dimension-name">{dimensionLabel(d.dimension)}</span>
                  <span class={dimensionStateClass(d.state)}>
                    {stateLabel(d.state)}
                  </span>
                  <span class="mastery-dimension-score">{Math.round(d.score * 100)}%</span>
                  <span class="mastery-dimension-count">{d.observation_count} obs</span>
                </li>
              {/each}
            </ul>

            {#if m.misconceptions.length > 0}
              <div class="mastery-misconceptions">
                <span class="mastery-misconceptions-title">Misconceptions</span>
                {#each m.misconceptions as mc (mc.text)}
                  <div class="mastery-misconception">
                    <span class="mastery-misconception-text" dir="auto">{mc.text}</span>
                    <span class="mastery-misconception-meta">
                      {mc.occurrences}×
                      {#if mc.resolved}
                        <span class="mastery-misconception-resolved">resolved</span>
                      {:else}
                        <span class="mastery-misconception-open">open</span>
                      {/if}
                    </span>
                  </div>
                {/each}
              </div>
            {/if}
          </section>
        {/each}
      {/if}
    </div>
  {:else if concept}
    <div class="mastery-panel">
      <div class="mastery-panel-header">
        <span class="mastery-panel-title">Concept mastery</span>
        <Button variant="secondary" text="Refresh" onClick={refresh} />
      </div>
      <section class="mastery-concept">
        <header class="mastery-concept-header">
          <span class="mastery-concept-label" dir="auto">{concept.label}</span>
          <span class="mastery-badges">
            <span class={stateBadgeClass(concept.state)}>{stateLabel(concept.state)}</span>
            <span class="mastery-confidence">confidence: {confidenceLabel(concept.confidence)}</span>
            <span class="mastery-evidence">{concept.observation_count} evidence</span>
          </span>
        </header>

        <ul class="mastery-dimensions">
          {#each concept.dimensions as d (d.dimension)}
            <li class="mastery-dimension">
              <span class="mastery-dimension-name">{dimensionLabel(d.dimension)}</span>
              <span class={dimensionStateClass(d.state)}>
                {stateLabel(d.state)}
              </span>
              <span class="mastery-dimension-score">{Math.round(d.score * 100)}%</span>
              <span class="mastery-dimension-count">{d.observation_count} obs</span>
            </li>
          {/each}
        </ul>

        {#if concept.misconceptions.length > 0}
          <div class="mastery-misconceptions">
            <span class="mastery-misconceptions-title">Misconceptions</span>
            {#each concept.misconceptions as mc (mc.text)}
              <div class="mastery-misconception">
                <span class="mastery-misconception-text" dir="auto">{mc.text}</span>
                <span class="mastery-misconception-meta">
                  {mc.occurrences}×
                  {#if mc.resolved}
                    <span class="mastery-misconception-resolved">resolved</span>
                  {:else}
                    <span class="mastery-misconception-open">open</span>
                  {/if}
                </span>
              </div>
            {/each}
          </div>
        {/if}
      </section>
    </div>
  {:else}
    <div class="mastery-panel">
      <span class="mastery-empty">No mastery evidence yet.</span>
    </div>
  {/if}
{/if}

<style>
  .mastery-panel {
    margin-top: 0.5rem;
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 8px;
    background: var(--surface-alt, #fafafa);
    padding: 0.5rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .mastery-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .mastery-panel-title {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text, #333);
  }

  .mastery-empty {
    font-size: 0.85rem;
    color: var(--muted, #888);
    font-style: italic;
  }

  .mastery-error {
    font-size: 0.85rem;
    color: var(--danger, #d9534f);
  }

  .mastery-concept {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-top: 0.25rem;
  }

  .mastery-concept + .mastery-concept {
    border-top: 1px solid var(--border, #e0e0e0);
    margin-top: 0.25rem;
  }

  .mastery-concept-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .mastery-concept-label {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text, #333);
    unicode-bidi: plaintext;
  }

  .mastery-badges {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
  }

  .mastery-badge {
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    padding: 0.1rem 0.4rem;
    border-radius: 999px;
    background: var(--surface, #fff);
    border: 1px solid var(--border, #e0e0e0);
    color: var(--muted, #888);
  }

  .mastery-badge.state-mastered {
    color: #1a7f37;
    border-color: #1a7f37;
  }
  .mastery-badge.state-strong {
    color: #2e7d32;
    border-color: #2e7d32;
  }
  .mastery-badge.state-developing {
    color: #8a6d00;
    border-color: #c9a227;
  }
  .mastery-badge.state-learning {
    color: #1558a8;
    border-color: #4a90d9;
  }
  .mastery-badge.state-weak {
    color: #a33;
    border-color: #d9534f;
  }
  .mastery-badge.state-unseen {
    color: var(--muted, #888);
    border-color: var(--border, #e0e0e0);
  }

  .mastery-confidence {
    font-size: 0.72rem;
    color: var(--muted, #888);
  }

  .mastery-evidence {
    font-size: 0.72rem;
    color: var(--muted, #888);
  }

  .mastery-dimensions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .mastery-dimension {
    display: grid;
    grid-template-columns: 8rem 7rem 3rem 4rem;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8rem;
    color: var(--text, #333);
  }

  .mastery-dimension-name {
    font-weight: 500;
  }

  .mastery-dimension-state {
    font-weight: 600;
  }
  .mastery-dimension-state.state-mastered {
    color: #1a7f37;
  }
  .mastery-dimension-state.state-strong {
    color: #2e7d32;
  }
  .mastery-dimension-state.state-developing {
    color: #8a6d00;
  }
  .mastery-dimension-state.state-learning {
    color: #1558a8;
  }
  .mastery-dimension-state.state-weak {
    color: #a33;
  }

  .mastery-dimension-score {
    text-align: right;
    color: var(--muted, #888);
    font-variant-numeric: tabular-nums;
  }

  .mastery-dimension-count {
    text-align: right;
    color: var(--muted, #888);
  }

  .mastery-misconceptions {
    margin-top: 0.25rem;
    padding: 0.4rem 0.5rem;
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 6px;
    background: var(--surface, #fff);
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .mastery-misconceptions-title {
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--muted, #888);
  }

  .mastery-misconception {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .mastery-misconception-text {
    font-size: 0.82rem;
    color: var(--text, #333);
    unicode-bidi: plaintext;
  }

  .mastery-misconception-meta {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.72rem;
    color: var(--muted, #888);
    white-space: nowrap;
  }

  .mastery-misconception-resolved {
    color: #1a7f37;
    font-weight: 600;
  }

  .mastery-misconception-open {
    color: #a33;
    font-weight: 600;
  }
</style>
