# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.14] - 2026-10-09

Answers a field check of v0.2.13 on Apple Silicon
([issue #3](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/3),
[report](docs/dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md)), contributed
by contributor [@LittleCoinCoin](https://github.com/LittleCoinCoin) in three PRs (#6, #7, #8). On macOS the sandbox, not process ownership, decides
what `hmn` can read: unsandboxed, every user's processes are listed, and the old `sudo` advice is
gone. Inside a sandbox, `hmn` now measures what is permitted and counts the rest
(`N unreadable — re-run outside the sandbox`), through a new `gpu_process_listing` and
`HypomnesisError::ProcessListDenied`, with `sysctl` enumeration where libproc is refused. On every
platform, `hmn ps` states each device it skips and exits `2` when none could be queried, where it
printed an empty table. The SPILL and `PAGED` cells read `n/a` where spill cannot exist, and
`process_exists(0)` finds `kernel_task`. Plan: [`docs/roadmap-v0.2.14.md`](docs/roadmap-v0.2.14.md).

### Added

- **macOS enumeration and names inside a sandbox that denies `process-info`**
  (`src/gpu/metal.rs`) — when `proc_listpids` is refused, `sysctl(KERN_PROC_ALL)`
  enumerates the processes. A name comes from `proc_pidpath`, and from the kernel's `p_comm`
  (cut at 16 bytes) only where `proc_pidpath` is refused, never for a process that is gone.
  A per-PID ledger read is bytes, denied, gone (`ESRCH`) or failed, and only a refusal (`EPERM`)
  counts as denied. Where libproc works, `proc_listpids` enumerates and `proc_pidpath` names.

- **`gpu_process_listing`, `GpuProcessListing` and `HypomnesisError::ProcessListDenied`**
  (`src/gpu/mod.rs`, `src/snapshot.rs`, `src/error.rs`) — `gpu_process_listing(device_index)`
  returns the rows `gpu_processes` returns and `denied_pids`, the PIDs whose GPU memory the
  platform refused to let the caller read, sorted by `pid`. On macOS it returns
  `ProcessListDenied { denied }` when at least one process was refused and none other than the
  caller's could be read; on Linux and Windows `denied_pids` is always empty and the error is
  never returned. The error's `Display` states the count and carries no remedy. `gpu_processes`
  keeps its signature and returns the same error; a partial denial, where the sandbox's own
  processes are readable, stays an `Ok` list.

- **`hmn ps` counts the processes a macOS sandbox hides** (`src/bin/hmn/ps.rs`, `format.rs`,
  `main.rs`) — the summary line reads `907 unreadable, 1 protected — re-run outside the
  sandbox` (one remedy for both counts; with nothing protected, `907 unreadable — re-run
  outside the sandbox`), and a refused process list is a failed device: `hmn: ps failed to query
  device 0: process list unreadable (N refused, none other than the caller's could be read) —
  re-run outside the sandbox` (` (skipped)` appended without `--device`), where a sandbox that
  denies only the ledger read printed `0 GPU processes found.`. `--pid N` counts only N among the
  refused processes, and `--exit-status` exits `2`, not `1`, for an empty listing in which a
  process the filters could match was unreadable. The count goes to stderr; `--json` and the table
  carry no field for it. On Windows and Linux the text is `N protected — re-run elevated for
  names`, and no process is ever refused there.

- **`hmn watch` notices the processes a macOS sandbox hides** (`src/bin/hmn/watch.rs`,
  `format.rs`, `main.rs`) — at attach, an explicit PID the caller was refused gets
  `hmn watch: pid=N is unreadable here; its rows will read 0 MiB — re-run outside the sandbox`,
  once, and is still watched; it is never also reported as naming no running process.
  `--follow-new` prints `hmn watch: device D: N unreadable — re-run outside the sandbox; they
  are not followed` once, and when nothing was selected, `found no GPU processes` carries the same
  count and remedy inside its parentheses, so the line says why. A refused
  process list at attach is `hmn: watch failed to query device D: process list unreadable (…) —
  re-run outside the sandbox`; the per-interval `sample failed` line repeats no remedy. On
  Windows and Linux no process is ever refused, so none of these lines appears.

### Changed

- **The SPILL and per-PID `PAGED` cells read `n/a` on Linux and macOS** (`src/bin/hmn/format.rs`,
  `ps.rs`, `watch.rs`) — where spill cannot exist, `hmn ps`'s and `hmn watch`'s SPILL cell and
  `hmn watch`'s closing per-PID `PAGED` cell now say `n/a` instead of `?`, which also means an
  unresolved name in NAME. `?` stays on Windows, where spill exists but cannot be read now
  (pre-`WDDM 2.0`, a non-NVIDIA adapter, a `PDH` hiccup, a build without `pdh`). The platform is
  decided at compile time through one core, `format::spill_cell_for`, that both cells share.
  Text output only: `--json` keeps `null`. From the v0.2.13 macOS field check, finding F6
  (`field_check_v0213/01-findings_v1.md` at `f03298a7bb`).

- **On macOS, `hmn ps` and `hmn watch` advise `re-run outside the sandbox`, not elevation**
  (`src/bin/hmn/format.rs`, `ps.rs`, `watch.rs`, `main.rs`) — the `hmn ps` summary's clause
  now reads `N protected — re-run outside the sandbox` and `hmn watch`'s growth hint reads
  `re-run outside the sandbox to identify`, where both advised an elevation that does not change
  what a macOS sandbox withholds. One compile-time selection, `format::remedy_text`, picks the
  text; the Windows and Linux text is unchanged, byte for byte. From the
  v0.2.13 macOS field check (`field_check_v0213/01-findings_v1.md` at `f03298a7bb`).

- **On macOS the sandbox, not process ownership, decides what `hmn` can read** (`README.md`,
  `docs/FAQ.md`, `ROADMAP.md`, rustdoc, `hmn --help`) — unsandboxed, `hmn ps` lists every user's
  processes with no elevation, so no macOS text advises `sudo`; README Limitations item 9 states
  what a sandbox refuses and what `hmn ps` then prints.
  From the v0.2.13 macOS field check (`field_check_v0213/01-findings_v1.md` at `f03298a7bb`),
  F1.

- **On macOS, `gpu_processes` returns an error where it returned an empty list**
  (`src/gpu/metal.rs`, `src/gpu/mod.rs`, `src/error.rs`) — when the `graphics_footprint` entry of
  the ledger template does not resolve, or when no other process's ledger read succeeds and none
  is refused, it returns `NoGpuSource`, where it returned an empty list; `hmn ps` then exits `2`,
  where it printed `0 GPU processes found.` and exited `0`, unsandboxed included. When the
  process list is enumerated but no process other than the caller's can be read, it returns
  `ProcessListDenied`, where it returned `NoGpuSource` or, under a sandbox that denies only the
  ledger read, an empty list. `gpu_process_listing` returns the same errors. `HypomnesisError` is
  `#[non_exhaustive]`, so a `match` on it has a wildcard arm.

### Fixed

- **`hmn ps --device 1` on an Apple Silicon Mac says the index is out of range**
  (`src/gpu/mod.rs`, `src/error.rs`) — it now prints `device index 1 out of range (have 1 devices)`,
  where it printed the `NoGpuSource` text naming four backends macOS lacks: `bounds_check` had no
  Metal arm, so `device_info`, `process_gpu_info` and `gpu_processes` fell through to
  `NoGpuSource`. The answer also holds in a sandbox that denies `process-info*`, since the Metal
  count comes from `sysctl`. On macOS the `NoGpuSource` text now names Metal, NVML and
  `nvidia-smi`; on Windows and Linux it is unchanged. From the
  v0.2.13 macOS field check (`field_check_v0213/01-findings_v1.md` at `f03298a7bb`), F2.

- **`hmn ps` states each device it skipped, and exits `2` when every device failed**
  (`src/bin/hmn/ps.rs`, `main.rs`) — without `--device`, a device whose query fails now prints
  `hmn: ps failed to query device N: … (skipped)`, and when every device it tried failed,
  `hmn ps` prints `hmn: ps: no device could be queried, so nothing could be listed` and exits
  `2` with no table, where it printed an empty table and exited `0` on every platform (the
  silent wrong answer `0 GPU processes found.`). `--json` prints nothing on that exit, not `[]`.
  With `--exit-status`, an empty listing that skipped a failed device exits `2`, not `1`: `1`
  means nothing matched on every device queried, and the job may sit on the skipped device. A
  host with no device to try is unchanged.

- **`process_exists(0)` on macOS answers `Some(true)`** (`src/gpu/metal.rs`, `src/gpu/kinfo.rs`,
  new) — `kernel_task` has no executable path, so `proc_pidpath` says `ESRCH` and PID 0 read as
  absent; `hmn watch 0` warned `names no running process`. When libproc gives no path,
  `process_exists` now asks `sysctl` `KERN_PROC_PID`, which finds it, so `hmn watch 0` no longer
  warns. A caller whose sandbox refuses libproc but allows `kern.proc` now gets `Some(true)` or
  `Some(false)` where it got `None`; with both refused the answer is `None` unless `proc_pidpath`
  said `ESRCH`, and a `kinfo_proc` record that does not fit `sysctl`'s buffer (`ENOMEM`) gives
  `None`. The record is read by a private `kinfo_proc` parser with no `unsafe`; its 648-byte
  layout (`p_pid` at 40, `p_comm` at 243) was verified on arm64 natively (Apple M3 Pro) and on
  `x86_64` under Rosetta 2, by the SDK header and a live read of the test process; native Intel
  hardware is untested. A zombie, exited but not yet reaped, now reads `Some(true)` (it read
  `Some(false)`), as on Linux. Issue #3, item 2 of
  [`docs/roadmap-v0.2.14.md`](docs/roadmap-v0.2.14.md).

### Documentation

- **The macOS limitation is stated once, and every other site points to it** (`README.md`,
  `docs/FAQ.md`, `src/lib.rs`, `src/gpu/mod.rs`, `src/bin/hmn/main.rs`) — README Limitations
  item 9 says that the sandbox, not process ownership, decides what `hmn` can read, that
  `hmn` measures what is permitted and counts the rest, and that `hmn ps` ends its summary
  with `N unreadable — re-run outside the sandbox`. The FAQ, the `--help` text and the
  rustdoc point to it. A bare `?` in the NAME column on macOS means both name lookups
  failed or the process is gone, and the capability table's Fallback cell reads
  `enumeration, names and lookups try libproc first, then sysctl`.

## [0.2.13] - 2026-09-30

Answers an askesis dogfooding report
([2026-09-28](docs/dogfooding-feedbacks/dogfooding-spill-verdict-wording-and-ps-filters.md)) from a
rented Linux RTX 5090 and a Windows RTX 5060 Ti. Its first finding was a bug present since v0.2.6:
where spill is not measurable, `hmn watch`'s summary said `no spill observed`. That is fixed, and
`hmn watch` now also says when it attached to a spill already under way, which its growth-based
verdict cannot count. `hmn ps` names the process being paged (`PAGED` / `device`) and states the
device's verdict once; `hmn watch` follows. `hmn ps` gains `--filter`, `--exit-status` and a
repeatable `--pid`, and an unlistable `--device` exits `2`. A new `process_exists` backs a warning
for a nonexistent explicit PID; `hmn watch`'s columns line up; `hmn --help` lists its commands
first. Plan: [`docs/roadmap-v0.2.13.md`](docs/roadmap-v0.2.13.md).

### Added

- **`hmn ps --filter <PATTERN>` — list processes by name** (`src/bin/hmn/ps.rs`, `main.rs`) —
  keeps only processes whose name contains the pattern, ignoring case, with exactly
  `hmn watch --filter`'s rule (both now call `ps::matches_any` and `ps::filterable_name`).
  Repeatable: a name matching any pattern qualifies. The patterns are echoed on the summary line
  (`1 GPU process found matching filter="canvas" (16.5 GiB committed total).`), and a process
  whose name cannot be resolved (`?`, `[protected]`, `[exited]`) is counted there
  (`2 unnamed not matched`) rather than dropped silently. A launcher that holds a wrapper
  script's PID can now ask "is my job on the GPU?" without a `pgrep` in front of `--pid`. From
  askesis's
  [2026-09-28 dogfooding report](docs/dogfooding-feedbacks/dogfooding-spill-verdict-wording-and-ps-filters.md),
  request 3.

- **`hmn ps --pid` is repeatable** (`src/bin/hmn/main.rs`, `ps.rs`) — `--pid A --pid B` lists
  a process matching any of them: a launcher's wrapper and its GPU child, or two chained runs.
  Repeated PIDs are dropped, and the summary echoes each once (`matching pid=15503,15534`). A
  comma-separated `--pid 1,2` is still rejected. Same report, request 6.

- **`hmn ps --exit-status`** (`src/bin/hmn/ps.rs`, `main.rs`) — opt-in: exit `1` when no process
  is listed, `0` when at least one is, as `pgrep` does. `hmn ps --filter canvas --exit-status ||
  echo "not on the GPU"` is then a one-line gate, as `hmn fits` already is for headroom. The
  default is unchanged: `hmn ps` exits `0` whether or not anything matched. Same report,
  request 4.

- **`hmn ps` says which process is being paged, and states the device's spill verdict once**
  (`src/bin/hmn/ps.rs`) — on a spilling device the SPILL column no longer repeats `SPILL` on
  every row, where `hmn.exe` at 0 MiB read exactly like the trainer being paged. The process
  being paged (its own SHARED at or above 256 MiB, the floor the spill condition itself uses)
  reads `PAGED`; the device's other processes read `device`; `no` and `?` are unchanged. The
  stderr summary states the verdict once per spilling device, with its evidence:
  `…; device 0 spilling: 154 MiB free, 2.1 GiB shared, 1 process paged.` Shared bytes and the
  paged count are summed over every process on the device, before any filter, so the verdict
  does not depend on what is displayed. `--json` rows gain `paged` (`true`/`false`/`null`) and
  `shared_share` (the row's fraction of the device's shared bytes, four decimals, or `null`),
  both `null` exactly when `spilling` is; `spilling` is unchanged. The mark means *paged*, not
  *cause*: the memory manager pages whatever it chooses (the askesis report's revision, citing
  [`dogfooding-spill-triage-watch-mode.md`](docs/dogfooding-feedbacks/dogfooding-spill-triage-watch-mode.md)'s
  third verdict). Live-validated on the RTX 5060 Ti with `tools/spillforge`: `spillforge.exe`
  at 423 MiB shared read `PAGED`, every other row `device`. Same report, request 2.

- **`hypomnesis::process_exists(pid) -> Option<bool>`** (`src/gpu/mod.rs`, `src/gpu/metal.rs`) —
  whether a PID names a running process, `None` when the platform cannot tell (never "no" by
  default). Linux reads `/proc/<pid>/status` and requires its `Tgid` to equal `pid`, so a thread
  ID is not taken for a process; Windows reuses the `Toolhelp32` snapshot `gpu_processes`
  already takes to name processes (`pdh` feature, no new `unsafe`); macOS calls `proc_pidpath`,
  where `ESRCH` means no such process (`metal` feature). Added for `hmn watch`'s warning about a
  nonexistent explicit PID. Same report, observation 1.

- **`hmn watch` warns when an explicit PID names no running process** (`src/bin/hmn/watch.rs`) —
  `hmn watch 999999` ran silently as if the PID existed, printing `0 MiB` rows, the same as a
  process that exists but holds no GPU memory yet (a trainer between stages). At attach, each
  explicit PID absent from the first sample is checked with `process_exists`, and one that
  names no running process gets `hmn watch: pid=999999 names no running process; its rows will
  read 0 MiB` on stderr. It is still watched, and the exit code is unchanged; a PID the platform
  cannot judge gets no warning. Nothing is guessed in its place: PID numbers carry no
  similarity. Same report, observation 1.

- **`hmn watch` names the paged process too** (`src/bin/hmn/watch.rs`, `format.rs`, `ps.rs`) —
  with the same rule and vocabulary as `hmn ps`: while the adapter spills, a row whose own SHARED
  is at least 256 MiB reads `PAGED` and the others `device`; `no` and `?` are unchanged.
  `--json` samples gain `paged` (`null` exactly when `spilling` is), `per_pid[]` entries gain
  `paged` (paged in at least one interval, `null` when spill was not measurable), and the text
  per-PID summary gains a `PAGED` column (`yes`/`no`/`?`). One cell renderer
  (`format::spill_cell`) and one rule (`ps::paged_verdict`) now serve both commands, so they
  cannot disagree about who is being paged. Found after the consistency pass: fixing `hmn ps`
  alone left `hmn watch` answering the same question the old way. Live-validated with
  `tools/spillforge`: attached before the spill, `spillforge.exe` read `PAGED` from its first
  spilling interval and `yes` in the summary.

- **`hmn watch` says when it attached to a spill already under way** (`src/bin/hmn/watch.rs`) —
  `watch` measures spill as shared-memory growth above its first sample, so a spill already under
  way at attach went into the baseline: the rows read `no` and the summary `no spill observed`
  while `hmn ps` said the device was spilling — a negative it did not measure, in `watch`'s main
  use (attach to a job that already looks slow). Present since v0.2.6; the library documents it
  ("start the tracker before the workload"), `hmn watch` did not. At attach, `watch` now takes
  `hmn ps`'s one-snapshot verdict and, if the device is already spilling, warns once (`device 0 is
  already spilling at attach (2.1 GiB shared); spill is measured as growth from here, so this
  spill will not be counted — hmn ps shows it`); the closing summary repeats it, and the `--json`
  summary gains `spilling_at_attach` (`true`/`false`/`null`). No verdict or exit code changes;
  counting such a spill needs the tracker itself to change (`ROADMAP.md`). Found live while
  validating `PAGED` in `watch`.

### Changed

- **`hmn --help` lists its commands before the per-platform Limitations** (`src/bin/hmn/main.rs`)
  — the Limitations, most of the text, moved from clap's `long_about` to `after_long_help`, so
  the command list is no longer several screens down; `hmn -h` gains a line saying where they
  are. No text was removed: sorted, the old and new `--help` outputs are line-for-line equal.
  Same report, observation 3.
- **`hmn ps`'s filters are one `PsFilters` value** (`src/bin/hmn/ps.rs`) — the `ROADMAP.md`
  refactor gated on a fourth `ps` filter being requested, which `--filter` is. It decides which
  rows are listed and words the summary's `matching …` clause, so the two cannot disagree.
  Byte-identical output.

### Fixed

- **`hmn watch`'s text rows line up under the column header** (`src/bin/hmn/watch.rs`,
  `format.rs`) — the header was printed once with fixed widths, while each interval's rows were
  sized to that interval's own cells, so a short name such as `canvas` pulled every later column
  left of its heading, and a long one such as `spillforge.exe` pushed them right. `Table` gains
  minimum column widths, and one `watch_table()` builds both the header line and every
  interval's rows from the same widths: `PID` now fits 7 digits (Linux's default `pid_max` is
  4 194 304), and `NAME` is as wide as the longest name watched at attach, 12 at least. Names
  are never cut. Only under `--follow-new` can a longer name enter later, widening its column for
  that interval. With short names the header differs from before only by one space after `PID`.
  Present since v0.2.6, on every platform. Same report, observation 2.
- **`hmn ps --device <N>` with an index it cannot list is an error** (`src/bin/hmn/ps.rs`,
  `main.rs`) — `hmn ps --device 3` on a one-GPU machine exited `0` with an empty table and
  `0 GPU processes found matching device=3.`, so a mistyped index read as an idle card, while
  `hmn fits 1GiB --device 3` already exited `2`. It now exits `2` with
  `hmn: ps failed to query device 3: device index 3 out of range (have 1 devices)`, and likewise
  when the named device's query fails outright. Without `--device`, a failing device is still
  skipped so one broken device does not hide the others. `run_ps` now returns the exit code
  itself, like `run_fits` and `run_watch`. Same report, request 5.
- **`hmn watch` no longer reports a spill check it could not run** (`src/bin/hmn/watch.rs`) —
  where spill is not measurable (every Linux and macOS run, and Windows without a usable adapter
  counter set), the text summary printed the all-zeros spill report: `peak dedicated 0 MiB` and
  `episodes 0 — no spill observed`, a measured-looking negative, while the `--json` summary said
  `"measurable": false`. It now prints `hmn watch: spill not measurable on this platform; per-PID
  VRAM below`, as `hmn spill` already did, then the per-PID table. Present since v0.2.6: the
  formatter checked only whether a tracker existed, never whether it could measure, and the test
  named for the unmeasurable case covered only the no-tracker one. The exit code is unchanged
  (`0` when no spill was observed, measurable or not). From askesis's
  [2026-09-28 dogfooding report](docs/dogfooding-feedbacks/dogfooding-spill-verdict-wording-and-ps-filters.md),
  request 1.

### Security

- **Release pipeline hardened against registry-token theft**, the root cause
  of the 2026-03-24 LiteLLM supply-chain compromise. Releases now publish
  through crates.io Trusted Publishing (OIDC), so no long-lived
  `CARGO_REGISTRY_TOKEN` exists to steal. The publish job needs maintainer
  approval through a `release` environment, runs only from a `v*` tag and
  restores no build cache. Every third-party GitHub Action is pinned to a full
  commit SHA, and Dependabot keeps the pins current. A new `deny` CI job runs
  `cargo-deny` against `deny.toml` (RustSec advisories and yanked crates,
  permissive licences only, crates.io as the only source).
- **No dependency code runs while the release job can mint a publish token.**
  The first Trusted Publishing workflow ran the tests (and so every
  dependency's build scripts and proc-macros) in the job that held
  `id-token: write`. `publish.yml` is now two jobs: `verify` builds, tests and
  packages with a read-only token, and `publish` holds `id-token: write` and
  runs `cargo publish --no-verify`, which compiles nothing. Every cargo command
  in `publish.yml` and `ci.yml` uses `--locked`.

## [0.2.12] - 2026-09-26

Two parts. Part 1 remediates the nine items of the
[2026-09-26 duplicate-code audit](docs/audits/2026-09-26-duplicate-code-audit.md), one commit
each. Part 2 answers a candle-mi dogfooding report
([2026-09-21](docs/dogfooding-feedbacks/dogfooding-watch-filter-by-identity.md)) whose
`hmn watch --follow-new --top 3` captures were 73.9% desktop rows: `hmn watch` now selects by name
(`--filter`) and size (`--min`), says which criterion is active, and opens every `--json` capture
with a `start` record — the release's one deliberate wire-format addition. On Linux, process names
are no longer cut to 15 bytes. Plan: [`docs/roadmap-v0.2.12.md`](docs/roadmap-v0.2.12.md).

### Added

- **`hmn watch --filter <PATTERN>` — follow a program by name, not by VRAM rank**
  (`src/bin/hmn/watch.rs`, `main.rs`) — auto-selection (one-shot, or re-run every interval under
  `--follow-new`) now considers only processes whose name contains the pattern, ignoring case,
  then keeps the top `--top` of those. Repeatable: a name matching any pattern qualifies.
  `hmn watch --follow-new --filter figure13_newline_patch --json` records only the workload,
  where `--top 3` alone recorded 73.9% desktop rows in candle-mi's Figure-13 captures. The active
  patterns appear on the stderr header line (`following top 3 by committed among names containing
  "figure13" (case-insensitive) …`), so a committed capture says how it was selected; without
  `--filter` the header is unchanged. Name resolution is guarded both ways: a followed process
  whose name briefly reads `[protected]` or `[exited]` keeps matching on the last name it resolved
  to, and a process whose name never resolves is announced once on stderr (`pid=N has no
  resolvable name; --filter cannot match it`) instead of being dropped silently. Combined with
  explicit PIDs it is a hard error (exit `2`), like `--follow-new`; a blank pattern is rejected.
  From candle-mi's
  [2026-09-21 dogfooding report](docs/dogfooding-feedbacks/dogfooding-watch-filter-by-identity.md),
  requests 1 and 3.
- **`hmn watch --min <SIZE>` — a size floor for auto-selection** (`src/bin/hmn/watch.rs`,
  `main.rs`) — considers only processes whose total footprint (`used_bytes + shared_used_bytes`)
  is at least SIZE, with the same definition and SIZE syntax as `hmn ps --min` (both now call
  `footprint_bytes`). Applied before `--filter`, so the unresolved-name notice skips processes too
  small to matter; re-applied every interval under `--follow-new`; shown on the header line (`…
  with footprint >= 2 GiB`). Combined with explicit PIDs it is a hard error (exit `2`). As the
  report itself found, a size floor is a proxy for identity rather than a substitute — useful for
  headroom questions and alongside `--filter`. Same report, requests 2 and 3.
- **`hmn watch --json` opens with a `{"kind":"start",...}` record** (`src/bin/hmn/watch.rs`) —
  written once at attach, before the first sample: `hmn_version`, the invocation (`argv`), the
  device and its name, `interval_ms`, `duration_ms`, and the `selection` (`mode` — `explicit`,
  `top` or `follow_new` — explicit `pids`, `top`, the `--filter` patterns as typed, `min_bytes`),
  rendered from the same `Selection` value as the stderr header. A capture now describes itself
  even when its `.err` stream is discarded, and one with a `start` record but no closing
  `summary` is known to have been cut short (hard-killed, or copied mid-run) — which the file
  alone could not show before. `t_ms` is `0` and `wall_clock` is the first sample's instant.
  `argv[0]` is reduced to its file name (`hmn.exe`, not the full path): these captures are
  committed to public repositories, and the full path would publish the operator's user name.
  **The one change to the default `--json` stream in this release** — additive, and no known
  consumer reads the stream positionally (this repo's own tests select records by `kind`, as
  the README describes), but a script that assumed line 1 is a `sample` must now skip the
  `start` record. Verified live by a new `#[ignore]` test, `tests/live_watch_filter.rs`, which
  runs `--follow-new --top 3 --filter SpillForge` over a real `spillforge` run: the `start`
  record comes first and carries the filter, and every sample and `per_pid` entry is
  `spillforge.exe` — the report's own regression case, 100% workload rows. Same report,
  observation 1.
- **Tests — `SpillReport` JSON key parity across every emitter** (`src/bin/hmn.rs`) — the
  adapter-level `SpillReport` object is spelled out by four independent emitters (`hmn spill
  --json` measurable and no-tracker, `hmn watch --json` summary measurable and no-tracker), and
  the existing shape tests pinned only each string's first fields and last. Three new tests pin
  the whole ordered key list against one canonical `SPILL_REPORT_JSON_KEYS`, via a test-only
  depth-aware key scanner that ignores nested `episodes[]` / `per_pid[]` keys (itself tested).
  Verified to discriminate: swapping two middle keys in one emitter, or renaming one, fails the
  new tests while every pre-existing test still passes. Audit item 1/9.
- **Tests — `spill_cell` pins the SPILL-column honesty contract** (`src/bin/hmn.rs`) — `SPILL` /
  `no` / `?`, asserted once for both surfaces that render it. Audit item 2/9.

### Changed

- **One SPILL-cell mapping for `hmn ps` and `hmn watch`** (`src/bin/hmn.rs`) — the `Some(true)` →
  `SPILL` / `Some(false)` → `no` / `None` → `?` mapping (the v0.2.11 honesty contract) and its
  justifying comment were written out twice, once per table; both now call one `const fn
  spill_cell`. Output unchanged. Audit item 2/9.
- **One MiB conversion and one adapter-name suffix for the `report` formatters** (`src/snapshot.rs`,
  `src/report.rs`) — the `bytes / 1_048_576` cast (with its `// CAST:` and `#[allow]`) was written
  seven times and the ` [<adapter name>]` suffix four times, across `Snapshot::ram_mb` / `vram_mb`,
  `GpuDeviceInfo::format_free` / `format_total` / `format_used` and
  `MemoryReport::format_before_after`. Each now exists once, as crate-private `bytes_as_mib` and
  `GpuDeviceInfo::name_suffix`. Output unchanged. `Snapshot::ram_mb` becomes a `const fn` as a
  consequence (additive: every existing call still compiles). Audit item 2/9.
- **One `nvidia-smi` spawn** (`src/gpu/nvidia_smi.rs`) — `query` and `query_compute_apps` each
  repeated the same subprocess spawn, exit-status check and four-arm `cfg(debug-output)` diagnostic
  `match`, differing only in the `--query-*` argument. Both now call `run_smi`. Under
  `debug-output`, the `--query-gpu` path's trace names its query the way the compute-apps trace
  already did (`[nvidia-smi debug] --query-gpu for idx=…`). Audit item 5/9.
- **One `PdhOpenQueryW`, one adapter-`LUID` lookup** (`src/gpu/pdh.rs`) — the open-query-into-guard
  sequence (with its `unsafe` block and SAFETY justification) and the `DXGI` `LUID` lookup with its
  error message were each written twice, for the per-process listing and the adapter spill query.
  Now `QueryGuard::open()` — which also makes the guard's "handle came from a successful open"
  invariant, relied on by its `Drop`, hold by construction — and `target_luid()`. Error messages
  unchanged. Audit item 5/9.
- **One writer for the `SpillReport` JSON contract** (`src/bin/hmn.rs`) — the nine-field
  adapter-level object was spelled out four times: the `SPILL_JSON_UNMEASURABLE` constant,
  `format_spill_json`, and both arms of `format_watch_summary_json`. All four now go through one
  `write_spill_report_fields(out, Option<&SpillReport>)`; `format_spill_json` takes an `Option`
  and the constant is gone. Verified byte-identical: all four outputs, captured before and after
  from the same fixtures, match to the byte (1,617 bytes), and the no-tracker test now asserts the
  exact former constant rather than its prefix and suffix. Audit item 6/9.
- **`src/bin/hmn.rs` split per subcommand into `src/bin/hmn/`** — the 4,609-line file (2,444
  production + 2,165 test lines when split; 3,618 lines two releases earlier) becomes `main.rs` (the `clap`
  definitions and dispatch), one module per subcommand (`summary.rs`, `ps.rs`, `spill.rs`,
  `watch.rs`, `fits.rs`), `format.rs` for the primitives several share (byte units, durations and
  timestamps, JSON escaping, table widths, SPILL glyphs, the `--interval` / `--min` / `fits`
  value parsers), and a `cfg(test)` `test_support.rs` for fixtures used by more than one module's
  tests. A pure move, done by script along the file's existing section banners: every one of the
  173 bin tests keeps its name (sorted name lists identical before and after), `--help` for `hmn`
  and every subcommand is byte-identical to a build of the previous commit, and `Cargo.toml` is
  unchanged (Cargo discovers `src/bin/hmn/main.rs`). Items shared across modules are `pub`
  inside their private module — clippy's `redundant_pub_crate` rejects `pub(crate)` /
  `pub(super)` directly under a binary's root. Audit item 7/9.
- **One `DXGI` adapter walker** (`src/gpu/dxgi.rs`, `CONVENTIONS.md`) — the six functions that
  each carried their own `CreateDXGIFactory1` + `EnumAdapters1` loop (`query`, `adapter_name`,
  `adapter_luid`, `adapter_dedicated_video_memory`, `enumerate_non_nvidia`, `device_count`) now
  share `walk_adapters`, a visitor-closure walker that owns the skip-a-bad-adapter policy (the
  behaviour item 3/9 brought to all six) and keeps COM pointers inside its own frame per
  `CONVENTIONS.md` Pattern 3. Thin `nth_nvidia_adapter` / `for_each_adapter` wrappers serve the
  per-index lookups and the exhaustive walks; `is_nvidia_dgpu`, `description` and `bytes` replace
  five copies of the NVIDIA filter, three of the UTF-16 name trim and four `usize → u64` casts.
  The file shrinks from 578 to 434 lines while gaining a module-doc section on the walker. Each
  function's factory-failure result is unchanged. Verified behaviour-preserving: all 16 ignored
  live `DXGI`/`PDH` tests pass, and their `debug-output` `DXGI` traces are identical to a build of
  the previous commit. The skip trace no longer names which lookup was walking (there is now one
  walker). `CONVENTIONS.md` Pattern 3 now routes all new enumeration through the walker. Audit
  item 8/9.
- **`NvmlSession` makes `NVML` init/shutdown pairing structural** (`src/gpu/nvml.rs`,
  `CONVENTIONS.md`) — `query`, `list_compute_processes` and `device_count` each hand-rolled the
  library load, the `nvmlInit_v2` / `nvmlShutdown` symbol lookups and init, then placed a
  `shutdown()` call on every later return path, guarded by a comment repeated in each: *"From
  here, every return path MUST call shutdown."* The pairing was correct (re-verified by the audit);
  it is now guaranteed by a type instead. `NvmlSession::open` returns a session only after a
  successful init, and its `Drop` calls `nvmlShutdown` exactly once, on every path. The entry
  points end their session with an explicit `drop(session)` at the same point they used to call
  `shutdown()`, so call order is unchanged, and because every resolved symbol borrows the session,
  an `NVML` call after that line no longer compiles (verified by mutation: `E0505`). Verified
  behaviour-preserving: `debug-output` `NVML` traces from the live tests are identical to a build
  of the previous commit, and all 9 live-GPU tests pass on Windows and on Linux (WSL2), where
  `list_compute_processes` runs. Two incidental differences: an entry point's own symbols are now
  resolved after `nvmlInit_v2` rather than before, so a missing symbol costs one balanced
  init/shutdown pair before the same `None`; and the init-failure trace names its caller for
  `query` too. Audit item 9/9 (part 1).
- **One column-table renderer** (`src/bin/hmn/format.rs`) — `hmn ps`'s listing, `hmn watch`'s
  interval rows and its closing per-PID block were three hand-rolled renderers of one shape: a
  `Vec` of cells per column, a `column_width` call per column, a six- or seven-deep `zip` chain and
  a `writeln!`. All three now build a `Table` and `render` it, differing only in their headers,
  cells and line prefixes. Verified byte-identical, not just shape-identical: all three renderers,
  run on fixtures including multibyte process names and empty inputs, produce the same 1,247
  bytes before and after, and every exact-string formatter test passes unchanged. Two new tests
  pin `Table`'s own contract (last column padded; a header-less render still sizes columns to
  their headers). Audit item 9/9 (part 2).
- **`PDH` error messages follow the crate's error-wording convention** (`src/gpu/pdh.rs`) — the
  `HypomnesisError::Pdh` messages read `"<Api> failed: 0x…"`; they now take the `CONVENTIONS.md`
  validation form `<noun> <problem> (<context>)` that `ram.rs`'s status-code failures already
  use, e.g. `PdhOpenQueryW failed (PDH_STATUS = 0xC0000BB8)` and `PdhEnumObjectItemsW failed
  (size query for GPU Process Memory, PDH_STATUS = 0x…)`. Visible through `HypomnesisError`'s
  `Display`; no code, test or document matched on the old text. Follow-up from the audit
  remediation.
- **Consistency pass over part 1** (`src/gpu/nvml.rs`, `src/gpu/pdh.rs`, `src/gpu/dxgi.rs`,
  `src/bin/hmn/`) — a mechanical re-check of everything the audit remediation touched found and
  fixed what the per-commit gates could not see: a `#[allow(unsafe_code)]` left on
  `AdapterMemQuery::open` after item 5 moved its only `unsafe` into `QueryGuard::open`; eleven
  `#[allow(clippy::missing_panics_doc)]` in the `hmn` modules that never suppressed anything (the
  lint applies only to exported functions, and a binary exports none — ten predate the audit, one
  was carried into new code by item 6); ten `unsafe` blocks in `nvml.rs` without a directly
  preceding `SAFETY` comment (now none, per clippy's `undocumented_unsafe_blocks`, on Windows and
  Linux); six intra-doc links in `nvml.rs` to items that exist on only one platform, which break
  the other platform's private-item docs (`CONVENTIONS.md`'s link-safety rule); three cfg
  silencers missing the `EXPLICIT` note `spill.rs` gives its own; `nvml.rs` helper docs still
  describing a hand-initialized `NVML` rather than an `NvmlSession`; and module docs not yet naming
  the new `Table` renderer. Found by converting every `#[allow]` in the touched files to
  `#[expect]` and compiling on Windows and Linux, stable and MSRV 1.88, default and all features
  — which also showed two suppressions needed only on MSRV, and kept. No behaviour change.
- **Test-module lint preambles trimmed to what each module uses** (`src/`, `CONVENTIONS.md`) —
  all fourteen `#[cfg(test)]` modules carried the same blanket
  `#[allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_docs_in_private_items)]`.
  Converting those preambles to `#[expect]` and compiling in twelve configurations — Windows,
  Linux and macOS (`aarch64-apple-darwin`, type-checked) × stable and MSRV 1.88 × default and all
  features — showed 33 of their 44 entries suppressed nothing anywhere they compile:
  `missing_docs_in_private_items` and `expect_used` in every module, `unwrap_used` in five. Those
  are removed; five modules now carry no preamble at all. The 11 that do fire stay
  (`unwrap_used` ×9, `panic` in `hmn`'s arg-parsing tests, `indexing_slicing` in `spill.rs`).
  `CONVENTIONS.md` now says test modules allow only what they trigger, and how to check it.
- **Dead lint allow on the live tests' `spillforge_path` helper** (`tests/live_watch*.rs`) — each
  of its three copies carried `#[allow(clippy::expect_used, clippy::panic)]`, but the helper
  fails through `assert!`, which neither lint covers; the same `#[expect]` check shows both
  unfulfilled. Removed; every other integration-test allow fires (checked on Windows, and for
  `macos_smoke.rs` via `aarch64-apple-darwin`).
- **`hmn watch` selection is one `Selection` value** (`src/bin/hmn/watch.rs`, `main.rs`) — the
  explicit / top-N / `--follow-new` modes, their argument validation and the stderr header's
  description of them now live in one type, ahead of v0.2.12 part 2's `--filter` / `--min`, so
  what a capture says it selected and what it actually selected are derived from the same value.
  The `--follow-new` + explicit-PIDs guard moves into `Selection::new`, still checked before any
  hardware call; its message, the three header clauses and exit codes are byte-identical (pinned
  by tests). `run_watch` drops from 7 parameters to 5. Part 2, item 1.
- **One definition of a process's total footprint** (`src/bin/hmn/ps.rs`) — `hmn ps --sort total`
  and `hmn ps --min` each spelled `used_bytes + shared_used_bytes`; both now call
  `footprint_bytes`, which `hmn watch --min` will reuse. Part 2, item 1.
- **One spelling of an optional JSON value** (`src/bin/hmn/format.rs`) — `hmn`'s hand-rolled
  `--json` emitters wrote "string or `null`" seven times (process and device names, driver
  version, episode end labels) and "`true`/`false`/`null`" or "number or `null`" three more, each
  as a four-line idiom — short enough to slip under the duplicate-code audit's six-line window.
  Now `json_string_or_null` and `json_value_or_null`, found while the `start` record was about to
  add further copies. Output byte-identical: every exact-string JSON test passes unchanged.
- **Part 2 consistency pass** (`src/bin/hmn/watch.rs`, `format.rs`, `main.rs`) — docs brought
  back in line with the code (`Selection`'s four consumers, `describe`'s byte-identity condition,
  `write_json`'s relation to the header, the sticky name's PID-reuse limit, a stale test comment);
  `unmatchable_notices`' `#[must_use]` now gives its reason; `parse_filter_pattern` joins the other
  clap value parsers in `format.rs`; new `json_string` for always-present JSON strings. No
  behaviour change.
- **The live tests' `spillforge_path` helper is shared** (`tests/common/mod.rs`) — with
  `tests/live_watch_filter.rs` it had reached three identical copies; `tests/live_watch.rs`,
  `live_watch_follow_new.rs` and `live_watch_filter.rs` now pull it in with `mod common;`. The
  summary-line lookup the audit flagged beside it (also three copies) moved there too, as
  `summary_line`, and the three tests' now-dead `clippy::panic` allows went with it.

### Fixed

- **Linux process names were cut to 15 bytes** (`src/gpu/proc_name.rs`, new) — `NVML` rows took
  their name from `/proc/<pid>/comm`, which the kernel truncates, so `figure13_newline_patch` was
  listed as `figure13_newlin` and `hmn watch --filter figure13_newline_patch` could never match
  it. A `comm` at the limit is now extended from the first longer name that starts with it: the
  `/proc/<pid>/exe` link's file name (same-user processes), else `argv[0]`'s from
  `/proc/<pid>/cmdline` (world-readable). Shorter names are unchanged. Found while reviewing
  part 2: WSL2's `NVML` reports no per-process rows, so this path had never run under `--filter`;
  two new unit tests read the real `/proc` of a long-named process and fail without the fix.
- **`DXGI` per-index lookups still aborted the whole adapter walk on one bad adapter**
  (`src/gpu/dxgi.rs`) — v0.2.10 fixed this in `enumerate_non_nvidia` and `device_count`, but four
  more walks with the same shape were missed: `query`, `adapter_name`, `adapter_luid` and
  `adapter_dedicated_video_memory`. A failed `IDXGIAdapter` cast or `GetDesc` on any adapter
  earlier in the raw enumeration order — an iGPU with a half-installed driver is the realistic case
  — made them return `None` for a healthy NVIDIA GPU behind it, silently losing the `DXGI` reading,
  the friendlier adapter name, the `PDH` adapter `LUID` match and the dedicated-capacity figure.
  They now skip the bad adapter and keep walking, exactly like the two fixed in v0.2.10. This also
  makes all six walks agree on NVIDIA index numbering: before, `device_count` skipped an unreadable
  adapter while the per-index lookups aborted on it. Failures on the *matched* adapter itself
  (`IDXGIAdapter3` cast, `QueryVideoMemoryInfo`) still return `None`, as before. Audit item 3/9.

### Documentation

- **`hmn watch --filter` / `--min` and the `start` record, documented** (`README.md`,
  `docs/tutorials/watching-a-running-job.md`, `docs/FAQ.md`) — the README's `hmn watch` section
  covers both flags, the header criterion and the new first `--json` record (with a
  `select(.kind == "sample")` note for scripts that assumed line 1 is a sample); the watch
  tutorial gains Step 5, "Follow one program, not the whole machine", built on real output from
  the reference machine; the FAQ gains "How do I make `hmn watch` record only my own job, not the
  desktop?". Part 2, item 5.
- **`MemoryReport` says what its `MB` means** (`src/report.rs`, `README.md`) — `ram_delta_mb`,
  `vram_delta_mb`, `format_delta` and `format_before_after` report and print `MiB`
  (`bytes / 1_048_576`) under an `MB` label kept for `candle-mi` parity. Every other `report`
  surface (`Snapshot::ram_mb`, the three `GpuDeviceInfo` formatters) already said so; these four
  now do too, as does the README's feature table. Names and output unchanged. Audit item 4/9.
- **Stale forward reference** (`src/snapshot.rs`) — `Snapshot::now`'s rustdoc still promised a
  long-lived `NVML` context "for v0.2", nine releases into 0.2.x; it now points at `ROADMAP.md`,
  where the item is speculative. Closes the half of the 2026-08-17 audit's item 3.4 that remained.
- **`test-helpers` manifest comment names all three builders** (`Cargo.toml`) — it named only
  `GpuDeviceInfoBuilder`; `GpuProcessEntryBuilder` and `SpillReportBuilder` are exposed too.
  Closes the 2026-08-17 audit's item 3.5.
- **`CHANGELOG.md` link references** — the bracketed version headings now resolve to GitHub
  compare views, as Keep a Changelog intends. Closes the 2026-08-17 audit's item 3.6.
- **`tests/live_watch_follow_new.rs`** — the comment justifying the duplicated `spillforge_path()`
  helper claimed Cargo has no lightweight way to share test helpers; `tests/common/mod.rs` is
  exactly that. The comment now names it and gives the real reason for not using it yet. Audit
  item 4/9.

## [0.2.11] - 2026-09-15

Driven by a candle-mi dogfooding report
([2026-09-14](docs/dogfooding-feedbacks/dogfooding-orphan-attribution-and-ps-spill-flag.md))
that field-validated v0.2.7 `watch --follow-new` (36 sequential processes, clean) and diagnosed
an 8× GPU slowdown via `hmn ps`'s SHARED column — two orphaned test binaries holding 8.6 GiB
plus the real job spilling 6.8 GiB into shared memory — but had to infer "spilling" by eye
because only `hmn watch` carried a spill signal. Four small, additive asks, in the report's own
priority order.

### Added

- **`hmn ps` gains a SPILL column / `spilling` JSON field** (`src/spill.rs`, `src/bin/hmn.rs`) —
  a new library function, `hypomnesis::snapshot_is_spilling(device_index) -> Option<bool>`,
  takes one live adapter-wide `PDH` sample (the same source `hmn spill`/`hmn watch` already use),
  sampled *before* the process listing so the SPILL verdict and the SHARED figures on the same
  row describe the same instant rather than straddling `gpu_processes()`'s own call duration, and
  applies a *single-snapshot* approximation of the v0.2.5 spill co-condition: adapter dedicated
  commit at or above the existing 85% threshold AND adapter shared-resident at or above the
  existing 256 MiB floor — an absolute floor rather than growth above a baseline, since a
  one-shot `ps` listing has no history to measure growth against. **Not equivalent** to
  `hmn watch`'s verdict for the same instant (see the function's rustdoc and the new `hmn --help`
  Limitations bullet). Computed once per device and broadcast to every row on it, matching
  `hmn watch`'s existing "same value on every row" shape. `ps --json` rows gain `"spilling":
  true|false|null` — `null`, never `false`, when spill isn't measurable here (non-Windows, built
  without the `pdh` feature, pre-`WDDM 2.0`, a non-NVIDIA adapter, a `PDH` hiccup, **or the
  adapter's dedicated capacity coming back unassessable**, a case the underlying threshold helper
  now propagates as `None` all the way through rather than silently reading as "measured, not
  spilling") — matching the crate's existing `measurable` honesty pattern (`SpillReport`,
  `is_spill_measurable()`). The text table gains a `SPILL` column: `SPILL` / `no` / `?` (the `?`
  — not `no` — for the unmeasurable case). `hmn watch`'s own SPILL column/`spilling` field gets
  the identical `Option<bool>` treatment in the same release, so the two commands' spill columns
  agree on what "can't tell" looks like.
- **`hmn watch --json` samples gain a `wall_clock` field** (`src/bin/hmn.rs`) — absolute UTC
  ISO-8601 with millisecond precision (`"2026-09-14T10:12:03.482Z"`), alongside the existing
  `t_ms` (relative to attach), captured at the same instant as `t_ms`'s own reference point —
  including the first sample, where the `t_ms`-zero `Instant` is now taken right alongside
  `wall_clock` rather than after the attach-time setup work (`SpillTracker::new`'s `PDH`
  enumeration, in particular) that used to sit between them. A new pure
  `iso8601_utc_millis`/`civil_from_days` pair formats it with no new dependency —
  proleptic-Gregorian civil-from-days integer arithmetic (Howard Hinnant's algorithm; Unix time
  has no leap seconds, so this is exact over every day count a real system clock can produce).
  The same value is shared across every row sampled in one interval. Lets a spill trace be joined
  against a log stamped with real local time (e.g. a training driver's own run log) mechanically
  instead of by hand-converting `t_ms` offsets.
- **`hmn ps --min <SIZE>`** (`src/bin/hmn.rs`) — hides rows below a total footprint
  (`used_bytes + shared_used_bytes`, not dedicated alone — "who is actually holding this card",
  matching `--sort total`'s definition), turning what used to need piping through `awk`/`jq` into
  a one-liner. A new shared `parse_size_bytes` parser (also used by `hmn fits`, below) accepts a
  bare byte count or a decimal number with `KiB`/`MiB`/`GiB`, the exact unit spellings `hmn`
  itself already prints (including the space `format_vram` always puts before the unit), so what
  the tool shows is always what it accepts back; an absurdly large value is rejected as a usage
  error rather than silently saturating to `u64::MAX`. `--min 0` is accepted as a valid no-op,
  unlike `--interval 0` elsewhere. The stderr summary line's filter clause composes
  `pid=`/`device=`/`min=` from a list now (was a fixed 2-filter match), so a fourth filter won't
  need another rewrite — and echoes `--min` through a new precise formatter rather than the
  table-column one, so a sub-MiB (or otherwise imprecisely-rounding) threshold is never
  misreported as the documented `--min 0` no-op.
- **`hmn fits <SIZE>`** (`src/bin/hmn.rs`) — a headroom predicate for gating a run script: exits
  `0` if `SIZE` fits in the target device's current free `VRAM` (`--device`, default `0`), `1` if
  it doesn't, `2` on a hard error (bad device) — deliberately parallel to `hmn watch`'s `0`/`1`/`2`
  contract. The message always states an exact headroom/shortfall margin, so a near-miss where
  `free`/`requested` round to the same displayed figure still reads unambiguously rather than as
  a self-contradiction. `free_bytes` nets out `reserved_bytes` on the NVML path only — the
  message and rustdoc are explicit that the Windows `DXGI`-alone fallback and macOS (a static
  working-set budget, not a live gauge) are narrower. Shares `--min`'s `parse_size_bytes` syntax.
  Prints one stderr line either way; no `--json` — the ask is specifically a scriptable exit code,
  not structured output. Live-verified on the reference RTX 5060 Ti: `hmn fits 1GiB` (exit `0`),
  `hmn fits 999GiB` (exit `1`), and a bad `--device` (exit `2`). Every long GPU run in the
  motivating dogfooding report was about to hand-roll this exact check before launching; this
  replaces six copies of it with one.

Two independent code-review passes (the second explicitly a fresh re-review at maximum effort,
run after the first's fixes had already landed) found and closed a further batch of issues in
the four features above before this entry was considered final: the SPILL/`spilling` honesty
contract silently collapsing to `Some(false)`/`false` for an unassessable adapter capacity or an
unmeasurable `hmn watch` tracker; the `wall_clock`/`t_ms` pairing skew on `watch`'s first sample;
`hmn fits`'s self-contradictory near-miss messages; `--min`'s sub-MiB summary misreport; and
`parse_size_bytes` silently saturating an out-of-range value instead of rejecting it. Two new
formatting helpers, `format_vram_precise` and `device_name_suffix`, were extracted along the way
to close the display-precision issues and a threefold-duplicated `" [name]"` idiom respectively.

## [0.2.10] - 2026-08-17

> *Audited, not assumed.*

A full-codebase self-audit — read as a dogfooding report in its own right
(Principle 1: every patch traces to a real adoption experience, and
auditing the crate against its own documented conventions is exactly
that, conducted first-party) — found three gaps sharing one shape:
silence. `hmn ps` capping silently at 64 processes, a malformed `DXGI`
adapter silently truncating enumeration, and a partially-broken driver
stack making `hmn` print nothing (or an indistinguishable `[]`) instead
of saying so. All three are now honest about what happened; a further
pass of independent adversarial review closed four more issues in the
fixes themselves before this release. The new `macos-latest` CI leg
(below) proved the point live on its very first run, immediately
surfacing a real three-release-old `dead_code` bug in `src/gpu/metal.rs`
that no prior release had ever compiled far enough to see. Full
findings:
[`docs/audits/2026-08-17-codebase-documentation-audit.md`](docs/audits/2026-08-17-codebase-documentation-audit.md).

### Fixed

- **NVML process-listing cap silently truncated `hmn ps` on busy multi-tenant devices** (`src/gpu/nvml.rs`) — `list_compute_processes` now retries with a heap-allocated buffer sized to NVML's own `NVML_ERROR_INSUFFICIENT_SIZE`-reported count when more than 64 compute processes exist, instead of silently keeping only the first 64. The fast-path 64-slot stack buffer is unchanged for the common case (fewer than 64 processes); the retry buffer is defensively capped at 65536 entries against a corrupted/malicious count report. `read_process_used` (the single calling-process lookup inside `query()`) is unaffected — this fix is scoped to the enumeration path `hmn ps` and the library's `gpu_processes()` use. The shared per-row sentinel/sanity filtering was extracted into a new pure `filter_process_rows` helper, gaining unit test coverage for the first time.
- **`hmn` (no subcommand) could print nothing — or an indistinguishable `[]` — on a partially-broken driver stack** (`src/bin/hmn.rs`) — if `Snapshot::all()` enumerated devices but every `device_info` call failed for them, text mode printed an empty string (indistinguishable from a working, silent success) and `--json` printed a bare `[]` (indistinguishable from genuine zero-device enumeration — `format_summary_json` skips the same unreadable entries `format_summary` does). Text mode now prints `hmn: N GPU(s) enumerated but none readable.`; `--json` prints the same line to stderr (mirroring `hmn ps`'s always-on stderr summary) while keeping the documented `[]` JSON shape on stdout unchanged.
- **`DXGI` non-NVIDIA adapter enumeration aborted the whole walk on one bad adapter** (`src/gpu/dxgi.rs`) — `enumerate_non_nvidia` and `device_count` now skip past an adapter whose `IDXGIAdapter` cast or `GetDesc` call fails and keep walking, instead of treating it as end-of-enumeration and silently dropping every adapter after it. `EnumAdapters1` failing remains the only true end-of-walk signal.
- **`src/gpu/metal.rs` failed `-D warnings` clippy the moment macOS actually compiled it — twice** (`src/gpu/metal.rs`) — caught live by this release's own new `macos-latest` CI leg (see Changed, below), across its first two real runs. First: `MetalQueryResult::current_usage` was computed via a real `ledger()` syscall on every `device_info()` call and then never read by its one call site (`device_info`'s dispatcher derives `used_bytes` from `total - free`, not from this field) — removed, along with the now-unneeded per-call syscall; `process_gpu_info` already computes the equivalent figure independently where it's actually used. `LEDGER_INFO` (a documented-but-unissued command constant from XNU's `ledger.h`, sibling to the two commands this module does issue) is kept as a zero-cost `#[allow(dead_code)]` reference rather than deleted, since it costs nothing and documents the complete 3-command family. Second: clippy's `redundant_closure` flagged `.get_or_init(|| objc2_metal::MTLCreateSystemDefaultDevice())` — but the suggested simplification (`.get_or_init(objc2_metal::MTLCreateSystemDefaultDevice)`) is a **false positive**: `MTLCreateSystemDefaultDevice` is `extern "C-unwind"`, which does not implement `FnOnce()`, so the "simplification" fails to compile with `E0277` — confirmed live via a real `cargo check --target aarch64-apple-darwin` before and after. The closure is required; the fix is `#[allow(clippy::redundant_closure)]` with a comment naming the trait-bound reason, not removing the closure. None of these three issues could have been caught before macOS was added to CI — `src/gpu/metal.rs` is `cfg(target_os = "macos")`-gated and had never compiled on any CI runner since it was added in v0.2.3.

### Changed

- **CI matrix gains macOS** (`.github/workflows/ci.yml`) — `macos-latest` × `{1.88, stable}` joins the existing Ubuntu/Windows legs; macOS has been a first-class supported platform (`src/gpu/metal.rs`, `tests/macos_smoke.rs`) since v0.2.3 but never compiled in CI before.
- **CI checks the documented non-default feature combinations** (`.github/workflows/ci.yml`) — `--no-default-features` (bare RSS-only) and the README's documented library-only set (`nvml,dxgi,pdh`, no `cli`) are now compiled on every matrix leg, not just the default and `--all-features` sets.
- **`publish.yml` verifies the pushed tag matches `Cargo.toml`'s version** before running the release gates, failing fast on a mismatched tag instead of publishing (or confusingly failing) with an unverified version.

### Documentation

- **`hmn --help` and its module doc** (`src/bin/hmn.rs`) — the `?`/elevation Limitations bullets and the `cli`-feature install line still described pre-v0.2.8 behavior (`OpenProcess`-only name resolution, default-off `cli`); rewritten to match shipped v0.2.8 behavior (`Toolhelp32Snapshot` fallback, `[exited]`/`[protected]`/`[kernel]` brackets, `cli` default-on install command).
- **`hypomnesis::gpu_processes` rustdoc** (`src/gpu/mod.rs`) — the Limitations section still promised `name: None` reaches callers on the Windows `PDH` path; rewritten to match the `GpuProcessEntry::name` field doc's already-correct v0.2.8 contract.
- **`ROADMAP.md` / `docs/roadmap-v0.2.8.md`** — corrected stale "not yet published" status for v0.2.8 (published 2026-08-04, with v0.2.9 shipped on top of it 2026-08-12); added the missing `docs/roadmap-v0.2.9.md` per-release index row.
- **`CONVENTIONS.md`** — the closing default-feature-set paragraph and the `cli` backend table row were last updated before `pdh`/`metal`/`cli` joined the default set and before `ctrlc` was added as a `cli` dependency; both corrected.
- **`HypomnesisError::Io`** (`src/error.rs`) — doc comment now states plainly that this crate never currently constructs it (reserved for a possible future I/O-based backend), instead of silently implying a live error path that doesn't exist.

## [0.2.9] - 2026-08-12

> *The same total. Now with the driver behind it.*

Adds `GpuDeviceInfo::driver_version: Option<String>` — the NVIDIA-branded
driver string (e.g. `"610.88"`), mirroring v0.2.4's `reserved_bytes`
addition. Driven by a `candle-mi` dogfooding report: its provenance log
stamps the Rust toolchain per verification run but not the GPU driver, and
a driver change can move floating-point results.

### Fixed

- **The `Used by` entry for `hf-fetch-model` pointed at the pre-transfer URL** (`README.md`). v0.2.7 corrected hypomnesis's own `repository` / `homepage` metadata on transfer to the `mi-for-the-rust-of-us` org but left this sibling link behind. Historical `CHANGELOG` and per-release roadmap entries are deliberately left as written.

### Added

- **`GpuDeviceInfo::driver_version: Option<String>`** (`src/snapshot.rs`) — new field on the `#[non_exhaustive]` struct. `Some` on the `NVML` and `nvidia-smi` paths; `None` on `DXGI`-alone, non-NVIDIA `DXGI` adapters, and `Metal` (macOS has no NVIDIA driver).
- **`nvmlSystemGetDriverVersion` query** (`src/gpu/nvml.rs`) — new `read_driver_version` helper, read once per `query()` session from the already-open `NVML` library (system-level, not device-scoped — the same driver serves every device index). Best-effort: symbol-lookup or call failure maps to `None`, same policy as `reserved_bytes`.
- **`nvidia-smi` fallback also supplies `driver_version`** (`src/gpu/nvidia_smi.rs`) — unlike `reserved_bytes`, `nvidia-smi` genuinely has this figure (`--query-gpu=memory.used,memory.total,driver_version`, one extra CSV column on the existing device-wide query, no second subprocess). CSV parsing extracted into a testable `parse_query_line` function.
- **`GpuDeviceInfoBuilder::driver_version`** setter (`src/snapshot.rs`, `test-helpers` feature) — defaults to `None`, mirroring the other optional setters.
- **`hmn` device summary renders the driver version** — `GPU 0 [NVIDIA GeForce RTX 5060 Ti]: free 14274 MiB / 16311 MiB (259 MiB reserved), driver 610.88`. Elided on backends that report `None`, so the line is unchanged where no driver string exists.
- **`hmn --json`** (new flag on the default, no-subcommand device summary) — emits the same per-GPU data as a JSON array instead of text: `{"index":N,"name":<string|null>,"total_bytes":N,"free_bytes":N,"used_bytes":N,"reserved_bytes":<number|null>,"driver_version":<string|null>}`. The dogfooding report assumed this surface already existed for the summary subcommand; it didn't, so this release adds it. Combining it with a subcommand (`hmn --json ps`) is a hard error (exit `2`) rather than silently applying to the summary or being dropped — each subcommand has its own `--json`.
- **`tests/live_gpu.rs::device_info_driver_version_is_plausible_when_present`** — live integration test asserting the driver string is non-empty and contains at least one digit when present. `#[ignore]`-gated like the other live-GPU tests.

## [0.2.8] - 2026-08-04

> *The tool you install should install. The name you can't be shown, someone else can.*

`cargo install hypomnesis` installing no binary, and most Windows `?` rows being nameable without elevation — two defects and two small asks from an askesis `canvas` dogfooding report ([2026-08-03](docs/dogfooding-feedbacks/dogfooding-install-no-binary-and-protected-names.md)) that ran a 38M-parameter training job on a rented RTX 5090 with no per-PID VRAM census, because the deploy script's `cargo install hypomnesis` exited `0` and silently installed nothing (`cli` was a default-off feature). The report's own diagnosis for the `?`-row defect was validated and corrected before implementation: it proposed switching `OpenProcess`'s query right from `PROCESS_QUERY_INFORMATION` to `PROCESS_QUERY_LIMITED_INFORMATION`, but `src/gpu/pdh.rs` already used the limited right (confirmed via `git log -p`, unchanged since v0.2.2) — a live P/Invoke test against `dwm.exe`/`csrss.exe` from a non-elevated shell showed **both** rights fail identically with `ERROR_ACCESS_DENIED`. The actual fix is a different Win32 mechanism entirely: `CreateToolhelp32Snapshot`, which reads process names from a system-wide enumeration without opening a per-process handle, so it isn't subject to the same access check. All four changes are additive; the `cli` default-feature flip is the one behavior change to the install experience itself (opt out via `--no-default-features`).

### Added

- **`cli` is now a default feature** (`Cargo.toml`) — `cargo install hypomnesis` installs the `hmn` binary out of the box, matching the polarity every GPU-source feature already uses (sources default-on; the tool that reads them now is too). `--features cli` is still accepted but redundant. Library-only consumers who don't want `clap`/`ctrlc` pulled in use `--no-default-features` and select source features explicitly.
- **`Toolhelp32Snapshot` name-resolution fallback (Windows)** (`src/gpu/pdh.rs`, `src/gpu/mod.rs`) — when `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `QueryFullProcessImageNameW` fails to name a PID (foreign-user, `SYSTEM`, or `PPL`-protected processes like `dwm.exe`/`csrss.exe`), a new `pdh::resolve_names_via_snapshot` takes one `CreateToolhelp32Snapshot` scan — batched once per `gpu_processes()` call (not once per unresolved PID), so `hmn watch`'s repeated interval polling doesn't repeat a full process-table walk per tick — and resolves the majority of former `?` rows to real names, non-elevated. Live-confirmed on the reference RTX 5060 Ti: both `dwm.exe` and `csrss.exe`, previously `?`, now resolve correctly.
- **`[exited]` / `[protected]` synthetic brackets (Windows-only)** replace the remaining unresolved `?` rows with an honest state instead of an anonymous placeholder: `[exited]` when the process exited between the VRAM sample and the name lookup (elevation would not help — a timing race, not a permission wall), `[protected]` when the snapshot fallback itself could not be taken at all (very rare). `hmn ps`'s summary-line protected count (`format_ps_summary`) now counts `None`, `[protected]`, and the pre-`WDDM 2.0` `nvidia-smi` fallback's literal `"?"` name string (a pre-existing case that was silently uncounted before this release, found and fixed during this release's consistency pass) — but deliberately excludes `[exited]`, fixing the exact overstatement ("today it counts everything unresolved") the dogfooding report flagged. `hmn watch`'s one-shot "unresolved PID grew" hint and its OS-PID-reuse name-change detector were both updated to treat `[protected]` as still-unresolved (the hint now fires for it) and to ignore a transient `[protected]`/`[exited]` flicker as a name change (preventing a false "PID recycled by the OS" baseline reset, and preventing a real subsequent reuse from being masked by a stale bracket value) — a correctness gap the bracket rendering would otherwise have introduced into existing `watch` logic that predates this release. The `[exited]`/`[protected]` distinction itself is Windows-only; Linux/macOS unresolved rows are unchanged (still a bare `?`/`None`) since there is no equivalent false-wall-vs-real-wall gap to collapse on those platforms.
- **`hmn ps --sort vram` / `--sort committed` aliases** (`src/bin/hmn.rs`) for `--sort dedicated` — the words the rest of the tool's own vocabulary already uses for the same quantity (the `ps` column header and `watch`'s `COMMITTED` column, respectively).
- **Tests** — ~15 new platform-independent unit tests (`szexefile_to_string` buffer decoding; `format_ps_summary`'s `[protected]`/`[exited]`/`[kernel]` bracket-counting cases; `--sort` alias parsing; `resolved_name`'s bracket-filtering; `process_sample`'s bracket-flicker-safety and growth-hint-on-`[protected]` cases). A new `#[ignore]`-gated live test in `tests/live_pdh.rs` asserts no `gpu_processes()` row is ever the literal `"?"` placeholder anymore.

### Changed

- **`GpuProcessEntry.name`'s Windows semantics** (`src/snapshot.rs`) — `None` essentially never reaches callers on Windows now; it is replaced by a real name, `[kernel]`, `[exited]`, or `[protected]`. Doc comment updated accordingly.

### Documentation

- **README** — "Binary (`hmn`)" install command, default feature-set prose, and the Feature Flags table updated for `cli`'s default-on status; Limitation 4 rewritten around the `[exited]`/`[protected]` vocabulary with a real `dwm.exe`/`csrss.exe` resolved-name capture from this release's live validation; `--sort` prose gains the `vram`/`committed` aliases.
- **FAQ** — the `?`-meaning entry rewritten around the snapshot fallback and the two new brackets, explicit about the Windows-only scope; `--sort` entry gains the aliases; upgrade command drops the now-redundant `--features cli`.
- **Tutorials** — install commands drop `--features cli`; pre-v0.2.8 `?`-row transcripts in `docs/tutorials/watching-a-running-job.md` annotated (not altered — they're real captures) noting the PIDs shown would now resolve; the unresolved-PID-growth-hint Gotcha updated for the `[protected]`/`[exited]` split.
- **`docs/roadmap-v0.2.8.md`** — new, following the established per-release roadmap structure.

## [0.2.7] - 2026-08-02

> *Follow the work, not just the machine. Sort by the question you're actually asking.*

`hmn watch --follow-new` and `hmn ps --sort` — two well-scoped asks from a candle-mi dogfooding report ([2026-07-27, extended 2026-08-01](docs/dogfooding-feedbacks/dogfooding-watch-follow-new.md)) that ran `hmn watch` alongside 19 sequential `cargo test` processes and found the adapter-level spill detection flawless (three real episodes, one a fast Mistral-7B spike candle-mi's own wall-clock heuristic had missed) while the per-PID half answered the wrong question — `watch`'s auto-selected set froze at attach, so none of the nineteen processes that actually caused the spills were ever attributed. Both features are additive and off-by-default/pre-v0.2.7-compatible; no library-surface change. The repo also transferred from `PCfVW/hypomnesis` to the `mi-for-the-rust-of-us` GitHub org this release, joining `anamnesis` and `candle-mi` — old URLs redirect, but this release carries the corrected `repository`/`homepage` crates.io metadata.

### Added

- **`hmn watch --follow-new`** (`src/bin/hmn.rs`) — auto-select mode only: re-runs the top-`--top` selection every interval instead of once at attach. A PID entering the followed set starts with a fresh baseline (first sighting); a PID leaving (exited, or dropped below rank `--top`) simply stops appearing in the live rows and is *finalized* into the closing summary's `per_pid[]` with its peak/baseline, instead of rendering `0` forever. A new `WatchState` wraps the existing per-PID `HashMap` with a `seen_order: Vec<u32>` (first-seen order, no duplicates) so the closing summary can list *everyone who mattered during the watch* rather than the fixed original set. Re-entry after a gap resumes existing history (no reset) — only the existing, unmodified OS-PID-reuse name-change detector resets a row. An empty first sample is not an error under `--follow-new` (the point is to wait for work to appear); combined with explicit PID(s) it's a hard error, exit `2`, checked before any device query. A new stderr breadcrumb (`entered pid=... (name); left pid=... (name)`) reports followed-set changes; purely cosmetic, doesn't affect the JSONL stream shape.
- **`hmn ps --sort <KEY>`** (`src/bin/hmn.rs`) — `dedicated` (default, unchanged pre-v0.2.7 order — "who do I kill to free VRAM?"), `shared` ("who is currently being paged out?" — a symptom, not a cause; a documented no-op ordering on Linux/macOS, where `shared_used_bytes` is always `0`), or `total` (dedicated + shared — "who is the biggest GPU-memory citizen overall?"). A new `ps_row_comparator(SortKey)` is shared between `run_ps` (user-selectable) and `select_top_n_pids` (always pinned to `Dedicated`) so the two orderings can't drift apart — the exact ask from the dogfooding report. Tie-breaks (name ascending, then PID ascending) are identical and unchanged across all three keys.
- **Tests** — ~35 new platform-independent unit tests across both features (comparator behavior per key including tie-break-preservation and a genuinely `Total`-only-discriminating case; `--sort` clap parsing; `WatchState`/`seen_order` first-seen-order bookkeeping including a PID that genuinely drops out of the followed set and resumes on re-entry; `resolve_watched_pids` auto-select/explicit-passthrough/empty-rows; the `format_followed_set_change` stderr-breadcrumb formatter; `--follow-new` clap parsing). A new `#[ignore]`-gated `tests/live_watch_follow_new.rs` spawns two *sequential* real `spillforge` forced-spill processes under `hmn watch --follow-new --json` and asserts both are tracked as distinct entries and finalized into the closing summary — the closest reproduction of the motivating "successive short-lived GPU processes" workload achievable with the existing fixture, and confirmed (by adversarial review) to actually fail if `--follow-new` regressed to the old frozen-set behavior.

### Changed

- **`select_top_n_pids`'s tie-break** (`src/bin/hmn.rs`) now shares `hmn ps`'s comparator (name ascending, then PID ascending) instead of its own PID-only rule — a deliberate, documented consequence of the "share the comparator" fix. At an exact tie on committed VRAM, `hmn watch`'s auto-selected top-N can now pick a *different* PID than it would have pre-v0.2.7 (confirmed live: exact ties are a real, not hypothetical, occurrence). Display-order-only changes elsewhere are unaffected.
- **`Cargo.toml`** `repository` / `homepage` updated to `https://github.com/mi-for-the-rust-of-us/hypomnesis`. `README.md` and `CONVENTIONS.md` cross-references to `anamnesis`/`candle-mi` (both already moved to the same org) updated; historical narrative (this file, `ROADMAP.md`'s PR #1 links, per-release roadmap docs) deliberately left as period record — old links redirect.

## [0.2.6] - 2026-07-25

> *Not a TUI. Same tracker, a timer instead of a wrapped child.*

`hmn watch [PID...]`: attach-to-a-running-PID spill triage. `hmn spill -- <command>` only wraps a *new* command; a rhyme-mdlm dogfooding report ([2026-07-25](docs/dogfooding-feedbacks/dogfooding-spill-triage-watch-mode.md)) hit that wall three times triaging a 15-hour, 3-seed training campaign — hand-rolling "two `hmn ps` samples minutes apart, diff by eye" every time to reach three distinct, all-correct verdicts (benign commit-vs-resident, a real resume-path `VRAM` leak found/fixed/verified, and a system-wide tenant-driven spill), because there was no way to attach to a trainer already hours into its run. `hmn watch` closes the gap as a pure CLI addition — **zero changes to `src/spill.rs` or `src/gpu/pdh.rs`**: it samples the unchanged `SpillTracker` (adapter-wide dedicated-saturation + shared-growth co-condition, live-tuned to 85% in v0.2.5) and the unchanged `gpu_processes()` (per-PID committed/shared bytes) on a timer instead of around a wrapped child.

### Added

- **`hmn watch [PID...]`** (`src/bin/hmn.rs`) — attaches to already-running PID(s), or auto-selects the top `--top` (default 5) processes by committed `VRAM` from the first sample when none are given, keeping that fixed set for the run. Prints one row per watched PID per interval (default `--interval 5s`): committed / shared `VRAM`, signed per-interval deltas, and a SPILL flag — the same adapter-wide `SpillTracker::is_spilling()` state `hmn spill` uses, replicated per row rather than re-derived. A watched PID absent from a sample renders `0 B`; `hmn watch` cannot distinguish "exited" from "currently holds no GPU memory" and does not auto-stop on this basis.
- **Duration-string `--interval` / `--duration`** — a hand-rolled parser (`500ms`, `30s`, `5m`, `1h`, or a bare number of seconds) rather than `hmn spill`'s raw-millisecond convention, tuned for an attach-and-leave-running tool where intervals are seconds-to-minutes. `--duration` is optional; omitted, `hmn watch` runs until Ctrl+C.
- **`0` / `1` / `2` exit-code contract** — `0` if spill was never observed, `1` if it was at least once, `2` on a hard error (bad `--device`, or nothing to auto-select) — designed for a watchdog script to branch on directly, without JSON parsing. Ctrl+C (new `ctrlc` dependency, `cli`-feature-only — a safe cross-platform API, no new `unsafe` code) and a natural `--duration` stop both print the same closing summary and set the same exit code.
- **Closing summary**: the adapter-level report reuses `hmn spill`'s exact rendering via a new `format_spill_report_with_prefix(prefix, report)` (generalized from the existing `format_spill_report`, which becomes a one-line wrapper — byte-identical output, unchanged tests), plus a new per-PID peak/baseline table.
- **`--json` streams JSON Lines** rather than a single blob — one `{"kind":"sample",...}` object per PID per interval as it happens, plus a closing `{"kind":"summary",...}` object (the `SpillReport` fields plus a `per_pid[]` peak/baseline array), matching `watch`'s live-tailing character rather than `hmn spill --json`'s one-shot-at-exit shape. The episode-array JSON serialization is shared with `hmn spill --json` via a new `write_episodes_json` helper extracted from `format_spill_json` (no behavior change to the existing `hmn spill --json` output).
- **Best-effort PID-reuse handling** — if the OS recycles a watched PID onto a different process mid-watch, a resolved-name change between samples is used as the signal to reset that row's baseline/peak, so the closing summary describes the new process rather than mixing two processes' readings. Found during a two-agent conventions-plus-adversarial-correctness pass on the diff (the same review process v0.2.5 used before commit); can't catch every case (two same-named processes trading a PID look identical) but closes the common one.
- **Unresolved-PID growth hint** — a watched `?` row whose cumulative committed or shared growth crosses 256 MiB since attach gets a one-shot stderr hint ("re-run elevated to identify"), directly implementing a "smaller observation" from the motivating dogfooding report.
- **`GpuProcessEntryBuilder`** (`src/snapshot.rs`, `test-helpers` feature) — synthetic `GpuProcessEntry` fixtures for downstream tests; `GpuProcessEntry` is `#[non_exhaustive]`, so `hmn watch`'s own `process_sample` unit tests needed it, the same circumstance that produced `SpillReportBuilder` in v0.2.5.
- **Tests** — ~40 new platform-independent `hmn` unit tests (duration parsing, signed-delta/format-delta arithmetic, top-N selection, exit-code mapping, row/summary text and JSON formatting, PID-reuse and unresolved-name-churn fixtures, clap arg parsing); a new `#[ignore]`-gated `tests/live_watch.rs` end-to-end test spawning the real compiled `hmn` binary against the `spillforge` forced-spill fixture (the same fixture that validated `hmn spill` in v0.2.5) via `env!("CARGO_BIN_EXE_hmn")`, asserting a real spilled episode and exit code `1`. Manual dogfooding additionally confirmed a genuine forced spill (one episode, ~1.4 GiB peak shared, exit `1`) and an idle-desktop auto-top-3 run with zero false positives (exit `0`).

### Documentation

- **New tutorial** [`docs/tutorials/watching-a-running-job.md`](docs/tutorials/watching-a-running-job.md) — built around the motivating dogfooding report's three-verdict campaign narrative; cross-links the existing [Is my run spilling?](docs/tutorials/is-my-run-spilling.md) tutorial for the episode-pattern-reading and per-process-attribution steps that transfer unchanged, rather than duplicating them.
- **New FAQ entry** ["`hmn spill` or `hmn watch` — which do I use?"](docs/FAQ.md#hmn-spill-or-hmn-watch--which-do-i-use).
- **README** gains an `hmn watch` subsection, a "Try it" transcript (a real forced-spill capture against `spillforge`, attached mid-run), a task-router entry, and updated feature-flag / documentation-index tables.
- **`ROADMAP.md`**'s "Carried forward" table resolves the `hmn watch (TUI live-refresh)` row — the rejected item was a curses-style redraw dashboard; what shipped is explicitly not that.

## [0.2.5] - 2026-07-22

> *Resident, not committed. Episodes, not a boolean.*

`WDDM` spill detection: surfaces the dedicated-`VRAM` → shared-system-memory paging signal that is invisible to every per-process `VRAM` counter the crate already exposes. Spill is **residency, not commitment** — the governing semantics come from a rhyme-mdlm dogfooding report ([2026-07-19](docs/dogfooding-feedbacks/dogfooding-wddm-spill-detection.md), Principle 1) that caught, live, the false-positive a commit-gap heuristic would have shipped: a compute-bound process committed ~1.8 GiB past dedicated while Task Manager's shared column sat flat at 0. The spill signal is PDH's **`Shared Usage`** (resident shared bytes), never `committed − dedicated` (reservation headroom). The maintainer's transient-spill observation (spill appearing, vanishing, reappearing as the working set hovers at the boundary) drives the API shape: an *instantaneous* `is_spilling()` / *latched* `has_spilled()` split and an **episode-based** `SpillReport` (five 2-second blips ≠ one 90-second spill — many short episodes ⇒ marginally over budget; one sustained ⇒ genuinely over). Fully additive under `#[non_exhaustive]`; no breaking change. Every number below was live-validated on the reference RTX 5060 Ti (16 GiB, Windows 11 / `WDDM`), including a forced-spill fixture (20 GiB hot working set) that produced a real 13.1 s episode with 3.1 GiB peak shared over a 163 MiB baseline.

### Added

- **`GpuProcessEntry::shared_used_bytes: u64`** (`src/snapshot.rs`) — per-process resident shared-system-memory bytes, the same quantity Task Manager's *Shared GPU memory* column shows. Populated on the Windows `PDH` path from the `\GPU Process Memory(*)\Shared Usage` counter (sibling of the `Dedicated Usage` counter v0.2.2 reads — same instance mangling, same single `PdhCollectQueryData` sample, no second collect); `0` on `NVML` / `nvidia-smi` / `Metal` rows (no shared-residency counter exists there). This — not `used_bytes`, which on `PDH` is `VidMm`'s dedicated *commit* — is the per-process spill signal.
- **`SpillTracker`** (`src/spill.rs`, new module — compiled on **every** platform, no new Cargo feature) — fold-over-observations spill tracker, cousin of `MemoryReport`. Consumer drives observation timing (`tracker.observe(label)` in their existing loop; no background thread, no callbacks); `is_spilling()` (instantaneous, may legitimately flicker), `has_spilled()` (latched, never reverts — what early-stop consumers want), `is_measurable()`, and `into_report()` are cheap queries over already-collected state. Builders: `with_dedicated_threshold(bytes)` (absolute early-warning override — e.g. fire at 12 GiB on a 16 GiB card, undercutting the 85% default) and `with_shared_growth_threshold(bytes)`. On Windows the tracker holds a **long-lived** `PDH` adapter query (counters added once; `GPU Adapter Memory` instances are stable, unlike per-process instances) and is documented `!Send`/`!Sync`; a failed sample is a *skipped* observation, never an error (measurement must not disturb the workload).
- **Two-sided spill condition with a baseline** — an observation spills iff adapter dedicated-resident ≥ threshold **and** shared-resident has risen ≥ `DEFAULT_SHARED_GROWTH_BYTES` (256 MiB) above its first-observation baseline. Shared has a *benign* baseline by design (staging/upload heaps — live-measured ~100–163 MiB idle on the reference card), so `shared > 0` alone is never spill. **`DEFAULT_DEDICATED_THRESHOLD_PCT` is 85, not the ~95 the design sketch assumed** — live tuning with the forced-spill fixture measured `VidMm`'s adapter-wide dedicated-resident *ceiling* at ≈ 88.6–91.3% of `DXGI` `DedicatedVideoMemory` even under maximal pressure, so a 95% threshold is unreachable (systematic false negatives) on the reference card.
- **`SpillReport` + `SpillEpisode`** (`src/spill.rs`) — episode-based end-of-run summary: per-episode `start_label` / `end_label` (`None` = still spilling at report time) / `peak_shared_bytes` / `observations` / `duration`, plus report-level peaks, baseline, capacity, observation count, and an honest `measurable: bool` (distinguishes "no spill occurred" from "this platform cannot tell"). Derived accessors: `spilled()`, `first_spill_label()`, `total_spill_duration()`, `longest_episode()`.
- **`is_spill_measurable() -> bool`** (`src/spill.rs`) — cross-platform-honest capability probe: `true` only on Windows + `pdh` with the `GPU Adapter Memory` counter set registered (`WDDM 2.0`+); `false` on Linux (normal `CUDA` OOMs rather than silently paging) and macOS (`UMA` — nothing to spill *into*). Portable consumers skip the early-stop path entirely instead of polling a tracker that can never fire.
- **`AdapterMemQuery`** (`src/gpu/pdh.rs`) — long-lived adapter-wide `PDH` query behind the tracker: enumerates `\GPU Adapter Memory(*)` instances (bare `luid_0x..._0x..._phys_N` names — new `parse_adapter_instance_name`, sharing a `parse_luid_tail` helper with the v0.2.2 process-instance parser), reads `Dedicated Usage` + `Shared Usage` per segment. **No `Dedicated Limit` counter exists in the set** (verified live via `typeperf -q`: only `Dedicated Usage` / `Shared Usage` / `Total Committed`), so the capacity comes from `DXGI`'s static `DedicatedVideoMemory` via the new `dxgi::adapter_dedicated_video_memory` — `0` is documented as "limit unknown" and the condition then never fires without an absolute override.
- **`hmn spill -- <command>`** (`src/bin/hmn.rs`) — `time(1)`-style wrapper: spawns the command with inherited stdio, polls the tracker at **`--interval <MS>` (default 100 ms, user-settable)**, prints the report to **stderr** on exit (stdout stays the wrapped command's), and **passes the wrapped command's exit code through** (`0..=255` exact; negative `NTSTATUS` / >255 map to `1`, never bit-truncated into a false success — live-verified with `cmd /c "exit 7"` → `$LASTEXITCODE == 7`). `--json` additionally emits the `SpillReport` as one JSON object on stdout (hand-rolled, no `serde`, mirroring `hmn ps --json`; check `measurable` before trusting `spilled: false`). `--device <INDEX>` selects the GPU. On non-measurable platforms the command still runs and stderr says `spill not measurable on this platform` instead of a misleading all-zeros report.
- **`SpillReportBuilder`** (`src/spill.rs`, `test-helpers` feature) — synthetic `SpillReport` fixtures for downstream tests (`SpillReport` is `#[non_exhaustive]`; the `hmn` binary's own formatter tests are the first consumer). Follows the `GpuDeviceInfoBuilder` pattern; entry point `SpillReport::builder()`.
- **Tests** — 15 platform-independent unit tests drive the pure fold core through synthetic sequences (`src/spill.rs`: flicker ⇒ exactly 3 episodes with the latch surviving recovery; benign-baseline and commit-gap fixtures ⇒ 0 episodes — the rhyme-mdlm regression; growth-boundary, zero-limit, override, zero-duration-fencepost, accessor cases); 19 new `hmn` unit tests (exit-code mapping, spill arg parsing incl. hyphen-value passthrough after `--`, report/JSON formatting via the builder, SHARED-column table/JSON rendering, duration helpers); 3 shape-only smoke tests (`tests/smoke.rs`, GPU-less-safe); 3 `#[ignore]`-gated live tests (`tests/live_pdh.rs` — measurability probe, **idle-desktop no-false-positive** over 50 × 100 ms observations, per-process shared sanity + Task Manager cross-check printout; the spill tests live here rather than `tests/live_gpu.rs` because this file's `windows + pdh` gate is exactly the spill precondition).

- **`tools/spillforge`** (repo-only, `publish = false` — auto-excluded from the crate package) — the forced-spill fixture behind the threshold tuning and release validation: a `D3D11` hog that uploads a configurable working set (default 20 GiB) past dedicated `VRAM` and keeps it hot with round-robin touches, producing a real, reproducible spill under `hmn spill`. Encodes the two measured `WDDM` lessons (commit alone never spills; idle working sets evict to backing store, not shared residency), so it doubles as a regression check for both sides of the detector.

### Documentation

- **README restructured to the `anamnesis` / `hf-fetch-model` house pattern** — a *"New to hypomnesis?"* task router (five intent-phrased entries), a *"Try it"* section of real transcripts from the release validation (including the genuine forced-spill episode), and a *Documentation* index table. Release headline rotated to v0.2.5 / v0.2.4.
- **`docs/FAQ.md`** (new) — eleven entries consolidating the recurring "this number looks wrong" questions whose answers were scattered across the README Limitations, CHANGELOG, and roadmap docs: commit vs resident, the benign SHARED baseline, the spill condition and its measured 85% threshold, per-platform zeros, `?` rows and elevation, no-`kill` scope discipline, KB 4490156 / R570 handling, threading, polling cost, and the `cargo install --force` upgrade gotcha.
- **`docs/tutorials/is-my-run-spilling.md`** (new) — the crate's first tutorial: wrap a run with `hmn spill`, read the episode pattern (many-short vs one-sustained), attribute per-PID via the SHARED column, react automatically (`jq -e` CI gate), and integrate `SpillTracker` in-process — all transcripts real, opening with the commit-gap trap the rhyme-mdlm report caught.

### Changed

- **`hmn ps` gains a SHARED column** (table, between VRAM and DEVICE) and a `"shared_used_bytes"` JSON field — real values on Windows (live: benign baselines of 0–63 MiB across the process table), documented `0` on Linux / macOS. `--help` Limitations gains the matching bullet (benign baseline is normal; growth while dedicated saturates is spill).
- **`pdh` internals refactored for two counter sets** (`src/gpu/pdh.rs`) — shared `add_counter` / `read_counter_bytes` / `enum_object_instances` helpers; `query_per_process_vram` now returns named-field `ProcessMemoryRow { pid, dedicated_committed_bytes, shared_used_bytes }` rows instead of `(u32, u64)` tuples (two same-typed byte counts are trivially transposable). Existing per-process behaviour byte-identical; per-instance failures remain best-effort (a row missing its shared counter degrades to `shared_used_bytes: 0` instead of being dropped).
- **`HypomnesisError::Pdh` doc widened** (`src/error.rs`) to name the `GPU Adapter Memory` counter set alongside `GPU Process Memory`; no new variant.
- **`CONVENTIONS.md` unsafe-scope and backend tables updated** to include the `pdh` (v0.2.2) and `metal` (v0.2.3) backends that postdated the tables, plus the v0.2.5 adapter-query surface.

## [0.2.4] - 2026-06-29

> *The same total. Now with the carve-out shown.*

Surfaces NVIDIA's driver/firmware **reserved** memory — the carve-out NVML holds *within* its reported `total` (the `nvmlMemory.total = reserved + free + used` identity). On the reference RTX 5060 Ti, `nvidia-smi -q -d MEMORY` prints `Total: 16311 MiB` and `Reserved: 259 MiB`, and hypomnesis now exposes the same **live-measured 259 MiB** via NVML's **v2** query (`nvmlDeviceGetMemoryInfo_v2`, R510+) as a new additive `GpuDeviceInfo::reserved_bytes: Option<u64>`. `reserved_bytes` is a *subset* of `total_bytes`, not an addition to it (memory available for allocation is `total - reserved`, which `free_bytes` already reflects). Driven by a candle-mi v0.1.16 dogfooding report (Principle 1 — every patch is informed by a real consumer's adoption experience); the report *inferred* a 73 MiB carve-out from `DXGI nominal − NVML total`, but that gap is board/ECC overhead sitting *below* NVML's `total` — a different quantity from the v2 `reserved` field (259 MiB), which the live query reports directly. Fully additive under `#[non_exhaustive]`; no breaking change.

### Added

- **`GpuDeviceInfo::reserved_bytes: Option<u64>`** (`src/snapshot.rs`) — device memory reserved for system use (driver or firmware): page tables, context/channel structures, ECC parity. `Some` only on the NVML path with an R510+ driver; `None` on older drivers and on every non-NVML backend (DXGI, nvidia-smi, Metal). It is a *subset* of `total_bytes` (NVML's `total = reserved + free + used`), so allocation headroom is `total_bytes - reserved_bytes`, which `free_bytes` already nets out. `total_bytes` is unchanged — the v1 figure, identical to `nvidia-smi`'s `Total` — preserving existing behaviour byte-for-byte.
- **NVML v2 memory query** (`src/gpu/nvml.rs`) — new `#[repr(C)] NvmlMemoryInfoV2` (`nvmlMemory_v2_t`: `version`, `total`, `reserved`, `free`, `used`) plus the `NVML_MEMORY_V2_VERSION` struct-version tag (`size_of | (2 << 24)`, required by the API or the call returns `NVML_ERROR_INVALID_ARGUMENT`). A best-effort `read_device_reserved` helper loads `nvmlDeviceGetMemoryInfo_v2` via `libloading`; on pre-R510 drivers the symbol is simply absent and the lookup fails gracefully (`reserved_bytes = None`). The v1 `nvmlDeviceGetMemoryInfo` total/free/used path is untouched — zero regression risk to the existing triplet.
- **`GpuDeviceInfoBuilder::reserved_bytes`** setter (`src/snapshot.rs`, `test-helpers` feature) — defaults to `None`, mirroring the other byte setters.
- **`hmn` device summary renders the carve-out** — `GPU 0 [NVIDIA GeForce RTX 5060 Ti]: free N MiB / 16311 MiB (259 MiB reserved)`. The reserved figure is a subset of the reported total (matching `nvidia-smi -q`'s `Total` / `Reserved` lines), and the parenthetical is elided on backends that report `None`, so the line is unchanged where no reserved figure exists.
- **`tests/live_gpu.rs::device_info_reserved_bytes_is_plausible_when_present`** — live integration test asserting the carve-out is non-zero and a subset of the reported total (`reserved < total`), with `total` within the 1 TiB sanity bound. `#[ignore]`-gated like the other live-GPU tests.

## [0.2.3] - 2026-06-10

> *Three platforms. Same contract. Resident-bytes everywhere.*

Adds first-class macOS support on Apple Silicon (Apple Silicon M-series). Process RSS, per-process GPU memory, and the compute-process listing come from libSystem syscalls (`task_info`, `ledger`, `sysctl`, `proc_listpids`, `proc_pidpath`) with no third-party Apple-framework wrapper. The device-wide GPU budget is read via `MTLDevice.recommendedMaxWorkingSetSize` through a minimal `objc2-metal` dependency — the only Apple-API call that isn't satisfiable from libSystem alone. Cross-platform `used_bytes` semantics are preserved (resident bytes — the macOS `graphics_footprint` ledger entry behaves the same way Windows `WorkingSetSize` and Linux `VmRSS` do under memory pressure). The `metal` Cargo feature joins the default set alongside `dxgi` and `pdh`; all three are platform-gated so the wrong-platform user pays nothing. Authored by contributor [@LittleCoinCoin](https://github.com/LittleCoinCoin) — PR #1.

### Added

- **macOS process RSS** via `task_info(TASK_VM_INFO_PURGEABLE).phys_footprint` (`src/ram.rs`) — new `darwin_ffi` submodule mirrors the existing `win_ffi` pattern: one `unsafe extern "C"` block for `task_info` and `mach_task_self`, one `#[repr(C)] TaskVmInfo` struct laid out per XNU `<mach/task_info.h>`, one `macos_rss()` returning `phys_footprint` (the kernel-ledger figure backing Activity Monitor's "Memory" column). The `process_rss()` dispatcher gains a `#[cfg(target_os = "macos")]` arm.
- **macOS per-process GPU memory** via `ledger(LEDGER_ENTRY_INFO_V2).graphics_footprint` (`src/gpu/metal.rs`, new file) — the BSD kernel ledger syscall reads the resident GPU-attributed bytes of any same-user PID without `task_for_pid` or any entitlement. The `graphics_footprint` entry index is discovered by name at first call via `LEDGER_TEMPLATE_INFO` and cached in a `OnceLock<i32>` (no hardcoded index — the kernel's entry ordering is not part of any stable ABI).
- **macOS device-wide GPU budget** via `MTLDevice.recommendedMaxWorkingSetSize` (`src/gpu/metal.rs`) — Apple's own kernel-projected GPU working-set budget, read once into a `OnceLock<Retained<ProtocolObject<dyn MTLDevice>>>` and reused for every subsequent `device_info()` call (sub-microsecond per-query cost after a one-time ~200 µs init). A `const _: fn() = || { … }` block compile-asserts that the cached type is `Send + Sync` so the static design is verified at build time.
- **macOS compute-process listing** via `proc_listpids(PROC_ALL_PIDS, …)` + per-PID `ledger` + `proc_pidpath` (`src/gpu/metal.rs`) — enumerates every same-user PID, reads each one's `graphics_footprint`, retains rows with footprint > 0, populates `name` via `proc_pidpath`. Cross-user PIDs surface as `EPERM` and are silently skipped; never panics, never returns `Err`. Run elevated (`sudo hmn ps`) to include cross-user PIDs.
- **`GpuQuerySource::Metal` variant** (`src/snapshot.rs`) — added always, unconditionally, mirroring the policy of `Dxgi`/`Nvml`/`NvidiaSmi`/`Pdh`: every variant is reachable from the crate root on every platform so cross-platform smoke tests can instantiate them.
- **`metal` Cargo feature**, added to the default set — gated by `cfg(all(target_os = "macos", feature = "metal"))` at the module-import site in `src/gpu/mod.rs` so Windows/Linux builds compile it out. Pattern mirrors the existing `dxgi`/`windows` target-conditional dep.
- **`tests/macos_smoke.rs`** — six `#[cfg(target_os = "macos")]`-gated integration tests covering RSS, device count, device info (Apple-brand name + ≥ 8 GiB total), `process_gpu_info` (Metal source, per-process flag), `gpu_processes` shape (closes parity with the cross-platform `gpu_processes_returns_result_or_no_gpu_source`), and `Snapshot::now` GPU-presence on macOS. Two tests are `#[ignore]`-gated because they require Apple Silicon with a usable Metal device.

### Changed

- **`gpu_processes()` dispatcher gains a macOS priority-0 arm** (`src/gpu/mod.rs`) — calls `metal::list_compute_processes(idx)` before falling through to the existing NVML / PDH / nvidia-smi chain. The same priority-0 macOS arm is added to `device_count`, `device_info`, and `process_gpu_info`. The macOS arm's output is sorted via the existing `sort_by_pid` helper for cross-backend consistency.
- **Capabilities tables in `README.md` and `src/lib.rs` gain a macOS column**, with one row per metric (`task_info`, `sysctl hw.memsize` + `MTLDevice.recommendedMaxWorkingSetSize`, `ledger.graphics_footprint`, `proc_listpids` + per-PID `ledger` + `proc_pidpath`, no fallback).
- **`tests/smoke.rs::gpu_processes_returns_result_or_no_gpu_source`** now accepts `GpuQuerySource::Metal` as a valid row source, alongside `Nvml`, `Pdh`, and `NvidiaSmi`. DXGI remains intentionally excluded — it cannot enumerate other PIDs.
- **`tests/live_gpu.rs::process_gpu_info_returns_expected_source_per_platform`** gains a `#[cfg(target_os = "macos")]` arm asserting `info.source == GpuQuerySource::Metal` and `info.is_per_process`, completing per-platform parity with the existing Windows + Linux arms.
- **`README.md` adds a "macOS UMA semantics: what `free_bytes` means" subsection** explaining that on Apple Silicon UMA the discrete-GPU "free vs total" mental model is replaced by `MTLDevice.recommendedMaxWorkingSetSize` (the kernel-projected GPU working-set budget) over `sysctl hw.memsize` (physical DRAM), and noting that per-process `used_bytes` exhibits resident-bytes volatility under memory pressure — identical to Windows `WorkingSetSize` and Linux `VmRSS`.
- **`hmn --help` Limitations expanded with two macOS bullets** (`src/bin/hmn.rs`) — one documenting the residency-eviction behaviour (per-call value drift on idle PIDs, mirroring Windows `WorkingSetSize` / Linux `VmRSS` semantics), one documenting the cross-user `EPERM` behaviour and the `sudo hmn ps` workaround. The cross-user bullet was the v0.2.3 pre-merge ask called out in the roadmap; added maintainer-side during merge prep.
- **`hmn ps` stderr summary gains a "committed total" figure** (`src/bin/hmn.rs`) — `format_ps_summary` now sums `used_bytes` across listed rows and renders the figure as a human-readable parenthetical: `hmn: N GPU processes found (X.Y GiB committed total[; M protected — re-run elevated for names]).`. The word "committed" hints at the `WDDM` commit-vs-resident distinction the Windows `PDH` backend exposes — summing across processes can exceed physical `VRAM` under `WDDM` (a real `WDDM` property, not a bug), and the wording prevents that from reading as broken. Elided entirely when `count == 0` because a zero-bytes total carries no information. 9 existing tests updated to expect the new wording; 2 new tests pin the `GiB` (3 × 4 GiB = 12.0 GiB) and `MiB` (2 × 256 MiB = 512 MiB) rendering across `format_vram`'s unit-selection boundary at 1 GiB. Dogfooding-driven UX addition recorded as a v0.2.3 ride-along.
- **`README.md` gains a "Composable workflows" subsection inside `## Binary (hmn)`** — documents `hmn ps --json` beyond what `--help` covers, with two `jq`-based recipes: top-5 GPU consumers (`hmn ps --json | jq 'sort_by(-.used_bytes) | .[:5]'`), and "terminate processes above a `VRAM` threshold" composed with the platform's native killer (Windows `ForEach-Object { taskkill /F /PID $_ }` and Unix `xargs -r kill -TERM` variants shown side-by-side). Includes a *"Why no `hmn kill`?"* sub-subsection explaining the scope-discipline rationale (a `hmn kill` subcommand was considered for v0.2.3 and rejected to preserve `hypomnesis`'s *"measurement, not control"* boundary; recording the decision in the README is cheaper than re-arguing it in a future PR). Dogfooding-driven UX addition recorded as a v0.2.3 ride-along — motivated by the observation that even the maintainer had forgotten `--json` existed.
- **`README.md` Limitations section gains macOS bullets 6 and 7** — residency-eviction behaviour (per-call value drift on idle PIDs, same resident-bytes semantics as `WorkingSetSize` and `VmRSS`) and cross-user `EPERM` behaviour (`sudo hmn ps` to include other-user PIDs). Closes the doc gap where the Capabilities table had a macOS column but the Limitations bullets only covered Windows and Linux.
- **`docs/hypomnesis-brief.md`** — tagline, capabilities bullets, comparison table, future-platforms line, and closing line all updated to reflect macOS as a shipped platform rather than a future consideration.

### Dependencies

- **`objc2-metal = "0.3"`** added under `[target.'cfg(target_os = "macos")'.dependencies]` as `optional = true` with `default-features = false, features = ["MTLDevice"]` — the minimal feature set that gates both `MTLCreateSystemDefaultDevice` and the device-property reads. Default features (every other `MTL*` type) are deliberately disabled.
- **`objc2 = "0.6"`** added as a sibling dep — already a transitive of `objc2-metal`, but the static `OnceLock<Retained<ProtocolObject<dyn MTLDevice>>>` in `src/gpu/metal.rs` names types from `objc2::rc` and `objc2::runtime`, so the crate must be a direct dependency.
- Both are pulled in only when the `metal` feature is active AND the target is `target_os = "macos"`. Windows and Linux dependency graphs are unchanged.

## [0.2.2] - 2026-06-02

> *Truer, not wider. Same `hmn ps` — fewer silent omissions on Windows.*

`v0.2.2` closes hypomnesis's most visible Windows gap: under consumer `WDDM 2.0`+, `hmn ps` previously reported `"0 compute processes found"` while dozens of processes were visibly holding GPU memory. The architectural reason was real — `NVML`'s per-process query returns `NVML_VALUE_NOT_AVAILABLE` under `WDDM`, the `nvidia-smi --query-compute-apps` fallback writes `[N/A]` that the parser drops, and `DXGI`'s `QueryVideoMemoryInfo` only answers for the calling process. The data was always there in Microsoft's video memory manager (`VidMm`); just not where any of hypomnesis's backends were looking. v0.2.2 adds a fourth Windows backend — **`PDH` (Performance Data Helper)** — that reads exactly the same `VidMm` data Task Manager's "Dedicated GPU memory" column surfaces. To the maintainer's knowledge, this is the first Rust crate to expose per-process VRAM for foreign processes on consumer Windows. Three waves of code (PDH FFI + dispatcher wiring + docs refresh) plus a security-relevant follow-up (PID 4 special-cased to `[kernel]`, threat-model note in the README) — all additive under the `#[non_exhaustive]` policy; no breaking changes. See [`docs/roadmap-v0.2.2.md`](docs/roadmap-v0.2.2.md) for the wave-by-wave rationale, and the [`#used_bytes` semantics](src/gpu/pdh.rs) section of the PDH module doc-comment for the WDDM commit-vs-resident caveat that Wave C documents.

### Added

- **`pdh` Cargo feature + `src/gpu/pdh.rs` (PDH FFI bindings and counter enumeration)** — Wave A of the v0.2.2 roadmap. New default-on Cargo feature (Windows-only effect; depends on `dxgi` for the adapter `LUID` walk) gating a new module that reads `\GPU Process Memory(*)\Dedicated Usage` from `pdh.dll`. The module exposes `pub(super) fn query_per_process_vram(device_index: u32) -> Result<Vec<(u32, u64)>>`, aggregating PDH's per-`(pid, segment)` rows into one entry per process before returning. Closes the per-process-memory visibility gap on consumer Windows / `WDDM`, where `NVML` returns `NVML_VALUE_NOT_AVAILABLE` and `nvidia-smi --query-compute-apps` writes `[N/A]`. Wave A scaffolding only — Wave B wires the module into the `gpu_processes()` dispatcher and adds Win32-native process-name lookup; until then the new code is gated `dead_code`-allowed at module level. Module doc-comment documents the KB 4490156 graphics-cache-flush drift (irrelevant to CUDA / compute workloads). 11 inline unit tests for the pure helpers (`parse_instance_name`, `parse_multi_string`). See [`docs/roadmap-v0.2.2.md`](docs/roadmap-v0.2.2.md) Wave A and the [PDH module doc-comment](src/gpu/pdh.rs) for the data-source rationale and the deferred segmented-API rationale.
- **`HypomnesisError::Pdh(String)` variant** (`src/error.rs`) — Wave A companion. Patch-safe under `#[non_exhaustive]`. Follows the existing one-variant-per-backend pattern (`Ram`, `Nvml`, `Dxgi`, `NvidiaSmi`); renders as `"PDH error: ..."` via `thiserror` derive. The `Display`-vs-structured-fields contract doc-comment updated to list `Self::Pdh` alongside the other backend-error variants.
- **`crate::gpu::dxgi::adapter_luid(idx) -> Option<(i32, u32)>` helper** — Wave A companion in `src/gpu/dxgi.rs`. Walks `EnumAdapters1` with the same NVIDIA-filter rule as the existing `query` / `adapter_name` helpers and returns the `(HighPart, LowPart)` pair of the `idx`-th NVIDIA adapter's `LUID`. Consumed by `pdh::query_per_process_vram` to correlate PDH counter instances against a specific adapter.
- **`GpuQuerySource::Pdh` variant** (`src/snapshot.rs`) — Wave B of v0.2.2. Patch-safe under `#[non_exhaustive]`. Identifies rows produced via the new Windows `PDH` path (memory from `VidMm` directly, names from `Win32`); unlike `Nvml`/`NvidiaSmi`, `Pdh` rows are **not** compute-only — they surface every GPU user (compositor, browsers, games, compute alike). The semantics shift is documented on both [`GpuQuerySource::Pdh`](src/snapshot.rs) and [`GpuProcessEntry`](src/snapshot.rs).
- **`pdh::name_from_pid_windows` + `basename_from_path` + `HandleGuard`** (`src/gpu/pdh.rs`) — Wave B companion: `Win32`-native process-name resolution via `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)` + `QueryFullProcessImageNameW(PROCESS_NAME_WIN32, ...)`. Cross-platform consistent with Linux `read_proc_comm(/proc/<pid>/comm)` and macOS `proc_pidpath` (per PR #1). Access-denied for cross-user / protected processes yields `name: None`. 7 inline unit tests cover `basename_from_path` across Windows `\` separators, forward slashes, mixed separators, bare names, empty input, trailing separators, and single-separator strings.
- **`tests/live_pdh.rs`** — three `#[ignore]`-gated live tests covering the `Windows + WDDM 2.0+` `PDH` path: PDH rows non-empty, plausible `(pid, used_bytes)` shape, at least one resolved process name. Live tests assert on *observable* invariants (the calling process is **not** required to be in the list, because `gpu_processes` only walks `DXGI` for metadata and doesn't register the caller with `VidMm` — documented in the test-file module doc).

### Changed

- **`gpu_processes()` dispatcher on Windows now uses `PDH` as primary backend** (`src/gpu/mod.rs`) — Wave B of v0.2.2. Order: `NVML` (Linux only — explicitly `cfg(target_os = "linux")`) → `PDH` (Windows primary) → `nvidia-smi` (universal fallback). On Windows, `NVML`'s per-process branch is gated out entirely because under `WDDM` it returns up to 64 rows with `used_gpu_memory == u64::MAX` (R570-class sentinel) that the existing filter drops to an empty vec — that "success with no rows" answer would block `PDH` from running. Closes the dogfooding finding documented in [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) (the maintainer's `hmn ps` going from "0 compute processes found" to **29 GPU processes with real VRAM bytes**, including their own `ollama.exe`, `Code.exe`, `firefox.exe`, etc.). The `hmn ps` semantic shifts from compute-only to all-GPU-users on Windows under this path; the limitations doc-comments reflect the new semantics.
- **`nvml::list_compute_processes` gated `cfg(target_os = "linux")`** (`src/gpu/nvml.rs`) — Wave B companion. The function has only ever been useful on Linux; the `R570`-class `u64::MAX` sentinel behaviour on Windows / `WDDM` makes its output meaningless there. Gating to Linux removes the dead-code warning the dispatcher reorder would otherwise produce, and documents the platform constraint at the function definition rather than scattered through the caller.
- **Deterministic + human-friendly sort order** (`src/gpu/mod.rs`, `src/bin/hmn.rs`) — Wave B follow-up. Library `gpu_processes()` now sorts by `pid` ascending via a new private `sort_by_pid()` helper called from each dispatcher branch — deterministic across calls, matches Unix `ps` / `top -p` / `NVML`'s `nvmlDeviceGetComputeRunningProcesses` convention. CLI `hmn ps` re-sorts by `(used_bytes desc, name, pid)` so the biggest consumers land at the top, the row a user asking "what's eating my GPU memory?" wants to see first. Ties broken by name then PID for stable cross-run order; duplicate-name processes like `msedgewebview2.exe` cluster together.
- **`hmn ps` stderr summary: `"compute process[es]"` → `"GPU process[es]"` + protected-count parenthetical** (`src/bin/hmn.rs`) — Wave C of v0.2.2. The wording shift reflects the `PDH`-path semantic (the list includes every GPU memory holder on Windows, not just `CUDA` processes). The new `(N protected — re-run elevated for names)` parenthetical is appended only when at least one returned row has `name: None` (mirrors the filter-clause "only when active" convention), surfacing the actionable hint that Administrator-level access would resolve those names. Empirically verified on the maintainer's machine: running elevated resolved 3 of 4 `?` rows (`dwm.exe`, `csrss.exe`, `CorsairCpuIdService.exe`); PID 4 (the Windows kernel pseudo-process) remained `?` even elevated because there's no executable image to read. `format_ps_summary` signature changed from `(count, pid_filter, device_filter)` to `(&[PsRow], pid_filter, device_filter)` so it can count protected rows itself; 6 existing tests updated, 5 new tests added (protected-count rendering: singular, plural, all-protected, zero-protected elides parenthetical, protected + filters both appear).
- **`hmn --help` Limitations + `README.md` `## Binary (hmn)` Limitations refresh** (`src/bin/hmn.rs`, `README.md`) — Wave C of v0.2.2. Reflects the post-Wave-B reality: limitation #1 splits per-platform (Linux/NVML compute-only, Windows/PDH all-GPU-users), new limitation about Windows `used_bytes` being WDDM's dedicated commit (not resident set, can exceed physical VRAM — matches Task Manager's `Dedicated GPU memory` column), revised `?` bullet naming the kernel pseudo-process at PID 4 as the guaranteed-unresolvable case (replaces the speculative "PPL-protected" framing from earlier drafts after empirical verification), pre-WDDM-2.0 nvidia-smi fallback documented as a distinct path.
- **`docs/hypomnesis-brief.md` Capabilities table: new PDH row** — Wave C of v0.2.2. Adds *"Per-process VRAM on Windows for foreign processes via `PDH` `\GPU Process Memory(*)\Dedicated Usage` (consumer `WDDM 2.0`+) | **No one — hypomnesis v0.2.2 was first**"* alongside the existing DXGI-calling-process row. Records the pioneering claim explicitly: as of v0.2.2, no other Rust crate exposes per-process foreign-process VRAM on consumer Windows.
- **`src/gpu/pdh.rs` module-level rustdoc: `# used_bytes semantics: dedicated commit, not resident set` section** — Wave C of v0.2.2. Documents that `Dedicated Usage` is WDDM's committed allocation total, not resident-on-GPU bytes, so heavy-graphics processes can show `used_bytes` exceeding the device's physical VRAM. Names the maintainer's empirically observed example (Firefox ~15 GiB committed on a 16 GiB card). Distinct from but related to the KB 4490156 caveat below: KB 4490156 is graphics-cache-flush drift, this is a fundamentally different commit-vs-resident distinction.
- **PID 4 rendered as `[kernel]` instead of `?`** (`src/gpu/pdh.rs`) — Wave C follow-up of v0.2.2. The Windows kernel pseudo-process has no executable image, so `QueryFullProcessImageNameW` fails for fundamental architectural reasons rather than privilege reasons — without a special case it shows as `?` and pollutes the "unresolvable even elevated" set. Wave C's protected-count parenthetical (`hmn ps` stderr: `(N protected — re-run elevated for names)`) is meant as a security-relevant hint distinguishing genuinely-foreign-user processes from the kernel; the special case for PID 4 makes the parenthetical meaningful. New `kernel_name_for_pid` pure helper (2 inline unit tests) is the single source of truth; called from `name_from_pid_windows` as an early-return before the FFI path.
- **Security note in `README.md` Limitations + matching paragraph in `hmn --help`** — Wave C follow-up of v0.2.2. Documents the threat-model implication of the `?` rendering: by construction, a `?` row that doesn't resolve under elevation is either a foreign-user / `SYSTEM` process, a `PPL`-protected process, or a transient race. None of these are intrinsically malicious, but on a single-user desktop an *unexpected* `?` row holding substantial VRAM is worth investigating — a malicious local process (including a privileged-or-cross-user AI agent) using GPU resources would land in exactly this set. Frames `hypomnesis`'s honesty about the gap as a defensive primitive, while explicitly stating the crate is a measurement tool, not a malware scanner.

## [0.2.1] - 2026-05-13

> *Sharper, not wider. Same surface — easier to test against, kinder to repeat callers.*

`v0.2.1` is a patch release composed entirely of wear-and-tear feedback from `hf-fetch-model 0.10.1`, the first external consumer to adopt `hypomnesis`. The five waves — `test-helpers` builder, `name_or_unknown` convenience, `format_total` / `format_used` parity for `report`-feature consumers, the `HypomnesisError` `Display`-vs-structured-fields contract, and a docs pass on the brief and the `README.md` "Used by" — are each small and additive under `#[non_exhaustive]`, but they land together because dogfooding produced a coherent set: every item closes a gap a real downstream developer actually hit, not a gap synthesised from imagined use. The motivating principle going forward — *every patch release is informed by at least one real consumer's adoption experience* — is itself one of the v0.2.1 deliverables. See [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) for the wave-by-wave rationale and [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) for the underlying adoption report.

### Added

- **`test-helpers` Cargo feature + `GpuDeviceInfoBuilder`** (`src/snapshot.rs`) — Wave A of the v0.2.1 roadmap. Default-off, additive feature exposing a chained builder (`GpuDeviceInfo::builder().index(...).name(...).total_bytes(...).free_bytes(...).used_bytes(...).build()`) so downstream test fixtures can synthesise `GpuDeviceInfo` values that `#[non_exhaustive]` would otherwise forbid via struct-literal syntax. Unblocks the render-path / arithmetic unit tests in `hf-fetch-model`'s `gpu_check.rs` and the equivalent in `candle-mi` when it adopts. The chosen design — feature-gated builder rather than positional `synthetic(...)` constructor — preserves the future-proofing `#[non_exhaustive]` provides: new fields on `GpuDeviceInfo` will be exposed as new defaulted setters here without breaking existing test code. 6 inline tests cover defaults, individual setters, and a full round-trip. Production code must never enable the feature; the doc-comment makes the semver caveat explicit. See [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) Wave A and [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) finding #1 for rationale.
- **`GpuDeviceInfo::name_or_unknown(&self) -> &str`** (`src/snapshot.rs`) — Wave B of the v0.2.1 roadmap. Always-on convenience method returning `self.name.as_deref().unwrap_or("unknown GPU")`. The wear-and-tear concern is **not** keystroke savings (it's one line at the call site) but consumer divergence: without an upstream nudge, multiple consumers will land on different fallback phrases (`"unknown GPU"` vs `"Unknown"` vs `"<unknown>"`). The doc-comment explicitly notes the string is **not** localized — consumers needing other languages match on `name` directly. 2 inline tests cover the `Some` and `None` paths. See [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) Wave B and [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) finding #3 for rationale.
- **README `## Used by` section + `docs/hypomnesis-brief.md` correction** — Wave E of the v0.2.1 roadmap. README's `## Used by` body replaced from "No consumers yet" to a one-liner listing `hf-fetch-model` (matching the convention used in `hf-fm`'s own README) plus a "Forthcoming" italics line preserving the `candle-mi` forward-reference. The brief's *"hf-fm uses ~10% of hypomnesis's API surface (`device_info` + `device_count`)"* sentence corrected to reflect actual v0.10.1 usage (only `device_info`; `device_count` deferred to the multi-GPU follow-up at `hf-fm` v0.10.4). The brief's stale "Phase 2 — blocked" Status line updated to "Phase 2 — shipped" with a cross-link to the dogfooding adoption report. Pure text edits, no code changes. See [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) Wave E and [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) finding #4 for rationale.
- **`HypomnesisError` `Display`-vs-structured-fields contract** (doc-only, `src/error.rs`) — Wave D of the v0.2.1 roadmap. Appends a `# Display vs structured fields` section to the `HypomnesisError` type-level doc-comment, codifying the convention that `Display` is the default English one-liner (suitable for logs and `?`-propagation) while structured fields (`DeviceIndexOutOfRange { index, count }`, the inner `String` of `Nvml` / `Dxgi` / `NvidiaSmi`) are the canonical source for consumers that need to localize, restyle for CLI / GUI / JSON output, or apply singular/plural agreement. The contract is testable: any future change that breaks the localization, restyle, or plural-agreement use cases is a contract violation rather than a debatable refactor. Pins down the choice `hf-fm 0.10.1` made (rendering `"1 device"` from the structured `count` field rather than the default `"1 devices"` `Display` string) so the next consumer doesn't re-discover it from scratch. No code changes. See [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) Wave D and [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) finding #5 for rationale.
- **`GpuDeviceInfo::format_total` and `GpuDeviceInfo::format_used`** under `#[cfg(feature = "report")]` (`src/snapshot.rs`) — Wave C of the v0.2.1 roadmap. Format `  GPU <idx>: total <T> MB[ [<adapter name>]]\n` and `  GPU <idx>: used <U> MB[ [<adapter name>]]\n` respectively, mirroring the existing `format_free` style exactly (two-space indent, `MB`-displayed-for-`MiB`, trailing newline, optional ` [<name>]` suffix omitted when `name` is `None`). Closes the format-the-three-numbers boilerplate gap for `report`-feature consumers — `candle-mi` v0.2 is the immediate adopter. Option (a) parity over option (b) `format_summary` and option (c) tuple-return: smallest API surface, strict parity extension of `format_free` with no new format to defend. 6 inline tests cover name-present / name-absent / edge-case (`free=0` for total, `used=0` for used) for each method, matching the existing `format_free` test suite shape. **Honest caveat:** does not retroactively serve `hf-fm 0.10.1` — `format_free`'s opinionated MB/indent/newline shape mismatches `hf-fm`'s GiB / no-indent / column-aligned output ([`gpu_check.rs:201-210`](https://github.com/PCfVW/hf-fetch-model/blob/main/src/gpu_check.rs#L201-L210)). The wave lands for `report`-feature consumers (candle-mi and future), not for `hf-fm`. See [`docs/roadmap-v0.2.1.md`](docs/roadmap-v0.2.1.md) Wave C and [`docs/hypomnesis-adoption.md`](docs/hypomnesis-adoption.md) finding #2 for rationale.

## [0.2.0] - 2026-05-06

> *Wider, not taller. Same job — more callers can ask, more devices can answer.*

`v0.2.0` widens the public API without breaking callers. Wave A adds the `report`-feature `format_free` / `print_free` helpers on `GpuDeviceInfo` for the LM-Studio-style headroom check. Wave B adds `Snapshot::all` for multi-adapter enumeration on Windows (NVIDIA dGPUs via `NVML` plus non-NVIDIA `DXGI` adapters such as Intel / AMD `iGPU`s). Wave C adds `gpu_processes` for compute-process listing and ships the `hmn` CLI binary behind a default-off `cli` feature. Public types remain `#[non_exhaustive]` per `v0.1.0` policy, so further additions (AMD `ROCm`, Apple Metal) can land in `0.2.x` patches without breaking callers. See [`docs/roadmap-v0.2.0.md`](docs/roadmap-v0.2.0.md) for the wave-by-wave rationale and the verification plan.

### Added

- **`hmn` CLI binary behind the `cli` feature** (`src/bin/hmn.rs`, new file) — Wave C of the v0.2.0 roadmap. Default-off feature so library users don't pull `clap`; install with `cargo install hypomnesis --features cli`. Two subcommands:
  - `hmn` (default): one-line-per-GPU device summary using `Snapshot::all`. Includes AMD / Intel iGPUs on Windows alongside NVIDIA dGPUs.
  - `hmn ps`: list compute processes holding GPU memory (CUDA-only). Flags `--pid PID`, `--device INDEX`, `--json`. Default output is a fixed-column text table (`PID NAME VRAM DEVICE`); `--json` emits a hand-rolled JSON array (no `serde` dep — keeps the CLI feature lean).
  - `--help` and the README spell out the documented limitations: compute-only enumeration, Windows process names may be `?` for protected processes, R570 `u64::MAX` sentinel and `used > total` checks applied per row, Windows attribution is `nvidia-smi`-backed (DXGI's `QueryVideoMemoryInfo` only answers for the calling process). 18 inline tests cover the formatting primitives.
- **`gpu_processes(device_index) -> Result<Vec<GpuProcessEntry>>`** (`src/gpu/mod.rs`) — new public library API listing every compute process holding GPU memory on the given device. Source priority: `NVML` (Linux primary; `nvmlDeviceGetComputeRunningProcesses_v3` for `(pid, used_bytes)` plus `/proc/<pid>/comm` for names; capped at 64 processes per device — the existing `NVML` stack-buffer size); `nvidia-smi` (Windows primary, Linux fallback; subprocess `--query-compute-apps=pid,process_name,used_memory --format=csv,noheader,nounits --id=N`). `DXGI` is intentionally not used — `IDXGIAdapter3::QueryVideoMemoryInfo` only answers for the calling process. Per-row WDDM bug parity: `u64::MAX` sentinel and `used > device_total` checks drop garbage rows rather than reporting them.
- **`GpuProcessEntry` public type** (`src/snapshot.rs`) — `#[non_exhaustive]` struct with `pid`, `name: Option<String>`, `used_bytes`, `source: GpuQuerySource`. Distinct from the existing `ProcessGpuInfo` (which describes the *calling* process); `GpuProcessEntry` is one row of an enumeration over **all** compute processes on a device. Re-exported from the crate root.
- **`nvml::list_compute_processes` (`src/gpu/nvml.rs`, `pub(super)`)** — new internal helper yielding `Vec<(u32, u64)>` of `(pid, used_bytes)` pairs for `gpu_processes` to wrap. Standalone `nvmlInit_v2` / `nvmlShutdown` cycle, mirrors the existing `query()` pattern. Reuses `NVML_MAX_PROCESSES = 64` and applies the same sentinel + sanity checks as `read_process_used`.
- **`nvidia_smi::query_compute_apps` + `ComputeApp` struct** (`src/gpu/nvidia_smi.rs`, `pub(super)`) — new internal helper spawning `nvidia-smi --query-compute-apps=pid,process_name,used_memory ...` and parsing the CSV. Robust to `process_name` values containing commas (Windows technically allows them in filenames): the parser splits on the **last** comma first to isolate `used_memory`, then on the **first** comma of the remainder to isolate `pid` from `name`. 7 inline parser tests cover basic / protected-name (`?`) / name-with-comma / empty-line / unparseable-pid / unparseable-memory / too-few-fields cases.
- **`read_proc_comm(pid)` Linux helper** (`src/gpu/mod.rs`, `#[cfg(all(target_os = "linux", feature = "nvml"))]`) — reads `/proc/<pid>/comm`, trims trailing newline, returns `None` on any read failure. Best-effort name resolution for the Linux NVML path.
- **Smoke test in `tests/smoke.rs`** for `gpu_processes(0)` — accepts `Ok(Vec)` (with sanity checks on PID and source) or the expected `Err(NoGpuSource | DeviceIndexOutOfRange)` on hosted runners.
- **Live test in `tests/live_gpu.rs`** for `gpu_processes(0)` — `#[ignore]`-gated; asserts `Ok(...)` and length-tolerantly validates whatever rows exist (compute-only enumeration may be empty on a vanilla test binary).
- **`Snapshot::all()` — multi-adapter enumeration** (`src/snapshot.rs`, `src/gpu/mod.rs`, `src/gpu/dxgi.rs`) — Wave B of the v0.2.0 roadmap. Returns one `Snapshot` per visible GPU, sharing a single `RSS` measurement (per-snapshot re-measurement would add no useful precision; the wall-time delta across the GPU walk is microseconds). Linux: enumerates NVIDIA dGPU(s) via `NVML`. Windows: enumerates NVIDIA dGPU(s) via `NVML` plus every other `DXGI` adapter that exposes `DedicatedVideoMemory > 0` or `SharedSystemMemory > 0`. Microsoft Basic Render Driver (`VendorId = 0x1414`) is always skipped. NVIDIA adapters are reported via `NVML` (correct device-wide totals, driver-side `free`); non-NVIDIA Windows adapters use `DXGI` for per-process `CurrentUsage` and `DedicatedVideoMemory`-or-`SharedSystemMemory` for `total_bytes` (the latter is the right number for `iGPU`s without `BIOS`-allocated `UMA`). Indices are contiguous: NVIDIA part is `NVML`-canonical 0..N-1, non-NVIDIA Windows extras get N, N+1, …  — and the non-NVIDIA indices are **not** addressable via `Snapshot::now`. Empty `Vec` when no GPUs are visible (RAM-only callers should use `process_rss` or `Snapshot::now`).
- **`DxgiAdapterEntry` + `enumerate_non_nvidia()`** in `src/gpu/dxgi.rs` (Windows + `dxgi` feature) — internal walker that returns one entry per non-NVIDIA, non-`MSBR` adapter with name, per-process `LOCAL` `CurrentUsage`, `DedicatedVideoMemory`, and `SharedSystemMemory`. Vendor ID is consumed inline during the walk (filter + `debug-output` line) and not stored on the entry. Best-effort: an adapter that doesn't expose `IDXGIAdapter3` contributes `0` for `current_usage` rather than failing the whole walk. Surfaces under `feature = "debug-output"` with one line per qualifying adapter.
- **`crate::gpu::dxgi_non_nvidia_devices(starting_index)`** in `src/gpu/mod.rs` (Windows + `dxgi` feature) — converts `DxgiAdapterEntry` rows into `(GpuDeviceInfo, ProcessGpuInfo)` pairs with sequential indices, ready for `Snapshot::all` to wrap. `total_bytes` selection: `DedicatedVideoMemory` when non-zero, else `SharedSystemMemory`. `is_per_process = true` because `DXGI`'s `CurrentUsage` is `WDDM`-aware.
- **`MICROSOFT_BASIC_VENDOR_ID = 0x1414`** in `src/gpu/dxgi.rs` — named const for the synthetic adapter every Windows install ships with; previously implicit in the `query`-path filter (which keyed on `VendorId == 0x10DE`), now explicit so `enumerate_non_nvidia` can exclude it without reproducing the magic number.
- **Smoke test in `tests/smoke.rs`** for `Snapshot::all()` — asserts every returned entry carries positive `ram_bytes`. Length-tolerant so it passes on CI runners (empty `Vec`) and on hardware alike.
- **Live test in `tests/live_gpu.rs`** for `Snapshot::all()` — `#[ignore]`-gated; asserts NVIDIA enumeration produces at least one entry with monotonic indices starting at 0, and on Windows checks the second entry (if present) is reported via `GpuQuerySource::Dxgi` and is per-process.
- **`GpuDeviceInfo::format_free` and `GpuDeviceInfo::print_free`** under `#[cfg(feature = "report")]` (`src/snapshot.rs`) — Wave A of the v0.2.0 roadmap. Format: `  GPU <idx>: free <N> MB / <T> MB[ [<adapter name>]]\n`. Mirrors the existing `Snapshot::ram_mb` / `vram_mb` convention (feature-gated `impl` block on the type, in the file that defines it) so callers reach for `dev.print_free()` rather than a free function. The motivating use case is the LM-Studio-style headroom check — *"if I load this model now, will it fit?"* — already a one-liner via the existing `free_bytes` field; this helper makes the printed reporting equally short. `print_free` delegates to `format_free`, locking the format under unit-test verification (4 inline tests covering name-present / name-absent / fully-allocated device / `print_*` smoke). The roadmap's proposed `pub fn free_bytes(&self) -> u64` method was dropped from this wave: `pub free_bytes: u64` already exists as a field on `GpuDeviceInfo`, satisfies the same ergonomics goal, and on the `NVML` path stores the driver's actual free count (which is more accurate than `total - used` once driver-side reservation/alignment is considered).

### Changed

- **`hmn ps` now writes a one-line summary to stderr after each run** (`src/bin/hmn.rs`) — `hmn: <N> compute process[es] found[ matching <filters>].`. Always printed (zero or non-zero count), including in `--json` mode, so interactive users get an unambiguous "command worked, here's the count" line without breaking stdout's scriptability — pipelines like `hmn ps | awk 'NR>1 {…}'` or `hmn ps --json | jq` continue to see only the table or JSON array on stdout. Filter clause is appended only when `--pid` and/or `--device` is active. Pluralisation handles 0 / 1 / N correctly. 6 inline tests for `format_ps_summary` cover the cross-product. Redirect `2>/dev/null` to suppress.
- **`GpuQuerySource::NvidiaSmi` rustdoc clarified** (`src/snapshot.rs`) — the variant doc previously said "(device-wide)", which was accurate for `ProcessGpuInfo` but misleading after Wave C added `GpuProcessEntry`, where each `NvidiaSmi`-sourced row is per-process (one row per `CUDA` process from `nvidia-smi --query-compute-apps`). The doc now spells out both contexts.
- **`src/lib.rs` Capabilities + Feature-flags tables updated** — added a new "Compute-process listing (other PIDs)" row (`nvidia-smi --query-compute-apps` on Windows, `NVML` + `/proc/<pid>/comm` on Linux) and a row for the `cli` feature.
- **`src/gpu/mod.rs` module doc** — "the three dispatchers" updated to "the four dispatchers" with `gpu_processes` listed alongside `device_count`, `device_info`, `process_gpu_info`.
- **`src/gpu/nvml.rs` module doc** — "two crate-internal entry points" updated to "three" with `list_compute_processes` listed alongside `query` and `device_count`. The R570 caveat now names both `query` and `list_compute_processes` as sentinel-detecting sites.
- **iGPU + dGPU verification claims corrected to reflect actual hardware** (`docs/roadmap-v0.2.0.md`, `README.md`, `tests/live_gpu.rs`) — the maintainer's Windows reference machine is `Ryzen 9 5950X + RTX 5060 Ti`. The 5950X has no integrated GPU (only the "G" Ryzen variants do), so multi-adapter scenarios cannot be verified end-to-end on this hardware. Previous wording in the roadmap, the README's `hmn` example output, and the `Snapshot::all` live test claimed `RTX 5060 Ti + AMD iGPU` — that claim was inherited from earlier drafts and never matched the actual hardware. The `enumerate_non_nvidia` code path runs correctly (debug-output traces confirm the `EnumAdapters1` walk) and is exercised by length-tolerant live tests; the multi-adapter assertions inside `snapshot_all_enumerates_nvidia_and_optional_extras` simply have not been triggered yet because `snaps.get(1)` returns `None` on this machine. The README now shows two output blocks: actual single-GPU output captured here, and a clearly-labelled illustrative multi-GPU example. Future hands-on verification waits on either an iGPU-equipped test machine or an external contributor's PR.
- **`GpuDeviceInfo::index` rustdoc expanded** (`src/snapshot.rs`) — clarifies that the field is `NVML`-canonical for `Snapshot::now` and the NVIDIA portion of `Snapshot::all`, and a synthetic post-NVIDIA index for non-NVIDIA Windows adapters surfaced by `Snapshot::all` (which are *not* addressable via `Snapshot::now`).
- **`K32GetProcessMemoryInfo` failure now includes the `GetLastError` code** (`src/ram.rs`) — the Windows `RAM` error message previously returned a static `"K32GetProcessMemoryInfo failed"` string with no diagnostic context. The `win_ffi` block now also imports `GetLastError`, and `windows_rss` formats the returned code into the `HypomnesisError::Ram` payload (`"K32GetProcessMemoryInfo failed (GetLastError = N)"`). Failure of this call on the current-process pseudo-handle is exceedingly rare, but if it does fire the user now has a `WinError` code to look up rather than an opaque message.
- **`Snapshot::now` rustdoc expanded with `# Performance` and per-process sections** (`src/snapshot.rs`) — surfaces two facts that were previously documented only in private modules / on `ProcessGpuInfo`:
  - **Performance:** each call performs a full `NVML` init/shutdown cycle (and, on Windows, a fresh `IDXGIFactory1` walk), adding a few milliseconds per call — fine for occasional sampling, less ideal for tight per-frame polling. A long-lived `NVML` context is planned for v0.2.
  - **Per-process vs device-wide:** when the dispatcher falls back to `nvidia-smi` (no `NVML`/`DXGI` available, or `WDDM` `NVML_VALUE_NOT_AVAILABLE`), `gpu.used_bytes` reflects the device-wide total, not the calling process. Callers needing true per-process accounting should check `gpu.is_per_process` before interpreting the value.

## [0.1.0] - 2026-04-29

First functional release. Wave 2 of Phase 1 — ports the actual measurement code from [candle-mi/src/memory.rs](https://github.com/PCfVW/candle-mi/blob/main/src/memory.rs) (889 lines) into the `0.0.1` placeholder skeleton.

### Added

- **Process `RSS` measurement** — `process_rss` (in `src/ram.rs`) returns the per-process resident-set size in bytes. Windows: `K32GetProcessMemoryInfo` → `WorkingSetSize` via an `unsafe extern "system"` block. Linux: `/proc/self/status` → `VmRSS`, parsing logic extracted as `parse_vmrss(&str)` for unit-testability (6 inline parsing tests for various `/proc/self/status` shapes).
- **`NVML` backend** (`src/gpu/nvml.rs`) — dynamically loads `libnvidia-ml.so.1` (Linux) or `nvml.dll` (Windows) via `libloading`. Symbols loaded: `nvmlInit_v2`, `nvmlShutdown`, `nvmlDeviceGetHandleByIndex_v2`, `nvmlDeviceGetMemoryInfo`, `nvmlDeviceGetComputeRunningProcesses_v3`, **`nvmlDeviceGetCount_v2`** (new vs candle-mi, for `device_count`), and **`nvmlDeviceGetName`** (new vs candle-mi, for the adapter name on the `NVML` path — Wave 2 decision #1). Two crate-internal entry points: `query(idx)` for combined per-process + device-wide queries in a single init/shutdown cycle, and `device_count()`. Includes the `R570` `u64::MAX` sentinel guard and `used > total` sanity check ported from candle-mi.
- **`DXGI` backend** (`src/gpu/dxgi.rs`, Windows-only) — walks `IDXGIFactory1::EnumAdapters1` filtering by NVIDIA vendor ID (`0x10DE`) + non-zero `DedicatedVideoMemory`, casts to `IDXGIAdapter3`, calls `QueryVideoMemoryInfo(DXGI_MEMORY_SEGMENT_GROUP_LOCAL)`. Three entry points: `query` (full per-process + device + name), `adapter_name` (lightweight name-only path that skips `QueryVideoMemoryInfo`), and `device_count`. The `WDDM`-aware per-process path is the only reliable per-process VRAM source on Windows.
- **`nvidia-smi` subprocess fallback** (`src/gpu/nvidia_smi.rs`) — runs `nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader,nounits --id=N` and parses the single CSV line. Saturating `MiB → bytes` conversion. The `--id=N` argument is new vs candle-mi (which hardcoded device 0). Returns `Option<NvidiaSmiResult>` (a struct with `used_bytes` and `total_bytes`), making the all-or-nothing invariant explicit at the type level.
- **Dispatchers** (`src/gpu/mod.rs`) — three public functions try backends in priority order:
  - `process_gpu_info`: `DXGI` (Windows) → `NVML` (Linux primary; `NVML_VALUE_NOT_AVAILABLE` under Windows `WDDM`) → `nvidia-smi` (device-wide, sets `is_per_process = false`).
  - `device_info`: `NVML` for total/free/used + `DXGI` for friendlier adapter `name` on Windows → `DXGI`-alone fallback (loose semantics: `CurrentUsage` is per-process, used as approximate device-wide `used` per Wave 2 decision #2) → `nvidia-smi` device-wide.
  - `device_count`: `NVML` → `DXGI`. Returns `HypomnesisError::NoGpuSource` when no enumeration backend is enabled or every enabled backend failed to report a count.
  - Returns `HypomnesisError::DeviceIndexOutOfRange` when `NVML` or `DXGI` reports a count and the requested index is past it.
- **`Snapshot::ram_mb()` and `Snapshot::vram_mb()`** under `#[cfg(feature = "report")]` (`src/snapshot.rs`) — located on `Snapshot` (rather than on `MemoryReport`) for parity with `candle-mi`'s `MemorySnapshot::ram_mb` / `vram_mb` API location, so candle-mi v0.2 adoption is a thin adapter wrapper rather than a code rewrite (Wave 2 decision #6).
- **`MemoryReport` real bodies** (`src/report.rs`) — `ram_delta_mb`, `vram_delta_mb`, `vram_qualifier` (`const fn`), plus the printing helpers `print_delta` / `print_before_after`. Output formats preserved verbatim from candle-mi for migration parity.
- **`MemoryReport::format_delta` and `format_before_after`** — `String`-returning siblings of the printing helpers (new vs candle-mi). Same byte-for-byte output as the `print_*` methods, but as an owned `String` for log frameworks (`tracing`, `log`), file output, or test assertions. The `print_*` methods now delegate here, locking the format under unit-test verification.
- **`examples/print_demo.rs`** — runnable demo (`cargo run --features report --example print_demo`) that takes two snapshots around a 50 MiB allocation and prints the delta + before→after via all four `MemoryReport` formatters. See `examples/README.md`.
- **`debug-output` feature diagnostics across all three GPU backends** (Wave 2 decision #7) — `eprintln!` traces at every `NVML` return code, every `DXGI` adapter resolved, every `nvidia-smi` parse step, plus final result summaries.
- **Inline unit tests** in `src/snapshot.rs` (6 tests for `Snapshot` construction + `ram_mb` / `vram_mb` conversion) and `src/report.rs` (15 tests for `ram_delta_mb` / `vram_delta_mb` / `vram_qualifier` / `format_delta` / `format_before_after` byte-for-byte string equality + `print_*` smoke).
- **Smoke tests** (`tests/smoke.rs`) extended with `process_rss > 0` and end-to-end `Snapshot::now` checks (no GPU dependency required — succeed on CI without NVIDIA hardware).
- **Live-GPU tests** (`tests/live_gpu.rs`) — `#[ignore]`-gated per Wave 2 decision #5; run via `cargo test -- --ignored` on a machine with an NVIDIA GPU + driver. 5 tests covering `device_count`, `device_info`, `process_gpu_info` (with platform-aware source assertions: `Dxgi` on Windows, `NvidiaSmi` or `Nvml` on Linux), `Snapshot::now`, and `DeviceIndexOutOfRange`. Verified locally on Windows (RTX 5060 Ti, native `NVML` + `DXGI`) and on Ubuntu WSL2 (`NVML` via NVIDIA's CUDA-on-WSL driver).

### Changed

- Backend module visibility flipped from `pub mod` to `mod` (Wave 2 decision #4) — the public API surface in `crate::gpu` is now exactly the three dispatchers; backend internals (`nvml`, `dxgi`, `nvidia_smi`) are crate-private. Items inside those private modules use `pub(super)` (rather than bare `pub`) to make the "visible only to the parent dispatcher module" intent explicit.
- `device_index: u32` semantics on Windows now filter by NVIDIA vendor ID (`0x10DE`) — more precise than candle-mi's `DedicatedVideoMemory > 0` alone on multi-vendor systems (Intel iGPU + AMD dGPU + NVIDIA dGPU).

## [0.0.1] - 2026-04-29

Initial scaffold (Phase 1 of the v0.1 plan; not for production use). The
function bodies are placeholders that compile and pass clippy under
`-D warnings`; Wave 2 ports the actual measurement code from
[candle-mi/src/memory.rs](https://github.com/PCfVW/candle-mi/blob/main/src/memory.rs)
(889 lines).

### Added

- **Crate scaffold** — `Cargo.toml` (edition 2024, MSRV 1.88, MIT OR Apache-2.0), source-file skeleton at `src/lib.rs`, `src/error.rs`, `src/snapshot.rs`, `src/ram.rs`, `src/gpu/{mod,nvml,dxgi,nvidia_smi}.rs`, and `src/report.rs` (gated on the `report` feature).
- **Public API types** — `HypomnesisError`, `Result`, `Snapshot`, `GpuDeviceInfo`, `ProcessGpuInfo`, `GpuQuerySource`, and (with the `report` feature) `MemoryReport`. All public enums and structs are `#[non_exhaustive]` for forward compatibility — new variants and fields land in patch releases without breaking callers. The brief's settled-decisions section explains why future-proofing rests on `#[non_exhaustive]` rather than on parameter type elaboration.
- **Feature flags** — defaults `nvml` (`NVML` dynamic load via `libloading`), `dxgi` (Windows per-process `VRAM` via `IDXGIAdapter3::QueryVideoMemoryInfo`, no-op on non-Windows), `nvidia-smi-fallback` (subprocess fallback when `NVML` / `DXGI` fail). Opt-in `report` (the `candle-mi` parity suite: `MemoryReport` + `print_delta` / `print_before_after` / `ram_mb` / `vram_mb`) and `debug-output` (raw `NVML` / `DXGI` values to stderr).
- **`CONVENTIONS.md`** — Grit + Grit-HMN extensions, aligned with [`anamnesis/CONVENTIONS.md`](https://github.com/PCfVW/anamnesis/blob/main/CONVENTIONS.md) and [`candle-mi/CONVENTIONS.md`](https://github.com/PCfVW/candle-mi/blob/main/CONVENTIONS.md). Adds `NVML` / `DXGI` / `K32GetProcessMemoryInfo` SAFETY guidance and a five-step recipe for adding a new GPU backend (e.g., AMD `ROCm`, Apple Metal). Drops the SIMD-specific anamnesis sections that don't apply to a measurement crate.
- **`docs/hypomnesis-brief.md`** — design document with v0.1 scope, settled-decisions section, and three-phase roadmap (Phase 1 = extraction, Phase 2 = `hf-fetch-model` adopts, Phase 3 = `candle-mi` migrates).
- **`README.md`** — project overview with badges (CI, crates.io, docs.rs, MSRV, license, unsafe-deny, NVIDIA NVML+DXGI), install, usage, capability matrix, feature flags, license, and development conventions. Mirrors the structure used in [`anamnesis/README.md`](https://github.com/PCfVW/anamnesis/blob/main/README.md).
- **`[package.metadata.docs.rs]`** — docs.rs builds with `all-features = true` and targets both `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`, exposing the Windows-only `dxgi` module on docs.rs alongside the cross-platform `nvml` path.

[Unreleased]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.14...HEAD
[0.2.14]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.13...v0.2.14
[0.2.13]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.12...v0.2.13
[0.2.12]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.11...v0.2.12
[0.2.11]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.10...v0.2.11
[0.2.10]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.9...v0.2.10
[0.2.9]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.7...v0.2.8
[0.2.7]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.6...v0.2.7
[0.2.6]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/mi-for-the-rust-of-us/hypomnesis/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/mi-for-the-rust-of-us/hypomnesis/releases/tag/v0.0.1
