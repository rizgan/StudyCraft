# Next Implementation Plan

This dependency-aware backlog is not authorization to implement it. Estimates include implementation and tests.

## P0 — Critical correctness and data integrity

| Order | Work item | Evidence / gap | Dependencies and concrete scope | Acceptance criteria and minimum tests | Size |
|---:|---|---|---|---|---|
| 1 | Make ingestion path-safe, collision-safe, and truthful | `commands/materials/study_material.rs:32-105`: joins caller filename into data directory; collisions overwrite; conversion errors swallowed and command returns success. | Safe basename/UUID storage; atomic writes; explicit stored/converting/indexing/ready/failed state; expose retry/reprocess; avoid DB/file drift. | Traversal rejected; duplicate names retain separate data; malformed PDF failure visible and recoverable; successful content reaches Markdown/index; injected failures leave truthful status. | M |
| 2 | Formalize schema migrations | `connection.rs:57-438` has setup + targeted alterations but no version ledger. | Ordered schema version table, upgrade steps for supported historical schemas, backup/transaction before destructive changes. | Legacy fixtures upgrade preserving IDs/history; reruns idempotent; migration failure leaves recoverable DB. | L |
| 3 | Enforce review/assessment state invariants | Session lifecycle and answer upsert exist; protect completed sessions and concurrent starts. | DB constraints/transactional transitions; disallow mutation after completion; protect concurrent start/submit. | Concurrent start yields one resumable session; completed session rejects answer; retries do not duplicate evidence; cascade/soft-delete tests. | M |
| 4 | Secure local boundary and secrets/config UX | Filename trust boundary; `tauri.conf.json` has `csp: null`; keyring exists. | Validate command inputs and paths, review CSP/IPC, redact secrets, validate provider IDs. | Traversal and malformed-input tests; secrets absent from logs/config export; boundary commands reject unsafe paths. | M |
| 5 | Reconcile scheduler settings and edge behavior | `flashcard.rs` hardcodes `[1,10]`, intervals and multipliers while settings expose parameters. | Map every setting or remove misleading fields; document same-day/relearning/caps; retain raw review events. | Frozen-clock tests for all ratings across New/Learning/Review; due times/caps and persisted restart verified. | M |

## P1 — Complete existing core workflows

| Order | Work item | Evidence / gap | Dependencies and scope | Acceptance criteria and minimum tests | Size |
|---:|---|---|---|---|---|
| 6 | Real desktop E2E: import → generation → review → restart | UI and backend paths exist; this audit did not launch Tauri or use a real provider. | Isolated app-data dir; real provider config or local compatible service; real PDF/Markdown import; exercise UI and relaunch. | Reproducible trace proves extracted text, persisted generated deck, review event, schedule and mastery after restart. Provider-unavailable case reports failure, never fake success. | L |
| 7 | Package and verify PDF sidecars | `tauri.conf.json` declares external binaries absent from tracked files; README requires manual PyInstaller. | Build/CI artifacts for supported OS targets, dependency bundling and Rust-launched binary test. | Clean checkout desktop package imports text PDF; malformed PDF and missing OCR errors shown; sidecar version recorded. | L |
| 8 | Finish assessment as supported workflow | Assessment UI/commands/DB tests exist; live provider untested; create payload source IDs/language are discarded; answer command fixes language to English; selected source scope is not retained. | Persist assessment source IDs/language; use selected sources on reassessment/remediation; pass selected language into evaluator; test expected content, state recovery, invalid response and report/remediation generation. | Create from selected sources; submit correct/partial/wrong with real provider in selected language; close/reopen/resume; report/remediation retain source scope; no duplicate event on retry. | L |
| 9 | Complete mastery UI and identity contract | Backend returns breadth/provenance; TypeScript projection drops breadth fields; concept ID is normalized label. | Show coverage/dimensions/evidence; decide stable concept identity and user scope; migration if identity changes. | Typed UI displays all backend fields; contract test; similar labels handled by explicit policy; evidence links to source. | M |
| 10 | Validate APKG with target importer | ZIP and DB readback tests exist, no real Anki import. | Validate against supported Anki package format/parser; implement import only if approved. | Anki-compatible parser/client imports; verify note/deck/card counts, Unicode, HTML and tags. | M |

## P2 — Next planned learning intelligence

| Order | Work item | Evidence / gap | Dependencies and scope | Acceptance criteria and minimum tests | Size |
|---:|---|---|---|---|---|
| 11 | FSRS migration preserving review history | Custom SM-2-like scheduler and raw review rows; no FSRS package/state. | Select/lock version/config; map ratings; derive/init memory state from history/current schedule; preserve immutable events; backup/rollback and dual-run validation. | Golden fixtures from authoritative FSRS implementation; migrate old DB preserving card IDs/history; restart preserves due/stability/difficulty; replay does not duplicate mastery evidence. | XL |
| 12 | Learner Profile | No learner model/profile table or identity. | Decide local single learner vs multi-user; aggregate real review/assessment/mastery with temporal windows. | Deterministic profile from raw events survives restart; user-scoping test if accounts are introduced. | L |
| 13 | Adaptive Study Plan | No plan or planning consumer. | Depends on reliable FSRS due queue and mastery/profile; add priorities, workload budgets, explanations, recalculation and Study UI. | Due/weak/prerequisite fixture matches documented policy; plan updates after review/restart; each choice has an explanation. | L |
| 14 | Learning-aware tutor | Generic source Q&A only. | Integrate source retrieval, assessment/mastery/profile; persist conversation; add misconception-aware hints/follow-up and provider errors. | Source-backed answer references retrieved material; misconception fixture changes hint; history persists without cross-user leakage. | L |
| 15 | Knowledge graph/map | No graph. | Stable concept identity/provenance first; persist typed prerequisite/related edges and confidence; visualize/update map. | Every edge has evidence and relation type; ingestion update idempotent; no arbitrary links; mastery styles nodes. | XL |
| 16 | Timed exam mode | Assessment has reports, no timer. | Reuse question snapshots/scoring; add exam config, time limits, lifecycle and historical comparison. | Clock boundary/restart tests; result reproducible; exam mode semantics separated from mastery assessment. | M |

## P3 — Expansion capabilities

| Order | Work item | Evidence / gap | Dependencies and scope | Acceptance criteria and minimum tests | Size |
|---:|---|---|---|---|---|
| 17 | Image source ingestion/OCR | Image understanding helper, no source pipeline. | Supported formats, OCR/vision extraction, provenance/chunk grounding. | Real diagram/text images create source content and traceable cards; provider outage visible. | L |
| 18 | Audio/video ingestion | TTS exists; ASR/video absent. | Select ASR model/provider; timestamp transcript/keyframe schema and multimodal retrieval. | Real clip transcript aligned with timestamps/keyframe; citations retain offsets; mixed-media generation regression. | XL |
| 19 | Language-learning exercises | Vocabulary metadata/TTS; no feedback loop. | Grammar/listening/pronunciation tasks, speech input/feedback and adaptive language dimensions. | Speaking/listening task completes with persisted feedback; provider failure is truthful; language fixtures. | L |
| 20 | Mobile/sync/commerce | No Flutter, server, auth, payments; historical proposal only. | Requires explicit product approval and backend design first. | Android/iOS build, offline review and sync-conflict tests; server-side entitlement verification/restoration/revocation tests before distribution. | XL |

## Recommended order

Complete P0 items 1–4, then P1 items 6–9 to establish a trustworthy desktop learning loop. Decide whether FSRS is approved before calling current scheduling complete; implement it before adaptive planning because due-state quality is a planner dependency. Profile and concept identity decisions precede graph/tutor integration. Mobile/commercial work should wait for an explicit product decision and is not required to verify the desktop MVP.
