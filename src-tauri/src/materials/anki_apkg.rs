//! Native Anki `.apkg` (Anki Package) export.
//!
//! An .apkg file is a ZIP archive containing:
//! - `collection.anki2` — a SQLite database (Anki 2.1.x schema) with the notes to import
//! - `metadata.json`    — package metadata (`version`, `deck`, `collection` fields)
//!
//! The database schema used here is the "Anki 2.1" subset that Anki 2.1.54+
//! (and Anki 23.10+) imports natively. Note field values in the `notes.data`
//! column are joined by `\x1f` (unit separator).

use crate::materials::flashcard::Flashcard;
use rusqlite::{params, Connection};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Minimal Anki 2.1 database schema (only the tables Anki needs to import a
/// deck; all other tables exist so Anki's import can migrate the file).
const ANKI_SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = MEMORY;
PRAGMA synchronous = OFF;

CREATE TABLE col (id INTEGER PRIMARY KEY, crt INTEGER, mod INTEGER, scm INTEGER, ver INTEGER, min INTEGER, usn INTEGER, ls INTEGER);
CREATE TABLE decks (id INTEGER PRIMARY KEY, hwp, nme TEXT, mod INTEGER, usn INTEGER, conf);
CREATE TABLE deckconf (id INTEGER PRIMARY KEY, name, mod INTEGER, usn INTEGER, newPerDay INTEGER, intvPerDay INTEGER, burPerDay INTEGER, relMinIntv INTEGER, revPerDay INTEGER, ease4 INTEGER, factor INTEGER, lapse INTEGER, dist INTEGER, dupe INTEGER, maxTaken INTEGER, ivlFct INTEGER, maxIvl INTEGER, initialEase INTEGER, hardFactor INTEGER, timer INTEGER, autoplay INTEGER, replay INTEGER, relearnAfter INTEGER, relearnStep INTEGER, newBootstrap INTEGER, newBulkSize INTEGER, newDelay INTEGER, newOrder INTEGER, dyn INTEGER, collapsed INTEGER, browserSortField TEXT, browserSortOrder INTEGER, maxCount INTEGER, maxTime INTEGER);
CREATE TABLE models (id INTEGER PRIMARY KEY, hwp, mod INTEGER, usn INTEGER, name, type INTEGER, tmpls, flds, cmpls, sortf, lazyNames INTEGER DEFAULT 0);
CREATE TABLE notes (id INTEGER PRIMARY KEY, hwp, mid, mod, guid, flags, data);
CREATE TABLE cards (id INTEGER PRIMARY KEY, nid, did, ord, mod, lmod, lpos, due, ivl, flags INTEGER DEFAULT 0, data);
CREATE TABLE graves (id, usn INTEGER, oid);
CREATE TABLE tags (id INTEGER PRIMARY KEY, name, nme, usn);
CREATE TABLE versions (name, value);
CREATE TABLE revlog (id INTEGER PRIMARY KEY, cid, usn, ease, ivl, lastIvl, factor, time, type INTEGER, reviewOrder, queue, lastQueue);
CREATE TABLE conf (id INTEGER PRIMARY KEY, name, mod, usn, pref, desc);
CREATE TABLE "check" (type INTEGER, lastCheck, count);
CREATE TABLE "schema" (version INTEGER, ver INTEGER, flags);
"#;

/// Note model: one "Front" and one "Back" field, single basic template.
const MODEL_TMPLS: &str = r#"[{"name":"Card 1","q":"{{Front}}","a":"{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}","ord":0}]"#;
const MODEL_FLDS: &str = r#"[{"name":"Front","ord":0},{"name":"Back","ord":1}]"#;
const MODEL_CMPLS: &str = "[]";
const MODEL_NAME: &str = "StudyCraft Basic";
const ANKI_DB_VERSION: i64 = 11;
const ANKI_SCHEMA_VERSION: i64 = 21;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Generate an Anki-style GUID (32 hex chars, URL-safe).
fn generate_guid(counter: u64) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    now_secs().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    counter.hash(&mut hasher);
    let a = hasher.finish();
    let mut hasher2 = DefaultHasher::new();
    a.hash(&mut hasher2);
    counter.wrapping_mul(0x9E3779B97F4A7C15).hash(&mut hasher2);
    let b = hasher2.finish();
    format!("{:016x}{:016x}", a, b)
}

/// Generate the `.apkg` file at `output_path` containing `flashcards` in the deck `deck_name`.
pub fn export_apkg(
    flashcards: &[Flashcard],
    deck_name: &str,
    output_path: &Path,
) -> Result<(), String> {
    if flashcards.is_empty() {
        return Err("Cannot export an empty deck".to_string());
    }

    let db_bytes = build_collection_db(flashcards, deck_name)?;
    let metadata = build_metadata_json(deck_name)?;

    let file = std::fs::File::create(output_path)
        .map_err(|e| format!("Failed to create file at {}: {}", output_path.display(), e))?;
    let mut zip = zip::ZipWriter::new(file);

    let db_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    zip.start_file("collection.anki2", db_opts)
        .map_err(|e| format!("Failed to write collection.anki2: {}", e))?;
    zip.write_all(&db_bytes)
        .map_err(|e| format!("Failed to write collection.anki2: {}", e))?;

    let meta_opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    zip.start_file("metadata.json", meta_opts)
        .map_err(|e| format!("Failed to write metadata.json: {}", e))?;
    zip.write_all(&metadata)
        .map_err(|e| format!("Failed to write metadata.json: {}", e))?;

    zip.finish()
        .map_err(|e| format!("Failed to finalize .apkg archive: {}", e))?;
    Ok(())
}

/// Build the `metadata.json` contents (package version 1).
fn build_metadata_json(deck_name: &str) -> Result<Vec<u8>, String> {
    let now = now_secs();
    let metadata = json!({
        "version": 1,
        "collection": {
            "mod": now,
            "scm": 0,
            "ver": ANKI_DB_VERSION,
            "dty": 0
        },
        "deck": {
            "name": deck_name,
            "conf": {
                "name": deck_name,
                "newPerDay": 20,
                "revPerDay": 9999,
                "burPerDay": 9999,
                "ease4": 1.3,
                "lapse": -8,
                "dist": 10,
                "initialEase": 2.5,
                "hardFactor": 1.2
            }
        }
    });
    serde_json::to_string(&metadata)
        .map(|s| s.into_bytes())
        .map_err(|e| format!("Failed to serialize metadata.json: {}", e))
}

/// Build the `collection.anki2` SQLite database and return its file bytes.
fn build_collection_db(flashcards: &[Flashcard], deck_name: &str) -> Result<Vec<u8>, String> {
    let now = now_secs();

    // Build the database in a temporary file (rusqlite has no in-memory-to-bytes API).
    let temp_path = std::env::temp_dir().join(format!(
        "studycraft_apkg_{}_{}.db",
        std::process::id(),
        now
    ));

    {
        let conn = Connection::open(&temp_path)
            .map_err(|e| format!("Failed to create Anki database: {}", e))?;
        conn.execute_batch(ANKI_SCHEMA_SQL)
            .map_err(|e| format!("Failed to create Anki schema: {}", e))?;

        // Collection row.
        conn.execute(
            "INSERT INTO col (id, crt, mod, scm, ver, min, usn, ls) VALUES (1, ?, ?, 0, ?, 0, -1, -1)",
            params![now, now, ANKI_DB_VERSION],
        )
        .map_err(|e| format!("Failed to insert collection row: {}", e))?;

        // Deck configuration.
        conn.execute(
            "INSERT INTO deckconf (id, name, mod, usn, newPerDay, intvPerDay, burPerDay, relMinIntv, revPerDay, ease4, factor, lapse, dist, dupe, maxTaken, ivlFct, maxIvl, initialEase, hardFactor, timer, autoplay, replay, relearnAfter, relearnStep, newBootstrap, newBulkSize, newDelay, newOrder, dyn, collapsed, browserSortField, browserSortOrder, maxCount, maxTime)
             VALUES (1, ?, ?, -1, 20, 9999, 9999, 1, 9999, 130, 250, -800, 1000, 0, 30, 100, 3650000, 250, 120, 0, 1, 0, 1000, 1000, '', 0, 10, 1, 0, 1, 'field:0', 0, 0, 0)",
            params![deck_name, now],
        )
        .map_err(|e| format!("Failed to insert deck configuration: {}", e))?;

        // Deck row.
        conn.execute(
            "INSERT INTO decks (id, hwp, nme, mod, usn, conf) VALUES (1, '', ?, ?, -1, 1)",
            params![deck_name, now],
        )
        .map_err(|e| format!("Failed to insert deck: {}", e))?;

        // Note model (basic Front/Back).
        conn.execute(
            "INSERT INTO models (id, hwp, mod, usn, name, type, tmpls, flds, cmpls, sortf)
             VALUES (1, '', ?, -1, ?, 0, ?, ?, ?, 0)",
            params![now, MODEL_NAME, MODEL_TMPLS, MODEL_FLDS, MODEL_CMPLS],
        )
        .map_err(|e| format!("Failed to insert note model: {}", e))?;

        // Notes and cards.
        for (i, fc) in flashcards.iter().enumerate() {
            let id = (i + 1) as i64;
            let guid = generate_guid(i as u64);
            // Anki field separator: unit separator (0x1f).
            let field_data = format!("{}\x1f{}", fc.front, fc.back);

            conn.execute(
                "INSERT INTO notes (id, hwp, mid, mod, guid, flags, data) VALUES (?, '', 1, ?, ?, 0, ?)",
                params![id, now, guid, field_data],
            )
            .map_err(|e| format!("Failed to insert note {}: {}", i, e))?;

            conn.execute(
                "INSERT INTO cards (id, nid, did, ord, mod, lmod, lpos, due, ivl, flags, data)
                 VALUES (?, ?, 1, 0, ?, 0, 0, 0, 0, 0, '')",
                params![id, id, now],
            )
            .map_err(|e| format!("Failed to insert card {}: {}", i, e))?;
        }

        // Schema version marker.
        conn.execute(
            "INSERT INTO \"schema\" (version, ver, flags) VALUES (?, ?, 1)",
            params![ANKI_SCHEMA_VERSION, ANKI_DB_VERSION],
        )
        .map_err(|e| format!("Failed to insert schema row: {}", e))?;

        conn.close().map_err(|(_, e)| format!("Failed to close Anki database: {}", e))?;
    }

    let db_bytes = std::fs::read(&temp_path)
        .map_err(|e| format!("Failed to read Anki database: {}", e))?;
    let _ = std::fs::remove_file(&temp_path);

    Ok(db_bytes)
}
