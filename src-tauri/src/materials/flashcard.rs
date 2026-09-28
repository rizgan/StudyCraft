use crate::constants;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Main flashcard struct representing a stored flashcard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flashcard {
    pub id: Option<i64>, // Database ID. None for just new cards, Some(id) for existing.
    pub deck_id: String, // UUID as string
    pub front: String,   // Question
    pub back: String,    // Answer
    pub position: i32,   // Position in deck

    // Spaced repetition fields
    pub due_date: DateTime<Utc>, // Time in which switches to "Pending" again
    pub interval_days: i32,      // Waiting period before seeing cards in the "Review" state again
    pub ease_factor: f64,        // Multiplier that decides how fast interval_days grows
    pub repetitions: i32,        // How many times user has successfully reviewed a card in a row
    pub status: CardStatus,      // Review status of the card
    pub last_reviewed: Option<DateTime<Utc>>, // Last time reviewed
    pub learning_step: i32,      // Tracks which sort-term minute-based interval the card is on

    // Metadata
    pub created_at: DateTime<Utc>, // Date of creation
    pub updated_at: DateTime<Utc>, // Date of last update
}

/// Internal card creation struct for new flashcards (without database fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFlashcard {
    pub front: String,
    pub back: String,
    pub position: i32,
    pub deck_id: Option<String>,
}

/// Card update struct for modifying existing flashcards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFlashcard {
    pub id: i64,
    pub front: String,
    pub back: String,
    pub position: i32,
}

/// Card status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CardStatus {
    #[serde(rename = "new")]
    New,
    #[serde(rename = "learning")]
    Learning,
    #[serde(rename = "review")]
    Review,
    #[serde(rename = "suspended")]
    Suspended,
}

/// Flashcard counts for deck statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashcardCounts {
    pub new_count: i32,
    pub learning_count: i32,
    pub review_count: i32,
    pub pending_count: i32,
    pub total_count: i32,
}

impl From<String> for CardStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "new" => CardStatus::New,
            "learning" => CardStatus::Learning,
            "review" => CardStatus::Review,
            "suspended" => CardStatus::Suspended,
            _ => CardStatus::New,
        }
    }
}

impl From<CardStatus> for String {
    fn from(status: CardStatus) -> Self {
        match status {
            CardStatus::New => "new".to_string(),
            CardStatus::Learning => "learning".to_string(),
            CardStatus::Review => "review".to_string(),
            CardStatus::Suspended => "suspended".to_string(),
        }
    }
}

impl Default for FlashcardCounts {
    fn default() -> Self {
        Self {
            new_count: 0,
            learning_count: 0,
            review_count: 0,
            pending_count: 0,
            total_count: 0,
        }
    }
}

impl Flashcard {
    pub fn calculate_next_review(&self, difficulty: &str) -> Flashcard {
        let now = Utc::now();
        let mut updated_card = self.clone();
        updated_card.last_reviewed = Some(now);
        updated_card.updated_at = now;

        // Anki learning steps in minutes: [1, 10]
        // TODO - Let user modify all review calculation parameters
        let learning_steps = vec![1, 10];

        match difficulty {
            constants::AGAIN => {
                // ALWAYS goes back to first learning step
                updated_card.repetitions = 0;
                updated_card.learning_step = 0;
                updated_card.status = CardStatus::Learning;

                // Reduce ease factor (only for graduated cards)
                if self.status == CardStatus::Review {
                    updated_card.ease_factor = f64::max(1.3, updated_card.ease_factor - 0.2);
                }

                // Back to first learning step (1 minute)
                updated_card.due_date = now + Duration::minutes(learning_steps[0]);
                updated_card.interval_days = 0;
            }

            constants::HARD => {
                match updated_card.status {
                    CardStatus::New => {
                        // New card answered "hard": goes to learning with special timing
                        updated_card.status = CardStatus::Learning;
                        updated_card.learning_step = 0;

                        let again_minutes = learning_steps[0] as f64;
                        let good_minutes = learning_steps[0] as f64; // First step to next step
                        let hard_minutes = ((again_minutes + good_minutes) / 2.0).round() as i64;

                        updated_card.due_date = now + Duration::minutes(hard_minutes.max(1));
                        updated_card.interval_days = 0;
                        // Slight ease reduction for hard on new cards
                        updated_card.ease_factor = f64::max(1.3, updated_card.ease_factor - 0.15);
                    }

                    CardStatus::Learning => {
                        let current_step = updated_card.learning_step as usize;

                        if current_step == 0 {
                            // First learning step: Hard = average of Again and Good
                            let again_minutes = learning_steps[0] as f64;
                            let good_minutes = if learning_steps.len() > 1 {
                                learning_steps[1] as f64
                            } else {
                                // If only one step, Good would graduate, so use a reasonable middle ground
                                ((learning_steps[0] as f64) * 2.0).min(1440.0) // Max 1 day
                            };
                            let hard_minutes =
                                ((again_minutes + good_minutes) / 2.0).round() as i64;

                            updated_card.due_date = now + Duration::minutes(hard_minutes.max(1));
                        } else {
                            // On any step after the first: Hard REPEATS the current step
                            let current_step_minutes = learning_steps[current_step];
                            updated_card.due_date = now + Duration::minutes(current_step_minutes);
                            // learning_step stays the same (repeats current step)
                        }

                        updated_card.ease_factor = f64::max(1.3, updated_card.ease_factor - 0.15);
                    }

                    CardStatus::Review => {
                        // Review card answered "hard"
                        updated_card.repetitions += 1;

                        // For hard, uses interval * 1.2 (Anki standard)
                        let new_interval = f64::max(1.0, (updated_card.interval_days as f64) * 1.2);
                        updated_card.interval_days = new_interval as i32;
                        updated_card.due_date =
                            now + Duration::days(updated_card.interval_days as i64);

                        // Reduce ease factor
                        updated_card.ease_factor = f64::max(1.3, updated_card.ease_factor - 0.15);
                    }

                    _ => {}
                }
            }

            constants::GOOD => {
                match updated_card.status {
                    CardStatus::New => {
                        // New card graduates to learning, goes to first step
                        updated_card.status = CardStatus::Learning;
                        updated_card.learning_step = 0;
                        updated_card.due_date = now + Duration::minutes(learning_steps[0]);
                        updated_card.interval_days = 0;
                    }

                    CardStatus::Learning => {
                        let current_step = updated_card.learning_step as usize;
                        if current_step < learning_steps.len() - 1 {
                            // Move to next learning step
                            updated_card.learning_step += 1;
                            let next_step_minutes = learning_steps[current_step + 1];
                            updated_card.due_date = now + Duration::minutes(next_step_minutes);
                        } else {
                            // Graduate from learning to review with 1 day interval
                            updated_card.status = CardStatus::Review;
                            updated_card.interval_days = 1;
                            updated_card.due_date = now + Duration::days(1);
                            updated_card.repetitions = 1;
                            updated_card.learning_step = 0;
                        }
                    }

                    CardStatus::Review => {
                        // Standard Anki scheduling for review cards
                        updated_card.repetitions += 1;

                        if updated_card.repetitions == 1 {
                            updated_card.interval_days = 1;
                        } else if updated_card.repetitions == 2 {
                            updated_card.interval_days = 6;
                        } else {
                            // Subsequent reviews - use ease factor
                            let new_interval: f64 =
                                (updated_card.interval_days as f64) * updated_card.ease_factor;
                            updated_card.interval_days = new_interval.round() as i32;
                            // Cap at reasonable maximum, 1 year
                            updated_card.interval_days = i32::min(updated_card.interval_days, 365);
                        }

                        updated_card.due_date =
                            now + Duration::days(updated_card.interval_days as i64);
                    }

                    _ => {}
                }
            }

            constants::EASY => {
                match updated_card.status {
                    CardStatus::New => {
                        // skip learning entirely
                        updated_card.status = CardStatus::Review;
                        updated_card.interval_days = 4; // Anki default easy interval
                        updated_card.due_date = now + Duration::days(4);
                        updated_card.repetitions = 1;
                        updated_card.learning_step = 0;
                        // Bonus to ease factor for easy
                        updated_card.ease_factor = f64::min(2.5, updated_card.ease_factor + 0.15);
                    }

                    CardStatus::Learning => {
                        // graduate immediately with easy interval
                        updated_card.status = CardStatus::Review;
                        updated_card.interval_days = 4;
                        updated_card.due_date = now + Duration::days(4);
                        updated_card.repetitions = 1;
                        updated_card.learning_step = 0;
                        updated_card.ease_factor = f64::min(2.5, updated_card.ease_factor + 0.15);
                    }

                    CardStatus::Review => {
                        updated_card.repetitions += 1;

                        if updated_card.repetitions == 1 {
                            // First review as easy
                            updated_card.interval_days = 4;
                        } else if updated_card.repetitions == 2 {
                            // Second review as easy, uses good interval + easy bonus
                            let base_interval: f64 = 6.0;
                            updated_card.interval_days = (base_interval * 1.3).round() as i32;
                        // Easy bonus
                        } else {
                            // Use ease factor with easy bonus (1.3x multiplier)
                            let new_interval: f64 = (updated_card.interval_days as f64)
                                * updated_card.ease_factor
                                * 1.3;
                            updated_card.interval_days = new_interval.round() as i32;
                            updated_card.interval_days =
                                i32::min(updated_card.interval_days, 36500);
                        }

                        updated_card.due_date =
                            now + Duration::days(updated_card.interval_days as i64);

                        // Increase ease factor (Anki increases by 0.15, max 2.5)
                        updated_card.ease_factor = f64::min(2.5, updated_card.ease_factor + 0.15);
                    }

                    _ => {}
                }
            }

            _ => {
                // Invalid difficulties are treated as "good" (program should not get here)
                return self.calculate_next_review(constants::GOOD);
            }
        }

        updated_card
    }
}

pub fn export_to_anki(
    flashcards: Vec<Flashcard>,
    tags: Option<Vec<String>>,
    format: &str,
) -> String {
    let tag_str = tags
        .map(|t| {
            t.iter()
                .map(|tag| {
                    // Anki tags cannot contain commas; keep spaces (Anki 23.10+)
                    tag.replace(',', "-")
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let mut rows: Vec<Vec<String>> = Vec::with_capacity(flashcards.len() + 1);
    rows.push(vec!["Front".into(), "Back".into(), "Tags".into()]);
    for card in &flashcards {
        rows.push(vec![
            escape_anki_field(&card.front),
            escape_anki_field(&card.back),
            tag_str.clone(),
        ]);
    }

    // Anki expects a UTF-8 BOM at the start of imported TSV/CSV files,
    // otherwise it may guess the encoding wrong and mangle non-Latin text
    // (Cyrillic, CJK, Arabic, ...).
    let mut out = if format.eq_ignore_ascii_case("csv") {
        export_csv(&rows)
    } else {
        export_tsv(&rows)
    };
    out.insert_str(0, "\u{FEFF}");
    out
}

fn escape_anki_field(text: &str) -> String {
    // Anki note fields are HTML. Escape HTML-significant characters first so
    // literal '<', '>', '&' in the user's text (e.g. code samples, math) do
    // not get parsed as tags. Then convert line breaks to <br> so the final
    // <br> tags remain intact.
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\t', "    ")
        .replace('\n', "<br>")
        .replace('\r', "")
}

fn export_tsv(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| row.join("\t"))
        .collect::<Vec<String>>()
        .join("\n")
}

fn export_csv(rows: &[Vec<String>]) -> String {
    let mut wtr = csv::Writer::from_writer(Vec::new());
    for row in rows {
        wtr.write_record(row).expect("CSV write failed");
    }
    let out = wtr.into_inner().expect("CSV into_inner failed");
    String::from_utf8(out).expect("CSV not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_card(front: &str, back: &str) -> Flashcard {
        Flashcard {
            id: None,
            deck_id: "test-deck".into(),
            front: front.into(),
            back: back.into(),
            position: 0,
            due_date: Utc::now(),
            interval_days: 0,
            ease_factor: 2.5,
            repetitions: 0,
            status: CardStatus::New,
            last_reviewed: None,
            learning_step: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // Strip the UTF-8 BOM that export_to_anki prepends for Anki.
    fn strip_bom(s: &str) -> &str {
        s.strip_prefix('\u{FEFF}').unwrap_or(s)
    }

    #[test]
    fn test_normal_english_card() {
        let cards = vec![make_card(
            "What is ownership?",
            "Rust's ownership system...",
        )];
        let result = export_to_anki(cards, None, "tsv");
        let lines: Vec<&str> = strip_bom(&result).lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "Front\tBack\tTags");
        assert_eq!(lines[1], "What is ownership?\tRust's ownership system...\t");
    }

    #[test]
    fn test_bom_present_in_tsv_and_csv() {
        let cards = vec![make_card("Q", "A")];
        assert!(export_to_anki(cards.clone(), None, "tsv").starts_with('\u{FEFF}'));
        assert!(export_to_anki(cards, None, "csv").starts_with('\u{FEFF}'));
    }

    #[test]
    fn test_html_special_chars_escaped() {
        let cards = vec![make_card(
            "if (a < b) && c > d",
            "a & b<br>literal",
        )];
        let result = export_to_anki(cards, None, "tsv");
        let line: &str = strip_bom(&result).lines().nth(1).unwrap();
        assert!(line.contains("if (a &lt; b) &amp;&amp; c &gt; d"));
        // Literal '<br>' typed by the user must stay literal, not become a tag
        assert!(line.contains("a &amp; b&lt;br&gt;literal"));
    }

    #[test]
    fn test_russian_text() {
        let cards = vec![make_card("Что такое Rust?", "Язык программирования")];
        let result = export_to_anki(cards, None, "tsv");
        assert!(result.contains("Что такое Rust?"));
        assert!(result.contains("Язык программирования"));
    }

    #[test]
    fn test_arabic_text() {
        let cards = vec![make_card("ما معنى كتاب؟", "Книга")];
        let result = export_to_anki(cards, None, "tsv");
        assert!(result.contains("ما معنى كتاب؟"));
        assert!(result.contains("Книга"));
    }

    #[test]
    fn test_comma_inside_content() {
        let cards = vec![make_card("List items", "apple, banana, cherry")];
        let result_csv = export_to_anki(cards.clone(), None, "csv");
        // In CSV, fields with commas are quoted
        assert!(result_csv.contains("\"apple, banana, cherry\""));

        let result_tsv = export_to_anki(cards, None, "tsv");
        // In TSV, commas are fine as-is
        assert!(result_tsv.contains("apple, banana, cherry"));
    }

    #[test]
    fn test_quotes_inside_content() {
        let cards = vec![make_card(
            "What is \"borrowing\"?",
            "A \"reference\" to data",
        )];
        let result_csv = export_to_anki(cards.clone(), None, "csv");
        assert!(result_csv.contains("\"What is \"\"borrowing\"\"?\""));
        assert!(result_csv.contains("\"A \"\"reference\"\" to data\""));

        let result_tsv = export_to_anki(cards, None, "tsv");
        assert!(result_tsv.contains("What is \"borrowing\"?"));
    }

    #[test]
    fn test_multiline_answer() {
        let cards = vec![make_card("Q", "line1\nline2\nline3")];
        let result = export_to_anki(cards, None, "tsv");
        // Newlines converted to <br>
        assert!(result.contains("line1<br>line2<br>line3"));
    }

    #[test]
    fn test_code_block() {
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let cards = vec![make_card("What does this do?", code)];
        let result = export_to_anki(cards, None, "tsv");
        assert!(result.contains("fn main() {"));
        assert!(result.contains("<br>"));
    }

    #[test]
    fn test_tab_character() {
        let cards = vec![make_card("Has\ttab", "value\twith\ttabs")];
        let result = export_to_anki(cards, None, "tsv");
        // Tabs replaced with 4 spaces
        let line: &str = result.lines().nth(1).unwrap();
        assert!(!line.contains("\t\t"));
        assert!(line.contains("Has    tab"));
        assert!(line.contains("value    with    tabs"));
    }

    #[test]
    fn test_empty_optional_field() {
        let cards = vec![make_card("Q", "A")];
        let result = export_to_anki(cards, None, "tsv");
        let lines: Vec<&str> = result.lines().collect();
        // Last column (tags) should be empty
        assert!(lines[1].ends_with('\t'));
    }

    #[test]
    fn test_tags_export() {
        let cards = vec![make_card("Q", "A")];
        let tags = vec!["rust".to_string(), "ownership".to_string()];
        let result = export_to_anki(cards, Some(tags), "tsv");
        let lines: Vec<&str> = strip_bom(&result).lines().collect();
        assert!(lines[1].ends_with("rust ownership"));
    }

    #[test]
    fn test_tag_with_comma_sanitized() {
        let cards = vec![make_card("Q", "A")];
        let tags = vec!["chapter,1".to_string(), "rust".to_string()];
        let result = export_to_anki(cards, Some(tags), "tsv");
        let lines: Vec<&str> = strip_bom(&result).lines().collect();
        assert!(lines[1].ends_with("chapter-1 rust"));
    }

    #[test]
    fn test_utf8_validity() {
        let cards = vec![
            make_card("English", "Response"),
            make_card("Русский текст", "Ответ"),
            make_card("ما معنى كتاب؟", "Книга"),
            make_card("日本語", "答え"),
        ];
        let result = export_to_anki(cards, None, "tsv");
        // If we get here without panic, it's valid UTF-8
        assert!(!result.is_empty());
    }

    #[test]
    fn test_apkg_export_creates_valid_package() {
        use crate::materials::anki_apkg::export_apkg;
        use std::io::Read;

        let cards = vec![
            make_card("What is Rust?", "A systems programming language"),
            make_card("Что такое Rust?", "Язык системного программирования"),
        ];

        let output_path = std::env::temp_dir().join(format!(
            "studycraft_apkg_test_{}.apkg",
            std::process::id()
        ));
        let db_path = std::env::temp_dir().join(format!(
            "studycraft_apkg_readback_{}.db",
            std::process::id()
        ));

        let result = export_apkg(&cards, "Test Deck", &output_path);
        assert!(result.is_ok(), "export failed: {:?}", result);
        assert!(output_path.exists());

        // Verify it is a valid ZIP archive containing the expected entries.
        let file = std::fs::File::open(&output_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).expect("apkg should be a valid zip");

        let names: Vec<String> = (0..archive.len())
            .map(|i| {
                archive
                    .by_index(i)
                    .unwrap()
                    .enclosed_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        assert!(names.contains(&"collection.anki2".to_string()));
        assert!(names.contains(&"metadata.json".to_string()));

        // Verify the embedded SQLite db opens and contains the notes.
        let db_bytes: Vec<u8> = {
            let mut entry = archive.by_name("collection.anki2").unwrap();
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            buf
        };
        std::fs::write(&db_path, &db_bytes).unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();

        let note_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(note_count, 2);

        let card_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM cards", [], |row| row.get(0))
            .unwrap();
        assert_eq!(card_count, 2);

        let field_data: String = conn
            .query_row(
                "SELECT data FROM notes WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            field_data,
            "What is Rust?\x1fA systems programming language"
        );

        let deck_name: String = conn
            .query_row("SELECT nme FROM decks WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(deck_name, "Test Deck");

        // Verify model definition is present.
        let model_name: String = conn
            .query_row("SELECT name FROM models WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(model_name, "StudyCraft Basic");

        // Verify metadata.json contents.
        let meta_str: String = {
            let mut entry = archive.by_name("metadata.json").unwrap();
            let mut s = String::new();
            entry.read_to_string(&mut s).unwrap();
            s
        };
        let meta: serde_json::Value = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta["version"], 1);
        assert_eq!(meta["deck"]["name"], "Test Deck");

        // Cleanup.
        let _ = std::fs::remove_file(&output_path);
        let _ = std::fs::remove_file(&db_path);

        // Empty deck must be rejected.
        let empty_path = std::env::temp_dir().join(format!("studycraft_apkg_empty_{}.apkg", std::process::id()));
        assert!(export_apkg(&[], "Empty", &empty_path).is_err());
    }
}
