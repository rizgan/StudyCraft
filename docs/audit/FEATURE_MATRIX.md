# Feature Matrix

Audit of the present tree at `3e3e57d`. Statuses describe implemented capability, not historical importance. “Test evidence” identifies repository test names that were inspected or run as noted in [AUDIT_EVIDENCE.md](AUDIT_EVIDENCE.md); test presence alone is not treated as production-path proof. `VERIFIED_COMPLETE` requires real execution with genuine data/dependencies; none of the multi-component desktop features reached that threshold in this environment.

## Product and ingestion

| ID | Feature / reconstructed requirement | Milestone evidence | Status | Entry / dependency / persistence and findings | User access / verification |
|---|---|---|---|---|---|
| A01 | Tauri desktop shell and app command bridge | Current architecture | IMPLEMENTED_NOT_VERIFIED | `src-tauri/src/main.rs:21-176` initializes `AppState`, manages it, registers commands; `src/App.svelte` invokes Tauri. Rust test/build passed; no GUI session. | Main UI. `npm run build` passed; no Tauri launch. |
| A02 | TXT and Markdown ingestion | README | IMPLEMENTED_NOT_VERIFIED | Supported extension allowlist is exactly `pdf`, `md`, `txt` (`src-tauri/src/constants.rs:26`); `commands/materials/study_material.rs:32-105` writes file and reads text; `:442-505` processes/stores Markdown and indexes. Persistent file + SQLite metadata. | Add material UI. No real import exercised. |
| A03 | PDF text extraction | README; legacy | IMPLEMENTED_NOT_VERIFIED | `services/file_service/converter.rs:35-72` launches converter sidecar; `src-python/convert_file_to_md.py:140-206` uses PyMuPDF4LLM. Sidecar binary absent from tracked tree; requires manual packaging described by README. | Add material UI; not executable end-to-end here. |
| A04 | Scanned PDF OCR | README | PARTIAL | `convert_file_to_md.py:15-83,140-194` supports Tesseract or RapidOCR only when installed; no guaranteed bundled OCR model/binary or language pack. | PDF import; dependency conditional and untested. |
| A05 | DOCX/other Office format ingestion | Historical scope | NOT_IMPLEMENTED | Allowlist is only PDF/Markdown/TXT (`constants.rs:26`); no DOCX/Office parser or converter path found. Other non-PDF supported inputs are decoded as UTF-8 text. | No DOCX import workflow. |
| A06 | Image upload, OCR/vision ingestion | Historical scope | PARTIAL | Image analysis command exists (`review_material.rs:573+`, `services/llm_service/vision.rs`), but this is a user-provided image-to-LLM helper in deck creator; no general image-source ingestion/OCR pipeline found. | Image understand UI for card generation, not content ingestion. Provider untested. |
| A07 | Website ingestion | README | IMPLEMENTED_NOT_VERIFIED | `study_material.rs:125-175` routes URL; `url_service/url_converter.rs:17-133` fetches and parses, with robots check; content is written/persisted/indexed. Network execution not attempted. | Study-material add link. Requires network. |
| A08 | GitHub/repository ingestion | README | IMPLEMENTED_NOT_VERIFIED | `git_converter.rs:9-165` clones repository, filters/reads text and creates Markdown; command routes Git URLs. Remote clone not run. | Add link. Requires network. |
| A09 | Chunking, local embeddings, vector search | README | IMPLEMENTED_NOT_VERIFIED | `vector_service.rs:47-148,157-374,385-569,610-812`; FastEmbed local AllMiniLM and sqlite-vec/FTS; embedding model downloaded/initialized separately; persistent vector DB. | Search bar and generation context; model/device download not tested. |
| A10 | Source metadata / provenance for study material and generated content | README implied | PARTIAL | Study material DB and source links exist; vector chunk metadata retains material ID/display name. Assessment payload declares `source_material_ids`, but the command discards them; generated flashcards do not have audited card-level source spans. No universal source-to-card citation record. | Materials UI. Partial traceability. |
| A11 | Deduplication / reprocessing | Historical | PARTIAL | Vector reindex/delete commands exist; no universal content digest dedupe. Processing failure after persisted insert is logged/swallowed in `study_material.rs:76-105`; retries/reprocessing recovery not shown as robust workflow. | Limited reindex command. No app-level verification. |
| A12 | Corrupt/unsupported input handling | Reliability | PARTIAL | Extension validation and converter errors exist, but failed conversion can leave a DB material/file and still return success (`study_material.rs:32-105`). | User can see material despite failed processing. |

## Generation and content management

| ID | Feature | Milestone evidence | Status | Entry / dependency / persistence and findings | User access / verification |
|---|---|---|---|---|---|
| B01 | AI flashcard generation | README | IMPLEMENTED_NOT_VERIFIED | `commands/llm.rs`, `manager.rs:515-951`, source chunks and configured `flyllm` providers; UI creator consumes result and writes deck via `create_flashcard_deck`. Provider calls real, no supported-provider execution here. | Flashcard creator; needs configured provider. |
| B02 | Language-flashcard generation | Language-related addition in HEAD | IMPLEMENTED_NOT_VERIFIED | `manager.rs:1015-1304`, prompt, language metadata persistence. Language deck storage is covered by `language_learning.rs`. No provider run. | Deck creator. |
| B03 | Classic multiple-choice test generation | README | IMPLEMENTED_NOT_VERIFIED | `manager.rs:1516-1694`; DB test CRUD and UI `TestCreator.svelte`. Genuine provider required; no E2E. | Review creation. |
| B04 | Free-response assessment generation | Session 6 (inferred from task prompt; repo commit says feature 38-46) | IMPLEMENTED_NOT_VERIFIED | `assessment.rs:87-148` calls LLM generator and validates nonempty expected content on create; generated questions persist in DB. External model untested. | Assessment Creator. |
| B05 | Explanation / ask study material | README | IMPLEMENTED_NOT_VERIFIED | `manager.rs:1367-1515,2515+`; search retrieves material chunks and LLM answers. No provider/network execution. | StudyMaterialsBar. |
| B06 | Structured output validation / invalid response handling | AI generation | PARTIAL | JSON cleaning/parsing/error paths exist in manager. Some workflows return empty arrays when there is no request/context; no broad retry/repair guarantee or fuzz/provider tests. | Indirect; provider not exercised. |
| B07 | Source-grounded generation and hallucination mitigation | Assessment generation | PARTIAL | Generation prompts and `expected_content` resolution use retrieved source chunks (`manager.rs:2822+`); assessment refuses empty reference content. But `CreateAssessmentCommand.source_material_ids` is never read or stored (`assessment.rs:25-38`, confirmed by compiler warning); reassessment/remediation deliberately fall back to **all indexed materials** (`assessment.rs:343-358,286-294`). No citation validation or claim-level grounding verification. | Assessment route; source selection/provenance is not preserved and real grounding quality is untested. |
| B08 | Duplicate prevention | AI generation | PARTIAL | Flashcard/assessment prompt normalization and in-batch filtering exist; no global semantic duplicate index. | Creator only. |
| B09 | Edit, save, restore decks/cards/tests | Legacy | IMPLEMENTED_NOT_VERIFIED | `review_material.rs` commands + SQLite operations; frontend creators/editors. DB-focused tests and code; no GUI round trip/restart. | MaterialsBoard CRUD. |
| B10 | Tags/search/filter | Legacy | IMPLEMENTED_NOT_VERIFIED | Tauri tag CRUD and frontend search-by-name; vector semantic search. No GUI test. | Search/filter UI. |

## Decks, review, assessment, mastery

| ID | Feature | Milestone evidence | Status | Entry / dependency / persistence and findings | User access / verification |
|---|---|---|---|---|---|
| C01 | Deck/card create, edit, delete, list, retrieval | README | IMPLEMENTED_NOT_VERIFIED | `commands/materials/review_material.rs:50-264`; `db_service/flashcard_decks.rs`; SQLite. | Cards and decks UI; not manually exercised. |
| C02 | Card-to-deck and tag relationships | Legacy | IMPLEMENTED_NOT_VERIFIED | SQLite foreign keys and tag operations; commands. | UI controls exist. No restart check. |
| C03 | Persisted review schedule and per-review history | Session 1–5 area; exact boundary absent | IMPLEMENTED_NOT_VERIFIED | `flashcard_decks.rs:344-456` transactional schedule update + raw review log; `queries.rs:863-890`; review event ID UNIQUE. | FlashcardReview UI calls command; DB tests exist. Not run through GUI. |
| C04 | Active scheduler / FSRS requirement | Session 9 plan only | PARTIAL | Active scheduler is custom Anki/SM-2-like `Flashcard::calculate_next_review`, `flashcard.rs:105-304`; hard-coded steps [1,10], no FSRS crate/config/review-state migration. | User sees Again/Hard/Good/Easy; functional old algorithm, FSRS not delivered. |
| C05 | Anki TSV/CSV export | README | IMPLEMENTED_NOT_VERIFIED | `flashcard.rs:310-380`, UI command `review_material.rs:494-571`; formatting unit cases exist. No external Anki import round trip. | Export menu. |
| C06 | APKG export / target compatibility | README | IMPLEMENTED_NOT_VERIFIED | `materials/anki_apkg.rs:1-190` creates ZIP and Anki SQLite structures; export test checks ZIP/readback. No Anki client or independent parser import validation. | Export menu. |
| C07 | Anki APKG import | Historical | NOT_IMPLEMENTED | No importer or `.apkg` input route found. | None. |
| C08 | CSV/TSV import | Historical | NOT_IMPLEMENTED | Export exists only; no import parser/command/UI. | None. |
| D01 | Assessment create/generate and expected answer persistence | Session 6 (reconstructed) | IMPLEMENTED_NOT_VERIFIED | UI `AssessmentCreator.svelte`; command `assessment.rs:87-148`; DB transaction and `expected_content`; actual LLM not executed. `language` and `source_material_ids` are declared on create request but not consumed/persisted. | Assessment workflow; current grading later uses English. |
| D02 | Assessment session, immutable question snapshots, answers, recovery | Session 6 | IMPLEMENTED_NOT_VERIFIED | Commands `assessment.rs:195-257,367-460`; DB snapshot operations; resume logic. `assessment_integrity.rs` tests persistence and repeat-open invariants, but uses the same `DbService` instance; it does not close/reopen an assessment DB or test app restart. | AssessmentReview UI; restart recovery unverified. |
| D03 | Strict/partial grading | Session 6 | IMPLEMENTED_NOT_VERIFIED | `manager.rs:2153-2248` calls evaluator and rejects unknown grades / invalid JSON; persisted evaluation. Requires provider; test suite tests parse boundaries, not real evaluation accuracy. | Assessment answer submit. |
| D04 | Assessment reports/remediation/reassessment | Session 6 | PARTIAL | `assessment.rs:258-658` handles reports and creates remediation/reassessment workflows, but source scope is not retained per assessment: helper explicitly loads all currently indexed materials (`assessment.rs:343-358`). New generation needs LLM and can drift from original studied source. DB lifecycle integration tests cover history, not provider workflow. | AssessmentReview UI; source provenance and live generation remain gaps. |
| D05 | Assessment timing/exam mode/comparison | Session 14 historical plan | PARTIAL | Assessment attempt timestamps/history and results exist; no exam timer/secure timed mode, difficulty selection workflow or historical trend comparison. | Some attempt review UI. |
| E01 | Deterministic mastery calculation / observed vs required dimensions / breadth gate | Session 8 claim | IMPLEMENTED_NOT_VERIFIED | `materials/concept_mastery.rs:227-542`; pure deterministic function, thresholds, coverage, `gate_state_by_breadth`; recall-only test exists. | MasteryPanel and commands. No production data E2E. |
| E02 | Raw evidence preservation, rebuild, ordering, idempotency | Session 8 claim | IMPLEMENTED_NOT_VERIFIED | `db_service/concept_mastery.rs:120-215,434-595`; assessment answers + review rows gathered and sorted by timestamp/numeric ID; review writes transactional/event-key dedupe; embedded tests include duplicate retry and ordering. | Assessment and flashcard hooks. Derived summary rebuilt on query, not persisted. |
| E03 | Misconception lifecycle/reassessment | Session 8 claim | IMPLEMENTED_NOT_VERIFIED | Stored assessment misconception feeds mastery, lifecycle is derived from chronological evidence (`concept_mastery.rs:448+`); tests exist. | Assessment UI shows reports/mastery. No real LLM misconception validation. |
| E04 | Flashcard/assessment integration, provenance, soft deletion | Session 8 claim | IMPLEMENTED_NOT_VERIFIED | Review hook writes evidence + concept/material occurrence; assessment answers are joined to snapshots; deleted materials excluded by queries; embedded DB tests cover these claims. | Mastery panel accessible from assessment; not globally integrated into adaptive UI. |
| E05 | Concept normalization/semantic deduplication/user scoping | Session 8 | PARTIAL | `concept_id_for_label` lowercases and collapses whitespace only; exact normalization test. Similar-but-not-equal labels remain different. No learner ID in schema, so one local global learner. | Single-user app. |
| E06 | Mastery API/UI projection | Session 8 corrective-pass claim | PARTIAL | Tauri commands registered and `MasteryPanel.svelte` mounted in assessment review. TS `ConceptMastery`/normalizer omit backend `observed_dimensions`, `required_dimensions`, and `coverage`, so frontend cannot expose all backend evidence breadth. UI scope is material/assessment, no app-wide learner view. | User-accessible limited panel. |

## Learning intelligence and expansion

| ID | Feature | Historical requirement | Status | Evidence / findings | User access / verification |
|---|---|---|---|---|---|
| F01 | Persistent learner profile/preferences/history | Sessions 10 plan (per prompt) | NOT_IMPLEMENTED | No learner/profile domain, identity, or profile tables found. General app settings are device-level. | Settings are preferences, not learner model. |
| F02 | Computed strong/weak topic profile | Sessions 10–11 | PARTIAL | `get_weak_concepts` provides derived mastery list, but no persistent profile/aggregation consumer. | Weak list API only; no adaptive profile screen. |
| F03 | Adaptive personalized study plan | Session 11 | NOT_IMPLEMENTED | No plan entity, priority calculation, daily load, persistence, explanation, or study-queue connection found. | None. |
| F04 | FSRS scheduler and migration | Session 9 | NOT_IMPLEMENTED | No `fsrs` dependency or state; current algorithm custom SM-2-like. | No FSRS. |
| F05 | AI tutor integrated with sources, profile, misconception/history | Session 12 | PARTIAL | General LLM question-answer/explanation with source retrieval exists (`StudyMaterialsBar`, manager); no persisted conversation, tutoring orchestration, hints/follow-ups, profile/misconception integration. | Generic source Q&A only. |
| F06 | Persistent prerequisite/concept graph and map visualization | Session 13 | NOT_IMPLEMENTED | No graph schema, relation service, map UI, or ingestion-to-graph pipeline found. | None. |
| F07 | Separate exam mode | Session 14 | PARTIAL | Assessment is genuine free-text quiz/feedback workflow; no timed exam lifecycle. | Assessment screen does not equal exam mode. |
| F08 | Audio ingestion / ASR / timestamped video / keyframes / mixed media | Historical expansion | PARTIAL | TTS synthesis service and pronunciation command exist, but no audio/video ingestion, ASR/Whisper, keyframe extraction, transcript alignment, or mixed media pipeline. | Limited speech synthesis for language cards. |
| F09 | Language vocabulary mode and metadata | Current added scope | IMPLEMENTED_NOT_VERIFIED | `materials/language_learning.rs`, language metadata schema/commands and generation prompt; `tests/language_learning.rs` covers mode, directions and diacritics with real SQLite, but the test named `language_deck_mode_survives_save_and_reopen` reads through the same `DbService` and does not reopen the DB. No external model or app UI execution. | Language toggle/deck creation; restart not verified. |
| F10 | Grammar/listening/pronunciation feedback/adaptive language mastery | Historical language expansion | PARTIAL | Translation/card tasks and TTS are present; no speech input/recognition feedback, grammar lesson pipeline, listening ingestion, or adaptive language plan. Mastery dimensions recognition/production exist. | Language-specific flashcards only. |
| F11 | Flutter Android/iOS client, sync/offline | Historical business concept | NOT_IMPLEMENTED | No Dart/Flutter project, auth/API service, sync engine, or mobile review application. Tauri icons/platform metadata are not a client. | None. |
| F12 | Auth, server decks, purchase entitlement/payment verification | Historical business concept | NOT_IMPLEMENTED | No server, identity/auth, purchase/IAP or entitlement backend. Current architecture stores local files/data. | None. |

## Reliability / evidence status

| ID | Feature | Status | Evidence / finding |
|---|---|---|---|
| R01 | SQLite durability and local user data | IMPLEMENTED_NOT_VERIFIED | SQLite schema and local data paths; tests use isolated temp DB. App restart E2E not exercised. |
| R02 | Schema/migration safety | PARTIAL | Ad hoc idempotent schema setup and targeted language/assessment legacy migrations; no numbered migration ledger or complete upgrade matrix. Assessment migration is covered by a legacy fixture. |
| R03 | Transactions/FK integrity | IMPLEMENTED_NOT_VERIFIED | Deck/review/assessment operations use transactions; tests check assessment cascade/snapshot preservation. No broad concurrent/failure injection. |
| R04 | Secrets | IMPLEMENTED_NOT_VERIFIED | `config/keystore.rs` uses OS keyring; configuration read/update exists. No OS keyring/provider flow exercised here. |
| R05 | Authentication/data isolation | NOT_IMPLEMENTED | Local single-user storage; no accounts or per-user keys. |
| R06 | File path/upload safety | PARTIAL | User-selected file UI narrows normal path but command trusts `file_name` then joins into materials path (`study_material.rs:32-62`); no canonical containment enforcement. Filename collision overwrite risk. Security test absent. |
| R07 | Retry/recovery/error reporting | PARTIAL | Many Rust paths return errors; ingestion swallows conversion/indexing errors in async processing and can leave incomplete records. No background job ledger/retry. |
| R08 | Frontend quality/build | IMPLEMENTED_NOT_VERIFIED | `npm run build` passed; warnings captured. No browser/Tauri UI test configuration discovered. |

## Status totals

Totals below count the 50 feature IDs above exactly once. They deliberately do not count “not verified” as complete.

| Status | Count |
|---|---:|
| VERIFIED_COMPLETE | 0 |
| IMPLEMENTED_NOT_VERIFIED | 30 |
| PARTIAL | 21 |
| STUB_OR_MOCK | 0 |
| NOT_IMPLEMENTED | 10 |
| BROKEN | 0 |
| UNKNOWN | 0 |
| **Total** | **61** |

The matrix contains 61 separately identified audit features. None is marked `VERIFIED_COMPLETE`: the DB-level Rust tests passed, but user-facing complete workflows requiring GUI/provider/sidecar/model execution were not exercised. No feature was forced into UNKNOWN; narrower gaps are expressed as PARTIAL or IMPLEMENTED_NOT_VERIFIED with stated limits.
