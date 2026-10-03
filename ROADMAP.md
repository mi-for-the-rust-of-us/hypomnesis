# hypomnesis — Roadmap

> *External RAM and VRAM, measured. Status snapshot — updated continuously as plans change.*

Per-release detail lives in [`docs/roadmap-vX.Y.Z.md`](docs/) (indexed below).
Shipped history lives in [`CHANGELOG.md`](CHANGELOG.md).
The crate's *why* lives in [`docs/hypomnesis-brief.md`](docs/hypomnesis-brief.md).

---

## Current state

**v0.2.13** shipped 2026-09-30. *Stop `hmn watch` from reporting a spill check it never ran, and let
`hmn ps` say who is being paged.* An askesis dogfooding report
([2026-09-28](docs/dogfooding-feedbacks/dogfooding-spill-verdict-wording-and-ps-filters.md))
found `hmn watch`'s text summary saying `no spill observed` on Linux, where spill is not
measurable — a bug since v0.2.6 — and asked for `hmn ps` to name the paged process, select by
name (`--filter`, which triggered the `PsFilters` refactor), gate on "nothing matched"
(`--exit-status`), take several `--pid`s and refuse an unlistable `--device`. A new
`process_exists` backs a warning for a nonexistent explicit PID. Validating it found `hmn watch` blind to a spill already under way at attach; it now says so, and the real fix un-gated the `spill_condition` item below. Detailed plan:
[`docs/roadmap-v0.2.13.md`](docs/roadmap-v0.2.13.md).

**v0.2.12** shipped 2026-09-26. *Clean the base first, then teach `hmn watch` to
follow a process by name.* Part 1 — done 2026-09-26 — remediated the nine items of a duplicate-code audit
([`docs/audits/2026-09-26-duplicate-code-audit.md`](docs/audits/2026-09-26-duplicate-code-audit.md))
— no copy-paste found, but one structural duplicate had already cost a defect (v0.2.10's `DXGI`
skip-on-bad-adapter fix reached 2 of 6 walks). Part 2 delivers a candle-mi dogfooding report's
asks for identity-based selection in `hmn watch` (`--filter`, `--min`, the criterion on the header,
and a `start` record opening `--json`)
([2026-09-21](docs/dogfooding-feedbacks/dogfooding-watch-filter-by-identity.md)); reviewing it
found Linux process names cut to 15 bytes, now reported in full. Detailed plan:
[`docs/roadmap-v0.2.12.md`](docs/roadmap-v0.2.12.md).

**v0.2.11** shipped 2026-09-15. *`hmn ps` learns to say "spilling" instead
of making the operator infer it.* A candle-mi dogfooding report
([2026-09-14](docs/dogfooding-feedbacks/dogfooding-orphan-attribution-and-ps-spill-flag.md))
field-validated v0.2.7's `--follow-new` (36 sequential processes, clean) and
then diagnosed an 8× GPU slowdown via `hmn ps`'s SHARED column — two
orphaned test binaries holding 8.6 GiB, the real job spilling 6.8 GiB into
shared memory — but had to reach the verdict by eye, since only `hmn watch`
carried a spill signal. Four small, additive asks, in the report's own
priority order: `hmn ps` gains a SPILL column / `spilling` JSON field (a
single-snapshot approximation of `hmn watch`'s spill co-condition, honestly
`null`/`?` rather than `false`/`no` when unmeasurable — a contract `hmn
watch`'s own `spilling` field now shares too); `hmn watch --json` samples
gain an absolute `wall_clock` timestamp (UTC ISO-8601, no new dependency)
alongside the existing relative `t_ms`; `hmn ps --min <SIZE>` hides rows
below a size threshold instead of piping through `awk`; and a new
`hmn fits <SIZE>` subcommand answers "will this job fit right now" with a
single gateable exit code. Two independent post-ship code-review passes (the
second explicitly a fresh re-review at maximum effort) then found and fixed
a further batch of real issues — most notably a narrow case where the new
SPILL honesty contract could still collapse "can't tell" into "not
spilling," and a genuine timing skew between `wall_clock` and `t_ms` on
`watch`'s first sample — each reproduced live before the fix and
re-verified live after. Three architectural suggestions from that review
(a unified spill-condition core, a `GpuDeviceInfo::fits()` library method, a
`PsFilters` struct) were deliberately left for later — see `Speculative:
v0.3.0`, below — rather than folded in unilaterally. Detailed plan:
[`docs/roadmap-v0.2.11.md`](docs/roadmap-v0.2.11.md).

The preceding **v0.2.10** shipped 2026-08-17. *Audited, not assumed.* A full-codebase
self-audit — read as a dogfooding report in its own right (Principle 1:
every patch traces to a real adoption experience, and auditing the crate
against its own documented conventions is exactly that, conducted
first-party) — found three gaps sharing one shape: silence. `hmn ps`
capped silently at 64 compute processes on busy multi-tenant devices; a
single malformed `DXGI` adapter could silently truncate the rest of an
enumeration; a partially-broken driver install could make `hmn` print
nothing — or, in `--json` mode, emit an indistinguishable `[]` — instead
of saying so. All three now say so. Also closes a three-release-old CI
gap (macOS never compiled in CI despite being first-class since v0.2.3)
— not a theoretical fix: the new leg's first two real runs on
`origin/main` immediately caught two genuine `-D warnings` failures in
`src/gpu/metal.rs`, both three releases old and invisible until this
exact leg ran for the first time — and adds a `publish.yml` guard
against a mistyped release tag. A second adversarial review pass on the
fixes themselves found and closed four further issues before this
release. Full audit:
[`docs/audits/2026-08-17-codebase-documentation-audit.md`](docs/audits/2026-08-17-codebase-documentation-audit.md).
Detailed plan: [`docs/roadmap-v0.2.10.md`](docs/roadmap-v0.2.10.md).

The preceding **v0.2.9** shipped 2026-08-12. *The same total. Now with
the driver behind it.* A `candle-mi` dogfooding report
([2026-08-12](docs/dogfooding-feedbacks/dogfooding-driver-version-provenance.md))
asked for the NVIDIA driver version to become part of `hypomnesis`'s
output: `candle-mi`'s `RESURRECTION.md` provenance log stamps the Rust
toolchain per verification run but not the GPU driver, and a driver
change can move floating-point results — not a hypothetical, since the
report's own reference machine hit a `DPC_WATCHDOG_VIOLATION` bugcheck
mid-run and needed a driver update (`591.86` → `610.88`) to recover.
Ships `GpuDeviceInfo::driver_version: Option<String>`, mirroring v0.2.4's
`reserved_bytes` addition — sourced from `NVML`
(`nvmlSystemGetDriverVersion`) and, since `nvidia-smi` can genuinely
supply this figure unlike `reserved_bytes`, also from the `nvidia-smi`
fallback. Rendered on the existing `hmn` device-summary line
(`..., driver 610.88`) and via a new `hmn --json` flag on the default
subcommand — the report assumed a JSON summary surface already existed;
this release adds one. Detailed plan:
[`docs/roadmap-v0.2.9.md`](docs/roadmap-v0.2.9.md).

The preceding **v0.2.8** shipped 2026-08-04. *The tool you install should
install. The name you can't be shown, someone else can.* An askesis
`canvas` dogfooding report
([2026-08-03](docs/dogfooding-feedbacks/dogfooding-install-no-binary-and-protected-names.md))
found `cargo install hypomnesis` completing with exit `0` and installing no
binary at all (`cli` was default-off), and diagnosed — with one detail
corrected before implementation — that most Windows `?` rows in `hmn
ps`/`hmn watch` are nameable without elevation. The report's proposed fix
for the `?` rows (switch `OpenProcess` to `PROCESS_QUERY_LIMITED_INFORMATION`)
turned out to already be shipping since v0.2.2; a live test confirmed both
query rights fail identically against `dwm.exe`/`csrss.exe`. The actual fix
is `CreateToolhelp32Snapshot`, batched once per `gpu_processes()` call, which
reads process names without opening a per-process handle. Ships four
changes: `cli` becomes a default feature; the `Toolhelp32Snapshot` fallback
(Windows-only); `[exited]`/`[protected]` brackets replacing the anonymous
`?` for what remains unresolved, with the summary-line protected count now
excluding `[exited]` (elevation can't help a process that's already gone);
`hmn ps --sort vram`/`committed` as aliases for `--sort dedicated`. A
related correctness gap in existing (pre-v0.2.8) `hmn watch` logic was found
and fixed while wiring the brackets in: the OS-PID-reuse detector and the
unresolved-PID growth hint both needed to treat a transient
`[protected]`/`[exited]` flicker as still-the-same-process, not evidence of
PID reuse. A separate consistency pass over the diff (conventions +
adversarial correctness, then a documentation-specific pass) found and fixed
a second, older gap in the same protected count: it never caught the
pre-`WDDM 2.0` `nvidia-smi` fallback's literal `?` name string, silently
uncounted since v0.2.2. Detailed plan: [`docs/roadmap-v0.2.8.md`](docs/roadmap-v0.2.8.md).

The preceding **v0.2.7** shipped 2026-08-02 — `hmn watch --follow-new` and `hmn ps
--sort`. *Follow the work, not just the machine. Sort by the question
you're actually asking.* A candle-mi dogfooding report
([2026-07-27, extended 2026-08-01](docs/dogfooding-feedbacks/dogfooding-watch-follow-new.md))
ran `hmn watch` alongside candle-mi's `scripts/resurrect.ps1` oracle suite —
19 sequential `cargo test` processes over 44 minutes — and found the
adapter-level spill machinery flawless (three real episodes, including a
fast 20-second Mistral-7B spike candle-mi's own wall-clock heuristic had
never caught) while the per-PID half answered the wrong question: `watch`'s
auto-selected set froze at the first sample, so none of the nineteen
processes that actually caused the spills were ever attributed. `--follow-new`
(auto-select mode only; a hard error combined with explicit PIDs) re-runs the
top-`--top` selection every interval instead of once at attach — a PID
entering starts fresh, a PID leaving is *finalized* into the closing
summary's `per_pid[]` instead of rendering `0` forever, tracked via a new
`WatchState`'s `seen_order` (first-seen, no duplicates). Re-entry after a gap
resumes existing history; only the existing OS-PID-reuse name-change
detector resets a row. A companion request from the same suite, filed
separately: `hmn ps --sort <dedicated|shared|total>`, sharing a single
`ps_row_comparator` with `hmn watch`'s auto-selection (`select_top_n_pids`,
always pinned to `Dedicated`) so the two orderings can't drift apart — a
deliberate, live-confirmed consequence being that `hmn watch`'s auto-selected
top-N can now pick a different PID than pre-v0.2.7 at an exact VRAM tie.
Both features validated the same way v0.2.6 was: two sequential real
`spillforge` forced-spill runs under `hmn watch --follow-new --json`,
correctly tracked as distinct entries with two separate spill episodes and
all seven ever-seen PIDs finalized into the summary, both manually and via a
new automated `#[ignore]`-gated end-to-end test. The repo also transferred
to the `mi-for-the-rust-of-us` GitHub org during v0.2.7 (joining `anamnesis`
and `candle-mi`); v0.2.7 carries the corrected crates.io metadata.
Detailed plan: [`docs/roadmap-v0.2.7.md`](docs/roadmap-v0.2.7.md).

The preceding **v0.2.6** shipped 2026-07-25 — `hmn watch [PID...]`, attach-to-a-running-PID
spill triage. *Not a TUI. Same tracker, a timer instead of a wrapped child.*
`hmn spill -- <command>` only wraps a *new* process; a rhyme-mdlm dogfooding
report ([2026-07-25](docs/dogfooding-feedbacks/dogfooding-spill-triage-watch-mode.md))
hit that wall three times triaging a 15-hour training campaign, hand-rolling
"two `hmn ps` samples minutes apart, diff by eye" every time because there
was no way to attach to a trainer already hours into its run. `hmn watch`
closes the gap as a pure CLI addition — zero changes to `src/spill.rs` or
`src/gpu/pdh.rs`: it samples the unchanged `SpillTracker` (adapter-wide
dedicated-saturation + shared-growth co-condition) and the unchanged
`gpu_processes()` (per-PID committed/shared bytes) on a timer, printing one
row per watched PID per interval with per-interval deltas and a live SPILL
flag. No PID given auto-selects the top `--top` (default 5) by committed
`VRAM` from the first sample, kept fixed for the run; explicit PIDs are
watched exactly as given. `--interval` / `--duration` take duration strings
(`30s`, `5m`, bare seconds — a hand-rolled parser, no new duration-parsing
dependency) rather than `hmn spill`'s raw milliseconds, tuned for an
attach-and-leave-running tool rather than a tight wrap. Ctrl+C (via the new
`ctrlc` dependency, `cli`-feature-only) and a natural `--duration` stop both
print the same closing summary — the same `SpillReport` shape `hmn spill`
emits, plus a per-PID peak/baseline table — and set the same exit-code
contract: `0` no spill observed, `1` spill observed at least once, `2` on a
hard error, designed for a watchdog script to check directly without parsing
JSON. `--json` streams JSON Lines (one `"kind":"sample"` object per PID per
interval, a closing `"kind":"summary"` object) rather than a single blob, to
match `watch`'s live-tailing character. Two small best-effort robustness
additions found during an adversarial pre-commit review (the same two-agent
conventions-plus-correctness pass v0.2.5 used): a watched PID whose resolved
name changes between samples (OS PID reuse) resets that row's baseline
rather than mixing two processes' readings, and an unresolved (`?`) watched
PID that grows past 256 MiB since attach gets a one-shot elevation hint.
Live-validated against the same `spillforge` forced-spill fixture that
validated `hmn spill` in v0.2.5 (both an automated `#[ignore]`-gated
end-to-end test spawning the real compiled binaries, and manual dogfooding
runs recorded in the roadmap doc) plus a real idle-desktop no-false-positive
run with auto-selected top-3 PIDs. This resolves the "Carried forward"
table's `hmn watch (TUI live-refresh)` row below — the rejected item was a
curses-style redraw dashboard; what shipped is explicitly not that. Detailed
plan: [`docs/roadmap-v0.2.6.md`](docs/roadmap-v0.2.6.md).

The preceding **v0.2.5** shipped 2026-07-22 — `WDDM` spill detection. *Resident, not committed. Episodes, not a boolean.* A `SpillTracker` that compiles on every platform (honest `is_spill_measurable()` returns `false` off-Windows) reads the `PDH` `\GPU Adapter Memory(*)` residency gauges and flags spill only when dedicated-resident saturates **and** shared-resident grows past its benign first-observation baseline — never from the `committed − dedicated` gap, per the rhyme-mdlm dogfooding report's live false-positive ([2026-07-19](docs/dogfooding-feedbacks/dogfooding-wddm-spill-detection.md)). Transient spills are first-class: an instantaneous `is_spilling()` / latched `has_spilled()` split plus an episode-based `SpillReport`. Per-process attribution rides along as additive `GpuProcessEntry::shared_used_bytes` (+ `hmn ps` SHARED column), and `hmn spill -- <command>` wraps any run `time(1)`-style (`--interval` default 100 ms, `--json`, exit-code pass-through). Two live-measured corrections to the design sketch: no `Dedicated Limit` counter exists in `PDH` (capacity comes from `DXGI` `DedicatedVideoMemory`), and the dedicated-saturation default is **85%**, not ~95% — a forced-spill fixture measured `VidMm`'s dedicated-resident ceiling at ≈ 88.6–91.3% of `DXGI` capacity, making 95% unreachable. Release-validated with a real 13.1 s spill episode (3.1 GiB peak shared) on the reference `RTX 5060 Ti`, produced by a forced-spill fixture preserved at [`tools/spillforge`](tools/spillforge/) (repo-only, `publish = false`) for future re-validation on new drivers or contributor hardware. Detailed plan: [`docs/roadmap-v0.2.5.md`](docs/roadmap-v0.2.5.md).

The preceding **v0.2.4** shipped 2026-06-29 — surfaces NVIDIA's driver/firmware **reserved** memory carve-out. *The same total. Now with the carve-out shown.* A new additive `GpuDeviceInfo::reserved_bytes: Option<u64>` exposes the carve-out NVML holds *within* its reported `total` (`total = reserved + free + used`) — **live-measured at 259 MiB** on the reference `RTX 5060 Ti`, byte-identical to `nvidia-smi -q -d MEMORY`'s `Reserved` line beside `Total: 16311 MiB`. It is a subset of `total_bytes`, so allocation headroom is `total_bytes − reserved_bytes` (which `free_bytes` already reflects). Sourced from NVML's v2 memory query (`nvmlDeviceGetMemoryInfo_v2`, R510+) with a graceful pre-R510 fallback to `None`; `total_bytes` is unchanged (the v1 figure = `nvidia-smi` `Total`). Driven by a [`candle-mi`](https://github.com/PCfVW/candle-mi) v0.1.16 dogfooding report — whose *inferred* 73 MiB carve-out (`DXGI nominal − NVML total`) the live v2 query revealed to be a *different* quantity (board/ECC overhead below NVML's `total`) from the true 259 MiB driver reservation. Detailed plan: [`docs/roadmap-v0.2.4.md`](docs/roadmap-v0.2.4.md).

The preceding **v0.2.3** shipped 2026-06-10 — first-class macOS support on Apple Silicon. *Three platforms. Same contract. Resident-bytes everywhere.* Contributor [@LittleCoinCoin](https://github.com/LittleCoinCoin)'s [PR #1](https://github.com/PCfVW/hypomnesis/pull/1) lands the macOS path: libSystem-only RAM + per-process GPU + compute-process listing (`task_info`, `ledger`, `sysctl`, `proc_listpids`, `proc_pidpath`), with `MTLDevice.recommendedMaxWorkingSetSize` via a minimal `objc2-metal` binding for the device-wide GPU budget. Two dogfooding-driven UX additions rode along: `hmn ps` stderr summary gains a "committed total" figure (signalling the `WDDM` commit-vs-resident distinction without naming it), and a "Composable workflows" `README.md` subsection documents `hmn ps --json` with two `jq` recipes — including a *"Why no `hmn kill`?"* scope-discipline note declining a hmn-side kill subcommand to preserve hypomnesis's measurement-not-control boundary. Field-validated post-release on H100 / GB200 (Linux) and a 48 GiB MacBook Pro alongside the contributor's M3 Pro daily-driver. The [PR #1 body](https://github.com/PCfVW/hypomnesis/pull/1) served as the per-release roadmap.

The preceding **v0.2.2** (2026-06-02) shipped the Windows `PDH` per-process backend — first Rust crate (to the maintainer's knowledge) to expose per-process `VRAM` for foreign processes on consumer Windows / `WDDM`, closing the dogfooding gap where `hmn ps` had silently dropped 27 processes including the maintainer's own `ollama.exe`. Detailed plan: [`docs/roadmap-v0.2.2.md`](docs/roadmap-v0.2.2.md).

---

## Speculative: v0.3.0

Items that *might* land, gated on real consumer demand:

- **Cross-platform "unmeasurable rows" diagnostic** — emit a count of detected-but-unmeasurable processes in the `hmn ps` stderr summary. Probably **never needed** now that v0.2.2 + v0.2.3 have shipped, because all three platforms reliably deliver bytes for every readable PID. Resurfaces only if an adopter reports a process they can't see, or if very-old Windows / `WDDM 1.x` environments matter to a real user.
- **`Option<u64>` for `GpuProcessEntry::used_bytes`** — breaking change, would be v0.3.0 not patch. Deferred unless a consumer specifically asks to *list* unmeasurable processes (rather than just count them).
- **`spilled` as `null` when spill is not measurable** (surfaced by the v0.2.14 field check, issue #3, F6, the JSON half) — `write_spill_report_fields` feeds both `hmn spill --json` and `hmn watch --json`, and on Linux and macOS, where spill cannot be measured, `spilled` reads `false`. Turning a `bool` into a `bool` or `null` is a type-shape change, so a minor bump under Principle 2; the maintainer agreed on 2026-10-02 to wait for v0.3.0. Written up in [the notice](__reports__/field_check_v0213/02-notice_spilled_null_v0.md).
- **Segmented per-process VRAM API** — sibling library function `query_per_process_vram_segmented()` returning one row per `(pid, segment)` from PDH's `pid_NNNN_luid_X_phys_N` instances, plus a `hmn ps --show-segments` (or similar) CLI flag. The v0.2.2 PDH backend internally enumerates segmented data before collapsing to per-PID totals; a future patch would promote the internal helper to `pub(super)` and add a sibling dispatcher entry. Gated on either a real consumer ask or hardware exhibiting multi-segment behaviour (single-partition GPUs collapse the two paths identically, so the maintainer's `RTX 5060 Ti` can't validate the segmented path).
- **`format_summary` / `format_free_used_total`** (deferred from v0.2.1 Wave C) — promote when a second `report`-feature consumer validates the shape.
- **Long-lived `NVML` context** — performance work, deferred from v0.2.0 / v0.2.1, no benchmark-loop consumer asking yet. Smaller than it was: since v0.2.12 every entry point already goes through one `NvmlSession` RAII guard (`src/gpu/nvml.rs`), so a long-lived context is a question of who owns and caches that session, not of re-plumbing init/shutdown by hand.
- **Builders for `ProcessGpuInfo` and `Snapshot` under `test-helpers`** — add per type as downstream tests demand. (Clause exercised twice so far, both times demanded by the `hmn` binary's own tests: `SpillReportBuilder` in v0.2.5, `GpuProcessEntryBuilder` in v0.2.6 — so `GpuProcessEntry` has left this list. `ProcessGpuInfo` and `Snapshot` are the two still pending.)
- **`hmn spill` partial report on Ctrl+C** (deferred from v0.2.5) — a `ctrlc` handler so an interrupted run still prints what was observed. **The original rationale has expired**: it was deferred "to keep v0.2.5 dependency-free", but v0.2.6 took `ctrlc` as a `cli`-feature dependency for `hmn watch`, so the cost is already paid and `run_spill` could reuse the same `Arc<AtomicBool>` + interruptible-sleep pattern `run_watch` already runs. What remains is the genuinely harder half, and it is a design question rather than a dependency one: Ctrl+C reaches the whole process group, so `hmn spill` must decide what it owes the *wrapped child* (has it died? should we wait? what exit code do we then pass through?) before it can report honestly. Current behaviour is documented in the `--help` text. Still un-gated by a consumer who actually loses a report they needed.
- **`SpillTracker` auto-reopen after driver reset / `TDR`** (deferred from v0.2.5) — today a reset invalidates the long-lived `PDH` query and every later `observe()` is a skipped observation (documented). Un-gated by a real consumer whose runs survive TDRs.
- **Per-process attribution inside `SpillTracker` / adapter `Total Committed` exposure** (deferred from v0.2.5) — the tracker's condition is deliberately adapter-scoped; per-PID shared attribution lives in `gpu_processes()`. Fold attribution into the report only if a consumer shows the two-step flow (`hmn spill` then `hmn ps`) losing the culprit in practice. **Near-miss on that gate in v0.2.7**: the candle-mi report *did* lose the culprit — the adapter shouted SPILL for 16 minutes while the per-PID table showed only desktop tenants — but the cause was `hmn watch`'s frozen PID set, not the tracker's adapter-scoping, and `--follow-new` fixed it entirely at the CLI layer. The library-level fold stays un-gated.
- **Harden the `hmn watch` PID-reuse reset against name-resolution races** (surfaced by the v0.2.7 report; still open post-v0.2.8) — the reset that detects OS PID reuse fires only when *both* the previous and current sample carry a resolved process name (v0.2.8: `resolved_name()` — excludes `None` and the new `[protected]`/`[exited]` brackets alongside the pre-existing exclusion), so a short-lived recycled PID whose name lookup loses the race silently keeps the old process's baseline. The report saw exactly this shape: a `firefox.exe` row peaking at an implausible 15.7 GB, timed with the big-model steps. Documented as best-effort in the `--help` text and the watch tutorial's Gotchas. v0.2.8 widened what counts as "unresolved" for this comparison (so a transient `[protected]`/`[exited]` flicker doesn't itself trigger a *false* reset) but did not close the underlying race — a genuinely new process whose name lookup loses the race on its first sample is still misattributed to the old baseline. A fix would need a second identity signal beyond the name (process start time is the obvious candidate, at the cost of a per-PID `OpenProcess`); un-gated by a consumer for whom the mis-attribution actually changes a diagnosis. v0.2.12's `--filter` shares the limit from the other side: a followed PID keeps its last resolved name, so if the OS reuses it for a process whose name cannot be resolved, that process inherits the old name and can match — the same second identity signal would close both.
- **Standalone CLI reference doc** — a systematic, flag-by-flag reference for all five subcommands (root device summary, `ps`, `spill`, `watch`, `fits` since v0.2.11; ~16 flags total), generated from or kept in lockstep with `hmn --help`'s actual output so it can't drift from the real flag set, rather than hand-duplicated prose. README's "Binary (`hmn`)" section is narrative/example-driven by design and `hmn --help` is already comprehensive at runtime — this would add a browsable/linkable version, not a missing capability. Not dogfooding-driven — surfaced in conversation while checking v0.2.8's documentation completeness; gated on an adopter actually wanting one.
- **Unified `spill_condition` core with pluggable thresholds** (surfaced by v0.2.11's second, independent post-ship code-review pass) — `fold` (backing `SpillTracker`/`hmn watch`/`hmn spill`) and `saturated_with_shared_floor` (backing `hmn ps`'s SPILL column, v0.2.11) each reimplement the two-sided spill predicate; only the dedicated-threshold arithmetic is actually shared (`default_dedicated_threshold`, also v0.2.11). A `SharedCriterion::GrowthAboveBaseline{baseline, margin} | AbsoluteFloor(bytes)` core would unify both *and* let `snapshot_is_spilling` accept the same threshold overrides `SpillTracker` already exposes (`with_dedicated_threshold` / `with_shared_growth_threshold`) — today a consumer who tunes those gets a *different* verdict from `hmn ps` for the identical adapter state, with no way to align them. **Un-gated by v0.2.13 (askesis report, found live):** the growth-only condition makes `hmn watch` blind to a spill already under way at attach — the baseline absorbs it, so the rows read `no` and the summary `no spill observed` while `hmn ps` says spilling — and attaching to a job that already looks slow is `watch`'s main use. v0.2.13 only says so (an attach-time warning, a summary line, `spilling_at_attach`). The fix is this core: let the tracker's verdict also accept `hmn ps`'s absolute floor, so `watch` detects, counts and marks (`PAGED`) such a spill. Open design questions: whether an episode can start at the first observation, and how `SpillReport` distinguishes spill present at attach from spill that grew. Library semantics change, so its own release.
- **`GpuDeviceInfo::fits(&self, size: u64) -> bool`** (surfaced by the same review) — `hmn fits`'s `size <= free_bytes` check lives only in the CLI binary (`src/bin/hmn/fits.rs`); a library consumer has no equivalent and would have to rediscover the per-backend caveats (NVML-only `reserved_bytes` netting, the `DXGI`-fallback per-process lower bound, macOS's static working-set budget) that today live only in `run_fits`'s doc comment. `format_free`'s own doc already names this exact use case ("if I load this model now, will it fit?"). Gated on a library consumer (not just `hmn` CLI users) asking for it; open design question if it ships — a bare `bool`, or also the headroom/shortfall margin `hmn fits`'s own message computes.
- **`hmn watch` follows a wrapper PID's GPU-holding descendants** (surfaced by v0.2.13's askesis
  report, observation 1) — a launcher holds its wrapper script's PID (`run_stages.sh`), which holds
  no GPU memory, while its child (`canvas`) does. v0.2.13 only warns about an explicit PID that
  names no running process, deliberately guessing nothing. The principled extension: when an
  explicit PID exists but holds no GPU memory and has GPU-holding descendants, watch those and say
  so (`pid=15503 holds no GPU memory; watching its descendant pid=15534 (canvas)`). Needs parent-PID
  data in the library on three platforms (Linux `/proc/<pid>/stat`, the `th32ParentProcessID`
  the Windows snapshot already returns, macOS `proc_pidinfo`) and a rule for several GPU-holding
  descendants. `hmn ps --filter` already answers the launcher's question by name; gated on a
  workload whose GPU process has no stable name.
- ~~**`PsFilters` struct for `hmn ps`**~~ — *Done in v0.2.13*, triggered as predicted: `hmn ps --filter`, the fourth `ps` filter, was requested by askesis's spill-verdict dogfooding report, so the refactor landed first, byte-identical (`docs/roadmap-v0.2.13.md`).
- **Text-table widths in characters, not bytes** (surfaced by v0.2.12's duplicate-code audit) — `hmn ps` / `hmn watch` tables size columns with `str::len` (bytes) but pad with `{:<w$}` (characters), so a row with a non-ASCII process name is over-padded and its later columns drift right. Kept as is in v0.2.12 at the maintainer's call, since any fix changes both commands' output; measuring in `chars()` fixes accented Latin names, display width (e.g. `unicode-width`, a new dependency) would also align East Asian wide characters. Un-gated by a user with non-ASCII process names who finds the drift in the way. The v0.2.14 field check re-surfaced it for CJK and emoji names, which macOS `p_comm` names can carry; the only non-ASCII text it measured was the header's `Δ`, so wide CJK and emoji names are untested.

---

## Carried forward (out of scope until specifically un-gated)

| Idea | Why deferred | What would un-gate it |
|------|--------------|----------------------|
| **AMD `ROCm` backend** (`rocm_smi_lib`) | Maintainer has no AMD dGPU; shipping untested `FFI` violates project discipline | Hardware access **or** a contributor PR with maintainer hardware coverage |
| **AMD iGPU on Linux** | `NVML` doesn't see it; needs separate Linux `DRM` / `sysfs` path | Same as AMD `ROCm` |
| **Intel Arc / Intel iGPU on Linux** | No backend in the crate; same Linux-`DRM` problem | Hardware access or contributor PR |
| **Apple Metal on Intel Macs** (legacy `AMD` / Intel discrete GPUs) | v0.2.3's `ledger` mechanism likely works, but no Intel-Mac test hardware | Intel-Mac test machine or contributor PR |
| **Strict-accounting `D3DKMTQueryStatistics` Windows backend** | "Reserved for system use. Do not use." per Microsoft docs; undocumented kernel-thunk surface | A real consumer who reports KB 4490156 drift biting their specific workload |
| ~~**`hmn watch` (TUI live-refresh)**~~ | *Resolved in v0.2.6* — but not as a TUI. The rejected item was an `nvtop`-style curses redraw dashboard; `hmn watch [PID...]` is a `time(1)`-style scrolling sampler (same discipline as `hmn spill`), un-gated by the rhyme-mdlm dogfooding report that tried the `hmn ps`-diff-by-eye workaround and explained why it was insufficient. See [`docs/roadmap-v0.2.6.md`](docs/roadmap-v0.2.6.md). | — |
| **`hmn` reading from another machine over SSH / RPC** | Out of scope; users run `ssh host hmn` | Not planned |
| **TUI / live mode (`hmn top`)** | That's `nvtop`'s job — `hmn watch` (v0.2.6) is deliberately not this: no redraw, no cursor control, plain scrolling output | Not planned |
| **Name the GPU clients a sandbox hides** (IORegistry `AGXDeviceUserClient`, needs an IOKit binding) | The entries survive a sandbox with pid, name and GPU time but no bytes, so they would be rows with no VRAM figure, and the crate has no IOKit FFI | A sandboxed consumer who needs to know which process holds the GPU, beyond the unreadable count v0.2.14 adds |

`#[non_exhaustive]` keeps every one of these additive — none requires a 1.0 bump.

---

## Per-release detail (index)

- [`docs/roadmap-v0.2.0.md`](docs/roadmap-v0.2.0.md) — shipped 2026-05-06. *Wider, not taller.* `Snapshot::all`, `gpu_processes`, `hmn` CLI, `report`-feature `format_free` / `print_free`.
- [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) — shipped 2026-05-13. *Sharper, not wider.* `test-helpers` builder, `name_or_unknown`, `format_total` / `format_used`, `HypomnesisError` `Display` contract, README "Used by" + brief refresh.
- [`docs/roadmap-v0.2.2.md`](docs/roadmap-v0.2.2.md) — shipped 2026-06-02. *Truer, not wider.* Windows `PDH` per-process backend, `?`-row security-relevant hint, PID 4 rendered as `[kernel]`.
- *v0.2.3 — no separate per-release document; the [PR #1](https://github.com/PCfVW/hypomnesis/pull/1) body served as the per-release roadmap.*
- [`docs/roadmap-v0.2.4.md`](docs/roadmap-v0.2.4.md) — shipped 2026-06-29. *The same total. Now with the carve-out shown.* NVML v2 `reserved` carve-out surfaced as additive `GpuDeviceInfo::reserved_bytes`, `hmn` summary parenthetical, pre-R510 graceful fallback.
- [`docs/roadmap-v0.2.5.md`](docs/roadmap-v0.2.5.md) — shipped 2026-07-22. *Resident, not committed. Episodes, not a boolean.* `WDDM` spill detection: `PDH` `Shared Usage` residency gauges, `SpillTracker` with `is_spilling()` / `has_spilled()` split + episode-based `SpillReport`, `GpuProcessEntry::shared_used_bytes` + `hmn ps` SHARED column, `hmn spill -- <command>` wrapper with exit-code pass-through. Threshold default live-tuned to 85% via a forced-spill fixture.
- [`docs/roadmap-v0.2.6.md`](docs/roadmap-v0.2.6.md) — shipped 2026-07-25. *Not a TUI. Same tracker, a timer instead of a wrapped child.* `hmn watch [PID...]`: attach-to-a-running-PID spill triage, pure CLI addition over the unchanged `SpillTracker` / `gpu_processes()`, auto top-N PID selection, duration-string `--interval` / `--duration`, `0`/`1`/`2` exit-code contract, JSON Lines streaming, `ctrlc`-backed graceful Ctrl+C summary, best-effort PID-reuse baseline reset.
- [`docs/roadmap-v0.2.7.md`](docs/roadmap-v0.2.7.md) — shipped 2026-08-02. *Follow the work, not just the machine. Sort by the question you're actually asking.* `hmn watch --follow-new`: re-run top-N selection every interval, departed PIDs finalized into `per_pid[]` via a new `WatchState` `seen_order` roster instead of frozen at attach. `hmn ps --sort <dedicated|shared|total>`: a shared `ps_row_comparator` between `hmn ps` and `hmn watch`'s auto-selection. GitHub org transfer to `mi-for-the-rust-of-us`.
- [`docs/roadmap-v0.2.8.md`](docs/roadmap-v0.2.8.md) — shipped 2026-08-04. *The tool you install should install. The name you can't be shown, someone else can.* `cli` becomes a default feature; a `CreateToolhelp32Snapshot` fallback (Windows-only, batched once per `gpu_processes()` call) collapses most `?` rows to real names non-elevated; `[exited]`/`[protected]` brackets replace the anonymous `?` for what remains, with the protected count excluding `[exited]`; `hmn ps --sort vram`/`committed` aliases.
- [`docs/roadmap-v0.2.9.md`](docs/roadmap-v0.2.9.md) — shipped 2026-08-12. *The same total. Now with the driver behind it.* `GpuDeviceInfo::driver_version: Option<String>`, sourced from `NVML` (`nvmlSystemGetDriverVersion`) and the `nvidia-smi` fallback, driven by a `candle-mi` dogfooding report whose provenance log stamped the Rust toolchain per run but not the GPU driver. Rendered on the `hmn` device-summary line and via a new `hmn --json` flag on the default subcommand — no JSON surface existed there before.
- [`docs/roadmap-v0.2.10.md`](docs/roadmap-v0.2.10.md) — shipped 2026-08-17. *Audited, not assumed.* A full-codebase self-audit read as a first-party dogfooding report under Principle 1: three silent-failure fixes (`hmn ps`'s 64-process `NVML` cap, `DXGI` adapter-walk abort-on-one-bad-adapter, `hmn`'s no-subcommand silent/`[]` output), a `macos-latest` CI leg closing a three-release-old blind spot (and catching two real bugs in `src/gpu/metal.rs` on its first two runs), a `publish.yml` tag/version guard, and a batch of stale-documentation corrections.
- [`docs/roadmap-v0.2.11.md`](docs/roadmap-v0.2.11.md) — shipped 2026-09-15. *`hmn ps` learns to say "spilling" instead of making the operator infer it.* A `candle-mi` dogfooding report's four asks, in priority order: `hmn ps` SPILL column / `spilling` JSON field (`hypomnesis::snapshot_is_spilling`, honestly `null`/`?` rather than `false`/`no` when unmeasurable — a contract `hmn watch`'s own `spilling` field now shares); `hmn watch --json` `wall_clock` field (dependency-free `iso8601_utc_millis`); `hmn ps --min <SIZE>`; `hmn fits <SIZE>` headroom predicate. Two independent post-ship review passes found and fixed a further batch of issues, most notably a narrow `Some(false)`-when-unmeasurable gap in the new SPILL honesty contract and a `wall_clock`/`t_ms` skew on `watch`'s first sample.
- [`docs/roadmap-v0.2.12.md`](docs/roadmap-v0.2.12.md) — shipped 2026-09-26. *Clean the base first, then teach `hmn watch` to follow a process by name.* Part 1 (done 2026-09-26): the nine items of the 2026-09-26 duplicate-code audit, one commit each (one `DXGI` robustness fix, eight behaviour-preserving refactors including a per-subcommand split of `src/bin/hmn.rs`), plus `PDH` error messages reworded to `CONVENTIONS.md`'s form and a mechanical consistency pass. Part 2: `hmn watch --filter` / `--min`, criterion on the header line, and a `start` record opening `--json`, from a candle-mi dogfooding report.
- [`docs/roadmap-v0.2.13.md`](docs/roadmap-v0.2.13.md) — shipped 2026-09-30. *Stop `hmn watch` from reporting a spill check it never ran, and let `hmn ps` say who is being paged.* The askesis spill-verdict dogfooding report: the `hmn watch` unmeasurable-summary fix; `hmn ps` `PAGED`/`device` cells and a once-stated device verdict, `--filter`, `--exit-status`, repeatable `--pid`, `--device` range errors; `process_exists`; aligned `watch` rows; `--help` reordered.

Foundational documents (not per-release):

- [`docs/hypomnesis-brief.md`](docs/hypomnesis-brief.md) — *why this crate exists*: Plato + the v0.1.x VRAM saga + the extraction rationale from `candle-mi`.
- [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) — `hf-fetch-model 0.10.1` dogfooding report (the basis of v0.2.1's wave list).

---

## Principles

1. **Every patch is informed by at least one real consumer's adoption experience.** Codified in v0.2.1's CHANGELOG intro; v0.2.2 follows it (driven by the `WDDM` `[N/A]` finding on a maintainer's RTX 5060 Ti); v0.2.3 follows it (driven by the contributor's actual macOS adoption); v0.2.4 follows it (driven by a `candle-mi` v0.1.16 dogfooding report asking for the NVML driver-reserved carve-out on the same RTX 5060 Ti); v0.2.5 follows it (driven by the maintainer's own `WDDM` spill scenario on the same RTX 5060 Ti / 16 GiB host, then course-corrected by a rhyme-mdlm dogfooding report's live commit-vs-residency false-positive before a line of code was written); v0.2.6 follows it (driven by a rhyme-mdlm dogfooding report's 15-hour field-validation campaign, which hit the "can't attach to an already-running PID" gap three separate times); v0.2.7 follows it (driven by a candle-mi dogfooding report running `hmn watch` against a 19-process sequential test suite, which found the per-PID auto-selection frozen at attach never saw any of the processes that caused the spills it correctly detected at the adapter level).
2. **Additive-by-default under `#[non_exhaustive]`.** New variants and fields land in patch releases. Type-shape changes (`u64 → Option<u64>`, etc.) are minor bumps, never patches.
3. **No new hardware backends without maintainer-accessible hardware or a contributor PR.** AMD `ROCm` and Apple Metal sat behind this gate until v0.2.3 (Apple Silicon via PR #1) un-gated half of it.
4. **Documented limitations beat papered-over half-fixes.** R570 `u64::MAX` sentinel, `WDDM` `NVML_VALUE_NOT_AVAILABLE`, KB 4490156 PDH drift, macOS cross-user `EPERM` — each is named in the source and README rather than hidden.
5. **`Display` is the default English one-liner; structured fields are canonical.** v0.2.1 Wave D's `HypomnesisError` contract — applies to every future error / measurement variant.
6. **One crate, one job.** *Tell you what's currently in this process's memory, precisely, across Windows, Linux, and macOS.* (macOS support shipped in v0.2.3 via [PR #1](https://github.com/PCfVW/hypomnesis/pull/1).) Anything that widens the job (system-wide free RAM, GPU temperature trends, live TUI, process termination) belongs in a different crate — see the *"Why no `hmn kill`?"* note in the README for the canonical example of scope discipline in action.

---

*Living document — update as plans evolve. Last revised 2026-09-30: **v0.2.13 shipped** (an askesis dogfooding report: `hmn watch`'s `no spill observed` where spill is not measurable, a bug since v0.2.6, fixed; `hmn ps` names the paged process and states the device verdict once, and gains `--filter`, `--exit-status`, a repeatable `--pid` and a `--device` range error, which triggered the `PsFilters` refactor; `process_exists` for a nonexistent-PID warning; aligned `watch` columns; `--help` reordered. Validating it found `hmn watch` blind to a spill already under way at attach — now stated, with the real fix un-gating the `spill_condition` item. Detailed plan: `docs/roadmap-v0.2.13.md`). Previous revisions: 2026-09-26 (**v0.2.12 shipped** (part 1 remediated the 2026-09-26 duplicate-code audit — `docs/audits/2026-09-26-duplicate-code-audit.md`; part 2 gave `hmn watch` identity-based selection — `--filter`, `--min`, the criterion on the header, a `start` record opening `--json` — from a candle-mi dogfooding report, and reviewing it found Linux process names cut to 15 bytes, now reported in full; detailed plan: `docs/roadmap-v0.2.12.md`)); 2026-09-15 (**v0.2.11 shipped** — a `candle-mi` dogfooding report's four asks, in priority order: `hmn ps` SPILL column / `spilling` JSON field via a new `hypomnesis::snapshot_is_spilling`, honestly `null`/`?` — never `false`/`no` — when unmeasurable, a contract `hmn watch`'s own pre-existing `spilling` field now shares too; `hmn watch --json` gains a dependency-free `wall_clock` field; `hmn ps --min <SIZE>`; a new `hmn fits <SIZE>` headroom predicate. Two independent post-ship code-review passes — the second an explicit fresh re-review at maximum effort across nine parallel angles — then found and fixed a further batch of real issues before this revision was called final: the new SPILL honesty contract could still collapse "can't tell" into "not spilling" in one narrow case; `hmn watch`'s first `--json` sample showed a genuine, live-measured `wall_clock`/`t_ms` timing skew; `hmn fits` could print a self-contradictory near-miss message; `hmn ps --min`'s summary line misreported a real sub-MiB filter as its documented no-op; `parse_size_bytes` silently saturated an out-of-range value instead of rejecting it. Three architectural suggestions from that review — a unified `spill_condition` core, `GpuDeviceInfo::fits()`, a `PsFilters` struct — were logged under `Speculative: v0.3.0` rather than implemented unilaterally. Detailed plan: `docs/roadmap-v0.2.11.md`); 2026-08-17 (**v0.2.10 shipped** — a full-codebase self-audit — read as a first-party dogfooding report under Principle 1 — found and fixed three silent-failure bugs sharing one shape: `hmn ps`'s `NVML` 64-process cap, `DXGI`'s abort-on-one-bad-adapter enumeration walk, and `hmn`'s no-subcommand silent/`[]` output on a partially-broken driver stack; closed a three-release-old CI gap (`macos-latest` never in the matrix despite macOS being first-class since v0.2.3); added a `publish.yml` tag/version guard; and corrected a batch of stale documentation (`hmn --help`, `gpu_processes` rustdoc, `ROADMAP.md`/`docs/roadmap-v0.2.8.md` publish status, `CONVENTIONS.md`'s feature list). A second adversarial review pass on the fixes themselves — independent finder agents plus a manual documentation-consistency sweep — found and closed four further issues before finalizing: `--json` shared the same silent-`[]` ambiguity the text-mode fix addressed but the first pass's own `CHANGELOG` entry incorrectly claimed didn't apply; the `NVML` retry path paid for a row-extraction copy even on hard-error returns; the debug-output trace lost NVML's reported-vs-captured count visibility on retry; and a defensive constant's justification cited an unverified Linux `pid_max` assumption. Full audit: `docs/audits/2026-08-17-codebase-documentation-audit.md`); 2026-08-12 (**v0.2.9 shipped** — `GpuDeviceInfo::driver_version: Option<String>` — driven by a `candle-mi` dogfooding report whose `resurrect.ps1` provenance log stamped the Rust toolchain per verification run but not the GPU driver, surfaced live when the reference machine hit a `DPC_WATCHDOG_VIOLATION` bugcheck mid-run and needed a driver update (`591.86` → `610.88`) to recover; sourced from `nvmlSystemGetDriverVersion` and, since `nvidia-smi` genuinely has the figure unlike `reserved_bytes`, the `nvidia-smi` fallback too; rendered on the `hmn` device-summary line and via a new `hmn --json` flag on the default subcommand, since no JSON surface existed there before); 2026-08-04 (**v0.2.8 shipped** — an askesis `canvas` dogfooding report against a rented RTX 5090 deploy found `cargo install hypomnesis` installing no binary and diagnosed the Windows `?`-row problem — one diagnosis detail, the specific `OpenProcess` query right, was checked against the source and a live test and found already-shipped-and-insufficient before implementation; the actual fix, `CreateToolhelp32Snapshot`, was implemented instead; a related correctness gap in pre-existing `hmn watch` PID-reuse/growth-hint logic was found and fixed while wiring in the new `[exited]`/`[protected]` bracket values; a follow-up consistency pass over the diff then found and fixed an older, adjacent gap — the protected count never caught the pre-`WDDM 2.0` `nvidia-smi` fallback's literal `?` name, uncounted since v0.2.2 — and a separate documentation pass, prompted by a direct question about whether the rendered `cargo doc` output had actually been checked (it hadn't, beyond "builds without warnings"), found and fixed a missed `README.md` "what's new" banner update, a stale security-note sentence made inaccurate by the `nvidia-smi` fix, and confirmed the rendered rustdoc HTML and every cross-referenced FAQ anchor were correct); 2026-08-02: **v0.2.7 shipped** (`hmn watch --follow-new` + `hmn ps --sort` — driven by a candle-mi dogfooding report against a 19-process sequential test suite; two independent two-agent conventions-plus-adversarial-correctness passes, one per feature, each finding and fixing a test that didn't actually discriminate the behavior it claimed to test plus several smaller issues; live validation via two sequential real `spillforge` forced-spill runs under `--follow-new`, both manually and via a new automated end-to-end test; same-release GitHub org transfer to `mi-for-the-rust-of-us`). Previous revisions: 2026-07-25 (v0.2.6 shipped — `hmn watch [PID...]`, same-day sequence: plan-mode design session resolving the rejected "TUI live-refresh" `hmn watch` item into a non-TUI `time(1)`-style sampler per a rhyme-mdlm dogfooding report; implementation as a pure CLI addition with zero library-surface changes; a two-agent conventions-plus-adversarial-correctness pass that found and fixed one compile-breaking test gap and added a best-effort PID-reuse baseline reset; live validation against the `spillforge` fixture via both an automated end-to-end test and manual dogfooding); 2026-07-22 (v0.2.5 shipped — `WDDM` spill detection, same-day sequence: morning scope revision correcting spill semantics to **residency, not commit** per the rhyme-mdlm dogfooding report of 2026-07-19 and adding transient-spill handling; implementation + live validation on the reference `RTX 5060 Ti` including a forced-spill fixture that tuned the dedicated-saturation default from the sketched ~95% down to the measured 85%); 2026-06-29 (v0.2.4 shipped; spill detection bumped one slot to v0.2.5, scope unchanged); 2026-06-13 (spill detection promoted from Speculative to Committed). Reviewer hint: for **shipped** details, the per-release roadmap (or PR body, for v0.2.3) is the authoritative source; for **forthcoming** plans, this document is the source until a per-release roadmap is drafted.*
