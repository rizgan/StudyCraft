import { invoke } from '@tauri-apps/api/core';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface LanguageMnemonic {
  /** Exact target word/expression in the target language (verbatim). */
  target_expression: string;
  /** The meaning in the learner's language. */
  meaning: string;
  /** A pronunciation aid (transliteration, IPA, or phonetic spelling). */
  transliteration: string;
}

export interface CardMnemonicResponse {
  card_id: number;
  is_suitable: boolean;
  unsuitable_reason: string | null;
  mnemonic_text: string;
  language: LanguageMnemonic | null;
  image_prompt: string | null;
  image_path: string | null;
}

export interface ImageGenerationStatus {
  enabled: boolean;
  available: boolean;
  model: string;
  size: string;
}

// ---------------------------------------------------------------------------
// Backend command names (mirror the #[tauri::command] fn names in
// src-tauri/src/commands/materials/mnemonic.rs).
// ---------------------------------------------------------------------------

const CMD = {
  generate: 'generate_card_mnemonic_cmd2',
  generateImage: 'generate_card_mnemonic_image_cmd2',
  get: 'get_card_mnemonic_cmd2',
  getDeck: 'get_deck_mnemonics_cmd2',
  delete: 'delete_card_mnemonic_cmd2',
  deleteImage: 'delete_card_mnemonic_image_cmd2',
  imageRaw: 'get_mnemonic_image_raw_cmd2',
  imageStatus: 'get_image_generation_status_cmd2',
} as const;

// Cache of object URLs so the same image is only read from disk once.
const imageUrlCache = new Map<string, string>();

// ---------------------------------------------------------------------------
// Low-level invoke wrappers
// ---------------------------------------------------------------------------

/**
 * Generate (or regenerate) the text mnemonic for a card.
 *
 * `languageMode` is 'on' for language-pair cards (target expression + meaning),
 * 'off' otherwise. Any existing image attachment is preserved.
 */
export async function generateCardMnemonic(opts: {
  cardId: number;
  fact: string;
  kind?: string;
  languageMode?: string;
  learnerProfile?: string;
}): Promise<CardMnemonicResponse> {
  return await invoke<CardMnemonicResponse>(CMD.generate, {
    cardId: opts.cardId,
    fact: opts.fact,
    kind: opts.kind ?? 'auto',
    languageMode: opts.languageMode ?? 'off',
    learnerProfile: opts.learnerProfile ?? null,
  });
}

/**
 * Generate (or regenerate) the mnemonic image for a card. Fails with a clear
 * error when image generation is disabled/unconfigured (no mock provider).
 */
export async function generateCardMnemonicImage(cardId: number): Promise<CardMnemonicResponse> {
  return await invoke<CardMnemonicResponse>(CMD.generateImage, { cardId });
}

/** Fetch the mnemonic for a single card (null if none). */
export async function getCardMnemonic(cardId: number): Promise<CardMnemonicResponse | null> {
  return await invoke<CardMnemonicResponse | null>(CMD.get, { cardId });
}

/** Fetch mnemonics for all cards in a deck. */
export async function getDeckMnemonics(deckId: string): Promise<CardMnemonicResponse[]> {
  return await invoke<CardMnemonicResponse[]>(CMD.getDeck, { deckId });
}

/** Delete a card's mnemonic row and its image file. */
export async function deleteCardMnemonic(cardId: number): Promise<void> {
  await invoke(CMD.delete, { cardId });
}

/** Remove only the image (keeps the text mnemonic). */
export async function deleteCardMnemonicImage(cardId: number): Promise<void> {
  await invoke(CMD.deleteImage, { cardId });
}

/** Report whether the image-generation capability is configured/available. */
export async function getImageGenerationStatus(): Promise<ImageGenerationStatus> {
  return await invoke<ImageGenerationStatus>(CMD.imageStatus);
}

/**
 * Load the stored mnemonic image bytes for a card and return a displayable
 * object URL (mirrors the `get_cover_image_raw` + blob pattern used for
 * study-material covers). Returns null when no image is set or the read fails.
 */
export async function loadMnemonicImageUrl(cardId: number): Promise<string | null> {
  if (!cardId) return null;
  const cached = imageUrlCache.get(cardId);
  if (cached) return cached;
  try {
    const bytes = await invoke<number[]>(CMD.imageRaw, { cardId });
    const blob = new Blob([new Uint8Array(bytes)], { type: 'image/png' });
    const url = URL.createObjectURL(blob);
    imageUrlCache.set(cardId, url);
    return url;
  } catch (e) {
    console.error('Failed to load mnemonic image:', e);
    return null;
  }
}

/** Invalidate the cached object URL for a card (call after image changes). */
export function invalidateMnemonicImageUrl(cardId: number): void {
  const cached = imageUrlCache.get(cardId);
  if (cached) {
    URL.revokeObjectURL(cached);
    imageUrlCache.delete(cardId);
  }
}
