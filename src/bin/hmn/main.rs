// SPDX-License-Identifier: MIT OR Apache-2.0

//! `hmn` — GPU memory CLI for `hypomnesis`, built via the (since v0.2.8)
//! default-on `cli` feature.
//!
//! Subcommands:
//!
//! - `hmn` (default) — one line per visible GPU with free / total `VRAM`.
//!   Uses [`hypomnesis::Snapshot::all`], so on Windows the AMD / Intel
//!   `iGPU` surfaces alongside the NVIDIA dGPU(s); on macOS the Apple
//!   Silicon `SoC` surfaces as a single `UMA` device. `--json` emits the
//!   same data as a JSON array instead of text.
//! - `hmn ps` — list processes holding GPU memory across one or all
//!   visible devices. On Linux (`NVML`) the list is compute-only; on
//!   Windows (`PDH`, `WDDM 2.0`+) the list includes every GPU memory
//!   holder (compositor, browsers, games, compute); on macOS (Metal
//!   ledger) the list enumerates every process holding
//!   `graphics_footprint` bytes the sandbox lets it read. See the `--help` Limitations text and
//!   the rustdoc for [`hypomnesis::gpu_processes`] for the
//!   per-platform breakdown.
//! - `hmn spill -- <command>` — run a command while polling
//!   [`hypomnesis::SpillTracker`] (`time(1)`-style wrapper, default
//!   100 ms interval), print a `SpillReport` to stderr when the
//!   command exits, and pass its exit code through. Windows /
//!   `WDDM`-only measurement; on other platforms the command still
//!   runs and the report says "spill not measurable".
//! - `hmn watch [PID...]` — attach to already-running PID(s) (or
//!   auto-select the top-N by committed VRAM) and sample
//!   [`hypomnesis::SpillTracker`] plus per-PID committed/shared VRAM on a
//!   timer, printing one row per PID per interval with deltas and the
//!   shipped v0.2.5 SPILL condition. `time(1)`-style scrolling sampler —
//!   not a TUI, same discipline as `hmn spill`. Exit code conveys whether
//!   spill was observed during the watch, for scripts/watchdogs.
//! - `hmn fits <SIZE>` (since v0.2.11) — headroom predicate: exit `0` if
//!   `SIZE` fits in the target device's current free VRAM, `1` if it
//!   doesn't, `2` on a hard error. Gateable from a run script instead of
//!   hand-rolling an `hmn --json | jq` check before every launch.
//!
//! Install with `cargo install hypomnesis` (the `cli` feature is
//! default-on since v0.2.8; `--features cli` is still accepted but
//! redundant).
//!
//! Layout (since v0.2.12): this file holds the `clap` definitions and
//! the dispatch in `main`; each subcommand lives in its own module
//! (`summary`, `ps`, `spill`, `watch`, `fits`), and the primitives they
//! share — byte units, durations, timestamps, JSON escaping, the
//! column-table renderer, the SPILL-cell glyphs — live in `format`. Each module
//! carries its own tests; fixtures used by more than one module's tests
//! live in `test_support`.

use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::fits::run_fits;
use crate::format::{parse_duration, parse_filter_pattern, parse_size_bytes};
use crate::ps::{PsFilters, SortKey, run_ps};
use crate::spill::run_spill;
use crate::summary::run_summary;
use crate::watch::{Selection, run_watch};

mod fits;
mod format;
mod ps;
mod spill;
mod summary;
#[cfg(test)]
mod test_support;
mod watch;

/// `hmn` CLI: device summary plus GPU-process listing.
#[derive(Parser, Debug)]
#[command(
    name = "hmn",
    version,
    about = "GPU memory CLI: device summary (default) + GPU-process listing (`hmn ps`).",
    long_about = "GPU memory CLI for hypomnesis.\n\
                  \n\
                  Default subcommand: prints one line per visible GPU with free / total VRAM. \
                  `--json` emits the same data as a JSON array instead (fields: index, name, \
                  total_bytes, free_bytes, used_bytes, reserved_bytes, driver_version).\n\
                  \n\
                  `hmn ps`: lists processes holding GPU memory.\n\
                  \n\
                  `hmn spill -- <command>`: runs a command while sampling WDDM spill state \
                  (resident shared-memory growth under dedicated-VRAM saturation), prints a \
                  SpillReport to stderr on exit, and passes the command's exit code through.\n\
                  \n\
                  `hmn watch [PID...]`: attaches to already-running PID(s) (or auto-selects the \
                  top `--top` processes by committed VRAM when none are given) and samples spill \
                  state plus per-PID VRAM on a timer, printing one row per PID per interval with \
                  deltas. Not a TUI — a scrolling time(1)-style sampler, same discipline as \
                  `hmn spill`. Exit code conveys whether spill was observed, for scripts/watchdogs.\n\
                  \n\
                  `hmn fits <SIZE>` (since v0.2.11): headroom predicate. Exit `0` if SIZE fits in \
                  the target device's current free VRAM (--device, default 0), `1` if it doesn't, \
                  `2` on a hard error. SIZE uses the same syntax as `hmn ps --min`: a bare byte \
                  count, or a decimal number with KiB/MiB/GiB. Prints one \
                  line to stderr either way; no --json — the point is a scriptable exit code, not \
                  structured output. Gateable from a run script instead of a hand-rolled \
                  `hmn --json | jq` check before every launch.",
    // The per-platform Limitations follow the command list rather than
    // precede it: in `long_about` they pushed the commands several screens
    // down. `-h` names where they are.
    after_help = "Per-platform limitations are listed at the end of `hmn --help`.",
    after_long_help = "Limitations (per-platform):\n\
                  - Linux / NVML backend is compute-only — only processes with an active CUDA \
                  context appear. Browsers using GPU compositing, games, and pure-graphics \
                  apps do not.\n\
                  - Windows / PDH backend (consumer WDDM 2.0+) lists EVERY GPU memory holder: \
                  the desktop compositor, browsers, games, and CUDA / compute alongside. The \
                  semantic shift from the Linux compute-only list is intentional and reflects \
                  what `VidMm` actually accounts for.\n\
                  - Windows `used_bytes` reflects WDDM's dedicated commit, not resident set. \
                  Under WDDM a process can commit GPU allocations exceeding physical VRAM — \
                  the kernel pages them via the shared system memory budget. Numbers \
                  exceeding the device's total VRAM are real, not bugs; they match Task \
                  Manager's `Dedicated GPU memory` column.\n\
                  - The SHARED column (Windows / PDH only) shows resident shared-system-memory \
                  bytes — the WDDM spill signal, matching Task Manager's `Shared GPU memory` \
                  column. A benign baseline (staging/upload heaps) is normal; growth while \
                  dedicated VRAM saturates is spill. Always 0 on Linux and macOS (no \
                  shared-residency counter exists there).\n\
                  - The SPILL column (`hmn ps`, since v0.2.11) is a *single-snapshot* \
                  approximation of `hmn watch`'s spill co-condition: adapter dedicated commit at \
                  or above the 85% threshold AND adapter shared-resident at or above 256 MiB — \
                  an absolute floor, not growth above a baseline, because a one-shot listing has \
                  no history to measure growth against. Not equivalent to `hmn watch`'s verdict \
                  for the same instant. On a spilling device (since v0.2.13) the process being \
                  paged — SHARED at or above the same 256 MiB floor — reads `PAGED` and the \
                  device's other processes read `device`; the summary line states the device's \
                  verdict once, with its free VRAM, shared bytes and paged count. `PAGED` names \
                  who is being paged, not who caused the pressure. When spill is not measured \
                  the cell reads \
                  `n/a` on Linux and macOS, where there is no shared-residency counter and so no \
                  spill to measure; `?` (not `no`) on Windows when spill exists but cannot be \
                  read now (pre-WDDM-2.0, a non-NVIDIA adapter, a PDH hiccup, or a build without \
                  the `pdh` feature). Neither is ever rendered as `no`, so neither can be misread \
                  as \"measured, not spilling\". Use `hmn watch`/`hmn spill` when the \
                  growth-over-baseline distinction matters.\n\
                  - On Windows, `?` in the NAME column is now rare (since v0.2.8): a \
                  `CreateToolhelp32Snapshot` fallback (the same mechanism `Get-Process`/Task \
                  Manager use) resolves most PIDs `OpenProcess` can't, including ordinary \
                  foreign-user/SYSTEM processes like `dwm.exe`/`csrss.exe`, non-elevated. \
                  What remains renders as `[exited]` (the process exited between the VRAM \
                  sample and the name lookup — elevation would not help) or `[protected]` \
                  (the snapshot fallback itself could not be taken — very rare; re-run \
                  elevated). The Windows kernel itself (PID 4) renders as `[kernel]`, not \
                  `?` or `[protected]`. This distinction is Windows-only; Linux and macOS \
                  unresolved rows remain a bare `?`/absent name. On Linux, run as the \
                  owning user or with `sudo` to resolve one.\n\
                  - Security note: a `[protected]` row (or, on Linux, a bare `?`) that \
                  does not resolve under elevation is worth investigating — by construction \
                  it is either a process owned by another user, a process running as \
                  SYSTEM/LOCAL SERVICE/NETWORK SERVICE, a PPL-protected process, or (rarely) \
                  the snapshot API itself failing. None of these are intrinsically \
                  malicious, but on a single-user desktop an unexpected one holding \
                  substantial VRAM is worth investigating. On macOS a bare `?` means both name \
                  lookups failed or the process is gone, and elevation does not change \
                  that; see README Limitations, item 9. The summary line's protected-count \
                  parenthetical counts `[protected]`/absent-name/the rare nvidia-smi-fallback \
                  literal `?` — not `[exited]`, since elevation can't help a process that's \
                  already gone. On macOS the same clause reads \
                  `re-run outside the sandbox`: a sandbox, not the user, withholds the name, \
                  and elevation does not lift it.\n\
                  - Pre-WDDM-2.0 Windows falls back to `nvidia-smi --query-compute-apps`, \
                  which is compute-only and may show `[N/A]` memory under consumer WDDM \
                  (parser drops those rows).\n\
                  - The R570 u64::MAX sentinel and used > total checks are applied per-row \
                  on NVIDIA backends; affected rows are dropped rather than reported as \
                  garbage.\n\
                  - macOS: `used_bytes` reflects currently-resident GPU pages \
                  (`graphics_footprint` ledger entry); the kernel evicts idle Metal pages, \
                  so the same PID may report different values across calls. Same \
                  resident-bytes semantics as Windows `WorkingSetSize` and Linux `VmRSS`.\n\
                  - macOS: the sandbox, not process ownership, decides what `hmn` can read — \
                  unsandboxed, every user's processes are listed and elevation does not \
                  help. `hmn` counts the processes it cannot read and exits `2` under \
                  `ps --exit-status` when it cannot tell; see README Limitations, item 9."
)]
struct Cli {
    /// Subcommand. Omitted for the default device-summary view.
    #[command(subcommand)]
    command: Option<Commands>,
    /// Emit the default device-summary view as a JSON array instead of
    /// text. Only meaningful with no subcommand — each subcommand
    /// (`ps`/`spill`/`watch`) already has its own `--json`, and combining
    /// this with one (e.g. `hmn --json ps`) is a hard error (exit `2`)
    /// rather than being silently ignored. One object per visible GPU:
    /// `index` (number), `name` (string or null),
    /// `total_bytes`/`free_bytes`/`used_bytes` (numbers), `reserved_bytes`
    /// (number or null), `driver_version` (string or null).
    #[arg(long)]
    json: bool,
}

/// Subcommand tree for `hmn`.
#[derive(Subcommand, Debug)]
enum Commands {
    /// List processes holding GPU memory. On Linux: compute-only via
    /// NVML. On Windows / WDDM 2.0+: every GPU memory holder via PDH
    /// (compositor, browsers, compute, etc.). On macOS: every
    /// process holding `graphics_footprint` ledger bytes that the
    /// sandbox lets it read (the sandbox, not process ownership, decides;
    /// see README Limitations, item 9); the processes it cannot read are
    /// counted on the summary line as unreadable. See `hmn --help`
    /// Limitations for the full per-platform breakdown.
    Ps {
        /// Keep only this PID. Repeatable (`--pid A --pid B`): a process
        /// matching any of them is listed — a launcher's wrapper and its
        /// GPU child, or two chained runs. The PIDs are echoed on the
        /// summary line. An unreadable process counts as a possible match,
        /// so `--pid N` reports only N among the unreadable ones.
        #[arg(long = "pid", value_name = "PID")]
        pids: Vec<u32>,
        /// Filter to a single GPU index. Default: every device reported
        /// by `device_count()`. An index that cannot be listed (out of
        /// range, or its query failing) is an error, exit `2`, rather than
        /// an empty table that would read as an idle card. Without
        /// `--device`, a device whose query fails is skipped with a stderr
        /// line ending `(skipped)`; when every device failed, the exit is
        /// `2` with no table.
        #[arg(long, value_name = "INDEX")]
        device: Option<u32>,
        /// Hide rows below this total footprint (`used_bytes +
        /// shared_used_bytes`, not dedicated alone — "who is actually
        /// holding this card", matching `--sort total`'s definition).
        /// Accepts a bare byte count or a number with `KiB`/`MiB`/`GiB`
        /// (e.g. `50MiB`, `1.5GiB`) — the same units `hmn` itself
        /// prints. `--min 0` is a valid no-op.
        #[arg(long, value_name = "SIZE", value_parser = parse_size_bytes)]
        min: Option<u64>,
        /// Keep only processes whose name contains PATTERN, ignoring
        /// case (`--filter canvas` matches `canvas` and `Canvas.exe`) —
        /// the same rule as `hmn watch --filter`. Repeatable: a name
        /// matching any one pattern qualifies. On macOS a name read from
        /// the kernel's `p_comm` is cut at 16 bytes, and a pattern cannot
        /// match past the cut. A process whose name cannot be resolved
        /// (`?`, `[protected]`, `[exited]`) cannot match; the summary line
        /// counts those rather than dropping them silently. The patterns
        /// are echoed on the summary line.
        #[arg(long = "filter", value_name = "PATTERN", value_parser = parse_filter_pattern)]
        filters: Vec<String>,
        /// Display order: `dedicated` ("who do I kill to free VRAM?",
        /// the default), `shared` ("who is currently being paged out?"
        /// — a symptom, not a cause; always a no-op ordering on Linux
        /// and macOS, where `shared_used_bytes` is always 0), or
        /// `total` (dedicated + shared, "who is the biggest GPU-memory
        /// citizen overall?"). Tie-breaks (name ascending, then PID
        /// ascending) are unchanged by this flag.
        #[arg(long, value_name = "KEY", default_value = "dedicated")]
        sort: SortKey,
        /// Emit a JSON array (one object per row) instead of the
        /// default text table. Each object has fields `pid` (number),
        /// `name` (string or null), `used_bytes` (number),
        /// `shared_used_bytes` (number — resident shared bytes, the
        /// WDDM spill signal; 0 off-Windows), `device_index` (number),
        /// `device_name` (string or null), `spilling` (true, false, or
        /// null — a single-snapshot approximation of the `hmn watch`
        /// spill co-condition, broadcast per device; null means "not
        /// measurable here", never collapsed into false), `paged` (true,
        /// false, or null — since v0.2.13, true when the device is
        /// spilling and this row's shared bytes are at least 256 MiB:
        /// this process is being paged), `shared_share` (number 0–1 to
        /// four decimals, or null — this row's fraction of the device's
        /// shared bytes over every process). `paged` and `shared_share`
        /// are null exactly when `spilling` is. Row order follows
        /// `--sort`.
        #[arg(long)]
        json: bool,
        /// Exit `1` when no process is listed, `0` when at least one is —
        /// as `pgrep` does — so `hmn ps --filter canvas --exit-status`
        /// answers "is my job on the GPU?" as a one-line gate. Off by
        /// default: without it, `hmn ps` exits `0` whether or not anything
        /// matched. A device named by `--device` that cannot be listed is
        /// still exit `2`. A listing where every device failed is also `2`,
        /// never `1`, and so is an empty listing that skipped a failed
        /// device or left unreadable a process the filters could match,
        /// since `1` means nothing matched on every device queried and no
        /// process the filters could match was unreadable.
        #[arg(long)]
        exit_status: bool,
    },
    /// Run a command while sampling WDDM spill state; print a
    /// `SpillReport` to stderr when it exits (stdout stays the
    /// wrapped command's). The wrapped command's exit code passes
    /// through.
    ///
    /// Spill = resident shared-system-memory growth while dedicated
    /// VRAM saturates — measurable on Windows / WDDM 2.0+ only. On
    /// Linux and macOS the command still runs, and the report is
    /// replaced by a "spill not measurable on this platform" note.
    ///
    /// Ctrl+C reaches the whole process group: hmn dies with the
    /// wrapped command and the report is lost (recording a partial
    /// report on interrupt is deliberately out of scope for now).
    Spill {
        /// Polling interval in milliseconds (minimum 1). Values below
        /// ~50 ms add PDH query cost without extra resolution (the
        /// GPU counters update on driver cadence) — documented, not
        /// clamped beyond the zero-floor.
        #[arg(long, value_name = "MS", default_value_t = 100, value_parser = clap::value_parser!(u64).range(1..))]
        interval: u64,
        /// GPU index to watch (NVML-canonical ordering).
        #[arg(long, value_name = "INDEX", default_value_t = 0)]
        device: u32,
        /// Also emit the `SpillReport` as a JSON object on stdout
        /// (after the wrapped command's own output; stderr keeps the
        /// human-readable block). Fields: `measurable`, `spilled`,
        /// `observations`, `baseline_shared_bytes`,
        /// `peak_shared_bytes`, `peak_dedicated_bytes`,
        /// `dedicated_limit_bytes`, `total_spill_duration_ms`,
        /// `episodes[]` (`start_label`, `end_label` or null,
        /// `peak_shared_bytes`, `observations`, `duration_ms`).
        /// Check `measurable` before trusting `spilled: false`.
        #[arg(long)]
        json: bool,
        /// The command to run (everything after `--`).
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            required = true,
            value_name = "COMMAND"
        )]
        command: Vec<String>,
    },
    /// Attach to already-running PID(s) and sample WDDM spill state plus
    /// per-PID VRAM on a timer; one row per watched PID per interval,
    /// with deltas and the shipped v0.2.5 SPILL condition. Not a TUI —
    /// a scrolling time(1)-style sampler, same discipline as `hmn spill`.
    ///
    /// With no PID given, auto-selects the top `--top` processes by
    /// committed VRAM from the first sample and keeps that fixed set for
    /// the run (or re-selects every interval with `--follow-new`);
    /// `--filter` and `--min` narrow that choice by name and by size. A
    /// watched PID that stops appearing in the per-process listing
    /// (exited, or simply holds no GPU memory right now) renders as 0
    /// bytes each interval — `hmn watch` does not distinguish the two;
    /// it does not auto-stop on this basis, use `--duration` or Ctrl+C.
    /// At attach it does check each explicit PID: one that names no
    /// running process gets a one-line warning on stderr (since
    /// v0.2.13), and so does one that is unreadable here (a macOS
    /// sandbox's refusal: its rows read 0 MiB); both are still watched.
    /// `--follow-new` says how many processes it cannot follow. Spill is
    /// measured as shared-memory growth above the first sample, so a
    /// spill already under way at attach is not counted; since v0.2.13 a
    /// warning says so at attach and the closing summary repeats it
    /// (`hmn ps` shows the current state).
    /// If the OS recycles a watched PID onto a different process
    /// mid-watch, a resolved-name change is used as a best-effort signal
    /// to reset that row's baseline rather than mixing two processes'
    /// readings.
    ///
    /// Runs until `--duration` elapses or Ctrl+C, printing a closing
    /// summary (adapter-level `SpillReport` plus per-PID peak/baseline;
    /// where spill is not measurable, a "spill not measurable on this
    /// platform" line in place of the report) and
    /// exiting `0` if spill was never observed, `1` if it was at least
    /// once, `2` on a hard error (bad device, nothing to auto-select, or
    /// `--follow-new` / `--filter` / `--min` combined with explicit
    /// PID(s)).
    Watch {
        /// Explicit PID(s) to watch. When omitted, auto-selects the top
        /// `--top` processes by committed VRAM from the first sample.
        #[arg(value_name = "PID")]
        pids: Vec<u32>,
        /// Sampling interval: digits followed by an optional unit (`ms`,
        /// `s`, `m`, `h`); bare digits are seconds. Shorter intervals
        /// catch brief flicker episodes at the cost of more PDH queries.
        #[arg(long, value_name = "DUR", default_value = "5s", value_parser = parse_duration)]
        interval: Duration,
        /// Stop after this long and print the closing summary (same
        /// duration-string format as `--interval`). Omitted: run until
        /// Ctrl+C.
        #[arg(long, value_name = "DUR", value_parser = parse_duration)]
        duration: Option<Duration>,
        /// Number of processes to auto-select by committed VRAM when no
        /// PID is given. Ignored when explicit PID(s) are passed.
        #[arg(long, value_name = "N", default_value_t = 5)]
        top: usize,
        /// Auto-select mode only: re-run the top-`--top` selection every
        /// interval instead of once at attach. A PID entering the
        /// followed set starts fresh (baseline = first sighting); a PID
        /// dropping out (exited, or fell below rank `--top`) simply
        /// stops appearing in the live rows and is finalized into the
        /// closing summary's `per_pid[]` with its peak/baseline, instead
        /// of rendering `0` rows forever. An empty first sample is not
        /// an error under this flag — the watch just starts empty and
        /// picks up work as it appears, which is the point. Combining
        /// this with explicit PID(s) on the command line is a hard
        /// error (exit `2`): there is no top-N to re-run against a fixed
        /// list, and explicit PIDs are watched exactly as given.
        #[arg(long)]
        follow_new: bool,
        /// Auto-select mode only: consider only processes whose name
        /// contains PATTERN, ignoring case (`--filter train` matches
        /// `train.exe` and `Train_Eval.EXE`), then keep the top `--top`
        /// of those. Repeatable: a name matching any one pattern
        /// qualifies. Composes with `--follow-new`, which re-applies it
        /// every interval. A followed process whose name briefly fails to
        /// resolve (`[protected]`, `[exited]`) keeps matching on the last
        /// name it resolved to; a process whose name never resolves
        /// cannot match, and is announced once on stderr rather than
        /// dropped silently. On macOS a name read from the kernel's
        /// `p_comm` is cut at 16 bytes, and a pattern cannot match past
        /// the cut. The active patterns appear on the stderr header line.
        /// Combining this with explicit PID(s) is a hard error (exit `2`).
        #[arg(long = "filter", value_name = "PATTERN", value_parser = parse_filter_pattern)]
        filters: Vec<String>,
        /// Auto-select mode only: consider only processes whose total
        /// footprint (`used_bytes + shared_used_bytes`, exactly as
        /// `hmn ps --min` measures it) is at least SIZE, then keep the
        /// top `--top` of those. Same SIZE syntax as `hmn ps --min`.
        /// Applied before `--filter`, and re-applied every interval
        /// under `--follow-new`. Shown on the stderr header line.
        /// Combining this with explicit PID(s) is a hard error (exit `2`).
        #[arg(long, value_name = "SIZE", value_parser = parse_size_bytes)]
        min: Option<u64>,
        /// GPU index to watch (NVML-canonical ordering).
        #[arg(long, value_name = "INDEX", default_value_t = 0)]
        device: u32,
        /// Emit JSON Lines to stdout instead of a text table. Since
        /// v0.2.12 the first line is a `{"kind":"start",...}` object —
        /// `hmn` version, the invocation (`argv`, with the program path
        /// reduced to its file name so a committed capture does not
        /// publish a user name or directory layout), device, interval and
        /// duration, and the `selection` (mode, explicit PIDs, `top`,
        /// `--filter` patterns, `--min` bytes) — so a capture describes
        /// itself, and one with a `start` but no `summary` is known to
        /// have been cut short. Then one `{"kind":"sample",...}` object
        /// per PID per interval as it happens, plus a final
        /// `{"kind":"summary",...}` object (the adapter `SpillReport`
        /// fields plus a `per_pid[]` peak/baseline array) when the watch
        /// ends. Each sample carries `t_ms`
        /// (relative to attach) and, since v0.2.11, `wall_clock`
        /// (absolute, UTC ISO-8601 with millisecond precision — the
        /// same value for every row in one interval, captured at the
        /// same instant as `t_ms`'s own reference point) for joining
        /// against a log stamped with real time, like a training
        /// driver's own run log. `spilling` is `true`/`false`/`null`
        /// (since v0.2.11) — `null`, never `false`, when this run has
        /// no measurable spill source, matching `hmn ps`'s SPILL
        /// column honesty contract. `paged` (since v0.2.13, same
        /// values, `null` exactly when `spilling` is) says whether this
        /// process is being paged — the adapter is spilling and its own
        /// shared bytes are at least 256 MiB, `hmn ps`'s rule — and each
        /// `per_pid[]` entry says whether it ever was. The summary's
        /// `spilling_at_attach` (since v0.2.13, `true`/`false`/`null`) is
        /// `hmn ps`'s verdict at attach: `true` means the baseline
        /// includes a spill already under way, which the episodes do not
        /// count. Pipeable to `jq -c` live.
        #[arg(long)]
        json: bool,
    },
    /// Headroom predicate: exit `0` if `SIZE` currently fits in the
    /// target device's free `VRAM`, `1` if it doesn't, `2` on a hard
    /// error (bad `--device`). Gateable from a run script (`hmn fits
    /// 12GiB || exit 1`) — the question that actually matters before
    /// launching a job is rarely "what's on the GPU" but "will this
    /// job fit right now", and this answers it in one command instead
    /// of a hand-rolled `hmn --json | jq` check repeated per script.
    /// Prints one line to stderr either way; no `--json` (the ask is
    /// specifically a scriptable exit code, not structured output).
    Fits {
        /// Size to check, in the same syntax as `hmn ps --min`: a bare
        /// byte count, or a decimal number with `KiB`/`MiB`/`GiB`.
        #[arg(value_name = "SIZE", value_parser = parse_size_bytes)]
        size: u64,
        /// GPU index to check (NVML-canonical ordering).
        #[arg(long, value_name = "INDEX", default_value_t = 0)]
        device: u32,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    // `--json` before a subcommand parses cleanly (clap has no opinion on
    // the combination) but would otherwise be silently dropped: only the
    // `None` arm below reads `cli.json`, so `hmn --json ps` would print
    // plain text with no warning — a script relying on `--json` landing
    // wherever it's typed would silently get prose instead of JSON.
    // Reject the combination loudly instead, same "hard error, exit 2"
    // convention as `watch --follow-new` + explicit PIDs below.
    if cli.json && cli.command.is_some() {
        eprintln!(
            "hmn: --json before a subcommand is ignored, not applied — each subcommand has \
             its own --json (e.g. `hmn ps --json`, not `hmn --json ps`)"
        );
        return std::process::ExitCode::from(2);
    }
    let outcome = match cli.command {
        None => run_summary(cli.json),
        // `ps` also bypasses the Ok/Err fold: an unlistable `--device` is
        // exit `2`, like `fits`, not the fold's generic `1`.
        Some(Commands::Ps {
            pids,
            device,
            min,
            filters,
            sort,
            json,
            exit_status,
        }) => {
            return run_ps(
                &PsFilters::new(&pids, device, min, filters),
                sort,
                json,
                exit_status,
            );
        }
        // `spill` bypasses the Ok/Err fold below: its exit code is the
        // wrapped command's, passed through — not hmn's own
        // success/failure.
        Some(Commands::Spill {
            interval,
            device,
            json,
            command,
        }) => return run_spill(interval, device, json, &command),
        // `watch` also bypasses the Ok/Err fold: its exit code conveys
        // whether spill was observed, not hmn's own success/failure.
        Some(Commands::Watch {
            pids,
            interval,
            duration,
            top,
            follow_new,
            filters,
            min,
            device,
            json,
        }) => {
            // Validated before any backend call: an invalid argument
            // combination fails fast without touching hardware at all.
            return match Selection::new(&pids, top, follow_new, &filters, min) {
                Ok(selection) => run_watch(&selection, interval, duration, device, json),
                Err(msg) => {
                    eprintln!("hmn: {msg}");
                    std::process::ExitCode::from(2)
                }
            };
        }
        // `fits` also bypasses the Ok/Err fold: its exit code conveys
        // whether the size fits, not hmn's own success/failure.
        Some(Commands::Fits { size, device }) => return run_fits(size, device),
    };
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hmn: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    // EXPLICIT: panic! is the standard "unreachable pattern in a test"
    // signal for the arg-parse destructuring assertions.
    clippy::panic
)]
mod tests {
    use super::*;

    // --- ps argument parsing (--sort) ---

    #[test]
    fn ps_args_sort_defaults_to_dedicated() {
        let cli = Cli::try_parse_from(["hmn", "ps"]).unwrap();
        let Some(Commands::Ps { sort, .. }) = cli.command else {
            panic!("expected Ps subcommand");
        };
        assert_eq!(sort, SortKey::Dedicated);
    }

    #[test]
    fn ps_args_sort_accepts_each_key() {
        for (flag, expected) in [
            ("dedicated", SortKey::Dedicated),
            ("shared", SortKey::Shared),
            ("total", SortKey::Total),
        ] {
            let cli = Cli::try_parse_from(["hmn", "ps", "--sort", flag]).unwrap();
            let Some(Commands::Ps { sort, .. }) = cli.command else {
                panic!("expected Ps subcommand");
            };
            assert_eq!(sort, expected, "--sort {flag}");
        }
    }

    #[test]
    fn ps_args_sort_accepts_dedicated_aliases() {
        // `vram` and `committed` are the words the rest of the tool's
        // own vocabulary uses for this quantity (the `ps` column header
        // and `watch`'s `COMMITTED` column, respectively) — both should
        // resolve to the same ordering as the canonical `dedicated`.
        for flag in ["vram", "committed"] {
            let cli = Cli::try_parse_from(["hmn", "ps", "--sort", flag]).unwrap();
            let Some(Commands::Ps { sort, .. }) = cli.command else {
                panic!("expected Ps subcommand");
            };
            assert_eq!(sort, SortKey::Dedicated, "--sort {flag}");
        }
    }

    #[test]
    fn ps_args_sort_rejects_unknown_key() {
        assert!(Cli::try_parse_from(["hmn", "ps", "--sort", "bogus"]).is_err());
    }

    #[test]
    fn ps_args_min_parses_size() {
        let cli = Cli::try_parse_from(["hmn", "ps", "--min", "50MiB"]).unwrap();
        let Some(Commands::Ps { min, .. }) = cli.command else {
            panic!("expected Ps subcommand");
        };
        assert_eq!(min, Some(50 * 1024 * 1024));
    }

    #[test]
    fn ps_args_min_defaults_to_none() {
        let cli = Cli::try_parse_from(["hmn", "ps"]).unwrap();
        let Some(Commands::Ps { min, .. }) = cli.command else {
            panic!("expected Ps subcommand");
        };
        assert_eq!(min, None);
    }

    #[test]
    fn ps_args_min_rejects_bad_size() {
        assert!(Cli::try_parse_from(["hmn", "ps", "--min", "bogus"]).is_err());
    }

    #[test]
    fn ps_args_filter_is_repeatable_and_defaults_to_none() {
        let cli =
            Cli::try_parse_from(["hmn", "ps", "--filter", "canvas", "--filter", "Python"]).unwrap();
        let Some(Commands::Ps { filters, .. }) = cli.command else {
            panic!("expected Ps subcommand");
        };
        assert_eq!(filters, ["canvas", "Python"]);
        let cli = Cli::try_parse_from(["hmn", "ps"]).unwrap();
        let Some(Commands::Ps { filters, .. }) = cli.command else {
            panic!("expected Ps subcommand");
        };
        assert!(filters.is_empty(), "{filters:?}");
    }

    #[test]
    fn ps_args_pid_is_repeatable_but_not_comma_separated() {
        let cli = Cli::try_parse_from(["hmn", "ps", "--pid", "15503", "--pid", "15534"]).unwrap();
        let Some(Commands::Ps { pids, .. }) = cli.command else {
            panic!("expected Ps subcommand");
        };
        assert_eq!(pids, [15503, 15534]);
        assert!(Cli::try_parse_from(["hmn", "ps", "--pid", "1,2"]).is_err());
    }

    #[test]
    fn ps_args_filter_rejects_blank_pattern() {
        assert!(Cli::try_parse_from(["hmn", "ps", "--filter", " "]).is_err());
    }

    // --- spill argument parsing ---

    #[test]
    fn spill_args_parse_trailing_command_with_hyphen_values() {
        let cli = Cli::try_parse_from([
            "hmn",
            "spill",
            "--interval",
            "50",
            "--",
            "python",
            "train.py",
            "--lr",
            "0.1",
        ])
        .unwrap();
        let Some(Commands::Spill {
            interval,
            device,
            json,
            command,
        }) = cli.command
        else {
            panic!("expected Spill subcommand");
        };
        assert_eq!(interval, 50);
        assert_eq!(device, 0);
        assert!(!json);
        assert_eq!(command, ["python", "train.py", "--lr", "0.1"]);
    }

    #[test]
    fn spill_args_default_interval_100() {
        let cli = Cli::try_parse_from(["hmn", "spill", "--", "sleep", "1"]).unwrap();
        let Some(Commands::Spill { interval, .. }) = cli.command else {
            panic!("expected Spill subcommand");
        };
        assert_eq!(interval, 100);
    }

    #[test]
    fn spill_args_requires_command() {
        assert!(Cli::try_parse_from(["hmn", "spill"]).is_err());
    }

    #[test]
    fn spill_args_rejects_zero_interval() {
        // A 0 ms interval would busy-loop PDH collects against the
        // wrapped command; floored at 1 by the value_parser range.
        assert!(Cli::try_parse_from(["hmn", "spill", "--interval", "0", "--", "x"]).is_err());
        assert!(Cli::try_parse_from(["hmn", "spill", "--interval", "1", "--", "x"]).is_ok());
    }

    // --- fits argument parsing ---

    #[test]
    fn fits_args_parses_size_and_default_device() {
        let cli = Cli::try_parse_from(["hmn", "fits", "12GiB"]).unwrap();
        let Some(Commands::Fits { size, device }) = cli.command else {
            panic!("expected Fits subcommand");
        };
        assert_eq!(size, 12 * 1024 * 1024 * 1024);
        assert_eq!(device, 0);
    }

    #[test]
    fn fits_args_device_override() {
        let cli = Cli::try_parse_from(["hmn", "fits", "500MiB", "--device", "1"]).unwrap();
        let Some(Commands::Fits { size, device }) = cli.command else {
            panic!("expected Fits subcommand");
        };
        assert_eq!(size, 500 * 1024 * 1024);
        assert_eq!(device, 1);
    }

    #[test]
    fn fits_args_bare_bytes() {
        let cli = Cli::try_parse_from(["hmn", "fits", "1048576"]).unwrap();
        let Some(Commands::Fits { size, .. }) = cli.command else {
            panic!("expected Fits subcommand");
        };
        assert_eq!(size, 1_048_576);
    }

    #[test]
    fn fits_args_requires_size() {
        assert!(Cli::try_parse_from(["hmn", "fits"]).is_err());
    }

    #[test]
    fn fits_args_rejects_bad_size() {
        assert!(Cli::try_parse_from(["hmn", "fits", "bogus"]).is_err());
    }

    // --- Watch clap arg parsing ---

    #[test]
    fn watch_args_defaults() {
        let cli = Cli::try_parse_from(["hmn", "watch"]).unwrap();
        let Some(Commands::Watch {
            pids,
            interval,
            duration,
            top,
            follow_new,
            filters,
            min,
            device,
            json,
        }) = cli.command
        else {
            panic!("expected Watch subcommand");
        };
        assert!(pids.is_empty(), "{pids:?}");
        assert!(filters.is_empty(), "{filters:?}");
        assert_eq!(min, None);
        assert_eq!(interval, Duration::from_secs(5));
        assert_eq!(duration, None);
        assert_eq!(top, 5);
        assert!(!follow_new);
        assert_eq!(device, 0);
        assert!(!json);
    }

    #[test]
    fn watch_args_explicit_pids_and_overrides() {
        let cli = Cli::try_parse_from([
            "hmn",
            "watch",
            "1234",
            "5678",
            "--interval",
            "30s",
            "--duration",
            "10m",
            "--device",
            "1",
            "--json",
        ])
        .unwrap();
        let Some(Commands::Watch {
            pids,
            interval,
            duration,
            device,
            json,
            ..
        }) = cli.command
        else {
            panic!("expected Watch subcommand");
        };
        assert_eq!(pids, [1234, 5678]);
        assert_eq!(interval, Duration::from_secs(30));
        assert_eq!(duration, Some(Duration::from_secs(600)));
        assert_eq!(device, 1);
        assert!(json);
    }

    #[test]
    fn watch_args_rejects_bad_duration() {
        assert!(Cli::try_parse_from(["hmn", "watch", "--interval", "bogus"]).is_err());
        assert!(Cli::try_parse_from(["hmn", "watch", "--duration", "0"]).is_err());
    }

    #[test]
    fn watch_args_top_override() {
        let cli = Cli::try_parse_from(["hmn", "watch", "--top", "10"]).unwrap();
        let Some(Commands::Watch { top, .. }) = cli.command else {
            panic!("expected Watch subcommand");
        };
        assert_eq!(top, 10);
    }

    #[test]
    fn watch_args_follow_new_flag() {
        let cli = Cli::try_parse_from(["hmn", "watch", "--follow-new"]).unwrap();
        let Some(Commands::Watch { follow_new, .. }) = cli.command else {
            panic!("expected Watch subcommand");
        };
        assert!(follow_new);
    }

    #[test]
    fn watch_args_filter_is_repeatable_and_kept_as_typed() {
        let cli = Cli::try_parse_from([
            "hmn",
            "watch",
            "--follow-new",
            "--filter",
            "train",
            "--filter",
            "Eval",
        ])
        .unwrap();
        let Some(Commands::Watch { filters, .. }) = cli.command else {
            panic!("expected Watch subcommand");
        };
        assert_eq!(filters, ["train", "Eval"]);
    }

    #[test]
    fn watch_args_filter_rejects_blank_pattern() {
        assert!(Cli::try_parse_from(["hmn", "watch", "--filter", ""]).is_err());
        assert!(Cli::try_parse_from(["hmn", "watch", "--filter", "  "]).is_err());
    }

    #[test]
    fn watch_args_min_uses_ps_size_syntax() {
        let cli = Cli::try_parse_from(["hmn", "watch", "--follow-new", "--min", "2GiB"]).unwrap();
        let Some(Commands::Watch { min, .. }) = cli.command else {
            panic!("expected Watch subcommand");
        };
        assert_eq!(min, Some(2 * 1024 * 1024 * 1024));
        assert!(Cli::try_parse_from(["hmn", "watch", "--min", "bogus"]).is_err());
    }

    #[test]
    fn watch_args_filter_with_explicit_pids_parses_clean() {
        // Like `--follow-new` + PIDs: clap accepts it, `Selection::new`
        // rejects it at runtime (exit `2`) — see its own tests.
        let cli = Cli::try_parse_from(["hmn", "watch", "1234", "--filter", "x"]).unwrap();
        let Some(Commands::Watch { pids, filters, .. }) = cli.command else {
            panic!("expected Watch subcommand");
        };
        assert_eq!(pids, [1234]);
        assert_eq!(filters, ["x"]);
    }

    #[test]
    fn watch_args_follow_new_with_explicit_pids_parses_clean() {
        // clap itself has no opinion on this combination —
        // `Selection::new` rejects it before any hardware call (exit
        // code 2; the guard is unit-tested in `watch.rs`). This test
        // only pins down that clap parsing itself doesn't reject the
        // combination — it has to reach `Selection::new` to be caught.
        let cli = Cli::try_parse_from(["hmn", "watch", "1234", "--follow-new"]).unwrap();
        let Some(Commands::Watch {
            pids, follow_new, ..
        }) = cli.command
        else {
            panic!("expected Watch subcommand");
        };
        assert_eq!(pids, [1234]);
        assert!(follow_new);
    }

    #[test]
    fn top_level_json_before_subcommand_parses_clean() {
        // clap itself has no opinion on this combination — `main` rejects
        // it at runtime (exit code 2, verified live/manually, not here:
        // it's a hard-error path before dispatch, same shape as
        // `watch_args_follow_new_with_explicit_pids_parses_clean` above).
        // This test only pins down that clap parsing itself doesn't
        // reject `hmn --json ps` — it has to reach `main`'s dispatch to
        // be caught.
        let cli = Cli::try_parse_from(["hmn", "--json", "ps"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Ps { .. })));
    }
}
