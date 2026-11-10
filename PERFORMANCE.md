# Velocimd performance implementation and verification

## Local issue #9 increment — incremental edit processing

This source-only increment is separate from the published desktop v0.1.6 and the
website's existing browser artifact. It is based on the preserved browser/desktop
parity source checkpoint, not a claim that the browser adapters were already on
public `main`. No release, installation or website deployment is included.

The actual `TextEdit` buffer records insert/delete operations in order. The line
index applies those operations using scalar and byte coordinates, avoiding a new
whole-document character scan after ordinary edits. The preview can reparse an
isolated, parser-confirmed plain paragraph while retaining unaffected blocks.
Structural/context-sensitive edits, replacements, missed revisions and unsupported
paragraph shapes deliberately use the complete parser. Offset/height rebasing can
still scale with line/block counts; this is not a constant-time editor claim.

A focused regression on a 330,003-byte document observed one 34-byte region parse
for its localized edit, with the whole-document parse count remaining at its
initial cold parse. This is measured parser/index work, **not** an end-to-end
keystroke-latency or frame-rate result. The real `TextEdit` regression covers
ordered edits and undo/redo; differential tests compare the full parser/index
through Unicode, CRLF and contextual Markdown cases.

Controller execution passed **161 Rust tests**, **21 parity-tool Node tests**,
formatting, strict all-target Clippy, and the wasm32 library compile check. The
isolated Linux/X11/llvmpipe native scenario passed startup, real editing/mode/theme
controls at 1x/2x, 55-note/1 MiB-note scrolling and Split synchronization, exact
large-note autosaves, inactive-note activation, narrow/light/dark layouts, and
close/reopen with all 53 untouched note bodies preserved. The controller inspected
representative narrow-light and normal-dark captures; no blank panes, corruption
or visible discard warning was present. Native/Xvfb cleanup was verified.

The legacy toolbar harness was corrected to the existing Edit/Split/Preview order
and single theme toggle; no toolbar product behavior changed. Independent SPEC
and QUALITY approval are required before controller acceptance. This increment
does not certify a newly delivered browser runtime, Windows/macOS GUI behavior,
or the remaining four performance issues. Keep compact current receipts and one
useful build cache; generated notes and superseded screenshots are disposable.

## Local issue #11 increment — asynchronous browser drafts

Accepted local source checkpoint `6ca37386d28f2ba684466debee7e83b453bda2ee`
moves browser drafts to transaction/readback-acknowledged, document-granular
IndexedDB storage. Legacy recovery bytes, writer coordination, newest-revision
acknowledgment and lease-drained teardown are preserved. An admitted write remains
uncertain until verified, so reverting to an older acknowledged body cannot hide a
possibly committed newer body. Explicit Retry survives an older in-flight failure.

Controller execution at that checkpoint passed 168 Rust, 24 Node and 88 browser
cases (17 inherited plus 27 persistence cases in each of Chrome and Firefox), with
independent specification and quality approval. A controlled Chrome comparison
used real keyboard edits in a 16-note, 4,050,675-byte legacy session: eight measured
pending-flush calls per variant, after discarding the first. Median synchronous
main-thread action duration was 28.3 ms for the pre-11 storage entrypoints versus
0.1 ms for the accepted async implementation. Each save was acknowledged before
the next edit. This measures synchronous blocking, not total commit latency or
whole-application typing latency; it is not a browser startup claim.

## Local issue #12 increment — guarded retained-layout reuse

The stock TextEdit, complete canonical galley, selection/caret/IME/undo machinery,
visible-row painting and gutter culling remain in use. The accelerated path covers
64 KiB through 4 MiB and at most 32,768 source lines. Ordinary notes and unsupported
native sizes/shapes retain the previous content-checked path; these eligibility
bounds never truncate source or change file/import/session limits.

An immediate derived epoch observes each successful actual buffer mutation before
the controller publishes its one completed revision. Trusted layouter calls also
require a live matching RecordingBuffer attachment; a type tag alone is not a
proof. Overflow and unknown origins fall back safely. Revision adoption requires
the exact final epoch and actual TextEdit output Arc. One active layout/numbered-row
index and a weak backend token are retained, not a history of document bodies or
width variants. These are bounds on the new cache, not a total process-memory cap;
stock widget undo snapshots and canonical full-layout payloads still exist.

The narrowly modified, locked epaint 0.34.1 crate is under
`vendor/epaint-0.34.1/`. `UPSTREAM.json` records the lock-authenticated original
archive, original-file inventory and exact upstream license sources. Only
`src/text/fonts.rs` and its type re-export are behavior/source changes relative to
that crate. New opt-in APIs validate font/cache/scale/galley identities, retain
paragraph children without cloning/hashing source, and force canonical rebuilding
on stale handles or missing children. Default layout and GC behavior are unchanged.
A same-pass identity check avoids refreshing all children twice for measurement
and widget layout; it never replaces the new-pass refresh. Include this modified
crate and its licenses in corresponding-source offers, and requalify the narrow
patch before changing the pinned backend version. Do not patch the shared registry.

In one paired, optimized native CPU qualification, large-document warm/scroll
layout calls requested zero outer source copies/hashes instead of one full job per
frame. Six typing frames reduced outer copy/hash operands from 3,744,084 to
2,016,050 bytes for the long paragraph and from 3,666,084 to 1,974,050 for short
lines. These are source-connected operation counts, not hardware traffic. Warm
frame medians were 0.1264 to 0.0647 ms (long paragraph) and 0.1146 to 0.0985 ms
(short lines). Timings are short sequential CPU samples, not a frame-rate promise;
typing results remain mixed and a single huge paragraph still needs full shaping.
Width changes still require a full canonical job; redundant source comparisons
are avoided but reflow itself is not virtualized. Normal-note copy/hash requests
are unchanged. Existing public token and actual-editor regressions cover retention
after idle, font/atlas replacement, selection/IME events, exact source and revision
publication. The production matrix can be invoked with
`ISSUE12_TRACKED_ONLY=1 cargo test --locked --release --lib actual_textedit_matrix -- --ignored --nocapture --test-threads=1`.

A real browser regression additionally found an inherited input-adapter conflict:
Chromium's hidden text-input history could echo the last Unicode insertion while
egui also performed redo. The identical failure reproduced on the accepted
pre-#12 baseline. The host now identifies the pinned SDK's actual newly created
text agent by startup element identity and cancels only its cancelable,
non-composing native historyUndo/historyRedo beforeinput actions. Keyboard events
still reach egui; normal text/composition and the separate HTML filename dialog
are untouched. Real selection/Unicode undo-redo and guard negative controls are
part of the browser suite; native input handling is unchanged.

This is a bounded retained-layout improvement, **not** incremental text storage or
a fully viewport-paged editor. It does not solve the remaining stock TextEdit
snapshots, giant-paragraph reflow or all large-document tail latency. The wider
issue remains open; local code/testing is not publication or deployment.

Historical target/scratch paths below are original execution pointers, not current
retention promises. Earlier generated targets were removed at the owner's request;
those historical measurements have not been relabeled as fresh execution.

## v0.1.6 integration

The performance implementation and both subsequent input fixes are included in the
v0.1.6 release candidate. The implementation was committed as `137aa2e`; `316eb8c`
adds shared scrolling from the Split preview, and `f01a398` prevents theme shortcuts
from inserting text into a focused editor. That performance/input integration candidate passed 127
tests, strict Clippy, native compilation and wasm32 compilation. Native tests cover
right-pane wheel scrolling and both theme shortcuts in Edit/Split, with exact note
preservation and ordinary typing as a positive control. Release integration also
consumes theme input when a higher-priority command arrives in the same frame;
the focused-editor regression covers both input orders with mode/Save commands,
including unchanged selected text, revision, autosave state, and saved bytes.

Subsequent desktop controls add the saved draggable divider, unified theme toggle,
toolbar ordering and picker handling described in README.md and RELEASE_NOTES.md.
Their additional regressions do not change the historical performance measurements
or the source-bound test counts recorded below.

The plan and qualification records below are historical evidence for the original
v0.1.5-based performance candidate. Their 122-test count predates the five input
regressions added afterward. Paths under `target/` and local scratch are local
verification receipts, not distributed assets; the source regressions and reusable
performance/native smoke scripts are included in the repository. No Windows/macOS
GUI certification or end-to-end input-latency claim is made.

## Original implementation plan

> **For Hermes:** Use the subagent-driven-development workflow with controller-owned integration and independent specification then quality review of the frozen combined candidate.

**Goal:** Implement all improvements accepted from the v0.1.5 performance assessment without changing the minimalist product interface or weakening file durability/recovery.

**Architecture:** Keep Rust/egui and ordinary files. Bound repaint work to visible content; retain revision/width/font-sensitive layouts; use one event-driven ordered persistence worker; restore active content before inactive clean files. No new services, database, polling worker, dependencies, release, installation, or graph maintenance.

**Tech Stack:** Existing Rust 2024, egui/eframe 0.34, pulldown-cmark, serde, standard-library channels/threads, tempfile atomic replacement.

**Baseline:** main/v0.1.5 `61243e987efbdc7f3128fc73fabc462cd3dd4a13`; only pre-existing untracked generated `graphify-out/`. Preserve unrelated navigation branch and installed application. Historical assessment evidence: `/home/logan/.hermes/cache/scratch/velocimd-performance-q7uvf6bi/assessment.md` (scratch retention applies). Do not edit that historical assessment.

## Acceptance and ownership

Controller owns `src/ui.rs`, new UI modules under `src/ui/`, UI regression tests, scripts/benchmarks, this plan/status document. Model/worker implementer owns only `src/document.rs`, `src/app_state.rs`, `src/file_io.rs`, new `src/persistence.rs`, module registration in `src/lib.rs`, and `tests/persistence_behaviors.rs`. No staging, commits or external writes by workers. Formatting must not cross owned files while another writer is active.

### R1 — Visible editor gutter and shared text layout

1. Add failing headless regressions for bounded off-screen gutter shapes and revision/width/font/theme invalidation.
2. Cull off-screen labels while preserving wrapped logical numbering.
3. Cache the actual editor galley by document ID, revision, wrap width, font and color/scale; share it with TextEdit's layouter rather than constructing a redundant full-document job each warm frame. Invalidate if input changes inside a frame.
4. Run editor coordinate/bounds and gutter tests. Compare 10k-line warm repaint against baseline; target >=70% reduction in the CPU-only fixture, without claiming a whole-app speedup.

### R2 — Preview block cache and virtualization

1. Add semantic segmentation, cache invalidation, bounded warm visible-render, scroll, width/theme and edit regressions before implementation.
2. Build Markdown block boundaries from parser top-level ranges, preserving list/table/code/reference behavior rather than arbitrary blank-line splitting. Keep Mermaid special blocks and source-line mapping.
3. Retain measured block heights and parsed-source identity across revisions for unchanged blocks; render visible blocks with overscan and measured/estimated spacers. Do not adopt the dependency's explicitly buggy hidden scrollable API.
4. Handle first layout, content edits, width/font/theme changes, image resize, and synchronized jump to a distant source block without stale/empty viewports.
5. Measure 1 MiB split repaint against original CPU baseline; target <8 ms warm CPU work on the current machine. Verify native appearance and scrolling, not just synthetic timings.

### P1 — Efficient and safe model writes

1. Regress folder changes with clean file-backed, dirty and pathless documents. Save only `dirty || path.is_none()`; never write an unloaded empty placeholder.
2. Retain canonical path ownership identities; normalize incoming aliases without repeatedly canonicalizing every other open file on each save. Preserve symlink-alias and duplicate-restoration refusal.
3. Add detached prepared writes and revision/path-aware completion. A completion can assign a successful first-save path but must not clear newer dirty edits or overwrite a later identity transition. Reserve unique first-save targets within a batch.

### P2 — Ordered persistence and session coalescing

1. Worker tests exercise actual staged writes plus held/failed writes, ordered barriers and revision-safe application. A normal enqueue must not wait for fsync.
2. Native worker blocks on a channel and wakes egui via a callback after completion. No periodic loop, timer retry or polling thread. WASM keeps a compile-safe synchronous fallback.
3. Worker accepts document batches, owned session snapshots (serialize/write off-thread), completion drain, and a flushing barrier. Coalesce superseded pending session snapshots without crossing document/barrier ordering.
4. Controller routes autosave/manual Save through the queue; ordinary session mutations debounce and cancel satisfied deadlines. Drain/apply before explicit rename/Save As/identity-changing operations. Shutdown drains old writes, applies completions, saves latest dirty revisions, then persists recovery. Failure retains dirty content in session snapshots.

#### Agreed worker/model API

`persistence::DocumentWrite`: public `document_id: u64`, `revision: u64`, `original_path: Option<PathBuf>`, `path: PathBuf`, `content: Arc<str>`, `overwrite: bool`.

`persistence::DocumentWriteResult`: public `request: DocumentWrite`, `result: Result<PathBuf, String>` (successful canonical target), `membership_changed: bool`.

`persistence::Completion`: `Documents(Vec<DocumentWriteResult>)` or `Session(Result<(), String>)`.

`persistence::SessionWrite`: owned opaque cloneable snapshot plus target; `write(self) -> Result<(), String>` can stay crate-private. Do not expose private persisted representation unnecessarily.

`PersistenceWorker::new(wake: impl Fn() + Send + Sync + 'static)`, `submit_documents(&self, Vec<DocumentWrite>) -> bool`, `submit_session(&self, SessionWrite) -> bool`, `drain(&self) -> Vec<Completion>`, `flush(&self) -> Vec<Completion>`. Drop drains and joins. Flush is an explicit shutdown/identity-change barrier, never a per-frame operation.

`AppState::prepare_dirty_writes(&self) -> Vec<DocumentWrite>`, `prepare_document_write(&self, index: usize) -> Option<DocumentWrite>`, `apply_write_result(&mut self, result: &DocumentWriteResult) -> bool` (whether the request still belongs to that document), `prepare_session_write(&self) -> anyhow::Result<SessionWrite>`, `ensure_document_loaded(&mut self, index: usize) -> bool`.

Controller permits only one outstanding document batch, prepares the next revision after completion, and keeps successful session snapshots ordered behind relevant writes. Return named errors/failed preparations rather than silently marking unsaved data clean.

### D1 — Directory menus

1. Regress Arc snapshot reuse and bounded row construction on 10k entries.
2. Share immutable cached entries, compute lowercase sort keys once, render visible rows with `show_rows` and correct item spacing.
3. Preserve existing external-change freshness (TTL on user-driven menu rendering, no new polling); invalidate only create/rename/delete/first-save membership changes, not content overwrites.
4. Target <1 ms warm CPU menu frame for 10k entries on this machine; retain ordering/navigation.

### L1 — Lazy clean inactive documents

1. Regress metadata-only restore, active content, activation, unreadable/oversized paths, conflicting recovery snapshots, duplicate owner repair and save/drop behavior.
2. Distinguish unloaded from loaded-empty documents explicitly with runtime state. Active content is loaded on session restore; inactive clean file-backed bodies remain deferred. Dirty/recovery buffers remain eager and authoritative.
3. Activation hydrates exactly once; saves and renames must never treat unloaded content as an empty file. Preserve the legacy on-disk session format and recovery behavior.
4. Compare 55 x 1 MiB restore bytes/memory and initial hydration count; only the active clean buffer should be hydrated before activation.

## Verification and review boundary

- Targeted tests RED before GREEN for each behavior; preserve original file safety and editor tests.
- `cargo fmt --all -- --check`; `cargo test --locked --all-targets`; `cargo clippy --all-targets --all-features --locked -- -D warnings`; `cargo check --target wasm32-unknown-unknown --locked`.
- Native `scripts/native-smoke.py` and toolbar smoke with isolated fixture config, plus performance scrolling/edit/late-write/reopen scenarios. An ImageMagick source-sheet delegate failure is separate from packaged native runtime evidence; use the internal MSVG renderer if necessary.
- Repeated optimized same-session paired benchmarks, with source/binary hashes and raw samples. CPU probes are not GPU or end-to-end typing measurements.
- Freeze full source/tests candidate; independent SPEC PASS followed by independent QUALITY approval, fix findings with regressions and requalify changed coverage before reporting all items complete.
- No push, tag, release, installation or Graphify refresh is authorized by this implementation request.

## Original candidate qualification (historical)

All requested implementation paths are assembled locally. Successor candidate v2 repairs blocking finding QUALITY-001 from the independent quality review of v1. The rejected source, reports, original 3-second timeout proof, and exact v1 native binary remain retained. The repair counts newline bytes from egui row metadata, enforces positive UTF-8 progress, and uses canonical whole-text layout rather than inventing a corrupt byte boundary if the engine cannot expose a consuming prefix. Fragment geometry is checked against whole layout, including blank boundary rows.

Controller qualification: **122 tests (zero failed/ignored/filtered)**, formatting, strict all-target/all-feature Clippy, native compilation, and wasm32 compilation passed. Authoritative logs: `target/performance-execution/controller/quality-001/{all-tests,clippy,wasm,build}.log`. The new encoded-newline/mixed Unicode/CRLF regression covers widths 1, 263 and 640, including fractional scale, actual byte progress and complete row/glyph/height parity. The exact public API proof on final source returned normally for 9,000 encoded newlines and the plain-text control: `target/performance-execution/controller/quality-001/final-proof/result.json`.

The full native scenario passed again at `target/performance-execution/native/run-20261105T085749489557Z/result.json`: 55-note startup, real End/Home, distant Split, exact autosave, public inactive-note activation, narrow/normal light/dark layouts, 1x/2x toolbar, graceful close/reopen, and 53 untouched note hashes. Both app exits and Xvfb cleanup are verified. Controller inspected matching source sections/no warnings in successor narrow-light and normal-dark captures. Linux/X11/llvmpipe only; not Windows/macOS runtime certification.

Four alternating paired rounds on final source are retained at `target/performance-execution/bench/run-20261105T085856437100Z/summary.json`:

| CPU-only median work | Released baseline | Candidate v2 |
| --- | ---: | ---: |
| 100 KiB / 10k-line Edit warm | 1.663 ms | 0.081 ms |
| 1 MiB Split warm | 17.099 ms | 0.213 ms |
| 1 MiB Split content-revision redraw | 29.156 ms | 5.869 ms |
| Rich ~100 KiB Preview warm | 9.768 ms | 0.264 ms |
| 10k-entry menu warm | 5.543 ms | 0.014 ms |
| Directory scan/sort | 7.262 ms | 4.259 ms |
| 55 x 1 MiB session restore, warm filesystem | 6.421 ms | 0.673 ms |

Only the active 1 MiB clean-file body is eagerly restored instead of all 55 MiB. These are retained buffer lengths, not total-process RSS. CPU probes exclude GPU presentation, full native shell and end-to-end keyboard latency.

Earlier failed native/revision candidates are retained, not waived. Repairs include paragraph-cache residency, post-scroll/geometry reconciliation, nonanimated source following, offscreen-refinement anchor compensation before painting, stable live widget IDs and invisible measurement UIs. Stable frames assert one completed pass; real URL/copy effects remain tested. Session snapshots are bounded to one in flight plus latest-state intent; selected Save As has an actual identity-barrier regression.

Only the preview implementation/new regression and this status section changed after v1 review. Exact unchanged-path coverage and fresh successor evidence are bound under `target/performance-execution/review/candidate-v2/`; focused SPEC then QUALITY successor decisions belong outside the frozen candidate under `target/performance-execution/review/`. Consult controller acceptance there for final review state. No commit, push, release, installation or Graphify maintenance. Installed v0.1.5 remains unchanged. No new dependency, service, database or polling worker.
