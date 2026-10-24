# Roadmap Gap Analysis

## Provenance and limits

The tracked repo contains README, Cargo/npm/Tauri configs, code comments and Git history, but no archived planning document, session transcript, ADR, 49-feature list, or session completion reports. Therefore the “49 features” and Sessions 1–14 chronology supplied in the audit request are historical leads, not repository-proven requirements. This audit does not invent the missing list. `README.md` is the only current product scope document; it describes study-material import, flashcards/tests, LLM generation/Q&A, spaced repetition, local vector indexing and statistics. Git history shows the public v2 release (2025-10-05), Anki export work (2025-10-12), provider settings (2025-10-15), then 2026 additions; latest `3e3e57d` dated 2026-10-02 says “feat: add mastery panel component and assessment logic.”

No commit or document is labeled Session 1–14. The mapping below uses supplied milestone descriptions only as a provisional alignment and marks exact scope/boundaries UNKNOWN where not independently recorded. Current tree evidence supersedes previous completion claims.

## Architecture decision reconciliation

| Topic | Proposed / historical | Confirmed in current repository | Status |
|---|---|---|---|
| Desktop stack | Rust + Tauri + Svelte | Tauri 2, Rust backend, Svelte 5/Vite. `Cargo.toml`, `package.json`, `tauri.conf.json`. | Current implementation |
| Local storage | Local DB / local learning | App data paths + SQLite domain DB + separate SQLite vector DB; source files local. | Current architecture |
| AI providers | Hosted/local LLM options | Provider configuration and `flyllm` integration exist; actual provider execution remains unverified. | Implemented but not tested with real credentials |
| Embeddings | Local vector search | FastEmbed + ONNX Runtime + sqlite-vec. | Code present; model run unverified |
| FSRS | Planned migration from SM-2 | Current code still uses custom SM-2/Anki-style logic; no FSRS package, migration or state found. | Planned, not present |
| Mastery | Concept evidence model | Rust engine derives state from raw assessment answers and flashcard review rows; UI panel only in assessment workflow. | Implemented subsystem, no real app E2E |
| Flutter/mobile/API/auth/payments | Historical commercial direction | No Dart project/server/API/auth/sync/payments found; platform icon assets do not prove app. | Proposal/deferred or superseded; not desktop MVP evidence |
| Original 49 features | Reportedly existed | No list found in tree/history available locally. | UNKNOWN; cannot faithfully reconcile item by item |

## Session-by-session evidence ledger

Status uses the requested vocabulary. “Historical report” means the goal is supplied by the audit prompt; the repo has no corresponding report. Code/commit names prove presence, not acceptance.

| Session | Supplied / reconstructed goal | Current evidence | Status |
|---:|---|---|---|
| 1 | Foundation; exact scope absent | Legacy Tauri app, local CRUD and SQLite paths predate latest feature commit; exact boundary/report absent. | UNKNOWN |
| 2 | Foundation; exact scope absent | Ingestion/vector/LLM code exists; no session evidence to map requirements. | UNKNOWN |
| 3 | Foundation; exact scope absent | Flashcard/test UI and persistence exist; no historical acceptance list. | UNKNOWN |
| 4 | Learning infrastructure; exact scope absent | Review history/statistics/vector indexing exist; session goal unrecorded. | UNKNOWN |
| 5 | Learning infrastructure; exact scope absent | Existing SM-2-like scheduler and review analytics; exact boundaries absent. | UNKNOWN |
| 6 | Assessment Engine | Latest commit includes assessment domain/commands/DB/UI and `assessment_integrity.rs`; external generation/grading untested. | IMPLEMENTED_NOT_VERIFIED |
| 7 | Inspect repository history/scope | Git can show code history, but no audit/decision record documenting Session 7 completion. | UNKNOWN |
| 8 | Concept Mastery Engine + corrective claims | Rust deterministic engine, DB evidence integration, command/UI bindings and many tests found. Test result is in evidence file; real app workflow remains unverified. | IMPLEMENTED_NOT_VERIFIED |
| 9 | Migrate SM-2 to FSRS without losing history | Active scheduler remains custom SM-2-like; raw review log schema exists, but no FSRS migration/package/state. | NOT_IMPLEMENTED |
| 10 | Learner Profile | No learner identity/profile model or computed-profile persistence found. | NOT_IMPLEMENTED |
| 11 | Adaptive Study Plan | No prioritization/daily-plan/persistent plan or UI found. | NOT_IMPLEMENTED |
| 12 | AI Tutor | Generic source-grounded LLM Q&A exists; no persistent tutor integrated with profile/mastery/misconceptions/history. | PARTIAL |
| 13 | Knowledge Map | No concept graph, prerequisite persistence, graph construction or visualization found. | NOT_IMPLEMENTED |
| 14 | Exam Mode | Assessments exist; timed exam lifecycle and historical comparison do not. | PARTIAL |

## Reconstructed scope from repository sources

Provenance: **R** README, **C** code/entrypoint, **P** supplied project context only, **U** no historical artifact.

| Area | Claim | Provenance and actual state |
|---|---|---|
| PDF/Markdown/TXT materials | Existing desktop capability | R+C; PDF conversion code and text read path. Bundled converter requires manual build. |
| Website/Git sources | Existing capability | R+C; HTTP/HTML parser and Git clone converter. Network E2E unverified. |
| Automatic chunk/vector index | Existing capability | R+C; persistent sqlite-vec/FTS and local embedding code. Model execution unverified. |
| Flashcard decks and tests | Existing capability | R+C; create/edit/store and UI paths. |
| LLM-generated cards/tests and explanations | Existing capability | R+C; provider-backed implementation, not exercised with live provider. |
| SM-2-style review and statistics | Existing capability | R+C; persistent schedule/review paths; algorithm quality not checked against an authoritative suite. |
| Anki export | Added in 2025 and README | R+C; TSV/CSV/APKG export only; no import/roundtrip validation. |
| Image understanding/mnemonics | Added in 2026 commits and README | C; provider-backed commands/UI; credentials/provider not exercised. |
| Free-text assessment | Latest commit | C; lifecycle and DB tests, live model grading unverified. |
| Concept mastery | Latest commit and supplied Session 8 | C+P; integrates raw events; frontend drops breadth fields and panel scope is assessment review. |
| FSRS/profile/adaptive plan/graph/tutor | Future plans | P only; FSRS absent, generic Q&A exists, other systems absent. No approval/ADR found. |
| Audio/video/language/mobile/commerce | Historical concept | P; language flashcards/TTS partly present; ingestion ASR/video/mobile/commercial absent. |

## Completion-report discrepancies

- Any claim that Session 9 FSRS migration completed is contradicted by `flashcard.rs:105-304` and absence of an FSRS dependency; this audit found no migration.
- Session 8 claims about deterministic aggregation, same-time ordering, misconception lifecycle, breadth gating, event idempotency and provenance are substantially supported by source and focused tests (`materials/concept_mastery.rs`; `services/db_service/concept_mastery.rs`). Claims of successful tests are only supported if the final run reports success; see [AUDIT_EVIDENCE.md](AUDIT_EVIDENCE.md). Real app/provider workflow remains unverified. Frontend currently drops `coverage` and observed/required dimensions.
- Assessment provenance is weaker than the request structure suggests: the creator sends selected source IDs, but Rust does not persist or consume them; reassessment and remediation select all indexed sources. This is visible in the dead-field compiler warning and `resolve_source_material_ids` implementation (`commands/materials/assessment.rs:25-38,343-358`).
- The current commit is not the original 49-feature roadmap. No evidence supports the historical list or claims that Sessions 1–14 shipped as sessions.
- README language about auto-indexing and broad source support is not a guarantee: vector model must be available, PDF sidecars must be packaged, and conversion failures can be swallowed while import returns success.
