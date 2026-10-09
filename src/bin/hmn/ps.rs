// SPDX-License-Identifier: MIT OR Apache-2.0

//! `hmn ps`: the GPU-process listing — its row model, filters
//! (`PsFilters`), sort keys and comparator, the paged-process mark, the
//! stderr summary line, and its text and JSON renderers.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::process::ExitCode;

use clap::ValueEnum;
use hypomnesis::spill::DEFAULT_SHARED_GROWTH_BYTES;
use hypomnesis::{
    GpuProcessEntry, device_count, device_info, gpu_process_listing, snapshot_is_spilling,
};

use crate::format::{
    REMEDY_OUTSIDE_SANDBOX, Table, failure_detail, format_vram, format_vram_precise,
    json_string_or_null, json_value_or_null, spill_cell, with_remedy,
};

/// One row of `hmn ps` output (binary-internal — not part of the
/// library's public API).
#[derive(Debug, Clone)]
pub struct PsRow {
    /// Process ID.
    pub pid: u32,
    /// Process name. `None` when no name source produced one.
    pub name: Option<String>,
    /// GPU memory used by this process in bytes (`WDDM` dedicated
    /// commit on the Windows `PDH` path).
    pub used_bytes: u64,
    /// Resident shared-system-memory bytes — the `WDDM` spill signal.
    /// `0` on non-Windows backends (no shared-residency counter).
    pub shared_used_bytes: u64,
    /// Zero-based device index (NVML-canonical).
    pub device_index: u32,
    /// Friendly device name (e.g. `RTX 5060 Ti`); `None` when
    /// `device_info` failed for this index.
    pub device_name: Option<String>,
    /// One-shot spill check ([`hypomnesis::snapshot_is_spilling`]) for
    /// this row's device, computed once per device and broadcast to
    /// every row on it — same "adapter-wide, same value on every row"
    /// shape `hmn watch`'s `spilling` field already uses. `None` when
    /// not measurable — on Linux and macOS, where spill cannot exist (the
    /// SPILL cell reads `n/a`), and on Windows pre-`WDDM 2.0`, on a
    /// non-NVIDIA adapter, or on a live `PDH` sample failure (the cell
    /// reads `?`) — never collapsed into `Some(false)`.
    pub spilling: Option<bool>,
    /// Whether this process is being *paged* — its device is spilling
    /// and its own SHARED is at least [`DEFAULT_SHARED_GROWTH_BYTES`],
    /// the floor the spill condition itself uses. `None` exactly when
    /// [`Self::spilling`] is. Says who is being paged, not who caused the
    /// pressure: the memory manager pages whatever it chooses.
    pub paged: Option<bool>,
    /// This process's fraction of its device's shared-resident bytes,
    /// summed over every process on the device (before any filter),
    /// `0.0..=1.0`. `0.0` when the device holds no shared bytes; `None`
    /// exactly when [`Self::spilling`] is.
    pub shared_share: Option<f64>,
}

/// Display-order key for `hmn ps --sort` (and, always pinned to
/// [`Self::Dedicated`], for `watch::select_top_n_pids`'s auto-selection).
///
/// Binary-internal dispatch enum, not a library type — matched
/// exhaustively by [`ps_row_comparator`], the sole place that
/// interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortKey {
    /// `used_bytes` (`WDDM` dedicated commit) descending — "who do I
    /// kill to free VRAM?". The default; matches `hmn ps`'s pre-v0.2.7
    /// fixed order exactly. Also accepts `vram` (the column header and
    /// the word the rest of the tool's help text uses for this
    /// quantity) and `committed` (the word `hmn watch`'s `COMMITTED`
    /// column uses for the same quantity) as aliases — same ordering,
    /// different vocabulary entry points, so users don't have to learn
    /// `hmn`-internal naming to reach for the default sort.
    #[value(alias = "vram", alias = "committed")]
    Dedicated,
    /// `shared_used_bytes` (resident shared-system-memory, the spill
    /// signal) descending — "who is currently being paged out?". A
    /// symptom, not a cause: a process high in SHARED has already lost
    /// the fight for dedicated VRAM. Always a no-op ordering on Linux
    /// and macOS, where `shared_used_bytes` is always `0`.
    Shared,
    /// `used_bytes + shared_used_bytes` descending — "who is the
    /// biggest GPU-memory citizen overall?". Outweighs `Dedicated` for
    /// processes that hold meaningful shared residency alongside their
    /// dedicated commit.
    Total,
}

/// A process's total GPU-memory footprint: committed plus shared-resident
/// bytes. The one definition behind `hmn ps --sort total`, `hmn ps --min`
/// and `hmn watch --min`, so the three cannot disagree about what "total"
/// means. Saturating, so a corrupt counter pair cannot wrap around.
#[must_use]
pub const fn footprint_bytes(used_bytes: u64, shared_used_bytes: u64) -> u64 {
    used_bytes.saturating_add(shared_used_bytes)
}

/// Whether a process holding `shared_used_bytes` of shared-resident
/// memory counts as *paged* on a spilling device: at least
/// [`DEFAULT_SHARED_GROWTH_BYTES`] (256 MiB), the floor the spill
/// condition itself applies adapter-wide. It clears the benign staging
/// baseline of an ordinary desktop process (tens of MiB; 110 MiB for an
/// editor in the askesis report).
#[must_use]
pub const fn is_paged(shared_used_bytes: u64) -> bool {
    shared_used_bytes >= DEFAULT_SHARED_GROWTH_BYTES
}

/// Whether a process is being paged, from its device's verdict
/// (`spilling`) and its own shared bytes: `None` when spill is not
/// measurable, `Some(false)` when the device is not spilling, else
/// [`is_paged`]. The one rule behind `hmn ps`'s and `hmn watch`'s `PAGED`.
#[must_use]
pub const fn paged_verdict(spilling: Option<bool>, shared_used_bytes: u64) -> Option<bool> {
    match spilling {
        Some(spilling) => Some(spilling && is_paged(shared_used_bytes)),
        None => None,
    }
}

/// A row's [`PsRow::paged`] and [`PsRow::shared_share`] from its device's
/// verdict (`spilling`), its own shared bytes, and the device's total
/// shared bytes over every process. Both `None` when `spilling` is — the
/// same "can't tell" the broadcast verdict carries.
#[must_use]
pub const fn paged_and_share(
    spilling: Option<bool>,
    shared_used_bytes: u64,
    device_shared_bytes: u64,
) -> (Option<bool>, Option<f64>) {
    let Some(spilling) = spilling else {
        return (None, None);
    };
    let share = if device_shared_bytes == 0 {
        0.0
    } else {
        // CAST: u64 → f64, byte counts; a ratio needs no more precision
        // than f64's 53-bit mantissa gives for any real GPU memory size.
        #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
        let ratio = shared_used_bytes as f64 / device_shared_bytes as f64;
        ratio.min(1.0)
    };
    (
        paged_verdict(Some(spilling), shared_used_bytes),
        Some(share),
    )
}

/// One spilling device's evidence for the summary line: what `hmn ps`
/// states once there instead of leaving the verdict to be read off every
/// row. Computed over every process on the device, before any filter, so
/// the device verdict does not depend on what is displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSpill {
    /// Zero-based device index (`NVML`-canonical).
    pub index: u32,
    /// The device's free `VRAM` in bytes, from `device_info`. `None`
    /// when that query failed.
    pub free_bytes: Option<u64>,
    /// Shared-resident bytes summed over every process on the device.
    pub shared_bytes: u64,
    /// How many processes on the device are paged ([`is_paged`]).
    pub paged: usize,
}

/// What the summary line reports beyond the listed rows: the processes
/// `--filter` could not judge, and the spilling devices.
#[derive(Debug, Clone, Default)]
pub struct SummaryNotes {
    /// Processes that passed `--pid` / `--min` but had no name `--filter`
    /// could match ([`PsJudgement::Unnamed`]).
    pub unnamed: usize,
    /// One entry per device whose verdict is `Some(true)`, in device
    /// order. Empty when no device is spilling or spill is not measurable.
    pub spilling: Vec<DeviceSpill>,
    /// The processes the caller was refused ([`hypomnesis::GpuProcessListing::denied_pids`])
    /// that `--pid` leaves relevant ([`PsFilters::relevant_denied`]), summed
    /// over the devices listed. Always `0` off macOS.
    pub unreadable: usize,
}

/// Build the row comparator for a given [`SortKey`], shared by `hmn ps`
/// (user-selectable via `--sort`) and `watch::select_top_n_pids` (always
/// [`SortKey::Dedicated`]) so the two orderings cannot silently drift
/// apart. Tie-breaks (name ascending, then PID ascending — stable
/// output across runs, and clusters duplicate-name processes like
/// `msedgewebview2.exe`) are identical regardless of the primary key.
pub const fn ps_row_comparator(key: SortKey) -> impl Fn(&PsRow, &PsRow) -> std::cmp::Ordering {
    move |a, b| {
        let primary = match key {
            SortKey::Dedicated => b.used_bytes.cmp(&a.used_bytes),
            SortKey::Shared => b.shared_used_bytes.cmp(&a.shared_used_bytes),
            SortKey::Total => footprint_bytes(b.used_bytes, b.shared_used_bytes)
                .cmp(&footprint_bytes(a.used_bytes, a.shared_used_bytes)),
        };
        primary
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.pid.cmp(&b.pid))
    }
}

/// The filters `hmn ps` applies, as one value: what `run_ps` lists and
/// what the summary line's ` matching …` clause says was applied, so
/// the two cannot disagree. Binary-internal, not a library type.
#[derive(Debug, Clone, Default)]
pub struct PsFilters {
    /// `--pid`, repeatable: list only these PIDs (any of them). Empty
    /// lists every PID. Deduplicated, in the order given, by
    /// [`Self::new`].
    pub pids: Vec<u32>,
    /// `--device`: list only this GPU index (`NVML`-canonical). `None`
    /// lists every device `device_count` reports. Chooses which devices
    /// are enumerated rather than testing rows, so [`Self::judge`] does
    /// not look at it.
    pub device: Option<u32>,
    /// `--min`: list only processes whose total footprint
    /// ([`footprint_bytes`]) is at least this many bytes. `None` applies
    /// no floor.
    pub min_bytes: Option<u64>,
    /// `--filter`: list only processes whose name contains one of these
    /// patterns, ignoring case ([`matches_any`]). Empty applies no name
    /// filter.
    pub patterns: Vec<String>,
}

/// What [`PsFilters::judge`] decided about one process. Binary-internal
/// dispatch enum, matched exhaustively by `run_ps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsJudgement {
    /// Passes every filter: listed.
    Listed,
    /// Rejected by a filter: not listed, not counted.
    Filtered,
    /// Passes `--pid` and `--min`, but has no name `--filter` can judge
    /// ([`filterable_name`]): not listed, and counted on the summary line
    /// so it is never dropped silently.
    Unnamed,
}

impl PsFilters {
    /// Build the filters from the parsed command line, dropping repeated
    /// PIDs (keeping the first occurrence) so the summary echoes each PID
    /// once.
    #[must_use]
    pub fn new(
        pids: &[u32],
        device: Option<u32>,
        min_bytes: Option<u64>,
        patterns: Vec<String>,
    ) -> Self {
        let mut seen = HashSet::new();
        Self {
            pids: pids.iter().copied().filter(|p| seen.insert(*p)).collect(),
            device,
            min_bytes,
            patterns,
        }
    }

    /// Whether `--pid` admits `pid`: every PID when none was given, else
    /// the ones named. The one `--pid` test, shared by [`Self::judge`] and
    /// [`Self::relevant_denied`].
    #[must_use]
    fn pid_selected(&self, pid: u32) -> bool {
        self.pids.is_empty() || self.pids.contains(&pid)
    }

    /// Judge a process on an already-selected device against the
    /// row-level filters (`--pid`, `--min`, then `--filter`).
    #[must_use]
    pub fn judge(&self, entry: &GpuProcessEntry) -> PsJudgement {
        let size_and_pid = self.pid_selected(entry.pid)
            && self.min_bytes.is_none_or(|min| {
                footprint_bytes(entry.used_bytes, entry.shared_used_bytes) >= min
            });
        if !size_and_pid {
            return PsJudgement::Filtered;
        }
        if self.patterns.is_empty() {
            return PsJudgement::Listed;
        }
        match filterable_name(entry.name.as_deref()) {
            Some(name) if matches_any(name, &self.patterns) => PsJudgement::Listed,
            Some(_) => PsJudgement::Filtered,
            None => PsJudgement::Unnamed,
        }
    }

    /// How many of the `denied` PIDs a listing with these filters could
    /// have held: every one without `--pid`, else the ones `--pid` names.
    /// A denied process's size and name are unknown, so `--min` and
    /// `--filter` cannot exclude it.
    #[must_use]
    pub fn relevant_denied(&self, denied: &[u32]) -> usize {
        denied.iter().filter(|&&pid| self.pid_selected(pid)).count()
    }

    /// The summary line's filter clauses (`pid=N[,N…]`, `device=M`,
    /// `min=X unit`, `filter="a","b"`), in that order, one per active
    /// filter; empty when no filter is active.
    #[must_use]
    fn clauses(&self) -> Vec<String> {
        let mut clauses = Vec::new();
        if !self.pids.is_empty() {
            let pids: Vec<String> = self.pids.iter().map(u32::to_string).collect();
            clauses.push(format!("pid={}", pids.join(",")));
        }
        if let Some(d) = self.device {
            clauses.push(format!("device={d}"));
        }
        if let Some(m) = self.min_bytes {
            // format_vram_precise, not format_vram: pid=/device= echo exact
            // values, and format_vram's MiB-below-1-GiB rounding would
            // print a real sub-MiB --min as "0 MiB" — indistinguishable
            // from the documented --min 0 no-op.
            clauses.push(format!("min={}", format_vram_precise(m)));
        }
        if !self.patterns.is_empty() {
            // Debug-quoted, as `hmn watch`'s header quotes them, so a
            // pattern holding a space or a comma reads unambiguously.
            let quoted: Vec<String> = self.patterns.iter().map(|p| format!("{p:?}")).collect();
            clauses.push(format!("filter={}", quoted.join(",")));
        }
        clauses
    }
}

/// Filter out `name` values that don't represent a genuinely resolved
/// process identity for `hmn watch`'s PID-reuse comparison (`watch::process_sample`):
/// `None` (unresolved) and the Windows-only `"[protected]"`/`"[exited]"`
/// synthetic brackets (still unresolved, just with more detail than a
/// bare `?`). `"[kernel]"` is deliberately *not* filtered — `PID 4` is
/// permanently the kernel and never flickers, so it is safe to treat as
/// a stable, comparable name.
#[must_use]
pub fn resolved_name(name: Option<&str>) -> Option<&str> {
    name.filter(|n| *n != "[protected]" && *n != "[exited]")
}

/// The name a `--filter` pattern can be matched against: a
/// [`resolved_name`] that is not the `nvidia-smi` fallback's literal `?`.
/// `[kernel]` (`PID 4`) is a real, stable name and passes. The one rule
/// behind `hmn ps --filter` and `watch::matchable_name` (which adds a
/// last-resolved fallback), so the two filters cannot disagree about what
/// can be matched.
#[must_use]
pub fn filterable_name(name: Option<&str>) -> Option<&str> {
    resolved_name(name).filter(|n| *n != "?")
}

/// Whether `name` contains any of `patterns`, ignoring case — the
/// matching rule of both `hmn ps --filter` and `hmn watch --filter`.
#[must_use]
pub fn matches_any(name: &str, patterns: &[String]) -> bool {
    let name = name.to_lowercase();
    patterns.iter().any(|p| name.contains(&p.to_lowercase()))
}

/// Whether `hmn ps` failed outright: at least one device was tried and
/// none of them answered. `tried == 0` (no device to try, because
/// `device_count()` failed) is not a failure, and one failed device
/// among several that answered is a skip, not a failure.
#[must_use]
const fn every_device_failed(tried: usize, failed: usize) -> bool {
    tried > 0 && failed >= tried
}

/// The stderr line for a device whose query failed:
/// `hmn: ps failed to query device <index>: <err>`, with ` (skipped)`
/// appended when `skipped` (no `--device`, so the listing carries on
/// without it). Both forms share one prefix; the suffix alone tells a
/// skipped device from a fatal `--device` failure.
#[must_use]
pub fn device_query_failure_line(
    index: u32,
    err: &impl std::fmt::Display,
    skipped: bool,
) -> String {
    if skipped {
        format!("hmn: ps failed to query device {index}: {err} (skipped)")
    } else {
        format!("hmn: ps failed to query device {index}: {err}")
    }
}

/// The closing stderr line when every device `hmn ps` tried failed, so
/// its exit `2` always has a stated reason.
const ALL_DEVICES_FAILED_LINE: &str =
    "hmn: ps: no device could be queried, so nothing could be listed";

/// The exit code of a listing that reached the end of `run_ps`: `0`
/// without `exit_status` or when rows are listed; with it and nothing
/// listed, `2` when a process the filters could match was unreadable
/// (`relevant_denied > 0`) or a tried device failed (`failed > 0`) — the
/// process may be the one that was refused, or sit on the skipped device —
/// and `1` otherwise: every device answered and nothing matched.
#[must_use]
const fn ps_exit_code(
    exit_status: bool,
    rows_empty: bool,
    failed: usize,
    relevant_denied: usize,
) -> u8 {
    if !exit_status || !rows_empty {
        0
    } else if relevant_denied > 0 || failed > 0 {
        2
    } else {
        1
    }
}

/// Run the `ps` subcommand: collect process rows for the selected
/// device(s) — sampling one live adapter-wide spill check per device
/// along the way (see [`snapshot_is_spilling`]) — apply `filters`, sort
/// per `--sort`, then emit either a text table or JSON.
///
/// Returns the exit code, bypassing `main`'s `Ok`/`Err` fold like
/// `run_fits` and `run_watch`: `0` normally; `1` under `exit_status`
/// (`--exit-status`) when no process is listed and every device tried
/// answered, so `hmn ps --filter canvas --exit-status` is a one-line "is
/// my job on the GPU?" gate, as `pgrep` is for processes; `2` when a
/// device named by `--device` cannot be listed — out of range (`device
/// index 3 out of range (have 1 devices)`, the library's own bounds
/// check) or failing outright. With `--device`, the user asked for that
/// device alone, and an empty table would read as an idle card.
///
/// Without `--device`, a device that fails is skipped with a stderr line
/// ending ` (skipped)` (`device_query_failure_line`), so one broken
/// device does not kill the whole listing. A device whose process list
/// was unreadable (`HypomnesisError::ProcessListDenied`) is such a
/// failure, and its line ends with the remedy ([`failure_detail`]).
/// When a device lists with some processes refused, the rows are
/// printed and the summary counts the refused ones that `--pid` leaves
/// relevant ([`PsFilters::relevant_denied`]).
/// When every device failed (at least one tried, none answering), `run_ps`
/// prints `ALL_DEVICES_FAILED_LINE` and exits `2` with nothing on
/// stdout: `--json` prints nothing on that exit, not `[]`, since a table
/// or `[]` would read as "idle". Under `exit_status`, an empty listing
/// that skipped a failed device or left a process the filters could match
/// unreadable exits `2`, not `1` (`ps_exit_code`): `1` means "queried,
/// nothing matched", and the job may sit on the skipped device or be the
/// process that was refused. With no device to try at all
/// (`device_count()` failed), nothing was skipped, and the listing exits
/// as before.
pub fn run_ps(filters: &PsFilters, sort: SortKey, json: bool, exit_status: bool) -> ExitCode {
    // device_count returning Err here means no enumeration backend is
    // enabled / every backend failed; treat as zero NVIDIA devices and
    // let the empty Vec fall through to the formatter (which prints
    // a header-only table or `[]`).
    let device_indices: Vec<u32> = filters.device.map_or_else(
        || (0..device_count().unwrap_or(0)).collect(),
        |idx| vec![idx],
    );

    let mut rows: Vec<PsRow> = Vec::new();
    // Processes `--filter` could not judge (no resolvable name), and the
    // spilling devices — what the summary line reports beyond the rows.
    let mut notes = SummaryNotes::default();
    // Devices whose query was attempted, and the ones that failed: every
    // tried device failing is an exit `2`, not an empty listing.
    let mut tried: usize = 0;
    let mut failed: usize = 0;
    for &idx in &device_indices {
        // Look up the device once: its name for the DEVICE column, its
        // free VRAM for a spilling device's summary clause. Failure here
        // is non-fatal: row's `device_name` falls back to None and the
        // formatter renders `GPU N` instead; the clause omits free VRAM.
        let info = device_info(idx).ok();
        let free_bytes = info.as_ref().map(|d| d.free_bytes);
        let device_name = info.and_then(|d| d.name);
        // One live spill sample per device (not per row): `snapshot_is_spilling`
        // is adapter-wide, so every row on this device gets the same
        // value — the same "broadcast" shape `hmn watch`'s `spilling`
        // field already uses. `None` (not measurable) on Linux and macOS,
        // where spill cannot exist (the cell reads n/a), and on Windows
        // pre-WDDM-2.0, a non-NVIDIA adapter, or a PDH hiccup (it reads ?).
        //
        // Sampled *before* gpu_process_listing(idx), not after: the SHARED
        // column on each row and the SPILL verdict broadcast onto it
        // should describe the same instant. Sampling after would let a
        // process's per-process PDH enumeration (which gpu_process_listing
        // performs) and the Toolhelp32Snapshot name-resolution walk
        // elapse in between — real time under load — so a job that
        // starts or stops spilling in that gap would show a SHARED
        // figure and a SPILL verdict from two different moments. This
        // does mean the PDH open+sample below still runs even for a
        // device every row of which the --pid/--min/--filter filters end up
        // dropping; that's the accepted trade (measured negligible on
        // the reference machine — see CHANGELOG) for not straddling
        // gpu_process_listing()'s own call duration, the same call-ordering
        // discipline `hmn watch`'s wall_clock/t_ms pairing uses.
        let spilling = snapshot_is_spilling(idx);
        tried += 1;
        let listing = match gpu_process_listing(idx) {
            Ok(listing) => listing,
            Err(e) if filters.device.is_some() => {
                let detail = failure_detail(&e, REMEDY_OUTSIDE_SANDBOX);
                eprintln!("{}", device_query_failure_line(idx, &detail, false));
                return ExitCode::from(2);
            }
            Err(e) => {
                let detail = failure_detail(&e, REMEDY_OUTSIDE_SANDBOX);
                eprintln!("{}", device_query_failure_line(idx, &detail, true));
                failed += 1;
                continue;
            }
        };
        notes.unreadable += filters.relevant_denied(&listing.denied_pids);
        let entries = listing.entries;
        // Over every process on the device, before any filter: a row's
        // share of the device's shared bytes, and the device's summary
        // clause, must not depend on which rows are displayed.
        let device_shared_bytes = entries
            .iter()
            .fold(0_u64, |sum, e| sum.saturating_add(e.shared_used_bytes));
        if spilling == Some(true) {
            notes.spilling.push(DeviceSpill {
                index: idx,
                free_bytes,
                shared_bytes: device_shared_bytes,
                paged: entries
                    .iter()
                    .filter(|e| is_paged(e.shared_used_bytes))
                    .count(),
            });
        }
        for entry in entries {
            match filters.judge(&entry) {
                PsJudgement::Listed => {}
                PsJudgement::Filtered => continue,
                PsJudgement::Unnamed => {
                    notes.unnamed += 1;
                    continue;
                }
            }
            let (paged, shared_share) =
                paged_and_share(spilling, entry.shared_used_bytes, device_shared_bytes);
            rows.push(PsRow {
                pid: entry.pid,
                name: entry.name,
                used_bytes: entry.used_bytes,
                shared_used_bytes: entry.shared_used_bytes,
                device_index: idx,
                // BORROW: clone — device_name is shared across all
                // rows for this device.
                device_name: device_name.clone(),
                spilling,
                paged,
                shared_share,
            });
        }
    }

    // Nothing on stdout, as on the `--device` failure path: a table or
    // `[]` after "no device could be queried" reads as "idle" to a script
    // that checks stdout alone.
    if every_device_failed(tried, failed) {
        eprintln!("{ALL_DEVICES_FAILED_LINE}");
        return ExitCode::from(2);
    }

    // Human-facing display order, per `--sort` (default: VRAM descending
    // so the biggest consumers land at the top — the row a user asking
    // "what's eating my GPU memory?" wants to see first). Tie-breaks
    // (name ascending for grouping duplicate-name processes like
    // `msedgewebview2.exe`, then PID ascending for stable order across
    // runs) are identical regardless of key — see `ps_row_comparator`.
    // The library's `gpu_process_listing()` returns rows PID-sorted; this
    // overrides that for display only.
    rows.sort_by(ps_row_comparator(sort));

    if json {
        print!("{}", format_ps_json(&rows));
    } else {
        print!("{}", format_ps_table(&rows));
    }
    // Human-readable summary on stderr — preserves stdout's scriptability
    // (header-only table or `[]` for empty) while giving interactive
    // users an unambiguous "command worked, here's the count" line.
    // Printed for every listing that reaches here, even when rows is
    // non-empty, so the message is a consistent confirmation rather than
    // an error indicator. Redirect 2>/dev/null to suppress.
    eprintln!("hmn: {}", format_ps_summary(&rows, filters, &notes));
    let code = ps_exit_code(exit_status, rows.is_empty(), failed, notes.unreadable);
    ExitCode::from(code)
}

/// The parenthetical's remedy clause, or `None` when nothing is counted:
/// `N unreadable` (processes the caller was refused) and `M protected`
/// (rows whose name could not be resolved), each present only when
/// non-zero, joined by `, `, then ` — ` and the one remedy
/// ([`with_remedy`]). On Windows and Linux only `M protected —
/// re-run elevated for names` can occur; on macOS the one remedy,
/// `re-run outside the sandbox`, covers both counts.
#[must_use]
pub fn remedy_clause(protected: usize, unreadable: usize, outside_sandbox: bool) -> Option<String> {
    let mut counts: Vec<String> = Vec::new();
    if unreadable > 0 {
        counts.push(format!("{unreadable} unreadable"));
    }
    if protected > 0 {
        counts.push(format!("{protected} protected"));
    }
    if counts.is_empty() {
        return None;
    }
    Some(with_remedy(&counts.join(", "), outside_sandbox))
}

/// Build the stderr summary string for `hmn ps`. Format:
/// `<N> GPU process[es] found[ matching <filters>][ (<parts>)][; device <D> spilling: [<F> free, ]<S> shared, <P> process[es] paged]….`,
/// where `<parts>` joins, with `; `, whichever of `<X.Y> <unit> committed
/// total`, the remedy clause (`<U> unreadable` and `<M> protected`, joined
/// by `, `, then ` — <remedy>`; see [`remedy_clause`]) and `<K> unnamed not
/// matched` apply, where `<remedy>` is `re-run elevated for names`
/// (Windows, Linux) or `re-run outside the sandbox` (macOS,
/// `outside_sandbox == true`), as [`with_remedy`] words it.
///
/// Three appendices after the noun, each elided when not applicable:
///
/// - **Filter clause** (` matching pid=N device=M min=X unit filter="a"`):
///   appended only when at least one filter is active, as
///   [`PsFilters::clauses`] words it. Supports any combination of
///   `--pid`, `--device`, `--min` and `--filter` (`--min` echoed via
///   [`format_vram_precise`], not [`format_vram`], so a sub-MiB or
///   otherwise-imprecise `--min` value is never misreported).
/// - **Committed-total parenthetical** (` (X.Y unit committed total)`,
///   formatted via [`format_vram`] so it renders as `MiB` below 1
///   `GiB` and `GiB` to one decimal place otherwise): appended only
///   when `count > 0`. The word "committed" hints at the `WDDM`
///   commit-vs-resident distinction the Windows `PDH` backend
///   exposes — summing `used_bytes` across processes can exceed
///   physical `VRAM` under `WDDM` (a real `WDDM` property, not a
///   bug), so naming the figure "committed total" prevents that from
///   reading as broken when a Windows user sees, say, 32 `GiB`
///   committed on a 16 `GiB` card. Elided entirely when `count == 0`
///   because a zero-bytes total carries no information.
///
///   The parenthetical carries an **unreadable count**
///   (`; U unreadable — re-run outside the sandbox`) when `notes.unreadable`
///   is non-zero: the processes the caller was refused
///   ([`hypomnesis::GpuProcessListing::denied_pids`]) that `--pid` leaves
///   relevant ([`PsFilters::relevant_denied`]). They are not listed, since
///   neither their size nor their name is known, so the count says how much
///   of the machine the listing does not cover. It is always `0` off macOS.
///
///   When at least one row is genuinely unresolvable, the parenthetical
///   carries a **protected continuation**
///   (`; M protected — re-run elevated for names` on Windows and Linux,
///   `; M protected — re-run outside the sandbox` on macOS, where
///   `outside_sandbox == true`) joined by `; `. When both counts are
///   non-zero they share one clause and say the remedy once
///   (`U unreadable, M protected — re-run outside the sandbox`). A row
///   counts as protected when `name.is_none()` (`NVML`'s
///   `/proc/<pid>/comm` unreadable on Linux; on macOS when no source gave a
///   name, as when a sandbox refuses both `proc_pidpath` and
///   `KERN_PROC_PID` — see [`hypomnesis::GpuProcessEntry::name`] and README
///   Limitations, item 9);
///   when `name` is exactly `Some("[protected]")` (the
///   Windows-only bracket meaning the `Toolhelp32Snapshot` fallback could
///   not be taken at all — see `hypomnesis::gpu_processes`'s Windows
///   path); or when `name` is the literal `Some("?")` string the
///   pre-`WDDM 2.0` `nvidia-smi` fallback writes for a row it couldn't
///   name itself (same "might resolve under elevation" meaning as the
///   other two — pre-existing, but not previously counted here). `PID 4`
///   (`[kernel]`) and `[exited]` rows deliberately
///   do **not** contribute to this count: `[kernel]` has no executable
///   image to resolve regardless of privilege, and `[exited]` means the
///   process was already gone by the time of the name lookup — elevation
///   would not have helped either case, so counting them would overstate
///   what re-running elevated could actually buy. As of v0.2.8, most
///   Windows `?` rows resolve to a real name via the snapshot fallback
///   before this function ever sees them; the count that remains is
///   genuinely foreign-user / `SYSTEM` / `PPL`-protected processes, or —
///   on Linux/macOS, where the fallback doesn't apply — any unresolved
///   row at all.
///
///   Under `--filter`, a third continuation, `; K unnamed not matched`,
///   counts the processes that passed `--pid` / `--min` but have no name
///   a pattern can be matched against ([`filterable_name`]) —
///   `notes.unnamed`, as `run_ps` counted them. They are not listed, so they never add to
///   the protected count; counting them keeps a filtered listing from
///   hiding a process silently. Shown even when no row is listed.
///
/// - **Device verdict** (`; device D spilling: F free, S shared, P
///   processes paged`), once per device in `notes.spilling`, after the
///   parenthetical. The SPILL column repeats a device's verdict on each
///   of its rows; this states it once, with the evidence: free `VRAM`
///   (omitted when `device_info` failed), the shared-resident bytes summed
///   over every process on the device, and how many of those are paged
///   ([`is_paged`]) — all counted before any filter.
///
/// "GPU process" / "GPU processes" (not the previous-release
/// "compute process" / "compute processes") because on the `PDH`
/// Windows path the list includes every GPU memory holder
/// (compositor, browsers, games, compute), not just `CUDA` contexts.
fn format_ps_summary_with(
    rows: &[PsRow],
    filters: &PsFilters,
    notes: &SummaryNotes,
    outside_sandbox: bool,
) -> String {
    let count = rows.len();
    let protected = rows
        .iter()
        .filter(|r| {
            r.name.is_none()
                || r.name.as_deref() == Some("[protected]")
                || r.name.as_deref() == Some("?")
        })
        .count();
    let committed_total: u64 = rows.iter().map(|r| r.used_bytes).sum();

    let noun = if count == 1 {
        "GPU process"
    } else {
        "GPU processes"
    };

    let mut out = format!("{count} {noun} found");

    let clauses = filters.clauses();
    if !clauses.is_empty() {
        let _ = write!(out, " matching {}", clauses.join(" "));
    }

    // The parenthetical: committed total, remedy clause (unreadable and
    // protected counts, one remedy), unnamed count, each present only when
    // it says something, joined by "; ". The word
    // "committed" hints at the WDDM commit-vs-resident distinction the
    // Windows backend exposes — summing `used_bytes` across processes can
    // exceed physical VRAM under WDDM (a real WDDM property, not a bug),
    // so naming the figure "committed total" prevents that from reading
    // as broken. The total is elided when `count == 0` because "0 MiB
    // committed total" carries no information; `protected` is then 0 too.
    let mut parts: Vec<String> = Vec::new();
    if count > 0 {
        parts.push(format!("{} committed total", format_vram(committed_total)));
    }
    parts.extend(remedy_clause(protected, notes.unreadable, outside_sandbox));
    if notes.unnamed > 0 {
        parts.push(format!("{} unnamed not matched", notes.unnamed));
    }
    if !parts.is_empty() {
        let _ = write!(out, " ({})", parts.join("; "));
    }

    // The device verdict, stated once per spilling device rather than
    // left to be read off a SPILL cell repeated on every row.
    for d in &notes.spilling {
        let _ = write!(out, "; device {} spilling: ", d.index);
        if let Some(free) = d.free_bytes {
            let _ = write!(out, "{} free, ", format_vram(free));
        }
        let noun = if d.paged == 1 { "process" } else { "processes" };
        let _ = write!(
            out,
            "{} shared, {} {noun} paged",
            format_vram(d.shared_bytes),
            d.paged
        );
    }

    out.push('.');
    out
}

/// [`format_ps_summary_with`] with this platform's remedy, [`REMEDY_OUTSIDE_SANDBOX`].
fn format_ps_summary(rows: &[PsRow], filters: &PsFilters, notes: &SummaryNotes) -> String {
    format_ps_summary_with(rows, filters, notes, REMEDY_OUTSIDE_SANDBOX)
}

/// Format `ps` rows as a fixed-column text table. Always prints the
/// header, even when `rows` is empty.
fn format_ps_table(rows: &[PsRow]) -> String {
    let mut table = Table::new(&["PID", "NAME", "VRAM", "SHARED", "DEVICE", "SPILL"]);
    for r in rows {
        table.push_row(vec![
            r.pid.to_string(),
            // BORROW: explicit to_owned — the table owns its cells; "?" is
            // the "can't tell" glyph for an unresolved name.
            r.name.as_deref().unwrap_or("?").to_owned(),
            format_vram(r.used_bytes),
            format_vram(r.shared_used_bytes),
            r.device_name
                .clone()
                .unwrap_or_else(|| format!("GPU {}", r.device_index)),
            // BORROW: explicit to_owned — the table owns its cells.
            spill_cell(r.spilling, r.paged).to_owned(),
        ]);
    }
    table.render(Some(""), "")
}

/// Format `ps` rows as a JSON array, one object per row. Hand-rolled
/// (no `serde` dep — keeps the `cli` feature lean for v0.2). Each
/// object: `{"pid":N,"name":<string|null>,"used_bytes":N,"shared_used_bytes":N,"device_index":N,"device_name":<string|null>,"spilling":<true|false|null>,"paged":<true|false|null>,"shared_share":<number|null>}`.
/// `spilling` is `null`, never `false`, when spill isn't measurable
/// here — see [`PsRow::spilling`]'s doc — and `paged` and `shared_share`
/// are `null` exactly when it is. `shared_share` is written to four
/// decimal places (`0.9048`). String values are JSON-escaped via
/// [`json_string_or_null`].
fn format_ps_json(rows: &[PsRow]) -> String {
    let mut out = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let name_json = json_string_or_null(row.name.as_deref());
        let device_name_json = json_string_or_null(row.device_name.as_deref());
        let spilling_json = json_value_or_null(row.spilling);
        let paged_json = json_value_or_null(row.paged);
        // Pre-formatted: a float's own `Display` could print `NaN`/`inf`.
        let share_json = json_value_or_null(row.shared_share.map(|f| format!("{f:.4}")));
        let _ = write!(
            out,
            r#"{{"pid":{},"name":{name_json},"used_bytes":{},"shared_used_bytes":{},"device_index":{},"device_name":{device_name_json},"spilling":{spilling_json},"paged":{paged_json},"shared_share":{share_json}}}"#,
            row.pid, row.used_bytes, row.shared_used_bytes, row.device_index,
        );
    }
    out.push_str("]\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-helpers")]
    use crate::test_support::entry;
    use crate::test_support::row;

    /// Like [`row`] but with a non-zero `shared_used_bytes` — for the
    /// SHARED-column / spill-signal specific tests.
    fn row_shared(pid: u32, name: Option<&str>, used_bytes: u64, shared_used_bytes: u64) -> PsRow {
        PsRow {
            pid,
            name: name.map(str::to_owned),
            used_bytes,
            shared_used_bytes,
            device_index: 0,
            device_name: None,
            spilling: None,
            paged: None,
            shared_share: None,
        }
    }

    /// Like [`row`] but with an explicit `spilling` — for the SPILL
    /// column / field specific tests.
    fn row_spilling(pid: u32, name: Option<&str>, spilling: Option<bool>) -> PsRow {
        PsRow {
            pid,
            name: name.map(str::to_owned),
            used_bytes: 0,
            shared_used_bytes: 0,
            device_index: 0,
            device_name: None,
            spilling,
            paged: None,
            shared_share: None,
        }
    }

    // --- PsFilters ---

    #[cfg(feature = "test-helpers")]
    #[test]
    fn ps_filters_default_lists_everything() {
        let f = PsFilters::default();
        assert_eq!(f.judge(&entry(1, Some("a.exe"), 0, 0)), PsJudgement::Listed);
        // Without --filter, an unnamed process is simply listed.
        assert_eq!(
            f.judge(&entry(2, None, 8 << 30, 1 << 30)),
            PsJudgement::Listed
        );
        let clauses = f.clauses();
        assert!(clauses.is_empty(), "{clauses:?}");
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn ps_filters_pid_and_min_both_apply() {
        const MIB: u64 = 1024 * 1024;
        let f = PsFilters {
            pids: vec![7],
            device: Some(0),
            min_bytes: Some(100 * MIB),
            patterns: Vec::new(),
        };
        // The footprint is used + shared, as `--min` documents.
        assert_eq!(
            f.judge(&entry(7, Some("a.exe"), 60 * MIB, 40 * MIB)),
            PsJudgement::Listed
        );
        assert_eq!(
            f.judge(&entry(7, Some("a.exe"), 60 * MIB, 39 * MIB)),
            PsJudgement::Filtered
        );
        assert_eq!(
            f.judge(&entry(8, Some("a.exe"), 200 * MIB, 0)),
            PsJudgement::Filtered
        );
        // `device` selects devices, not rows: it never rejects an entry.
        let f = PsFilters {
            device: Some(3),
            ..PsFilters::default()
        };
        assert_eq!(f.judge(&entry(1, None, 0, 0)), PsJudgement::Listed);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn ps_filters_patterns_are_a_case_insensitive_substring_or() {
        let f = PsFilters {
            patterns: vec!["CANVAS".to_owned(), "python".to_owned()],
            ..PsFilters::default()
        };
        assert_eq!(
            f.judge(&entry(1, Some("canvas"), 0, 0)),
            PsJudgement::Listed
        );
        assert_eq!(
            f.judge(&entry(2, Some("Python.exe"), 0, 0)),
            PsJudgement::Listed
        );
        assert_eq!(
            f.judge(&entry(3, Some("firefox.exe"), 0, 0)),
            PsJudgement::Filtered
        );
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn ps_filters_patterns_count_unnameable_rows_instead_of_dropping_them() {
        let f = PsFilters {
            patterns: vec!["canvas".to_owned()],
            ..PsFilters::default()
        };
        for name in [None, Some("?"), Some("[protected]"), Some("[exited]")] {
            assert_eq!(
                f.judge(&entry(1, name, 0, 0)),
                PsJudgement::Unnamed,
                "{name:?}"
            );
        }
        // `[kernel]` is a real name: judged by the pattern, not counted.
        assert_eq!(
            f.judge(&entry(4, Some("[kernel]"), 0, 0)),
            PsJudgement::Filtered
        );
        // `--pid` / `--min` apply first: a row they reject is not counted.
        let f = PsFilters {
            pids: vec![9],
            patterns: vec!["canvas".to_owned()],
            ..PsFilters::default()
        };
        assert_eq!(f.judge(&entry(1, None, 0, 0)), PsJudgement::Filtered);
    }

    #[test]
    fn matches_any_is_a_case_insensitive_substring_or() {
        let pats = ["Figure13".to_owned(), "python".to_owned()];
        assert!(matches_any("figure13_newline_patch.exe", &pats));
        assert!(matches_any("PYTHON.EXE", &pats));
        assert!(!matches_any("dwm.exe", &pats));
    }

    // --- failed and skipped devices ---

    #[test]
    fn every_device_failed_needs_a_device_tried_and_none_answering() {
        for ((tried, failed), want) in [
            ((0, 0), false),
            ((1, 0), false),
            ((1, 1), true),
            ((2, 1), false),
            ((2, 2), true),
        ] {
            assert_eq!(
                every_device_failed(tried, failed),
                want,
                "(tried, failed) = ({tried}, {failed})"
            );
        }
    }

    #[test]
    fn device_query_failure_line_for_a_named_device_is_unchanged() {
        assert_eq!(
            device_query_failure_line(3, &"boom", false),
            "hmn: ps failed to query device 3: boom"
        );
    }

    #[test]
    fn device_query_failure_line_for_a_skipped_device_ends_with_skipped() {
        assert_eq!(
            device_query_failure_line(3, &"boom", true),
            "hmn: ps failed to query device 3: boom (skipped)"
        );
    }

    #[test]
    fn ps_exit_code_table_pins_the_exit_status_rule() {
        for ((exit_status, rows_empty, failed, relevant_denied), want) in [
            ((false, false, 0, 0), 0),
            ((false, true, 0, 0), 0),
            ((false, true, 1, 0), 0),
            ((true, false, 0, 0), 0),
            ((true, false, 1, 0), 0),
            ((true, true, 0, 0), 1),
            ((true, true, 1, 0), 2),
            ((true, true, 2, 0), 2),
            // With the rows above, all 16 cells of the four inputs: a
            // failed device with listed rows and no exit status, and every
            // cell with a relevant denied PID.
            ((false, false, 1, 0), 0),
            ((false, false, 0, 1), 0),
            ((false, false, 1, 1), 0),
            ((false, true, 0, 1), 0),
            ((false, true, 1, 1), 0),
            ((true, false, 0, 1), 0),
            ((true, false, 1, 1), 0),
            ((true, true, 0, 1), 2),
            ((true, true, 1, 1), 2),
        ] {
            assert_eq!(
                ps_exit_code(exit_status, rows_empty, failed, relevant_denied),
                want,
                "(exit_status, rows_empty, failed, relevant_denied) = \
                 ({exit_status}, {rows_empty}, {failed}, {relevant_denied})"
            );
        }
    }

    // --- relevant_denied ---

    #[test]
    fn relevant_denied_counts_every_denied_pid_or_only_the_pid_asked_for() {
        assert_eq!(PsFilters::default().relevant_denied(&[1, 2, 3]), 3);
        assert_eq!(filters(&[2, 3], None, None).relevant_denied(&[1, 2, 3]), 2);
        assert_eq!(filters(&[9], None, None).relevant_denied(&[1, 2, 3]), 0);
    }

    #[test]
    fn relevant_denied_ignores_min_and_filter() {
        // A denied PID's size and name are unknown, so neither `--min` nor
        // `--filter` can exclude it.
        // BORROW: `to_owned` builds the filter's `String` from a literal.
        let f = PsFilters::new(&[], None, Some(1 << 30), vec!["canvas".to_owned()]);
        assert_eq!(f.relevant_denied(&[1, 2, 3]), 3);
        // BORROW: `to_owned` builds the filter's `String` from a literal.
        let f = PsFilters::new(&[2], None, Some(1 << 30), vec!["canvas".to_owned()]);
        assert_eq!(f.relevant_denied(&[1, 2, 3]), 1);
    }

    // --- resolved_name (PID-reuse comparison filter) ---

    #[test]
    fn resolved_name_passes_through_real_names() {
        assert_eq!(resolved_name(Some("python.exe")), Some("python.exe"));
    }

    #[test]
    fn resolved_name_passes_through_kernel_bracket() {
        // [kernel] (PID 4) is permanently stable and never flickers —
        // safe to treat as a comparable name, unlike [protected]/[exited].
        assert_eq!(resolved_name(Some("[kernel]")), Some("[kernel]"));
    }

    #[test]
    fn resolved_name_filters_protected_and_exited_brackets() {
        assert_eq!(resolved_name(Some("[protected]")), None);
        assert_eq!(resolved_name(Some("[exited]")), None);
    }

    #[test]
    fn resolved_name_filters_none() {
        assert_eq!(resolved_name(None), None);
    }

    #[test]
    fn filterable_name_excludes_only_the_non_names() {
        assert_eq!(filterable_name(Some("canvas")), Some("canvas"));
        assert_eq!(filterable_name(Some("[kernel]")), Some("[kernel]"));
        assert_eq!(filterable_name(Some("[protected]")), None);
        assert_eq!(filterable_name(Some("[exited]")), None);
        assert_eq!(filterable_name(Some("?")), None);
        assert_eq!(filterable_name(None), None);
    }

    // --- paged mark (request 2) ---

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    #[test]
    fn is_paged_uses_the_spill_conditions_floor() {
        assert!(!is_paged(255 * MIB));
        assert!(is_paged(256 * MIB));
        // The askesis report's benign baselines are not paged.
        assert!(!is_paged(48 * MIB));
        assert!(!is_paged(110 * MIB));
    }

    #[test]
    fn paged_and_share_is_none_exactly_when_spill_is_unmeasurable() {
        assert_eq!(paged_and_share(None, 2 * GIB, 4 * GIB), (None, None));
    }

    #[test]
    fn paged_and_share_marks_only_on_a_spilling_device() {
        assert_eq!(
            paged_and_share(Some(true), GIB, 4 * GIB),
            (Some(true), Some(0.25))
        );
        // Not spilling: never paged, but the share is still reported.
        assert_eq!(
            paged_and_share(Some(false), GIB, 4 * GIB),
            (Some(false), Some(0.25))
        );
        assert_eq!(
            paged_and_share(Some(true), 48 * MIB, 4 * GIB).0,
            Some(false)
        );
    }

    #[test]
    fn paged_and_share_is_zero_on_a_device_without_shared_bytes() {
        assert_eq!(paged_and_share(Some(false), 0, 0), (Some(false), Some(0.0)));
    }

    /// A row on device 0 with the given verdict, SHARED and mark.
    fn row_marked(spilling: Option<bool>, shared: u64, paged: Option<bool>) -> PsRow {
        PsRow {
            spilling,
            paged,
            shared_used_bytes: shared,
            ..row(1, Some("a.exe"), 0, 0, None)
        }
    }

    #[test]
    fn paged_verdict_follows_the_device_verdict() {
        assert_eq!(paged_verdict(None, 2 * GIB), None);
        assert_eq!(paged_verdict(Some(false), 2 * GIB), Some(false));
        assert_eq!(paged_verdict(Some(true), 2 * GIB), Some(true));
        assert_eq!(paged_verdict(Some(true), 48 * MIB), Some(false));
    }

    // --- format_ps_table ---

    // The SPILL cell of a row whose spill was not read (`spilling: None`):
    // `?` on Windows, where spill exists; `n/a` elsewhere, where it cannot.
    // Padded to three characters, so each table literal below holds one
    // interpolation and the rest of the cell's padding.
    #[cfg(windows)]
    const UNKNOWN_SPILL: &str = "?  ";
    #[cfg(not(windows))]
    const UNKNOWN_SPILL: &str = "n/a";

    #[test]
    fn format_ps_table_empty_prints_header_only() {
        let s = format_ps_table(&[]);
        // Header line ends with newline; widths default to header lengths.
        assert_eq!(s, "PID  NAME  VRAM  SHARED  DEVICE  SPILL\n");
    }

    #[test]
    fn format_ps_table_single_row() {
        let r = row(
            12345,
            Some("python.exe"),
            8_589_934_592, // 8 GiB
            0,
            Some("RTX 5060 Ti"),
        );
        let s = format_ps_table(&[r]);
        let expected = format!(
            "PID    NAME        VRAM     SHARED  DEVICE       SPILL\n\
             12345  python.exe  8.0 GiB  0 MiB   RTX 5060 Ti  {UNKNOWN_SPILL}  \n"
        );
        assert_eq!(s, expected);
    }

    #[test]
    fn format_ps_table_protected_name_renders_question_mark() {
        // Column widths: PID=3 (header), NAME=4 (header), VRAM=7
        // ("256 MiB"), SHARED=6 (header), DEVICE=11 ("RTX 5060 Ti"),
        // SPILL=5 (header — "?" and "n/a" are shorter). Two-space
        // separators. The NAME `?` is on every platform.
        let r = row(99, Some("?"), 268_435_456, 0, Some("RTX 5060 Ti"));
        let s = format_ps_table(&[r]);
        let expected = format!(
            "PID  NAME  VRAM     SHARED  DEVICE       SPILL\n\
             99   ?     256 MiB  0 MiB   RTX 5060 Ti  {UNKNOWN_SPILL}  \n"
        );
        assert_eq!(s, expected);
    }

    #[test]
    fn format_ps_table_missing_name_renders_question_mark() {
        // Missing name (None) renders identically to the protected `?`
        // case — both go through the `unwrap_or("?")` path.
        let r = row(99, None, 268_435_456, 0, Some("RTX 5060 Ti"));
        let s = format_ps_table(&[r]);
        let expected = format!(
            "PID  NAME  VRAM     SHARED  DEVICE       SPILL\n\
             99   ?     256 MiB  0 MiB   RTX 5060 Ti  {UNKNOWN_SPILL}  \n"
        );
        assert_eq!(s, expected);
    }

    #[test]
    fn format_ps_table_spill_column_renders_paged_device_no_and_unknown() {
        // Since v0.2.13 a spilling device's verdict is not broadcast as
        // `SPILL` on every row: the paged process reads `PAGED`, the
        // device's other processes `device`.
        let rows = [
            PsRow {
                name: Some("a.exe".to_owned()),
                ..row_marked(Some(true), 0, Some(true))
            },
            PsRow {
                name: Some("b.exe".to_owned()),
                ..row_marked(Some(true), 0, Some(false))
            },
            row_spilling(2, Some("c.exe"), Some(false)),
            row_spilling(3, Some("d.exe"), None),
        ];
        let s = format_ps_table(&rows);
        assert!(s.contains("a.exe  0 MiB  0 MiB   GPU 0   PAGED "));
        assert!(s.contains("b.exe  0 MiB  0 MiB   GPU 0   device"));
        assert!(s.contains("c.exe  0 MiB  0 MiB   GPU 0   no    "));
        assert!(s.contains(&format!("d.exe  0 MiB  0 MiB   GPU 0   {UNKNOWN_SPILL}   ")));
    }

    #[test]
    fn format_ps_table_falls_back_to_gpu_n_when_no_device_name() {
        let r = row(99, Some("python.exe"), 268_435_456, 3, None);
        let s = format_ps_table(&[r]);
        assert!(s.contains("python.exe  256 MiB  0 MiB   GPU 3"));
    }

    #[test]
    fn format_ps_table_shared_column_renders_nonzero_bytes() {
        // A genuinely spilling row: 16 GiB dedicated commit, 2 GiB
        // resident shared. The SHARED cell goes through the same
        // format_vram path as VRAM.
        let r = row_shared(
            77,
            Some("py.exe"),
            16 * 1024 * 1024 * 1024,
            2 * 1024 * 1024 * 1024,
        );
        let s = format_ps_table(&[r]);
        assert!(s.contains("16.0 GiB  2.0 GiB"));
    }

    // --- format_ps_json ---

    #[test]
    fn format_ps_json_empty() {
        assert_eq!(format_ps_json(&[]), "[]\n");
    }

    #[test]
    fn format_ps_json_single_row() {
        let r = row(
            12345,
            Some("python.exe"),
            8 * 1_048_576,
            0,
            Some("RTX 5060 Ti"),
        );
        let s = format_ps_json(&[r]);
        assert_eq!(
            s,
            "[{\"pid\":12345,\"name\":\"python.exe\",\"used_bytes\":8388608,\"shared_used_bytes\":0,\"device_index\":0,\"device_name\":\"RTX 5060 Ti\",\"spilling\":null,\"paged\":null,\"shared_share\":null}]\n"
        );
    }

    #[test]
    fn format_ps_json_null_name() {
        let r = row(42, None, 0, 0, None);
        let s = format_ps_json(&[r]);
        assert_eq!(
            s,
            "[{\"pid\":42,\"name\":null,\"used_bytes\":0,\"shared_used_bytes\":0,\"device_index\":0,\"device_name\":null,\"spilling\":null,\"paged\":null,\"shared_share\":null}]\n"
        );
    }

    #[test]
    fn format_ps_json_two_rows_comma_separated() {
        let a = row(1, Some("a.exe"), 1_048_576, 0, Some("GPU"));
        let b = row(2, Some("b.exe"), 2_097_152, 0, Some("GPU"));
        let s = format_ps_json(&[a, b]);
        assert_eq!(
            s,
            "[{\"pid\":1,\"name\":\"a.exe\",\"used_bytes\":1048576,\"shared_used_bytes\":0,\"device_index\":0,\"device_name\":\"GPU\",\"spilling\":null,\"paged\":null,\"shared_share\":null},\
             {\"pid\":2,\"name\":\"b.exe\",\"used_bytes\":2097152,\"shared_used_bytes\":0,\"device_index\":0,\"device_name\":\"GPU\",\"spilling\":null,\"paged\":null,\"shared_share\":null}]\n"
        );
    }

    #[test]
    fn format_ps_json_spilling_true_false_null() {
        let rows = [
            row_spilling(1, Some("a.exe"), Some(true)),
            row_spilling(2, Some("b.exe"), Some(false)),
            row_spilling(3, Some("c.exe"), None),
        ];
        let s = format_ps_json(&rows);
        assert!(s.contains(r#""pid":1,"name":"a.exe","used_bytes":0,"shared_used_bytes":0,"device_index":0,"device_name":null,"spilling":true"#));
        assert!(s.contains(r#""pid":2,"name":"b.exe","used_bytes":0,"shared_used_bytes":0,"device_index":0,"device_name":null,"spilling":false"#));
        assert!(s.contains(r#""pid":3,"name":"c.exe","used_bytes":0,"shared_used_bytes":0,"device_index":0,"device_name":null,"spilling":null"#));
    }

    #[test]
    fn format_ps_json_paged_and_shared_share() {
        let paged = PsRow {
            spilling: Some(true),
            paged: Some(true),
            shared_share: Some(1.9 / 2.1),
            ..row(26476, Some("canvas.exe"), 0, 0, None)
        };
        let unmeasured = row(5, Some("a"), 0, 0, None);
        let s = format_ps_json(&[paged, unmeasured]);
        assert!(s.contains(r#""spilling":true,"paged":true,"shared_share":0.9048}"#));
        assert!(s.contains(r#""spilling":null,"paged":null,"shared_share":null}"#));
    }

    #[test]
    fn format_ps_json_nonzero_shared_bytes() {
        let r = row_shared(7, Some("py.exe"), 1_048_576, 424_242);
        let s = format_ps_json(&[r]);
        assert!(s.contains("\"used_bytes\":1048576,\"shared_used_bytes\":424242,"));
    }

    #[test]
    fn format_ps_json_escapes_quotes_in_name() {
        let r = row(1, Some(r#"weird"name"#), 0, 0, None);
        let s = format_ps_json(&[r]);
        assert!(s.contains(r#""name":"weird\"name""#));
    }

    // --- format_ps_summary (stderr count line) ---

    /// Build `n` `PsRow`s with resolved names — used by tests that
    /// focus on count and filter clauses, not the protected-count
    /// parenthetical (which is exercised separately).
    fn unprotected_rows(n: u32) -> Vec<PsRow> {
        (0..n)
            .map(|i| row(1000 + i, Some("test.exe"), 0, 0, None))
            .collect()
    }

    /// Build `n` `PsRow`s with `name: None` — used to exercise the
    /// protected-count parenthetical.
    fn protected_rows(n: u32) -> Vec<PsRow> {
        (0..n).map(|i| row(2000 + i, None, 0, 0, None)).collect()
    }

    /// The `--pid` / `--device` / `--min` filters as one [`PsFilters`].
    fn filters(pids: &[u32], device: Option<u32>, min_bytes: Option<u64>) -> PsFilters {
        PsFilters::new(pids, device, min_bytes, Vec::new())
    }

    /// Summary notes carrying only an unnamed count.
    fn unnamed(n: usize) -> SummaryNotes {
        SummaryNotes {
            unnamed: n,
            ..Default::default()
        }
    }

    /// The askesis report's spilling RTX 5060 Ti, device 0: 154 MiB free,
    /// 2.1 GiB shared over every process, one of them paged.
    fn askesis_spill() -> DeviceSpill {
        DeviceSpill {
            index: 0,
            free_bytes: Some(154 * MIB),
            shared_bytes: 2 * GIB + 100 * MIB,
            paged: 1,
        }
    }

    #[test]
    fn format_ps_summary_zero_no_filters() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(0),
                &filters(&[], None, None),
                &SummaryNotes::default()
            ),
            "0 GPU processes found."
        );
    }

    #[test]
    fn format_ps_summary_one_no_filters() {
        // Singular noun, no filter clause. `used_bytes: 0` rows still
        // get a committed-total parenthetical (the figure is 0 MiB —
        // honest, even when uninteresting).
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(1),
                &filters(&[], None, None),
                &SummaryNotes::default()
            ),
            "1 GPU process found (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_many_no_filters() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(7),
                &filters(&[], None, None),
                &SummaryNotes::default()
            ),
            "7 GPU processes found (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_with_pid_filter() {
        // Zero rows → no parenthetical at all (committed-total
        // elides; the filter clause still appears).
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(0),
                &filters(&[12345], None, None),
                &SummaryNotes::default()
            ),
            "0 GPU processes found matching pid=12345."
        );
    }

    #[test]
    fn format_ps_summary_with_device_filter() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(2),
                &filters(&[], Some(0), None),
                &SummaryNotes::default()
            ),
            "2 GPU processes found matching device=0 (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_with_both_filters() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(1),
                &filters(&[99], Some(1), None),
                &SummaryNotes::default()
            ),
            "1 GPU process found matching pid=99 device=1 (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_with_min_filter() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(0),
                &filters(&[], None, Some(50 * 1024 * 1024)),
                &SummaryNotes::default()
            ),
            "0 GPU processes found matching min=50 MiB."
        );
    }

    #[test]
    fn format_ps_summary_sub_mib_min_filter_is_not_misreported_as_zero() {
        // Regression: format_ps_summary used to echo --min through
        // format_vram, so a genuine 512 KiB filter read back as
        // "min=0 MiB" — indistinguishable from the documented --min 0
        // no-op, even though rows were actually being hidden.
        let s = format_ps_summary(
            &unprotected_rows(0),
            &filters(&[], None, Some(512 * 1024)),
            &SummaryNotes::default(),
        );
        assert_eq!(s, "0 GPU processes found matching min=512 KiB.");
    }

    #[test]
    fn format_ps_summary_with_all_three_filters() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(1),
                &filters(&[99], Some(1), Some(50 * 1024 * 1024)),
                &SummaryNotes::default()
            ),
            "1 GPU process found matching pid=99 device=1 min=50 MiB (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_states_the_device_verdict_once() {
        let rows = vec![row(26476, Some("canvas.exe"), 14 * GIB, 0, None)];
        let notes = SummaryNotes {
            unnamed: 0,
            spilling: vec![askesis_spill()],
            ..Default::default()
        };
        assert_eq!(
            format_ps_summary(&rows, &PsFilters::default(), &notes),
            "1 GPU process found (14.0 GiB committed total); device 0 spilling: \
             154 MiB free, 2.1 GiB shared, 1 process paged."
        );
    }

    #[test]
    fn format_ps_summary_device_verdict_without_free_and_plural() {
        let notes = SummaryNotes {
            unnamed: 0,
            spilling: vec![
                DeviceSpill {
                    free_bytes: None,
                    paged: 0,
                    ..askesis_spill()
                },
                DeviceSpill {
                    index: 1,
                    paged: 2,
                    ..askesis_spill()
                },
            ],
            ..Default::default()
        };
        // The verdict holds even when a filter lists nothing.
        assert_eq!(
            format_ps_summary(&unprotected_rows(0), &filters(&[7], None, None), &notes),
            "0 GPU processes found matching pid=7; device 0 spilling: 2.1 GiB shared, \
             0 processes paged; device 1 spilling: 154 MiB free, 2.1 GiB shared, 2 processes paged."
        );
    }

    #[test]
    fn format_ps_summary_echoes_repeated_pids_once_each() {
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(0),
                &filters(&[15503, 15534, 15503], None, None),
                &SummaryNotes::default()
            ),
            "0 GPU processes found matching pid=15503,15534."
        );
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn ps_filters_repeated_pids_are_an_or() {
        let f = filters(&[3, 5], None, None);
        assert_eq!(f.judge(&entry(3, Some("a"), 0, 0)), PsJudgement::Listed);
        assert_eq!(f.judge(&entry(5, Some("b"), 0, 0)), PsJudgement::Listed);
        assert_eq!(f.judge(&entry(4, Some("c"), 0, 0)), PsJudgement::Filtered);
    }

    #[test]
    fn format_ps_summary_echoes_filter_patterns_debug_quoted() {
        let f = PsFilters {
            min_bytes: Some(1024 * 1024 * 1024),
            patterns: vec!["canvas".to_owned(), "a b".to_owned()],
            ..PsFilters::default()
        };
        assert_eq!(
            format_ps_summary(&unprotected_rows(1), &f, &SummaryNotes::default()),
            "1 GPU process found matching min=1 GiB filter=\"canvas\",\"a b\" (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_counts_unnamed_rows_even_when_none_listed() {
        let f = PsFilters {
            patterns: vec!["canvas".to_owned()],
            ..PsFilters::default()
        };
        assert_eq!(
            format_ps_summary(&unprotected_rows(0), &f, &unnamed(2)),
            "0 GPU processes found matching filter=\"canvas\" (2 unnamed not matched)."
        );
        assert_eq!(
            format_ps_summary(&unprotected_rows(1), &f, &unnamed(1)),
            "1 GPU process found matching filter=\"canvas\" (0 MiB committed total; 1 unnamed not matched)."
        );
    }

    // -- committed-total parenthetical (non-zero VRAM) --

    #[test]
    fn format_ps_summary_with_committed_total_gib() {
        // 3 rows at 4 GiB each → 12 GiB committed total, formatted
        // with one decimal place to match `format_vram`'s GiB output.
        const FOUR_GIB: u64 = 4 * 1024 * 1024 * 1024;
        let rows = vec![
            row(1001, Some("a.exe"), FOUR_GIB, 0, None),
            row(1002, Some("b.exe"), FOUR_GIB, 0, None),
            row(1003, Some("c.exe"), FOUR_GIB, 0, None),
        ];
        assert_eq!(
            format_ps_summary(&rows, &filters(&[], None, None), &SummaryNotes::default()),
            "3 GPU processes found (12.0 GiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_with_committed_total_mib() {
        // 2 rows at 256 MiB each → 512 MiB, below 1 GiB threshold,
        // formatter renders as MiB.
        const QUARTER_GIB: u64 = 256 * 1024 * 1024;
        let rows = vec![
            row(1001, Some("a.exe"), QUARTER_GIB, 0, None),
            row(1002, Some("b.exe"), QUARTER_GIB, 0, None),
        ];
        assert_eq!(
            format_ps_summary(&rows, &filters(&[], None, None), &SummaryNotes::default()),
            "2 GPU processes found (512 MiB committed total)."
        );
    }

    // -- protected-count parenthetical --

    #[test]
    fn format_ps_summary_one_protected_appends_parenthetical() {
        let mut rows = unprotected_rows(3);
        rows.extend(protected_rows(1));
        assert_eq!(
            format_ps_summary_with(
                &rows,
                &filters(&[], None, None),
                &SummaryNotes::default(),
                false
            ),
            "4 GPU processes found (0 MiB committed total; 1 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_many_protected_appends_parenthetical() {
        let mut rows = unprotected_rows(28);
        rows.extend(protected_rows(4));
        assert_eq!(
            format_ps_summary_with(
                &rows,
                &filters(&[], None, None),
                &SummaryNotes::default(),
                false
            ),
            "32 GPU processes found (0 MiB committed total; 4 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_all_protected() {
        let rows = protected_rows(3);
        assert_eq!(
            format_ps_summary_with(
                &rows,
                &filters(&[], None, None),
                &SummaryNotes::default(),
                false
            ),
            "3 GPU processes found (0 MiB committed total; 3 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_zero_protected_elides_protected_part_keeps_total() {
        // No protected rows → no `M protected …` clause, but the
        // committed-total parenthetical still appears.
        assert_eq!(
            format_ps_summary(
                &unprotected_rows(5),
                &filters(&[], None, None),
                &SummaryNotes::default()
            ),
            "5 GPU processes found (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_protected_with_filters_both_appear() {
        let mut rows = unprotected_rows(2);
        rows.extend(protected_rows(1));
        assert_eq!(
            format_ps_summary_with(
                &rows,
                &filters(&[42], Some(0), None),
                &SummaryNotes::default(),
                false
            ),
            "3 GPU processes found matching pid=42 device=0 (0 MiB committed total; 1 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_bracket_protected_string_counts_as_protected() {
        // Windows-only `[protected]` synthetic name (the
        // Toolhelp32Snapshot fallback itself could not be taken) counts
        // toward the same "re-run elevated" hint as a bare `name: None`
        // row, even though `name` is `Some` here.
        let mut rows = unprotected_rows(2);
        rows.push(row(3000, Some("[protected]"), 0, 0, None));
        assert_eq!(
            format_ps_summary_with(
                &rows,
                &filters(&[], None, None),
                &SummaryNotes::default(),
                false
            ),
            "3 GPU processes found (0 MiB committed total; 1 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_nvidia_smi_question_mark_counts_as_protected() {
        // Pre-existing (not v0.2.8-introduced) case: the pre-WDDM-2.0
        // `nvidia-smi` fallback writes a literal `"?"` name string rather
        // than `None` for a row it couldn't identify. This carries the
        // same "might resolve under elevation" meaning as `None`/
        // `[protected]` and must count toward the hint too — previously
        // it silently didn't, understating the count exactly the way
        // `[exited]` would have overstated it.
        let mut rows = unprotected_rows(2);
        rows.push(row(3002, Some("?"), 0, 0, None));
        assert_eq!(
            format_ps_summary_with(
                &rows,
                &filters(&[], None, None),
                &SummaryNotes::default(),
                false
            ),
            "3 GPU processes found (0 MiB committed total; 1 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_with_outside_sandbox_says_the_macos_remedy() {
        let mut rows = unprotected_rows(3);
        rows.extend(protected_rows(1));
        let s = format_ps_summary_with(
            &rows,
            &filters(&[], None, None),
            &SummaryNotes::default(),
            true,
        );
        assert_eq!(
            s,
            "4 GPU processes found (0 MiB committed total; 1 protected — re-run outside the sandbox)."
        );
    }

    #[test]
    fn format_ps_summary_with_outside_sandbox_leaves_other_summaries_alone() {
        let rows = unprotected_rows(2);
        let f = filters(&[42], Some(0), None);
        let outside = format_ps_summary_with(&rows, &f, &unnamed(3), true);
        let elevated = format_ps_summary_with(&rows, &f, &unnamed(3), false);
        assert_eq!(outside, elevated);
        assert_eq!(
            outside,
            "2 GPU processes found matching pid=42 device=0 (0 MiB committed total; 3 unnamed not matched)."
        );
    }

    // These two twins are what pins `REMEDY_OUTSIDE_SANDBOX` per platform.
    #[cfg(target_os = "macos")]
    #[test]
    fn format_ps_summary_on_macos_says_re_run_outside_the_sandbox() {
        assert_eq!(
            format_ps_summary(
                &protected_rows(2),
                &filters(&[], None, None),
                &SummaryNotes::default()
            ),
            "2 GPU processes found (0 MiB committed total; 2 protected — re-run outside the sandbox)."
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn format_ps_summary_off_macos_says_re_run_elevated() {
        assert_eq!(
            format_ps_summary(
                &protected_rows(2),
                &filters(&[], None, None),
                &SummaryNotes::default()
            ),
            "2 GPU processes found (0 MiB committed total; 2 protected — re-run elevated for names)."
        );
    }

    #[test]
    fn format_ps_summary_bracket_exited_string_does_not_count_as_protected() {
        // `[exited]` means the process was already gone by the time of
        // the name lookup — elevation would not have helped, so it must
        // NOT inflate the protected count (this is the exact
        // overstatement the v0.2.8 dogfooding report flagged).
        let mut rows = unprotected_rows(2);
        rows.push(row(3001, Some("[exited]"), 0, 0, None));
        assert_eq!(
            format_ps_summary(&rows, &filters(&[], None, None), &SummaryNotes::default()),
            "3 GPU processes found (0 MiB committed total)."
        );
    }

    #[test]
    fn format_ps_summary_kernel_bracket_does_not_count_as_protected() {
        // `[kernel]` (PID 4) has no executable image to resolve
        // regardless of privilege — unchanged pre-v0.2.8 behaviour,
        // re-asserted here alongside the new bracket-counting tests.
        let mut rows = unprotected_rows(2);
        rows.push(row(4, Some("[kernel]"), 0, 0, None));
        assert_eq!(
            format_ps_summary(&rows, &filters(&[], None, None), &SummaryNotes::default()),
            "3 GPU processes found (0 MiB committed total)."
        );
    }

    // -- unreadable-count clause --

    /// Summary notes carrying only an unreadable count.
    fn unreadable(n: usize) -> SummaryNotes {
        SummaryNotes {
            unreadable: n,
            ..Default::default()
        }
    }

    #[test]
    fn format_ps_summary_unreadable_counts_with_the_macos_remedy() {
        assert_eq!(
            format_ps_summary_with(
                &unprotected_rows(0),
                &filters(&[], None, None),
                &unreadable(907),
                true
            ),
            "0 GPU processes found (907 unreadable — re-run outside the sandbox)."
        );
    }

    #[test]
    fn format_ps_summary_unreadable_and_protected_say_the_remedy_once() {
        let mut rows = unprotected_rows(3);
        rows.extend(protected_rows(1));
        let s = format_ps_summary_with(&rows, &filters(&[], None, None), &unreadable(907), true);
        assert_eq!(
            s,
            "4 GPU processes found (0 MiB committed total; 907 unreadable, 1 protected — re-run outside the sandbox)."
        );
        assert_eq!(s.matches("re-run").count(), 1, "{s}");
    }

    #[test]
    fn format_ps_summary_unreadable_zero_changes_nothing() {
        let mut rows = unprotected_rows(3);
        rows.extend(protected_rows(1));
        let f = filters(&[], None, None);
        assert_eq!(
            format_ps_summary_with(&rows, &f, &unreadable(0), true),
            "4 GPU processes found (0 MiB committed total; 1 protected — re-run outside the sandbox)."
        );
        assert_eq!(
            format_ps_summary_with(&rows, &f, &unreadable(0), false),
            "4 GPU processes found (0 MiB committed total; 1 protected — re-run elevated for names)."
        );
        // No clause at all: neither a count nor a remedy.
        assert_eq!(
            format_ps_summary_with(&unprotected_rows(0), &f, &unreadable(0), true),
            "0 GPU processes found."
        );
    }

    // --- ps_row_comparator / SortKey ---

    /// Like [`row`] but with an explicit `shared_used_bytes`, needed to
    /// exercise `SortKey::Shared` / `SortKey::Total`.
    fn row_full(pid: u32, name: &str, used_bytes: u64, shared_used_bytes: u64) -> PsRow {
        PsRow {
            pid,
            name: Some(name.to_owned()),
            used_bytes,
            shared_used_bytes,
            device_index: 0,
            device_name: None,
            spilling: None,
            paged: None,
            shared_share: None,
        }
    }

    fn sorted_pids(rows: &mut [PsRow], key: SortKey) -> Vec<u32> {
        rows.sort_by(ps_row_comparator(key));
        rows.iter().map(|r| r.pid).collect()
    }

    #[test]
    fn ps_row_comparator_dedicated_descending() {
        let mut rows = vec![
            row_full(1, "a.exe", 1_000, 9_000),
            row_full(2, "b.exe", 5_000, 0),
            row_full(3, "c.exe", 3_000, 0),
        ];
        assert_eq!(sorted_pids(&mut rows, SortKey::Dedicated), vec![2, 3, 1]);
    }

    #[test]
    fn ps_row_comparator_shared_descending() {
        let mut rows = vec![
            row_full(1, "a.exe", 1_000, 9_000),
            row_full(2, "b.exe", 5_000, 0),
            row_full(3, "c.exe", 3_000, 2_000),
        ];
        assert_eq!(sorted_pids(&mut rows, SortKey::Shared), vec![1, 3, 2]);
    }

    #[test]
    fn ps_row_comparator_total_descending_differs_from_dedicated_and_shared() {
        // pid 1: total 10_000 (highest) but neither dedicated- nor
        // shared-highest alone — only `total` puts it first.
        let mut rows = vec![
            row_full(1, "a.exe", 4_000, 6_000),
            row_full(2, "b.exe", 8_000, 0),
            row_full(3, "c.exe", 0, 7_000),
        ];
        assert_eq!(sorted_pids(&mut rows, SortKey::Total), vec![1, 2, 3]);
        // Confirms neither single-field key would have produced this order.
        assert_eq!(sorted_pids(&mut rows, SortKey::Dedicated), vec![2, 1, 3]);
        assert_eq!(sorted_pids(&mut rows, SortKey::Shared), vec![3, 1, 2]);
    }

    #[test]
    fn ps_row_comparator_tie_break_identical_across_keys() {
        // Two rows tied on every numeric field: every key must fall
        // through to the same name-then-PID tie-break.
        let mut rows = vec![
            row_full(20, "b.exe", 1_000, 1_000),
            row_full(10, "a.exe", 1_000, 1_000),
        ];
        for key in [SortKey::Dedicated, SortKey::Shared, SortKey::Total] {
            assert_eq!(sorted_pids(&mut rows, key), vec![10, 20], "key {key:?}");
        }
    }

    #[test]
    fn ps_row_comparator_total_saturates_instead_of_overflowing() {
        // Pathological but must not panic: plain `+` on two `u64::MAX`
        // values panics in debug builds (and silently wraps in
        // release); `saturating_add` does neither and still orders
        // this row correctly ahead of a small, unambiguous total.
        let mut rows = vec![
            row_full(1, "a.exe", u64::MAX, u64::MAX),
            row_full(2, "b.exe", 1_000, 0),
        ];
        assert_eq!(sorted_pids(&mut rows, SortKey::Total), vec![1, 2]);
    }
}
