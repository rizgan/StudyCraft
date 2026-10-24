# Implementation Audit — StudyCraft

**Audit date:** 2026-10-24  
**Scope:** current working tree at `3e3e57d` (`master`), Windows x86_64 / MSVC environment  
**Method:** source tracing + requested build/test attempts. No source code was changed.

## Executive assessment

StudyCraft is an existing local-first Tauri desktop study-material application with functioning-looking (and mostly genuinely implemented) file/link ingestion, local SQLite persistence, local semantic indexing, configurable LLM generation, editable flashcard/test content, SM-2-like scheduling, study statistics, Anki export, and newly added assessment/mastery workflows. The Svelte production bundle and Rust test suite built successfully in this audit. No interactive Tauri run or real-provider request was performed.

**Verdict: a partially integrated desktop learning application, with several real learning workflows and a newer set of assessment/mastery subsystems; not the historical all-in-one AI learning platform.** The current code is not a collection of mere mocks: production routes connect UI to Rust commands and SQLite. But online model-backed paths, local embedding downloads, real document sidecars, provider credentials, GUI interactions and restart behavior were not all exercised end to end, so the broad “working” claim must remain qualified. FSRS is not implemented: current card scheduling is explicitly SM-2/Anki-style. No Flutter app, adaptive plan, learner profile, tutor, concept graph, or exam mode was found.

### Verified from this audit

- `npm run build` completed with Vite 6 and emitted `dist` assets. It reported numerous Svelte accessibility warnings and a >500 kB JS chunk. This verifies frontend compilation only.
- `cargo test --locked --all-targets` passed: 107 library tests, 10 assessment integrity tests, and 8 language-learning integration tests (125 total). `cargo fmt --all -- --check` passed. `cargo clippy --locked --all-targets` failed on two `never_loop` lints and emitted 147 warnings; see the evidence log.
- The production Tauri entrypoint registers UI-called commands (`src-tauri/src/main.rs:21-176`); the flow is not disconnected scaffolding.
- Rust code has transactional SQLite writes for deck/review operations and assessment attempt snapshots (`src-tauri/src/services/db_service/flashcard_decks.rs:141-212,344-456`; `src-tauri/src/services/db_service/assessments.rs`; `src-tauri/tests/assessment_integrity.rs`).
- Real flashcard review events are recorded beside an SM-2 state update; assessment answers are persisted with structured LLM evaluation; mastery reads those stored events (`src-tauri/src/services/db_service/flashcard_decks.rs:344-456`, `src-tauri/src/commands/materials/assessment.rs:367-415`, `src-tauri/src/services/db_service/concept_mastery.rs:120-215`).
- These are source-level traces and database-focused tests, not proof of successful real desktop/provider workflows. The matrix has 61 rows: 0 `VERIFIED_COMPLETE`, 30 `IMPLEMENTED_NOT_VERIFIED`, 21 `PARTIAL`, 10 `NOT_IMPLEMENTED`, and 0 each of `STUB_OR_MOCK`, `BROKEN`, `UNKNOWN`. See status matrix and evidence log.

### Architecture and maturity

The repo is a single Tauri 2 desktop application: Svelte 5/Vite frontend, Rust command layer, local SQLite domain database, separate SQLite/sqlite-vec + FTS index, local FastEmbed AllMiniLML6V2 model, and configured remote/local LLM providers via `flyllm`. PDFs are converted through separately bundled Python sidecars; website and Git repository conversion are Rust services. There is no independent HTTP API server, multi-user/auth layer, job queue, app-level migration framework, or Flutter source tree. Icons for Android/iOS in Tauri assets do not constitute client apps.

The app supports multiple review-material kinds: flashcard decks, classic multiple-choice tests, and free-text assessments. The latest local commit adds assessment and mastery logic/UI (`3e3e57d`, 59 files changed), not a general rewrite or proof of completion. Git has no session-by-session reports or original 49-item roadmap in the current tree. README is a product summary, not implementation evidence.

### Highest-impact gaps / risks

1. **Assessment cannot yet be called verified end to end.** There are real UI→Tauri→provider→SQLite paths and strong DB history tests, but provider-backed question creation/grading and GUI restart recovery were not run. Grading is fixed to English at the command boundary (`assessment.rs:395`), despite generation language inputs.
2. **Assessment source provenance is incomplete.** The creator sends `source_material_ids`, but Rust does not consume or persist that field; reassessment/remediation chooses all indexed study materials (`assessment.rs:25-38,343-358`).
3. **Mastery is computed on demand and not a persistent learner model.** Raw evidence is durable; summary is rebuilt deterministically. Identity is normalized label only (case + whitespace); no fuzzy semantic deduplication and no user isolation exist. Current UI shows a per-assessment mastery panel; profile/adaptive plan consumers do not exist.
4. **Scheduling is an older custom SM-2/Anki-style algorithm**, with hardcoded learning steps and some inconsistent caps (`flashcard.rs:105-304`). No FSRS dependency/config/state/migration exists.
5. **Ingestion success reporting is optimistic.** File conversion errors are logged and swallowed after material/file insertion (`commands/materials/study_material.rs:32-105`); the command can report upload success despite unusable/unindexed content. Name-derived paths and collision handling need a security/data-integrity review.
6. **Bundled desktop builds depend on Python binaries** referenced as `externalBin` (`tauri.conf.json`) but absent from tracked files; README requires manual PyInstaller packaging. PDF workflow was not executed.
7. **Single-user local data, no authentication/sync/mobile/commercial services.** This may be acceptable for the desktop product but supersedes the historical broad business concept.

## Readiness by workflow

| Workflow | Audit conclusion |
|---|---|
| Create/edit/list local study materials | Implemented; Svelte commands and SQLite/filesystem paths exist. Real app interaction not run. |
| Import PDF and convert to Markdown | Implemented in code; depends on separately built sidecar; unverified in this environment. OCR optional and may fail without OCR engine. |
| Add website/Git repo | Implemented in Rust; actual remote fetch/clone was not run. |
| Local semantic search | Implemented with local embedding and sqlite-vec; model readiness is a hard dependency for meaningful search; unverified. |
| Generate flashcards/tests with LLM | Provider-backed code and UI exist; credentials/models required; unverified with genuine provider. |
| Create/review flashcard deck | Persistent code path exists, custom SM-2 update and review log are transactional; full Tauri UI/restart not run. |
| Export TSV/CSV/APKG | Export code/UI and APKG archive test exist; importer/Anki compatibility round trip was not performed. No import path found. |
| Take classic multiple-choice test | Existing path appears functional and persistent session stats exist; not end-to-end executed. |
| Take AI free-text assessment | UI and backend lifecycle exist; needs real LLM, DB-focused persistence tests; end-to-end unverified. |
| Inspect concept mastery | Real derived from persisted assessment/review evidence; visible panel added; no real-app workflow run. |
| AI tutor, adaptive plan, profile, graph, exams, FSRS | Not implemented in current source. |

## Status policy

Feature statuses in [FEATURE_MATRIX.md](FEATURE_MATRIX.md) use exactly the requested vocabulary. `VERIFIED_COMPLETE` is reserved for functionality with real execution evidence. Since this audit could not run interactive UI, external providers, Python sidecars, or embedding-model workflows, those capabilities are not marked verified-complete even where their code appears substantive. The full counts are in the feature matrix.
