//! Integration tests for the language-learning core (session 7A).
//!
//! Each test runs against a private, freshly created database file (never
//! the real application database) and verifies that language-learning decks
//! keep their identification and their structured vocabulary data across
//! save/restart/reopen, that general decks stay general, and that cards
//! without language metadata still work.

use app_lib::materials::flashcard::{export_to_anki, NewFlashcard};
use app_lib::materials::language_learning::{
    render_translation_card, CardDirectionChoice, CardTask, FlashcardLanguage,
    LanguageLearningSettings, LearningDirection, VocabularyItem,
};
use app_lib::services::db_service::{
    DbService, FlashcardDeckOperations, FlashcardLanguageOperations, ReviewMaterialOperations,
};

/// Opens a brand-new database for a single test. The file is created under
/// the OS temp dir and removed when the service is dropped.
fn new_db_service() -> (DbService, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp.path().join("studycraft_language.db");
    let service = DbService::new_for_tests(&db_path).expect("failed to open test database");
    (service, temp)
}

fn language_settings() -> LanguageLearningSettings {
    LanguageLearningSettings {
        learning_language: "Arabic".to_string(),
        base_language: "English".to_string(),
    }
}

fn flashcard(front: &str, back: &str, position: i32) -> NewFlashcard {
    NewFlashcard {
        front: front.to_string(),
        back: back.to_string(),
        position,
        deck_id: None,
    }
}

fn language_metadata(
    card_id: i64,
    direction: LearningDirection,
    task: CardTask,
    target: &str,
    meaning: &str,
) -> FlashcardLanguage {
    FlashcardLanguage {
        card_id,
        direction,
        task,
        item: VocabularyItem {
            target_expression: target.to_string(),
            meaning: meaning.to_string(),
            pronunciation: None,
            example: None,
            example_translation: None,
            context: None,
            grammatical_information: None,
        },
    }
}

/// Creating a deck in language-learning mode must persist the mode and the
/// per-card structured vocabulary; reopening the database must surface the
/// exact same data (Arabic without tashkeel stays untouched).
#[test]
fn language_deck_mode_survives_save_and_reopen() {
    let (db, _temp) = new_db_service();

    let deck_id = "lang-deck-mode";
    let cards = vec![
        flashcard("كتاب", "book", 0),
        flashcard("قِطَّة", "cat", 1),
        flashcard("日本語", "Japanese language", 2),
    ];
    let language = vec![
        Some(language_metadata(
            0,
            LearningDirection::Recognition,
            CardTask::Translation,
            "كتاب",
            "book",
        )),
        Some(language_metadata(
            0,
            LearningDirection::Production,
            CardTask::Translation,
            "قِطَّة",
            "cat",
        )),
        Some(language_metadata(
            0,
            LearningDirection::Recognition,
            CardTask::UnderstandingInContext,
            "日本語",
            "Japanese language",
        )),
    ];

    db.create_flashcard_deck(
        deck_id,
        "Arabic Basics",
        &[],
        &cards,
        Some(&language_settings()),
        &language,
    )
    .expect("failed to create language deck");

    // The deck must be identified as language-learning content.
    let material = db
        .get_review_material(deck_id)
        .expect("failed to load deck")
        .expect("deck missing after create");
    let settings = material
        .language_learning
        .as_ref()
        .expect("deck must carry its language-learning identification");
    assert_eq!(settings.learning_language, "Arabic");
    assert_eq!(settings.base_language, "English");

    // The per-card structured vocabulary must be persisted 1:1 with the
    // cards, in card order, with diacritics preserved exactly.
    let stored = db
        .get_flashcard_language_for_deck(deck_id)
        .expect("failed to load language metadata");
    assert_eq!(stored.len(), 3, "expected one metadata row per card");

    assert_eq!(stored[0].item.target_expression, "كتاب");
    assert_eq!(stored[0].item.meaning, "book");
    assert_eq!(stored[0].direction, LearningDirection::Recognition);
    assert_eq!(stored[0].task, CardTask::Translation);

    assert_eq!(stored[1].item.target_expression, "قِطَّة");
    assert_eq!(stored[1].direction, LearningDirection::Production);

    assert_eq!(stored[2].item.target_expression, "日本語");
    assert_eq!(stored[2].task, CardTask::UnderstandingInContext);

    // The flashcards themselves are untouched standard front/back pairs.
    let cards = db.get_flashcards(deck_id).expect("failed to load cards");
    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0].front, "كتاب");
    assert_eq!(cards[1].back, "cat");
    assert_eq!(cards[2].front, "日本語");
}

/// A deck created without language-learning mode must stay general: no
/// identification flag and no per-card language metadata.
#[test]
fn general_deck_stays_general() {
    let (db, _temp) = new_db_service();

    let deck_id = "general-deck";
    db.create_flashcard_deck(
        deck_id,
        "General Deck",
        &[],
        &[flashcard("What is X?", "X is a thing", 0)],
        None,
        &[],
    )
    .expect("failed to create general deck");

    let material = db
        .get_review_material(deck_id)
        .expect("failed to load deck")
        .expect("deck missing");
    assert!(
        material.language_learning.is_none(),
        "general deck must not be identified as language-learning"
    );

    let stored = db
        .get_flashcard_language_for_deck(deck_id)
        .expect("failed to load language metadata");
    assert!(
        stored.is_empty(),
        "general deck must have no per-card language metadata"
    );
}

/// Arabic diacritics (tashkeel/harakat) must survive storage byte-for-byte:
/// a word written without diacritics must not gain them, and one written
/// with them must not lose them.
#[test]
fn arabic_diacritics_are_preserved_exactly() {
    let (db, _temp) = new_db_service();

    let deck_id = "lang-deck-diacritics";
    let cards = vec![
        flashcard("كتاب", "book (no diacritics)", 0),
        flashcard("كِتَاب", "book (full diacritics)", 1),
    ];
    let language = vec![
        Some(language_metadata(
            0,
            LearningDirection::Recognition,
            CardTask::Translation,
            "كتاب",
            "book",
        )),
        Some(language_metadata(
            0,
            LearningDirection::Recognition,
            CardTask::Translation,
            "كِتَاب",
            "book",
        )),
    ];

    db.create_flashcard_deck(
        deck_id,
        "Diacritics",
        &[],
        &cards,
        Some(&language_settings()),
        &language,
    )
    .expect("failed to create deck");

    let stored = db
        .get_flashcard_language_for_deck(deck_id)
        .expect("failed to load language metadata");
    assert_eq!(
        stored[0].item.target_expression, "كتاب",
        "word without tashkeel must not gain tashkeel"
    );
    assert_eq!(
        stored[1].item.target_expression, "كِتَاب",
        "tashkeel must not be stripped or normalized"
    );
}

/// Optional linguistic fields (example, example translation, context,
/// grammatical information, pronunciation) must round-trip when present and
/// stay absent when not provided. A card must work with no metadata at all.
#[test]
fn optional_linguistic_fields_round_trip() {
    let (db, _temp) = new_db_service();

    let deck_id = "lang-deck-optional";
    let cards = vec![
        flashcard("What is 'سَلام' in: \"مرحبا، سلام!\"?", "greeting: hello", 0),
        flashcard("plain card", "plain answer", 1),
    ];
    let rich = FlashcardLanguage {
        card_id: 0,
        direction: LearningDirection::Recognition,
        task: CardTask::UnderstandingInContext,
        item: VocabularyItem {
            target_expression: "سَلام".to_string(),
            meaning: "greeting: hello".to_string(),
            pronunciation: Some("salam".to_string()),
            example: Some("مرحبا، سلام!".to_string()),
            example_translation: Some("Hi, hello!".to_string()),
            context: Some("informal greeting".to_string()),
            grammatical_information: Some("noun, duāʾī greeting".to_string()),
        },
    };
    let language = vec![Some(rich), None];

    db.create_flashcard_deck(
        deck_id,
        "Optional Fields",
        &[],
        &cards,
        Some(&language_settings()),
        &language,
    )
    .expect("failed to create deck");

    let stored = db
        .get_flashcard_language_for_deck(deck_id)
        .expect("failed to load language metadata");
    assert_eq!(stored.len(), 1, "only the card with metadata is stored");
    let item = &stored[0].item;
    assert_eq!(item.target_expression, "سَلام");
    assert_eq!(item.pronunciation.as_deref(), Some("salam"));
    assert_eq!(item.example.as_deref(), Some("مرحبا، سلام!"));
    assert_eq!(item.example_translation.as_deref(), Some("Hi, hello!"));
    assert_eq!(item.context.as_deref(), Some("informal greeting"));
    assert_eq!(
        item.grammatical_information.as_deref(),
        Some("noun, duāʾī greeting")
    );

    // The card without metadata still exists and works as a plain card.
    let cards = db.get_flashcards(deck_id).expect("failed to load cards");
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[1].front, "plain card");
}

/// Toggling a deck's language-learning mode on and off must be reflected in
/// the persisted identification without touching its cards.
#[test]
fn language_mode_can_be_toggled_persistently() {
    let (db, _temp) = new_db_service();

    let deck_id = "lang-deck-toggle";
    db.create_flashcard_deck(
        deck_id,
        "Toggle Deck",
        &[],
        &[flashcard("a", "b", 0)],
        None,
        &[],
    )
    .expect("failed to create deck");

    let settings = LanguageLearningSettings {
        learning_language: "Japanese".to_string(),
        base_language: "English".to_string(),
    };
    db.update_review_material_language_learning(deck_id, Some(&settings))
        .expect("failed to enable mode");

    let material = db
        .get_review_material(deck_id)
        .expect("failed to load deck")
        .expect("deck missing");
    assert_eq!(
        material
            .language_learning
            .as_ref()
            .map(|s| s.learning_language.as_str()),
        Some("Japanese"),
        "mode must be persisted when enabled"
    );

    db.update_review_material_language_learning(deck_id, None)
        .expect("failed to disable mode");

    let material = db
        .get_review_material(deck_id)
        .expect("failed to load deck")
        .expect("deck missing");
    assert!(
        material.language_learning.is_none(),
        "mode must be cleared when disabled"
    );

    // Cards are untouched by the toggle.
    let cards = db.get_flashcards(deck_id).expect("failed to load cards");
    assert_eq!(cards.len(), 1);
}

/// A pre-language-learning database file (review_materials without the new
/// columns) must open cleanly: existing decks become readable and are NOT
/// identified as language-learning.
#[test]
fn legacy_database_opens_without_language_columns() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp.path().join("studycraft_legacy_language.db");
    let conn = rusqlite::Connection::open(&db_path).expect("failed to open legacy db");

    conn.execute_batch(
        "
        CREATE TABLE review_materials (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            type TEXT NOT NULL,
            last_review TIMESTAMP,
            created_at TIMESTAMP NOT NULL,
            updated_at TIMESTAMP NOT NULL,
            deleted BOOLEAN NOT NULL DEFAULT 0
        );
    ",
    )
    .expect("create legacy review_materials");
    conn.execute(
        "INSERT INTO review_materials (id, name, type, last_review, created_at, updated_at, deleted)
         VALUES ('legacy-deck', 'Legacy Deck', 'flashcard_deck', NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 0)",
        [],
    )
    .expect("insert legacy deck");
    conn.close().expect("close legacy db");

    let mut service = DbService::new_for_tests(&db_path).expect("failed to open migrated db");

    let material = service
        .get_review_material("legacy-deck")
        .expect("failed to load legacy deck")
        .expect("legacy deck missing");
    assert!(
        material.language_learning.is_none(),
        "legacy deck must not be identified as language-learning"
    );

    // The deck must remain usable after migration.
    let cards = service
        .get_flashcards("legacy-deck")
        .expect("failed to load cards");
    assert_eq!(cards.len(), 0);

    // Existing rows must stay readable and writable after the ALTER.
    service
        .update_review_material_last_review("legacy-deck")
        .expect("failed to update legacy row");
    let material = service
        .get_review_material("legacy-deck")
        .expect("failed to reload legacy deck")
        .expect("legacy deck missing");
    assert!(material.language_learning.is_none());
}

/// Anki export must work for language-learning decks: the standard
/// front/back cards export, and the structured vocabulary is never needed
/// for the export.
#[test]
fn anki_export_works_for_language_decks() {
    let (db, _temp) = new_db_service();

    let deck_id = "lang-deck-anki";
    let cards = vec![flashcard("كِتَاب", "book", 0), flashcard("book", "كِتَاب", 1)];
    let language = vec![
        Some(language_metadata(
            0,
            LearningDirection::Recognition,
            CardTask::Translation,
            "كِتَاب",
            "book",
        )),
        Some(language_metadata(
            0,
            LearningDirection::Production,
            CardTask::Translation,
            "كِتَاب",
            "book",
        )),
    ];

    db.create_flashcard_deck(
        deck_id,
        "Anki Export",
        &[],
        &cards,
        Some(&language_settings()),
        &language,
    )
    .expect("failed to create deck");

    let flashcards = db.get_flashcards(deck_id).expect("failed to load cards");
    let exported = export_to_anki(flashcards, Some(vec!["arabic".to_string()]), "txt");
    assert!(
        exported.contains("كِتَاب"),
        "export must contain the Arabic target expression with tashkeel"
    );
    assert!(exported.contains("book"));
}

/// The deterministic reverse-direction transform must produce the canonical
/// recognition and production texts for the same vocabulary item, and a
/// "both" deck choice must expand to exactly those two per-card directions.
#[test]
fn reverse_direction_cards_are_deterministic() {
    let item = VocabularyItem {
        target_expression: "كِتَاب".to_string(),
        meaning: "book".to_string(),
        pronunciation: None,
        example: None,
        example_translation: None,
        context: None,
        grammatical_information: None,
    };

    let (recognition_front, recognition_back) =
        render_translation_card(&item, LearningDirection::Recognition);
    assert_eq!(recognition_front, "كِتَاب");
    assert_eq!(recognition_back, "book");

    let (production_front, production_back) =
        render_translation_card(&item, LearningDirection::Production);
    assert_eq!(production_front, "book");
    assert_eq!(production_back, "كِتَاب");

    assert_eq!(
        CardDirectionChoice::Both.expand(),
        vec![
            LearningDirection::Recognition,
            LearningDirection::Production
        ]
    );
    assert_eq!(
        CardDirectionChoice::Recognition.expand(),
        vec![LearningDirection::Recognition]
    );
    assert_eq!(
        CardDirectionChoice::Production.expand(),
        vec![LearningDirection::Production]
    );
}
