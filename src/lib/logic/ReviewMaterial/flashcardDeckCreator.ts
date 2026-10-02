import { invoke } from '@tauri-apps/api/core';

export interface StudyMaterial {
  id: string;
  name?: string;
  display_name?: string;
  file_name?: string;
  cardsToGenerate?: number;
  study_material_original?: string;
  markdown_path?: string;
}

export interface FlashcardDeck {
  id: string;
  name: string;
  type: 'flashcard_deck';
  cards_count: number;
  last_review: string;
  tags: string[];
  progress: number;
}

export interface Card {
  front: string;
  back: string;
  /**
   * Optional generated language metadata for this card. One card carries
   * its own metadata, so adding/removing/editing cards can never shift
   * metadata onto a different card. Absent (null) for cards without
   * language data.
   */
  language?: LanguageFlashcard | null;
}

export interface DeckFormData {
  name: string;
  language: string;
  tags: string[];
  selectedMaterials: StudyMaterial[];
  concept: string;
  is_language_learning?: boolean;
  learning_language?: string;
  base_language?: string;
  card_direction?: 'recognition' | 'production' | 'both';
}

export interface GeneratedCard {
  question: string;
  answer: string;
}

export interface LanguageLearningSettings {
  learning_language: string;
  base_language: string;
}

export interface LanguageVocabulary {
  target_expression: string;
  meaning: string;
  pronunciation?: string | null;
  example?: string | null;
  example_translation?: string | null;
  context?: string | null;
  grammatical_information?: string | null;
}

export interface LanguageFlashcard {
  question: string;
  answer: string;
  vocabulary?: LanguageVocabulary | null;
  direction?: 'recognition' | 'production' | null;
  task?: 'translation' | 'understanding_in_context' | null;
}

export interface FlashcardLanguage {
  card_id: number;
  direction: 'recognition' | 'production';
  task: 'translation' | 'understanding_in_context';
  item: LanguageVocabulary;
}

export interface FlashcardDeckCreationResponse {
  review_material_id: string;
  review_material_name: string;
  cards_count: number;
}

export class FlashcardDeckCreatorLogic {
  private cards: Card[] = [];
  private currentStep: number = 1;
  private isCreating: boolean = false;
  private isGenerating: boolean = false;
  private error: string = '';
  private generationStatus: string = '';

  get getCurrentStep(): number {
    return this.currentStep;
  }

  get getCards(): Card[] {
    return this.cards;
  }

  get getIsCreating(): boolean {
    return this.isCreating;
  }

  get getIsGenerating(): boolean {
    return this.isGenerating;
  }

  get getError(): string {
    return this.error;
  }

  get getGenerationStatus(): string {
    return this.generationStatus;
  }

  isFormValid(formData: DeckFormData): boolean {
    return formData.name.trim() !== '' && this.cards.every(card => 
      card.front.trim() !== '' && card.back.trim() !== ''
    );
  }

  isStepComplete(step: number, formData: DeckFormData): boolean {
    switch (step) {
      case 1:
        return formData.name.trim() !== '';
      case 2:
        return this.cards.length > 0 && this.cards.every(card => 
          card.front.trim() !== '' && card.back.trim() !== ''
        );
      default:
        return false;
    }
  }

  canEnableProvider(provider: string): boolean {
    return true;
  }

  addCard(): void {
    this.cards = [...this.cards, { front: '', back: '', language: null }];
  }

  addCardWithContent(front: string, back: string): void {
    this.cards = [...this.cards, { front, back, language: null }];
  }

  removeCard(index: number): void {
    this.cards = this.cards.filter((_, i) => i !== index);
  }

  updateCard(index: number, side: 'front' | 'back', value: string): void {
    this.cards[index][side] = value;
  }

  async moveToNextStep(formData: DeckFormData): Promise<void> {
    if (this.currentStep < 3) {
      if (this.currentStep === 1 && formData.selectedMaterials.length > 0) {
        await this.generateFlashcards(formData);
      } else {
        this.currentStep++;
      }
    }
  }

  moveToPreviousStep(): void {
    if (this.currentStep > 1) {
      this.currentStep--;
    }
  }

  removeMaterial(materialToRemove: StudyMaterial, currentMaterials: StudyMaterial[]): StudyMaterial[] {
    return currentMaterials.filter(material => material.id !== materialToRemove.id);
  }

  updateMaterialCardCount(materialId: string, count: number, materials: StudyMaterial[]): StudyMaterial[] {
    return materials.map(material => 
      material.id === materialId ? { ...material, cardsToGenerate: count } : material
    );
  }

  getTotalCardsToGenerate(materials: StudyMaterial[]): number {
    return materials.reduce((sum, material) => sum + (material.cardsToGenerate || 5), 0);
  }

  private buildLanguageRequest(
    formData: DeckFormData,
    materialsRequest: { material_id: string; cards_to_generate: number }[],
  ) {
    return {
      materials: materialsRequest,
      concept: formData.concept || null,
      learning_language: formData.learning_language || '',
      base_language: formData.base_language || '',
      direction: formData.card_direction || 'both',
    };
  }

  async generateFlashcards(formData: DeckFormData): Promise<void> {
    if (formData.selectedMaterials.length === 0) {
      this.currentStep++;
      return;
    }

    this.isGenerating = true;
    this.error = '';
    this.generationStatus = 'Preparing study materials...';

    try {
      console.log('Preparing to generate flashcards for:', formData.selectedMaterials);
      
      const materialsRequest = formData.selectedMaterials.map(material => {
        if (!material.id) {
          console.warn('Missing material ID for material:', material);
          return null;
        }
        
       
        return {
          material_id: material.id, 
          cards_to_generate: material.cardsToGenerate || 5
        };
      }).filter(req => req !== null); 

      if (materialsRequest.length === 0) {
        throw new Error('No valid materials with IDs found');
      }

      console.log('Sending generation request:', materialsRequest);
      this.generationStatus = formData.concept
        ? `Generating flashcards focused on "${formData.concept}"...`
        : 'Sending request to AI model...';

      const isLanguageLearning =
        formData.is_language_learning === true &&
        !!formData.learning_language &&
        !!formData.base_language;

      let generatedCards: GeneratedCard[];
      const generatedLanguageCards: LanguageFlashcard[] = [];
      if (isLanguageLearning) {
        const languageCards = await invoke('generate_language_flashcards', {
          request: this.buildLanguageRequest(formData, materialsRequest),
        }) as LanguageFlashcard[];
        generatedLanguageCards.push(...languageCards);
        generatedCards = languageCards.map((card) => ({
          question: card.question,
          answer: card.answer,
        }));
      } else {
        generatedCards = await invoke('generate_flashcards', {
          request: {
            materials: materialsRequest,
            language: formData.language || 'English',
            concept: formData.concept || null
          }
        }) as GeneratedCard[];
      }

      console.log('Generated cards:', generatedCards);
      this.generationStatus = `Successfully generated ${generatedCards.length} flashcards`;

      if (generatedCards && Array.isArray(generatedCards) && generatedCards.length > 0) {
        this.cards = generatedCards.map((card: GeneratedCard, index: number) => ({
          front: card.question,
          back: card.answer,
          language: generatedLanguageCards[index] ?? null
        }));

        setTimeout(() => {
          this.currentStep = 2;
          this.isGenerating = false;
          console.log('Advanced to step 2, cards:', this.cards);
        }, 1000);
      } else {
        this.error = 'No flashcards were generated. Please try again or create cards manually.';
        this.isGenerating = false;
        this.currentStep = 2;
      }
    } catch (err) {
      console.error('Failed to generate flashcards:', err);
      this.error = `Failed to generate flashcards: ${err}. Please check the material format and try again.`;
      this.isGenerating = false;
      this.currentStep = 2;
    }
  }

  async createFlashcardDeck(formData: DeckFormData, deckId: String): Promise<FlashcardDeck> {
    if (!this.isFormValid(formData)) {
      throw new Error('Form is not valid');
    }

    this.isCreating = true;
    this.error = '';

    try {
      const cardData = this.cards.map((card, index) => ({
        front: card.front,
        back: card.back,
        deck_id: "",
        position: index,
        id: null
      }));
      
      const isLanguageLearning =
        formData.is_language_learning === true &&
        !!formData.learning_language &&
        !!formData.base_language;

      const languageLearning = isLanguageLearning
        ? {
            learning_language: formData.learning_language!,
            base_language: formData.base_language!,
          }
        : null;

      // Per-card language metadata: each card carries its own generated
      // metadata, so manually added cards are null and edited cards keep
      // theirs.
      const cardLanguage = this.cards.map((card) => {
        const generated = card.language ?? null;
        if (!generated || !generated.vocabulary) {
          return null;
        }
        const item: LanguageVocabulary = {
          target_expression: generated.vocabulary.target_expression,
          meaning: generated.vocabulary.meaning,
          pronunciation: generated.vocabulary.pronunciation ?? null,
          example: generated.vocabulary.example ?? null,
          example_translation: generated.vocabulary.example_translation ?? null,
          context: generated.vocabulary.context ?? null,
          grammatical_information:
            generated.vocabulary.grammatical_information ?? null,
        };
        return {
          card_id: 0, // assigned by the database on insert
          direction: generated.direction ?? 'recognition',
          task: generated.task ?? 'translation',
          item,
        } satisfies FlashcardLanguage;
      });

      const result = await invoke('create_flashcard_deck', {
        id: deckId,
        name: formData.name,
        tags: formData.tags,
        cards: cardData,
        language_learning: languageLearning,
        card_language: cardLanguage
      }) as FlashcardDeckCreationResponse;

      console.log('Created flashcard deck:', result);
      
      const newDeck: FlashcardDeck = {
        id: result.review_material_id,
        name: result.review_material_name,
        type: 'flashcard_deck',
        cards_count: this.cards.length,
        last_review: new Date().toISOString().split('T')[0],
        tags: formData.tags,
        progress: 0
      };

      return newDeck;
    } catch (err) {
      console.error('Failed to create flashcard deck:', err);
      this.error = `Failed to create flashcard deck: ${err}`;
      throw err;
    } finally {
      this.isCreating = false;
    }
  }

  reset(): void {
    this.cards = [];
    this.currentStep = 1;
    this.isCreating = false;
    this.isGenerating = false;
    this.error = '';
    this.generationStatus = '';
  }
}