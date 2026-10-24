# Audit Evidence and Reproduction

**Snapshot:** 2026-10-24, branch `master`, HEAD `3e3e57d`; `git status --short` was empty before audit work. Repo-wide source scan and Git history inspection were read-only. Audit documents were then added under `docs/audit/`; no production source files were edited.

## Environment and constraints

- Windows x86_64 MSVC shell, Node dependencies already present (`node_modules`), Rust/Cargo available.
- No Tauri GUI interaction, local Ollama/provider credentials, PDF sidecar executables, OCR engine/language packs, embedding model readiness, real user DB, mobile device, or Anki installation were verified.
- No production data was used. Tests are designed around isolated temp DBs; the audit did not call provider services or modify the normal app database.
- No API credentials were read or emitted. No dependency installation command was run. `cargo test` built locked crates as part of the requested verification.

## Commands and results

| Command | Result | What it establishes / limits |
|---|---|---|
| `git status --short` (before audit docs) | No output; clean tracked tree | Current source snapshot not hiding uncommitted application changes. Audit docs are new untracked files afterwards. |
| `git log --oneline --decorate -30`; full reverse history | 10 commits on local master, HEAD `3e3e57d`; no session reports/ADR/roadmap file in tracked docs | Supporting chronology only, not proof of requirements. Latest commit adds assessment/mastery and other code. |
| `rg --files` (repo inventory, excluding build/dependency directories) | Tauri/Svelte/Rust/Python files present; no `.dart` project, server workspace, SQL migration directory, CLI/API server | Establishes architecture absence within tracked source. Tauri Android/iOS icon directories alone do not prove a client. |
| `rg` scans for TODO/FIXME/XXX/HACK/todo!/unimplemented!/panic!/mock/dummy/placeholder/simulated | One app TODO on icon; scheduler TODO; reachable startup panic; test assertion panics; HTML placeholders; no production fake LLM provider found by scan | Search was contextualized manually; no placeholder string alone treated as defect. |
| `npm run build` | **PASS**, Vite 6.0.6; 3558 modules transformed; `dist` generated. Warnings: numerous accessibility warnings, dynamic import/static import warning, large JS chunk (670.91 kB). | Frontend compiler only; not app runtime/IPC behavior. The generated `dist` is ignored/build output, not source change. |
| `cargo test --locked --all-targets` from `src-tauri` | **PASS**, exit code 0. 107 library tests + 10 assessment integration tests + 8 language-learning integration tests = **125 passed, 0 failed**. Rust compiler emitted unused/dead-code/deprecation/private-interface warnings. | Genuine Rust unit/DB integration tests, mostly temp SQLite databases. Does not exercise Tauri GUI, provider, PDF sidecar, embedding model or real app-data restart. |
| `cargo fmt --all -- --check` from `src-tauri` | **PASS**, exit code 0, no output. | Formatting validation only. |
| `cargo clippy --locked --all-targets` from `src-tauri` | **FAIL**, exit code 1. Clippy reported 147 warnings and two `never_loop` errors at `services/db_service/flashcard_decks.rs:497` and `services/db_service/mnemonics.rs:182`; final message: could not compile lib due to 2 errors. Re-running with `--message-format=short` confirmed the same. | Code compiles under `cargo test`; Clippy's stricter lint gate rejects loops that return on their first row. Both queries target ID-keyed rows, so the observed issue is a lint/maintainability issue, not demonstrated wrong output. |
| `rg -n "fsrs|SM.?2|..." src src-tauri/src` | Scheduler fields and `calculate_next_review` present; uses hardcoded learning steps and Anki/SM-2-like math; no FSRS package/code | Source inspection, not scheduler fixture execution. |
| `rg --files` for Flutter/mobile/migration/CI paths | No Dart project, no app-level migration files or migration ledger; Tauri icons contain mobile assets | Repository absence check; does not assess unpublished external repos. |

## Test inventory inspected

These are test definitions and their intended coverage; test results are only valid after the Cargo invocation completes successfully.

- `src-tauri/src/materials/concept_mastery.rs`: unit cases include `recall_only_is_not_overall_mastered`, `full_breadth_reaches_mastered`, `language_breadth_reaches_mastered`, `misconception_tracked_and_resolved`, `deterministic_same_inputs_same_output`, and `numeric_evidence_id_orders_same_timestamp_deterministically`.
- `src-tauri/src/services/db_service/concept_mastery.rs`: local temp DB tests include assessment/review raw evidence, rebuild, idempotent flashcard retry, shared concepts/provenance, same-time event order, soft deletes and assessment answer timestamp preservation.
- `src-tauri/tests/assessment_integrity.rs`: attempt snapshots, reassessment history, cascades, soft deletion, reopen/resume, report reconstruction and legacy migration.
- `src-tauri/tests/language_learning.rs`: language metadata, diacritics, legacy schema and basic Anki text export. Despite the `language_deck_mode_survives_save_and_reopen` name/comment, that test writes and reads through the same `DbService`; it does not close and reopen the database.
- `src-tauri/tests/assessment_integrity.rs`: despite “reopen” test names, those cases call the assessment-open resolver repeatedly on the same `DbService`. The legacy migration case closes a raw fixture connection before opening through `DbService`, but that is migration coverage, not restart recovery for a running assessment.
- `src-tauri/src/materials/flashcard.rs`: CSV/TSV formatting cases and APKG ZIP/database readback. No independent Anki import round trip.
- The tests exercise domain/storage methods, not a running Tauri command bridge, live LLM, real PDF sidecar, embedding provider, or frontend workflow.

## Source-level execution traces

1. **Study material path:** Svelte `StudyMaterialsBar`/`MaterialsBoard` invokes Tauri commands. `src-tauri/src/main.rs:21-176` registers them. `commands/materials/study_material.rs:32-105` accepts bytes/path, writes original, persists metadata, converts PDF or reads text; `:442-535` creates markdown/cover and indexes. The vector index uses FastEmbed and a separate sqlite-vec DB (`services/vector_service/vector_service.rs:47-148,157-374`). No genuine document was fed through it in this audit.
2. **Flashcard review path:** `FlashcardReview.svelte` reveals/rates card, calls review command (`commands/materials/review_material.rs:119-154`), which reaches `DbService::update_flashcard_after_review`; transaction checks `review_event_id`, computes scheduler state, writes card state and appends raw log (`services/db_service/flashcard_decks.rs:344-456`). The raw event supports mastery rebuild. Actual UI action and restart were not performed.
3. **Assessment path:** `AssessmentCreator.svelte` invokes generate/create; command calls `LlmService`, checks questions/reference text and stores assessment; `AssessmentReview.svelte` starts/resumes a persisted snapshot, submits text to provider evaluator, stores grade, loads report, completes session. Relevant code `commands/materials/assessment.rs:87-148,195-257,367-460`; UI `src/lib/components/ReviewMaterial/AssessmentReview.svelte:54-190`. DB attempts/integrity are exercised by tests; no live model run.
4. **Mastery path:** assessment answer rows and flashcard review rows become `MasteryObservation`s (`services/db_service/concept_mastery.rs:120-215`), ordered by timestamp and numeric ID; pure calculation at `materials/concept_mastery.rs:227-542`; commands registered in `main.rs:107-110`; assessment review mounts `MasteryPanel.svelte` (around line 484). No real user evidence/restart was exercised; UI projection omits backend breadth fields.
5. **Anki export path:** deck export command in `review_material.rs:494-571` routes TSV/CSV text and APKG writer `materials/anki_apkg.rs:71-104`. Local tests inspect archive/database contents; no target importer was run.

## Current command completion update

`cargo test --locked --all-targets` completed successfully. The 107 library tests include concept mastery recalculation/idempotency/same-timestamp ordering, APKG structure readback and parsing helpers; the two integration binaries add 18 DB lifecycle/language persistence tests. This makes the Session 8 corrective-pass claims on the deterministic aggregation and evidence-preservation mechanics well supported by executable domain/storage tests. It does **not** validate a complete Tauri learning workflow or live LLM judgment.

Clippy is not clean: 147 warnings are emitted, including unused/dead code, deprecated APIs and style findings. Two `never_loop` lints are errors because `get_flashcard` and `get_card_mnemonic` loop over a query result but immediately return from the first iteration. Their predicates select primary-key IDs, limiting this to a non-idiomatic query pattern under the inspected schema; no regression was asserted or fixed as part of this audit.

## Reproduction notes

From repository root:

```powershell
npm run build
```

From `src-tauri`:

```powershell
cargo test --locked --all-targets
```

For a true app workflow, first build the platform-specific executables required by `src-tauri/tauri.conf.json` (`dist/conversor`, `dist/cover-extractor`) as documented in `README.md`; provide an isolated app-data path and configured real provider/local model, then launch Tauri. Import a known small PDF and Markdown fixture, inspect resulting Markdown and vector chunks, create a generated deck/assessment, complete reviews/answers, close/reopen, and compare persisted schedule, raw review event IDs, assessment snapshots/answers and mastery result. Record provider identity/version and do not replace it with a mock while reporting E2E status.
