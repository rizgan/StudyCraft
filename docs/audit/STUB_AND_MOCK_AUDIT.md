# Stub and Mock Audit

**Scope:** reachable production paths and apparently unused services in the current tree. A `TODO` is not automatically a stub; findings below explain effect and reachability. Search evidence: `rg -n "TODO|FIXME|XXX|HACK|todo!|unimplemented!|panic!|mock|dummy|placeholder|simulat" src src-tauri`.

## Findings

| ID / severity | Finding | Reachability and concrete effect | Evidence |
|---|---|---|---|
| S1 / High | Ingestion can report success after conversion failure | `add_study_material` writes original file and DB record, then catches a conversion error, prints it, and still returns `File uploaded, processing started`. User may get an apparently imported material with no usable Markdown/index and no retry job. | `src-tauri/src/commands/materials/study_material.rs:32-105`; later indexing errors are logged in `:442-535`. |
| S2 / High | File path is formed from caller-supplied `file_name` without containment check | Reachable Tauri command joins `materials_dir` and a renderer-provided filename. Normal file picker likely passes basename, but command boundary itself does not canonicalize/reject separators or traversal; malicious renderer input could escape target directory. Same stem files overwrite prior source/Markdown/cover. | `study_material.rs:32-62`; no test found. |
| S3 / Medium | Empty semantic query is represented as all-zero embedding | `create_embedding` returns a 384-dimensional zero vector for empty text. This is not a semantic result; search callers should guard/return empty. Reachability is limited to empty input. | `src-tauri/src/services/vector_service/embedding_provider.rs:59-69`. |
| S4 / Medium | App startup defaults obscure config-load errors | On `AppState::new`, config failure prints fatal-looking diagnostics but falls back to default; provider configuration may silently be absent. Vector service failure panics and prevents app startup. | `src-tauri/src/state.rs:21-64`. |
| S5 / Medium | Runtime migration strategy is implicit setup-time schema alteration | `DbService::setup_tables` plus ad hoc migration methods do real work, but there is no numbered migration ledger, rollback plan or upgrade coverage across all historical versions. Future schema drift/data-loss risk; not itself a fake implementation. | `src-tauri/src/services/db_service/connection.rs:57-438`. |
| S6 / Medium | Assessment difficulty reader silently coerces invalid stored values to recall | Generation/parser rejects unknown values in some flows, but DB deserialization falls back to Recall, potentially changing mastery dimension without surfacing corruption. | `src-tauri/src/services/db_service/assessments.rs:17-63`; mastery evidence parser also logs and falls back in `concept_mastery.rs:557+`. |
| S7 / Medium | Frontend mastery projection discards breadth fields | Rust returns `observed_dimensions`, `required_dimensions`, `coverage`; TS `ConceptMastery` and `normalizeConceptMastery` omit them. The UI therefore cannot explain why a concept is not mastered or display the claimed breadth state. | `src-tauri/src/materials/concept_mastery.rs:171-194`; `src/lib/logic/ReviewMaterial/conceptMastery.ts:41-53,184-193`; panel. |
| S8 / Low | Scheduler configuration fields do not govern all scheduling constants | Settings persist review parameters, while `calculate_next_review` hardcodes learning steps and intervals/multipliers in multiple branches. UI-visible settings can imply broader configurability than the actual scheduler uses. | `src-tauri/src/materials/flashcard.rs:105-304`; `config/app_settings.rs:11-40,219-250`; TODO at flashcard.rs:112. |
| S9 / Low | Build contains dead/unused code and warnings | Rust build reports 42 warnings (unused imports/variables, private interface, dead code). This is not proof of a broken user feature; it is evidence of cleanup/abandoned paths. | `cargo test` compile output recorded in evidence. |
| S11 / Low (quality gate) | Clippy currently fails on two loops that always return on the first row | `get_flashcard` and `get_card_mnemonic` iterate a query result then return from the first item; their queries use card IDs, so the inspected schema's primary key means at most one row. `cargo test` compiles and passes, but `cargo clippy --all-targets` exits 1. No runtime failure was demonstrated. | `src-tauri/src/services/db_service/flashcard_decks.rs:497-502`; `src-tauri/src/services/db_service/mnemonics.rs:182-187`; lint logs summarized in [AUDIT_EVIDENCE.md](AUDIT_EVIDENCE.md). |
| S10 / Low | No production fake LLM provider found | LLM manager uses configured providers and propagates failed responses; image-generation module explicitly says no mock fallback. No `todo!`/`unimplemented!` production placeholder found in the inspected search. This is positive evidence, but providers were not called. | `src-tauri/src/services/llm_service/manager.rs:2681+`; `services/image_generation/mod.rs:1-93`; `commands/materials/mnemonic.rs` docs. |

| S12 / Medium | Assessment source IDs and selected language are accepted by the UI payload but discarded; reassessment selects every indexed source | Creator sends `sourceMaterialIds` and `language`; Rust request declares both fields but does not read them (compiler dead-field warning). `resolve_source_material_ids` ignores assessment ID and returns all indexed study materials, so reassessment/remediation may use unrelated documents; answer grading passes literal English. | `src/lib/logic/ReviewMaterial/assessment.ts:372-402`; `src-tauri/src/commands/materials/assessment.rs:25-38,343-358,395`; rustc warning recorded in test/clippy output. |
| S13 / Medium | Test names/comments claim database reopen when the test retains the same service connection | `language_deck_mode_survives_save_and_reopen` creates a single `DbService`, writes and reads using that instance; it never drops/reopens it. Several assessment tests titled “reopen” test the business operation `resolve_assessment_session_to_open` on the same connection, not process/database restart. Their assertions still establish same-connection persistence and lifecycle behavior but not restart recovery. | `src-tauri/tests/language_learning.rs:67-154`; `src-tauri/tests/assessment_integrity.rs:380-434`. |

## Explicit non-findings

- `panic!` in `state.rs` is reachable but intentionally fail-fast for the required vector service, not a fake success path; it is a startup reliability risk (S4).
- `panic!` calls in tests are assertions, not production placeholders. `tts_service.rs` has a panic for unsupported provider enum in internal matching; config limits the supported implementation to OpenAI-compatible TTS but this branch should still be checked if provider enum expands.
- `placeholder` matches in Svelte mostly mean HTML input placeholders, not fake functionality.
- Test-only DB helpers and fixtures in `src-tauri/tests` are not production mocks.
- The lack of FSRS/adaptive/profile/graph/mobile features is absence, not a stub; see feature matrix.

## Unused / weakly integrated dependencies and modules

- `fastembed`, `ort`, `sqlite-vec` are genuinely imported into vector-service code; model/network/device execution remains environment-dependent.
- `chromiumoxide` is used for webpage rendering/preview, while website ingestion has a separate HTTP/scraper converter.
- `tts_service.rs` exists with an OpenAI-compatible provider, but language-learning UI/runtime linkage is limited; do not infer speech practice from its presence.
- `flyllm` is actively used in manager code; declared providers do not imply each provider has been tested.
- `lucide-react` is declared but the frontend imports `lucide-svelte`; likely unused dependency.
- No `fsrs`, Flutter/Dart app, vector-graph/knowledge-map, authentication/payment SDK, or server API implementation was found.

## Prioritized corrective work

1. Make ingestion return and persist a truthful processing state; expose retry and source-specific error; add path-containment/collision protection.
2. Add migration versioning/upgrade fixtures and reject invalid persisted enums instead of silently changing semantics.
3. Return all mastery breadth/provenance fields in the frontend and present deterministic explanations.
4. Make settings accurately map to scheduler parameters; then plan the FSRS migration separately with retained history.
