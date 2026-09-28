<script lang="ts">
  import "../../styles/variables.css";
  import { onMount, onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';

  import Button from '../Shared/Button.svelte';
  import { toastStore } from '../../stores/toastStore';
  import {
    generateCardMnemonic,
    generateCardMnemonicImage,
    getCardMnemonic,
    deleteCardMnemonic,
    deleteCardMnemonicImage,
    getImageGenerationStatus,
    loadMnemonicImageUrl,
    invalidateMnemonicImageUrl,
    type CardMnemonicResponse,
    type ImageGenerationStatus,
  } from '../../logic/ReviewMaterial/mnemonic';

  // ------------------------------------------------------------------ Props
  /** Real flashcard id (required). The panel is a no-op until a real id exists. */
  export let cardId: number | null | undefined;
  /** The card front (question / target term). */
  export let front: string;
  /** The card back (answer / definition). */
  export let back: string;
  /** When true, language-pair mode is enabled (target + meaning distinct). */
  export let languageMode: boolean = false;
  /** Free-form learner profile to bias the mnemonic style. */
  export let learnerProfile: string = '';
  /** Fired whenever the mnemonic state changes (so the parent can react). */
  export let onMnemonicChange: (m: CardMnemonicResponse | null) => void = () => {};

  // ------------------------------------------------------------------ State
  let mnemonic: CardMnemonicResponse | null = null;
  let isGeneratingText = false;
  let isGeneratingImage = false;
  let imageAvailable = false;
  let imageUrl: string | null = null;
  let error = '';
  let expanded = false;

  // ----------------------------------------------------------------- Load
  async function refresh() {
    if (!cardId) return;
    try {
      const m = await getCardMnemonic(cardId);
      mnemonic = m;
      onMnemonicChange(m);
      if (m?.image_path) await loadImage();
    } catch (e) {
      console.error('Failed to load mnemonic:', e);
    }
  }

  async function loadImage() {
    if (!cardId || !mnemonic?.image_path) {
      imageUrl = null;
      return;
    }
    invalidateMnemonicImageUrl(cardId);
    imageUrl = await loadMnemonicImageUrl(cardId);
  }

  async function refreshImageStatus() {
    try {
      const status = await getImageGenerationStatus();
      imageAvailable = status.available;
    } catch {
      imageAvailable = false;
    }
  }

  onMount(async () => {
    await refresh();
    await refreshImageStatus();
  });

  onDestroy(() => {
    if (imageUrl) URL.revokeObjectURL(imageUrl);
  });

  // -------------------------------------------------------------- Actions
  async function handleGenerateText() {
    if (!cardId) return;
    isGeneratingText = true;
    error = '';
    try {
      const fact = `${front} — ${back}`;
      const m = await generateCardMnemonic({
        cardId,
        fact,
        kind: 'flashcard',
        languageMode: languageMode ? 'on' : 'off',
        learnerProfile: learnerProfile || undefined,
      });
      mnemonic = m;
      onMnemonicChange(m);
      expanded = true;
      if (!m.is_suitable) {
        toastStore.warning('The model judged this fact unsuitable for a mnemonic.');
      } else {
        toastStore.success('Mnemonic generated');
      }
    } catch (e) {
      console.error('Failed to generate mnemonic:', e);
      error = e instanceof Error ? e.message : 'Failed to generate mnemonic';
      toastStore.error(error);
    } finally {
      isGeneratingText = false;
    }
  }

  async function handleGenerateImage() {
    if (!cardId) return;
    isGeneratingImage = true;
    error = '';
    try {
      const m = await generateCardMnemonicImage(cardId);
      mnemonic = m;
      onMnemonicChange(m);
      expanded = true;
      await loadImage();
      toastStore.success('Mnemonic image generated');
    } catch (e) {
      console.error('Failed to generate mnemonic image:', e);
      error = e instanceof Error ? e.message : 'Failed to generate mnemonic image';
      toastStore.error(error);
    } finally {
      isGeneratingImage = false;
    }
  }

  async function handleDeleteImage() {
    if (!cardId) return;
    try {
      await deleteCardMnemonicImage(cardId);
      mnemonic = (await getCardMnemonic(cardId)) ?? null;
      onMnemonicChange(mnemonic);
      imageUrl = null;
      toastStore.success('Mnemonic image removed');
    } catch (e) {
      console.error('Failed to delete mnemonic image:', e);
      toastStore.error('Failed to remove mnemonic image');
    }
  }

  async function handleDelete() {
    if (!cardId) return;
    if (!confirm('Delete this mnemonic (and its image, if any)?')) return;
    try {
      await deleteCardMnemonic(cardId);
      mnemonic = null;
      imageUrl = null;
      onMnemonicChange(null);
      toastStore.success('Mnemonic deleted');
    } catch (e) {
      console.error('Failed to delete mnemonic:', e);
      toastStore.error('Failed to delete mnemonic');
    }
  }

  function toggleExpanded() {
    expanded = !expanded;
  }
</script>

{#if cardId}
  <div class="mnemonic-panel">
    <div class="mnemonic-header" role="button" tabindex="0" on:click={toggleExpanded} on:keydown={(e) => e.key === 'Enter' && toggleExpanded()}>
      <span class="mnemonic-title">
        <span class="mnemonic-icon" aria-hidden="true">
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 2a7 7 0 0 0-7 7c0 2 1 3.5 2 4.5V17h10v-3.5c1-1 2-2.5 2-4.5a7 7 0 0 0-7-7z"></path>
            <line x1="9" y1="20" x2="15" y2="20"></line>
          </svg>
        </span>
        {#if mnemonic && mnemonic.is_suitable}
          Mnemonic
        {:else if mnemonic}
          <span class="mnemonic-unsuitable">Not suitable for a mnemonic</span>
        {:else}
          No mnemonic yet
        {/if}
      </span>
      <span class="mnemonic-chevron" class:expanded={expanded}>{expanded ? '▾' : '▸'}</span>
    </div>

    {#if expanded}
      <div class="mnemonic-body" transition:fade={{ duration: 150 }}>
        {#if mnemonic && mnemonic.is_suitable}
          <div class="mnemonic-text">{mnemonic.mnemonic_text}</div>

          {#if mnemonic.language}
            <div class="mnemonic-language">
              <div class="mnemonic-lang-row">
                <span class="mnemonic-lang-label">Target</span>
                <span class="mnemonic-lang-value">{mnemonic.language.target_expression}</span>
              </div>
              <div class="mnemonic-lang-row">
                <span class="mnemonic-lang-label">Meaning</span>
                <span class="mnemonic-lang-value">{mnemonic.language.meaning}</span>
              </div>
              <div class="mnemonic-lang-row">
                <span class="mnemonic-lang-label">Pronunciation</span>
                <span class="mnemonic-lang-value">{mnemonic.language.transliteration}</span>
              </div>
            </div>
          {/if}

          {#if imageUrl}
            <div class="mnemonic-image-wrap">
              <img src={imageUrl} alt="" class="mnemonic-image" aria-label="Mnemonic association image" />
              <button class="mnemonic-remove-image" on:click={handleDeleteImage}>Remove image</button>
            </div>
          {/if}
        {:else if mnemonic}
          <div class="mnemonic-unsuitable-text">
            {mnemonic.unsuitable_reason || 'The model judged this fact unsuitable for a mnemonic.'}
          </div>
        {/if}

        {#if error}
          <div class="mnemonic-error">{error}</div>
        {/if}

        <div class="mnemonic-actions">
          <Button
            variant="secondary"
            text={isGeneratingText ? 'Generating…' : (mnemonic && mnemonic.is_suitable ? 'Regenerate' : 'Generate mnemonic')}
            onClick={handleGenerateText}
            disabled={isGeneratingText || isGeneratingImage}
          />
          <Button
            variant="secondary"
            text={isGeneratingImage ? 'Creating image…' : (imageUrl ? 'Regenerate image' : 'Generate image')}
            onClick={handleGenerateImage}
            disabled={isGeneratingText || isGeneratingImage || !imageAvailable}
          />
          {#if mnemonic}
            <Button variant="danger" text="Delete" onClick={handleDelete} disabled={isGeneratingText || isGeneratingImage} />
          {/if}
        </div>

        {#if !imageAvailable}
          <div class="mnemonic-hint">
            Image generation is not configured. Enable it in Settings to create mnemonic images.
          </div>
        {/if}
      </div>
    {/if}
  </div>
{:else}
  <div class="mnemonic-panel mnemonic-panel--pending">
    <span class="mnemonic-title">
      <span class="mnemonic-icon" aria-hidden="true">
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 2a7 7 0 0 0-7 7c0 2 1 3.5 2 4.5V17h10v-3.5c1-1 2-2.5 2-4.5a7 7 0 0 0-7-7z"></path>
          <line x1="9" y1="20" x2="15" y2="20"></line>
        </svg>
      </span>
      <span class="mnemonic-pending">Mnemonics available after the card is saved</span>
    </span>
  </div>
{/if}

<style>
  .mnemonic-panel {
    margin-top: 0.5rem;
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 8px;
    background: var(--surface-alt, #fafafa);
    overflow: hidden;
  }

  .mnemonic-panel--pending {
    opacity: 0.7;
  }

  .mnemonic-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    cursor: pointer;
    user-select: none;
  }

  .mnemonic-header:focus-visible {
    outline: 2px solid var(--accent, #4a90d9);
    outline-offset: -2px;
  }

  .mnemonic-title {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text, #333);
  }

  .mnemonic-icon {
    display: inline-flex;
    color: var(--accent, #4a90d9);
  }

  .mnemonic-unsuitable {
    font-size: 0.8rem;
    color: var(--muted, #888);
    font-weight: 500;
  }

  .mnemonic-pending {
    font-size: 0.8rem;
    color: var(--muted, #888);
    font-weight: 500;
  }

  .mnemonic-chevron {
    font-size: 0.8rem;
    color: var(--muted, #888);
    transition: transform 0.15s ease;
  }

  .mnemonic-chevron.expanded {
    transform: rotate(0deg);
  }

  .mnemonic-body {
    padding: 0 0.75rem 0.75rem;
    border-top: 1px solid var(--border, #e0e0e0);
  }

  .mnemonic-text {
    margin-top: 0.5rem;
    font-size: 0.9rem;
    line-height: 1.5;
    color: var(--text, #333);
    white-space: pre-wrap;
  }

  .mnemonic-unsuitable-text {
    margin-top: 0.5rem;
    font-size: 0.85rem;
    font-style: italic;
    color: var(--muted, #888);
  }

  .mnemonic-language {
    margin-top: 0.75rem;
    padding: 0.6rem 0.75rem;
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 6px;
    background: var(--surface, #fff);
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .mnemonic-lang-row {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
  }

  .mnemonic-lang-label {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted, #888);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    min-width: 7rem;
  }

  .mnemonic-lang-value {
    font-size: 0.9rem;
    color: var(--text, #333);
    unicode-bidi: plaintext;
  }

  .mnemonic-image-wrap {
    position: relative;
    margin-top: 0.75rem;
  }

  .mnemonic-image {
    width: 100%;
    max-width: 420px;
    max-height: 300px;
    object-fit: contain;
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 6px;
    background: var(--surface, #fff);
  }

  .mnemonic-remove-image {
    margin-top: 0.3rem;
    font-size: 0.75rem;
    color: var(--muted, #888);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .mnemonic-remove-image:hover {
    color: var(--danger, #d9534f);
  }

  .mnemonic-error {
    margin-top: 0.5rem;
    font-size: 0.8rem;
    color: var(--danger, #d9534f);
  }

  .mnemonic-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }

  .mnemonic-hint {
    margin-top: 0.6rem;
    font-size: 0.75rem;
    color: var(--muted, #888);
  }
</style>
