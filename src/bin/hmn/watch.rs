// SPDX-License-Identifier: MIT OR Apache-2.0

//! `hmn watch`: attach to running PIDs (or follow the top-N by committed
//! `VRAM`) and sample per-PID usage plus adapter spill state on a timer —
//! a scrolling `time(1)`-style sampler, not a TUI.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use hypomnesis::{
    GpuProcessEntry, SpillReport, SpillTracker, device_info, gpu_process_listing, process_exists,
    snapshot_is_spilling,
};

use crate::format::{
    REMEDY_OUTSIDE_SANDBOX, RemedyPurpose, Table, device_name_suffix, duration_ms, failure_detail,
    format_vram, format_vram_precise, iso8601_utc_millis, json_string, json_string_or_null,
    json_value_or_null, paged_cell, remedy_text, spill_cell, with_remedy,
};
use crate::ps::{
    PsRow, SortKey, filterable_name, footprint_bytes, matches_any, paged_verdict,
    ps_row_comparator, remedy_clause, resolved_name,
};
use crate::spill::{format_spill_report_with_prefix, write_spill_report_fields};

/// Minimum unresolved-PID cumulative growth (bytes, either committed or
/// shared) that triggers the one-shot "unresolved process grew" stderr
/// hint. Same magnitude as [`hypomnesis::spill`]'s
/// `DEFAULT_SHARED_GROWTH_BYTES` (256 MiB) — large enough to clear
/// ordinary counter jitter, small enough to fire well before a real
/// leak becomes a problem.
const UNRESOLVED_GROWTH_HINT_BYTES: u64 = 256 * 1024 * 1024;

/// How long a single sleep chunk in the watch loop lasts before
/// re-checking the Ctrl+C flag — keeps interrupt latency low even when
/// `--interval` is minutes long.
const WATCH_SLEEP_CHUNK: Duration = Duration::from_millis(200);

/// Per-watched-PID bookkeeping accumulated across the whole watch.
struct WatchedPidState {
    /// Committed bytes at the first sample — the baseline the closing
    /// summary reports growth against.
    baseline_used_bytes: u64,
    /// Shared-resident bytes at the first sample.
    baseline_shared_bytes: u64,
    /// Committed bytes at the previous sample — the per-interval delta
    /// is computed against this, then it is overwritten.
    prev_used_bytes: u64,
    /// Shared-resident bytes at the previous sample.
    prev_shared_bytes: u64,
    /// Highest committed reading seen across the watch.
    peak_used_bytes: u64,
    /// Highest shared-resident reading seen across the watch.
    peak_shared_bytes: u64,
    /// Name from the most recent sample that saw this PID; `None` once
    /// no sample has ever resolved one.
    last_name: Option<String>,
    /// Whether the unresolved-growth hint has already fired for this
    /// PID (fires at most once per watch).
    growth_hint_fired: bool,
    /// Whether this PID read `PAGED` in at least one interval
    /// ([`paged_verdict`]) — the closing summary's per-PID verdict.
    ever_paged: bool,
}

impl WatchedPidState {
    /// Fresh state seeded from a PID's first sample: baseline, previous,
    /// and peak all start at the first reading, so the first interval's
    /// delta is `+0`.
    const fn new(used_bytes: u64, shared_bytes: u64) -> Self {
        Self {
            baseline_used_bytes: used_bytes,
            baseline_shared_bytes: shared_bytes,
            prev_used_bytes: used_bytes,
            prev_shared_bytes: shared_bytes,
            peak_used_bytes: used_bytes,
            peak_shared_bytes: shared_bytes,
            last_name: None,
            growth_hint_fired: false,
            ever_paged: false,
        }
    }
}

/// Accumulated per-PID watch state, plus the order PIDs were first seen
/// in.
///
/// A plain `HashMap<u32, WatchedPidState>` would lose insertion order —
/// fine when the watched PID set is fixed for the whole run (the
/// pre-`--follow-new` case, where the closing summary iterates the
/// original fixed list instead), but under `--follow-new` the closing
/// summary must instead walk *every* PID ever tracked (departed or
/// still active) in a deterministic, meaningful order. `seen_order`
/// records that order — chronological, first sighting — without pulling
/// in an ordered-map dependency: [`WatchState::track`] is the sole
/// insertion point and is the only place a PID is ever appended to it,
/// exactly once, the first time that PID is seen.
struct WatchState {
    /// Per-PID accumulated state, keyed by PID.
    by_pid: HashMap<u32, WatchedPidState>,
    /// PIDs in first-seen order. Never contains a duplicate — see
    /// [`WatchState::track`].
    seen_order: Vec<u32>,
}

impl WatchState {
    /// Fresh, empty state.
    fn new() -> Self {
        Self {
            by_pid: HashMap::new(),
            seen_order: Vec::new(),
        }
    }

    /// Get the existing entry for `pid`, or seed a fresh one from
    /// `(used_bytes, shared_bytes)` and record `pid` in
    /// [`Self::seen_order`] — but only on this, the *first* time `pid`
    /// is tracked. A PID that later drops out of the followed set and
    /// re-enters keeps its original `seen_order` position and its
    /// existing history (no reset; see [`process_sample`]'s doc comment
    /// for why re-entry is deliberately not treated as a new process).
    fn track(&mut self, pid: u32, used_bytes: u64, shared_bytes: u64) -> &mut WatchedPidState {
        // `self.seen_order` and `self.by_pid` are disjoint fields, so
        // borrowing the former up front and capturing it in the
        // `or_insert_with` closure below (which only runs, and so only
        // pushes, on an actual insertion) needs no unwrap/expect/entry
        // double-lookup to track first-seen order.
        let seen_order = &mut self.seen_order;
        self.by_pid.entry(pid).or_insert_with(|| {
            seen_order.push(pid);
            WatchedPidState::new(used_bytes, shared_bytes)
        })
    }
}

/// One PID's rendered row for one interval — output of [`process_sample`],
/// consumed by the text-table and JSONL formatters.
struct WatchSampleRow {
    /// Process ID.
    pid: u32,
    /// Process name from this sample; `None` renders as `?`.
    name: Option<String>,
    /// Committed bytes this sample.
    used_bytes: u64,
    /// Signed delta vs. the previous sample (negative = freed).
    used_delta: i64,
    /// Shared-resident bytes this sample.
    shared_bytes: u64,
    /// Signed delta vs. the previous sample.
    shared_delta: i64,
    /// Adapter-wide instantaneous spill state at this sample
    /// ([`SpillTracker::is_spilling`]) — the same value on every row
    /// sharing this interval's timestamp; spill is a device-level
    /// phenomenon, not a per-PID one. `None` when spill isn't
    /// measurable here (no tracker constructed, or
    /// `SpillTracker::is_measurable` is false for this instance —
    /// Linux/macOS, no `pdh` feature, no `GPU Adapter Memory` counter
    /// set) — never collapsed into `Some(false)`, matching `hmn ps`'s
    /// SPILL column/`spilling` field contract.
    spilling: Option<bool>,
    /// Whether this process is being paged this interval
    /// ([`paged_verdict`]: the device is spilling and its shared bytes are
    /// at least 256 MiB, the rule `hmn ps` uses). `None` exactly when
    /// [`Self::spilling`] is. Who is being paged, not who caused it.
    paged: Option<bool>,
}

/// End-of-watch peak/baseline summary for one watched PID.
pub struct WatchPidSummary {
    /// Process ID.
    pub pid: u32,
    /// Name from the most recent sample that resolved one.
    pub name: Option<String>,
    /// Committed bytes at the first sample.
    pub baseline_used_bytes: u64,
    /// Highest committed reading across the watch.
    pub peak_used_bytes: u64,
    /// Shared-resident bytes at the first sample.
    pub baseline_shared_bytes: u64,
    /// Highest shared-resident reading across the watch.
    pub peak_shared_bytes: u64,
    /// Whether this process was paged in at least one interval. `None`
    /// when spill was not measurable for this run.
    pub paged: Option<bool>,
}

/// Signed byte delta `current - previous`.
///
/// VRAM byte counts are far below `i64::MAX` (2^63) for any real GPU, so
/// the widening cast cannot lose information.
const fn signed_delta(current: u64, previous: u64) -> i64 {
    // CAST: u64 → i64, VRAM byte counts (< 2^53 in practice) fit
    // trivially; deltas can be negative (VRAM freed), which u64 cannot
    // represent.
    #[allow(clippy::as_conversions, clippy::cast_possible_wrap)]
    let (c, p) = (current as i64, previous as i64);
    c - p
}

/// Human-readable signed VRAM delta: `"+700 MiB"`, `"-1.2 GiB"`,
/// `"+0 B"` for an exact-zero delta (avoids a `"-0 …"` reading for
/// negative deltas that round to zero under [`format_vram`]'s MiB
/// granularity).
fn format_delta(bytes: i64) -> String {
    if bytes == 0 {
        return "+0 B".to_owned();
    }
    let sign = if bytes < 0 { '-' } else { '+' };
    format!("{sign}{}", format_vram(bytes.unsigned_abs()))
}

/// `u8` exit code conveying whether spill was observed during a watch:
/// `0` clean, `1` spill observed at least once. Hard-error paths (bad
/// device or nothing to auto-select, from [`run_watch`]; an invalid
/// argument combination, from [`Selection::new`] via `main`) return `2`
/// directly, bypassing this mapping.
const fn watch_exit_code(spilled: bool) -> u8 {
    if spilled { 1 } else { 0 }
}

/// Select the PIDs to watch when none were given explicitly: sort by
/// [`ps_row_comparator`] under [`SortKey::Dedicated`] — always the
/// dedicated-descending key, sharing `run_ps`'s exact comparator
/// (including its name/PID tie-break chain) so the two orderings can't
/// silently drift — and take the first `n`. Pure — the auto-selection
/// policy is unit-testable without any FFI.
fn select_top_n_pids(rows: &[PsRow], n: usize) -> Vec<u32> {
    let mut sorted: Vec<&PsRow> = rows.iter().collect();
    let cmp = ps_row_comparator(SortKey::Dedicated);
    sorted.sort_by(|a, b| cmp(a, b));
    sorted.into_iter().take(n).map(|r| r.pid).collect()
}

/// Sleep for `total`, checking `interrupted` every
/// [`WATCH_SLEEP_CHUNK`] so a Ctrl+C during a long `--interval` is
/// noticed promptly rather than only after the full sleep elapses.
fn sleep_interruptibly(total: Duration, interrupted: &AtomicBool) {
    let mut remaining = total;
    while remaining > Duration::ZERO && !interrupted.load(Ordering::Relaxed) {
        let step = remaining.min(WATCH_SLEEP_CHUNK);
        std::thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
}

/// Fold one sample into `state`, observe the spill tracker, and return
/// one rendered [`WatchSampleRow`] per watched PID (in `watched`'s
/// order). A watched PID absent from `rows` renders as `0 B` / `0 B` for
/// this interval — `hmn watch` cannot distinguish "exited" from
/// "currently holds no GPU memory" and does not try to (see the `Watch`
/// subcommand's doc comment).
///
/// `watched` need not be the same slice across calls — under
/// `--follow-new` it's recomputed every interval — and a PID re-entering
/// `watched` after an absence resumes its existing [`WatchState`] entry
/// rather than starting fresh: [`WatchState::track`] only seeds a PID
/// once, the first time it's ever seen, by design (an OS process
/// legitimately dipping below rank `--top` for one interval and
/// recovering is not a new process; the name-change reset below is the
/// intentionally narrower signal for genuine OS PID reuse).
///
/// Emits the one-shot `?`-row growth hint to stderr the first interval
/// an unresolved watched PID's cumulative growth crosses
/// [`UNRESOLVED_GROWTH_HINT_BYTES`], worded by [`unresolved_growth_hint`]
/// with this platform's remedy ([`REMEDY_OUTSIDE_SANDBOX`]: `re-run
/// elevated to identify` on Windows and Linux, `re-run outside the
/// sandbox to identify` on macOS) — "unresolved" here means `name` is
/// `None` or (Windows-only, since v0.2.8) the `"[protected]"` bracket;
/// `"[exited]"` does not count, since a process already confirmed gone
/// cannot meaningfully "grow". Also detects a watched PID being recycled
/// by the OS mid-watch (its [`resolved_name`] changes between samples)
/// and resets that PID's baseline/peak so deltas describe the new
/// process rather than mixing two processes' readings — best-effort
/// (unresolved-name churn can't be distinguished from reuse).
fn process_sample(
    rows: &[GpuProcessEntry],
    state: &mut WatchState,
    watched: &[u32],
    elapsed: Duration,
    tracker: Option<&mut SpillTracker>,
) -> Vec<WatchSampleRow> {
    // `None` — not `Some(false)` — when there's no way to tell:
    // `tracker: None` (construction failed) or this instance isn't
    // measurable (`SpillTracker::is_measurable`). Matches `hmn ps`'s
    // `spilling` field contract; a plain `is_some_and` here would
    // collapse "can't tell" into "measured, not spilling", which is
    // exactly the misreading that field's honesty promise exists to
    // prevent.
    let spilling: Option<bool> = tracker.and_then(|t| {
        t.observe(format!("+{:.1}s", elapsed.as_secs_f64()));
        t.is_measurable().then(|| t.is_spilling())
    });

    let mut out = Vec::with_capacity(watched.len());
    for &pid in watched {
        let found = rows.iter().find(|r| r.pid == pid);
        let (name, used_bytes, shared_bytes) = found.map_or((None, 0, 0), |r| {
            (r.name.clone(), r.used_bytes, r.shared_used_bytes)
        });

        let entry = state.track(pid, used_bytes, shared_bytes);

        // The OS can recycle a PID mid-watch: a resolved name that
        // changes between samples is the only signal `hmn watch` has
        // that "pid" now names a different process than the one it
        // baselined against. Treat it as a fresh attach — reset
        // baseline/peak/prev to this sample so the closing summary and
        // this row's delta describe the *new* process, not a mix of
        // both. `None` on either side (still unresolved, or a
        // transient resolution race) is not treated as a change — nor
        // is the Windows-only `"[protected]"`/`"[exited]"` synthetic
        // brackets, which can flicker in and out for one interval (e.g.
        // a transient `Toolhelp32Snapshot` failure) without the
        // underlying process actually changing — see `resolved_name`
        // and the `last_name` update below, both of which stay sticky
        // across an unresolved sample.
        if let (Some(old), Some(new)) = (
            resolved_name(entry.last_name.as_deref()),
            resolved_name(name.as_deref()),
        ) && old != new
        {
            entry.baseline_used_bytes = used_bytes;
            entry.baseline_shared_bytes = shared_bytes;
            entry.prev_used_bytes = used_bytes;
            entry.prev_shared_bytes = shared_bytes;
            entry.peak_used_bytes = used_bytes;
            entry.peak_shared_bytes = shared_bytes;
            entry.growth_hint_fired = false;
            eprintln!(
                "hmn watch: pid={pid} name changed ({old} → {new}) — likely PID reuse by the OS; baseline reset"
            );
        }

        let used_delta = signed_delta(used_bytes, entry.prev_used_bytes);
        let shared_delta = signed_delta(shared_bytes, entry.prev_shared_bytes);
        entry.prev_used_bytes = used_bytes;
        entry.prev_shared_bytes = shared_bytes;
        entry.peak_used_bytes = entry.peak_used_bytes.max(used_bytes);
        entry.peak_shared_bytes = entry.peak_shared_bytes.max(shared_bytes);
        entry.last_name = resolved_name(name.as_deref())
            .map(ToOwned::to_owned)
            .or_else(|| entry.last_name.clone());

        let still_unresolved = name.is_none() || name.as_deref() == Some("[protected]");
        if !entry.growth_hint_fired && still_unresolved {
            let grown = used_bytes
                .saturating_sub(entry.baseline_used_bytes)
                .max(shared_bytes.saturating_sub(entry.baseline_shared_bytes));
            if grown >= UNRESOLVED_GROWTH_HINT_BYTES {
                entry.growth_hint_fired = true;
                eprintln!(
                    "{}",
                    unresolved_growth_hint(pid, grown, REMEDY_OUTSIDE_SANDBOX)
                );
            }
        }

        let paged = paged_verdict(spilling, shared_bytes);
        if paged == Some(true) {
            entry.ever_paged = true;
        }

        out.push(WatchSampleRow {
            pid,
            name,
            used_bytes,
            used_delta,
            shared_bytes,
            shared_delta,
            spilling,
            paged,
        });
    }
    out
}

/// The one-shot stderr hint [`process_sample`] prints when an unresolved
/// watched PID has grown by `grown_bytes` since attach, ending with the
/// [`remedy_text`] for `outside_sandbox` and [`RemedyPurpose::Identify`].
fn unresolved_growth_hint(pid: u32, grown_bytes: u64, outside_sandbox: bool) -> String {
    format!(
        "hmn watch: unresolved pid={pid} grew +{} since attach — {}",
        format_vram(grown_bytes),
        remedy_text(outside_sandbox, RemedyPurpose::Identify)
    )
}

/// Format one interval's rows as a text table (no header — the caller
/// prints the column header once up front). Columns are at least the
/// header's widths ([`watch_table`] with the same `name_width`), so rows
/// line up under the header printed once. Only a cell wider than that —
/// under `--follow-new`, a longer name entering after attach — widens its
/// column, for that interval alone (`--json` is the stable-shape option
/// for scripts).
fn format_watch_rows_text(elapsed: Duration, rows: &[WatchSampleRow], name_width: usize) -> String {
    let mut table = watch_table(name_width);
    for r in rows {
        table.push_row(vec![
            r.pid.to_string(),
            // BORROW: explicit to_owned — the table owns its cells; "?" is
            // the "can't tell" glyph for an unresolved name.
            r.name.as_deref().unwrap_or("?").to_owned(),
            format_vram(r.used_bytes),
            format_delta(r.used_delta),
            format_vram(r.shared_bytes),
            format_delta(r.shared_delta),
            // BORROW: explicit to_owned — the table owns its cells.
            spill_cell(r.spilling, r.paged).to_owned(),
        ]);
    }
    let time_label = format!("+{:.1}s", elapsed.as_secs_f64());
    table.render(None, &format!("{time_label:<WATCH_TIME_WIDTH$}  "))
}

/// The text-mode column set: headers, and the minimum width of each, which
/// the header line always has and each interval's rows keep unless a cell
/// is wider — so rows line up under a header printed once, before the loop.
/// `PID` fits 7 digits (Linux's default `pid_max` is 4 194 304); `NAME`'s
/// 12 is a floor that [`watch_name_width`] raises to the watched names.
const WATCH_COLUMNS: [(&str, usize); 7] = [
    ("PID", 7),
    ("NAME", 12),
    ("COMMITTED", 9),
    ("\u{394}COMMIT", 9),
    ("SHARED", 9),
    ("\u{394}SHARED", 9),
    ("SPILL", 5),
];

/// Width of the `TIME` column, the rows' prefix (`+12.5s`).
const WATCH_TIME_WIDTH: usize = 8;

/// An empty table over [`WATCH_COLUMNS`], its `NAME` column at least
/// `name_width` wide: the one source of the header line and of every
/// interval's rows, so the two cannot drift apart.
#[must_use]
fn watch_table(name_width: usize) -> Table {
    let headers: Vec<&'static str> = WATCH_COLUMNS.iter().map(|&(h, _)| h).collect();
    let widths: Vec<usize> = WATCH_COLUMNS
        .iter()
        .map(|&(h, w)| if h == "NAME" { w.max(name_width) } else { w })
        .collect();
    Table::new(&headers).with_min_widths(&widths)
}

/// The `NAME` column width for this watch: the longest name among the
/// `watched` PIDs in the attach-time listing `rows`, as the table measures
/// widths (bytes), so the header printed once fits every name it will
/// head. The [`WATCH_COLUMNS`] floor applies on top. An unresolved or
/// absent PID contributes the `?` it renders as. Names are never cut: a
/// name is the process's identity, and what `--filter` matches.
#[must_use]
fn watch_name_width(rows: &[GpuProcessEntry], watched: &[u32]) -> usize {
    watched
        .iter()
        .map(|pid| {
            rows.iter()
                .find(|e| e.pid == *pid)
                .and_then(|e| e.name.as_deref())
                .map_or(1, str::len)
        })
        .max()
        .unwrap_or(0)
}

/// Format the watch column header line (text mode), printed once before
/// the loop starts, with its `NAME` column at least `name_width` wide.
fn format_watch_header_text(name_width: usize) -> String {
    watch_table(name_width).render(Some(&format!("{:<WATCH_TIME_WIDTH$}  ", "TIME")), "")
}

/// Format one interval's rows as JSON Lines: one `"kind":"sample"`
/// object per row, newline-terminated, ready to pipe to `jq -c`.
/// `wall_clock` (captured at the same instant as `t_ms`'s `elapsed`,
/// just before this interval's `gpu_process_listing()` call — not after,
/// so the two timestamps in one sample never straddle the query's own
/// duration) is the same for every row in the interval — formatted
/// once via [`iso8601_utc_millis`], not per row.
fn format_watch_rows_json(
    elapsed: Duration,
    wall_clock: SystemTime,
    rows: &[WatchSampleRow],
) -> String {
    let mut out = String::new();
    let wall_clock_json = iso8601_utc_millis(wall_clock);
    for row in rows {
        let name_json = json_string_or_null(row.name.as_deref());
        let spilling_json = json_value_or_null(row.spilling);
        let paged_json = json_value_or_null(row.paged);
        let _ = writeln!(
            out,
            r#"{{"kind":"sample","t_ms":{},"wall_clock":"{wall_clock_json}","pid":{},"name":{name_json},"used_bytes":{},"used_delta_bytes":{},"shared_used_bytes":{},"shared_delta_bytes":{},"spilling":{spilling_json},"paged":{paged_json}}}"#,
            duration_ms(elapsed),
            row.pid,
            row.used_bytes,
            row.used_delta,
            row.shared_bytes,
            row.shared_delta,
        );
    }
    out
}

/// Format the end-of-watch per-PID peak/baseline block (text mode).
/// Empty `per_pid` renders as an empty string (nothing to show). Data
/// rows are indented to sit under the header's columns, past its
/// `hmn watch: per-PID` lead-in.
fn format_watch_per_pid_block(per_pid: &[WatchPidSummary]) -> String {
    if per_pid.is_empty() {
        return String::new();
    }
    let mut table = Table::new(&[
        "PID",
        "NAME",
        "BASELINE COMMIT",
        "PEAK COMMIT",
        "BASELINE SHARED",
        "PEAK SHARED",
        "PAGED",
    ]);
    for p in per_pid {
        table.push_row(vec![
            p.pid.to_string(),
            // BORROW: explicit to_owned — the table owns its cells; "?" is
            // the "can't tell" glyph for an unresolved name.
            p.name.as_deref().unwrap_or("?").to_owned(),
            format_vram(p.baseline_used_bytes),
            format_vram(p.peak_used_bytes),
            format_vram(p.baseline_shared_bytes),
            format_vram(p.peak_shared_bytes),
            // BORROW: explicit to_owned — the table owns its cells;
            // `paged_cell` renders `n/a` or `?` when spill was not
            // measurable, through the same core as the SPILL column.
            paged_cell(p.paged).to_owned(),
        ]);
    }
    let header_prefix = "hmn watch: per-PID  ";
    table.render(Some(header_prefix), &" ".repeat(header_prefix.len()))
}

/// Format the closing summary in text mode: the adapter-level report
/// (via [`format_spill_report_with_prefix`] under the `hmn watch`
/// prefix) when spill was measurable, else a one-line notice — followed
/// either way by the per-PID block.
///
/// Two notices, because there are two ways not to measure: no tracker
/// at all (construction failed), or a tracker that exists but cannot
/// measure (`SpillReport::measurable` false — every Linux and macOS
/// run, and Windows without a usable adapter counter set). The second
/// must not reach [`format_spill_report_with_prefix`]: its all-zeros
/// report would print `peak dedicated 0 MiB` and `no spill observed`,
/// a measured-looking negative. It says what `hmn spill` says instead.
///
/// When the device was already spilling at attach (`spilling_at_attach`),
/// a line after the report says the baseline includes that spill, so its
/// episode count covers only growth beyond it.
fn format_watch_summary_text(
    report: Option<&SpillReport>,
    spilling_at_attach: Option<bool>,
    per_pid: &[WatchPidSummary],
) -> String {
    let mut out = match report {
        Some(r) if r.measurable => {
            let mut block = format_spill_report_with_prefix("hmn watch", r);
            if spilling_at_attach == Some(true) {
                block.push_str(
                    "hmn watch: the device was already spilling at attach; the baseline above \
                     includes that spill, so episodes count only growth beyond it\n",
                );
            }
            block
        }
        // BORROW: explicit to_owned — the summary is built as an owned String.
        Some(_) => {
            "hmn watch: spill not measurable on this platform; per-PID VRAM below\n".to_owned()
        }
        // BORROW: explicit to_owned — the summary is built as an owned String.
        None => {
            "hmn watch: spill tracking unavailable for this run; per-PID VRAM below\n".to_owned()
        }
    };
    out.push_str(&format_watch_per_pid_block(per_pid));
    out
}

/// Format the closing summary as one JSON object:
/// `{"kind":"summary",...adapter SpillReport fields...,"spilling_at_attach":<true|false|null>,"per_pid":[...]}`.
/// `spilling_at_attach` is `hmn ps`'s one-snapshot verdict taken at
/// attach (`null` when not measurable): `true` means the baseline
/// includes a spill already under way, which the episodes do not count.
/// `report: None` (spill tracking unavailable) emits the same
/// all-zeros `"measurable":false` shape `hmn spill --json` uses — both
/// go through [`write_spill_report_fields`] — so scripted consumers
/// always parse one shape either way.
pub fn format_watch_summary_json(
    report: Option<&SpillReport>,
    spilling_at_attach: Option<bool>,
    per_pid: &[WatchPidSummary],
) -> String {
    let mut out = String::from(r#"{"kind":"summary","#);
    write_spill_report_fields(&mut out, report);
    let _ = write!(
        out,
        r#","spilling_at_attach":{},"per_pid":["#,
        json_value_or_null(spilling_at_attach)
    );
    for (i, p) in per_pid.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let name_json = json_string_or_null(p.name.as_deref());
        let _ = write!(
            out,
            r#"{{"pid":{},"name":{name_json},"baseline_used_bytes":{},"peak_used_bytes":{},"baseline_shared_bytes":{},"peak_shared_bytes":{},"paged":{}}}"#,
            p.pid,
            p.baseline_used_bytes,
            p.peak_used_bytes,
            p.baseline_shared_bytes,
            p.peak_shared_bytes,
            json_value_or_null(p.paged),
        );
    }
    out.push_str("]}\n");
    out
}

/// How `hmn watch` chooses the PIDs it follows.
///
/// The one value that the selection itself ([`Self::select`]), the
/// stderr header's description of it ([`Self::describe`]), the
/// nothing-to-select messages ([`Self::criterion`]) and the `--json`
/// `start` record's `selection` object ([`Self::write_json`]) are all
/// derived from, so what a capture *says* it selected and what it
/// actually selected cannot drift apart. Built by [`Self::new`], which rejects
/// argument combinations that only make sense for auto-selection before
/// any hardware is touched.
pub struct Selection {
    /// Explicit PIDs from the command line, deduplicated, in the order
    /// given. Non-empty means explicit mode: exactly these are watched.
    explicit: Vec<u32>,
    /// How many processes auto-selection keeps (`--top`). Unused in
    /// explicit mode.
    top: usize,
    /// Whether auto-selection re-runs every interval (`--follow-new`)
    /// rather than once at attach.
    follow_new: bool,
    /// `--filter` patterns, as typed. Empty means no name filter; a name
    /// qualifies when it contains any one of them, ignoring case.
    filters: Vec<String>,
    /// `--min`: the smallest total footprint, in bytes, a process needs to
    /// be considered — measured by `ps::footprint_bytes`, exactly as
    /// `hmn ps --min` does. `None` means no threshold.
    min_bytes: Option<u64>,
}

/// The outcome of one [`Selection::select`] call.
struct Selected {
    /// The PIDs to watch, in selection order.
    pids: Vec<u32>,
    /// PIDs `--filter` could not judge: no resolvable name in this sample
    /// and none remembered from an earlier one. Always empty without
    /// `--filter`.
    unmatchable: Vec<u32>,
}

impl Selection {
    /// Build the selection from `hmn watch`'s parsed arguments.
    ///
    /// # Errors
    ///
    /// Returns the user-facing message (without the `hmn: ` prefix) when
    /// `--follow-new`, `--filter` or `--min` is combined with explicit
    /// PIDs: each shapes auto-selection, and there is no top-N to shape
    /// for a fixed list.
    pub fn new(
        pids: &[u32],
        top: usize,
        follow_new: bool,
        filters: &[String],
        min_bytes: Option<u64>,
    ) -> Result<Self, String> {
        let mut seen = HashSet::new();
        let explicit: Vec<u32> = pids.iter().copied().filter(|p| seen.insert(*p)).collect();
        if !explicit.is_empty() {
            if follow_new {
                return Err(
                    "watch --follow-new only applies to auto-selection; drop --follow-new \
                     or the explicit PID list"
                        .to_owned(),
                );
            }
            if !filters.is_empty() {
                return Err(
                    "watch --filter only applies to auto-selection; drop --filter or the \
                     explicit PID list"
                        .to_owned(),
                );
            }
            if min_bytes.is_some() {
                return Err(
                    "watch --min only applies to auto-selection; drop --min or the explicit \
                     PID list"
                        .to_owned(),
                );
            }
        }
        Ok(Self {
            explicit,
            top,
            follow_new,
            filters: filters.to_vec(),
            min_bytes,
        })
    }

    /// Resolve which PIDs to watch from one sample: the explicit PIDs
    /// unchanged in explicit mode (always watched exactly as given);
    /// otherwise the processes passing `--min`, then `--filter` (all of
    /// them without either), then the top `top` of those by committed
    /// `VRAM` — sharing `hmn ps`'s own comparator via
    /// [`select_top_n_pids`], so the two orderings cannot drift apart.
    ///
    /// Under `--filter`, a row is judged by [`matchable_name`]: its
    /// current resolved name, else the name `state` last resolved for that
    /// PID. A row with neither is reported in [`Selected::unmatchable`]
    /// instead of being dropped silently.
    ///
    /// Called once before the watch loop always, and again every interval
    /// under `--follow-new` (cheap: `rows` numbers in the tens, and
    /// `--interval` is 5s+ apart by default).
    #[must_use]
    fn select(&self, rows: &[GpuProcessEntry], state: &WatchState) -> Selected {
        if !self.explicit.is_empty() {
            return Selected {
                pids: self.explicit.clone(),
                unmatchable: Vec::new(),
            };
        }
        let mut unmatchable = Vec::new();
        // device_index / device_name / spilling / paged / shared_share are
        // unused by
        // SortKey::Dedicated's comparator (pid / used_bytes / name only) —
        // defaulted rather than threaded through from the caller, which has
        // no device-name or live-spill context of its own to give.
        let ps_rows: Vec<PsRow> = rows
            .iter()
            .filter(|e| {
                self.min_bytes
                    .is_none_or(|min| footprint_bytes(e.used_bytes, e.shared_used_bytes) >= min)
            })
            .filter(|e| {
                if self.filters.is_empty() {
                    return true;
                }
                let sticky = state
                    .by_pid
                    .get(&e.pid)
                    .and_then(|s| s.last_name.as_deref());
                matchable_name(e.name.as_deref(), sticky).map_or_else(
                    || {
                        unmatchable.push(e.pid);
                        false
                    },
                    |name| matches_any(name, &self.filters),
                )
            })
            .map(|e| PsRow {
                pid: e.pid,
                // BORROW: clone — e is borrowed from `rows`.
                name: e.name.clone(),
                used_bytes: e.used_bytes,
                shared_used_bytes: e.shared_used_bytes,
                device_index: 0,
                device_name: None,
                spilling: None,
                paged: None,
                shared_share: None,
            })
            .collect();
        Selected {
            pids: select_top_n_pids(&ps_rows, self.top),
            unmatchable,
        }
    }

    /// The stderr header's mode clause, given how many PIDs were
    /// selected at attach — e.g. `following top 3 by committed among
    /// names containing "train" (case-insensitive) (re-selected every
    /// interval), 1 initially`. Without `--filter` or `--min`,
    /// byte-identical to the clauses `hmn watch` printed before either
    /// existed.
    #[must_use]
    fn describe(&self, initially: usize) -> String {
        let top = self.top;
        let criterion = self.criterion();
        if self.follow_new {
            format!(
                "following top {top} by committed{criterion} (re-selected every interval), \
                 {initially} initially"
            )
        } else if self.explicit.is_empty() {
            format!("watching {initially} PID(s) (top {top} by committed{criterion})")
        } else {
            format!("watching {initially} PID(s)")
        }
    }

    /// The auto-selection criterion beyond "top N by committed", as a
    /// clause to splice after it (leading space included), or the empty
    /// string when there is none — e.g. ` among names containing "a" or
    /// "b" (case-insensitive) with footprint >= 2 GiB`. Patterns are
    /// `Debug`-quoted, so one containing a quote or a space reads
    /// unambiguously; the threshold is rendered by [`format_vram_precise`],
    /// as `hmn ps`'s summary line renders its own `--min`.
    #[must_use]
    fn criterion(&self) -> String {
        let mut out = String::new();
        if !self.filters.is_empty() {
            let patterns: Vec<String> = self.filters.iter().map(|f| format!("{f:?}")).collect();
            let _ = write!(
                out,
                " among names containing {} (case-insensitive)",
                patterns.join(" or ")
            );
        }
        if let Some(min) = self.min_bytes {
            let _ = write!(out, " with footprint >= {}", format_vram_precise(min));
        }
        out
    }

    /// Write the `start` record's `"selection"` object into `out`: `mode`
    /// (`"explicit"`, `"top"` or `"follow_new"`), the explicit `pids`
    /// (empty in the auto modes), `top` (`null` in explicit mode, where it
    /// is ignored), the `--filter` patterns as typed, and `min_bytes`
    /// (`null` without `--min`) — the machine-readable counterpart of the
    /// mode and criterion [`Self::describe`] puts into words on the stderr
    /// header.
    fn write_json(&self, out: &mut String) {
        let mode = match (self.explicit.is_empty(), self.follow_new) {
            (false, _) => "explicit",
            (true, true) => "follow_new",
            (true, false) => "top",
        };
        let pids: Vec<String> = self.explicit.iter().map(u32::to_string).collect();
        let top = json_value_or_null(self.explicit.is_empty().then_some(self.top));
        let filters: Vec<String> = self.filters.iter().map(|f| json_string(f)).collect();
        let _ = write!(
            out,
            r#"{{"mode":"{mode}","pids":[{}],"top":{top},"filters":[{}],"min_bytes":{}}}"#,
            pids.join(","),
            filters.join(","),
            json_value_or_null(self.min_bytes),
        );
    }
}

/// The invocation as the `start` record stores it: every argument as
/// typed, except the program path, reduced to its file name (`hmn.exe`,
/// not `C:\Users\<name>\…\hmn.exe`). `--json` captures are routinely
/// committed to public repositories, so the record must not publish the
/// operator's user name or directory layout. A non-UTF-8 argument is kept,
/// lossily, rather than dropped.
#[must_use]
fn recorded_argv(args: impl IntoIterator<Item = std::ffi::OsString>) -> Vec<String> {
    args.into_iter()
        .enumerate()
        .map(|(i, a)| {
            let arg = if i == 0 {
                std::path::Path::new(&a)
                    .file_name()
                    .map_or_else(|| a.clone(), std::ffi::OsStr::to_os_string)
            } else {
                a
            };
            // BORROW: explicit to_string_lossy + into_owned — the record
            // stores text; a non-UTF-8 argument is kept, lossily.
            arg.to_string_lossy().into_owned()
        })
        .collect()
}

/// Format the `--json` stream's first record, written once at attach
/// before the first sample: which `hmn` build ran, the invocation, the
/// device, the timing, and how PIDs were chosen ([`Selection::write_json`]).
///
/// It makes a capture self-describing and its truncation detectable from
/// the file alone: a `start` record with no closing `summary` means the
/// run was cut short (hard-killed, or the file copied mid-run), which the
/// stream could not show before v0.2.12. `t_ms` is `0` and `wall_clock` is
/// the first sample's own instant, so the two line up.
#[must_use]
fn format_watch_start_json(
    wall_clock: SystemTime,
    argv: &[String],
    device: u32,
    device_name: Option<&str>,
    interval: Duration,
    duration: Option<Duration>,
    selection: &Selection,
) -> String {
    let argv_json: Vec<String> = argv.iter().map(|a| json_string(a)).collect();
    let mut out = String::new();
    let _ = write!(
        out,
        r#"{{"kind":"start","t_ms":0,"wall_clock":"{}","hmn_version":"{}","argv":[{}],"device":{device},"device_name":{},"interval_ms":{},"duration_ms":{},"selection":"#,
        iso8601_utc_millis(wall_clock),
        env!("CARGO_PKG_VERSION"),
        argv_json.join(","),
        json_string_or_null(device_name),
        duration_ms(interval),
        json_value_or_null(duration.map(duration_ms)),
    );
    selection.write_json(&mut out);
    out.push_str("}\n");
    out
}

/// The name `--filter` judges a row by: its current name if that is a
/// genuinely resolved one ([`filterable_name`], the rule `hmn ps --filter`
/// uses too), else `sticky` — the name this PID last resolved to in an
/// earlier sample, so a followed process whose name flickers to
/// `[protected]` for one interval is not evicted by it. `None` when
/// neither is available.
///
/// Best-effort, like `hmn watch`'s PID-reuse handling generally: if a
/// followed process exits and the OS reuses its PID for a process whose
/// name cannot be resolved, the sticky name carries over and the new
/// process can match. A reuse by a process whose name *does* resolve is
/// judged by that name, as it should be.
#[must_use]
fn matchable_name<'a>(current: Option<&'a str>, sticky: Option<&'a str>) -> Option<&'a str> {
    filterable_name(current).or_else(|| filterable_name(sticky))
}

/// The attach-time stderr warning for a device that is already spilling
/// when `hmn watch` attaches (`spilling_at_attach`, `hmn ps`'s one-snapshot
/// verdict), or `None`. `hmn watch` measures spill as shared-memory growth
/// above its first observation, so a spill already under way at attach is
/// taken into that baseline and not counted: saying so up front keeps a
/// later `no spill observed` from reading as a measured negative. The
/// shared figure is summed over the device's processes in `rows`, as
/// `hmn ps` sums it.
#[must_use]
fn spilling_at_attach_notice(
    device: u32,
    spilling_at_attach: Option<bool>,
    rows: &[GpuProcessEntry],
) -> Option<String> {
    (spilling_at_attach == Some(true)).then(|| {
        let shared = rows
            .iter()
            .fold(0_u64, |sum, e| sum.saturating_add(e.shared_used_bytes));
        format!(
            "hmn watch: device {device} is already spilling at attach ({} shared); spill is \
             measured as growth from here, so this spill will not be counted — `hmn ps` shows it",
            format_vram(shared)
        )
    })
}

/// The attach-time stderr warnings for explicit PIDs that name no running
/// process: one per PID in `explicit` that `listed` (the first sample)
/// does not hold, that is not in `denied` (the PIDs the caller was refused,
/// which [`denied_pid_notices`] names instead, so one PID never gets both
/// notices) and that `exists` answers `Some(false)` for. A PID in the
/// listing plainly exists and is not asked about; `None` ("can't tell")
/// says nothing. Such a PID is still watched, as before, so a typo is
/// told apart from a process that merely holds no GPU memory yet — the
/// one distinction `hmn watch` can make at attach; mid-watch it still
/// cannot tell "exited" from "holds no GPU memory". `exists` is
/// `hypomnesis::process_exists` in `run_watch`, a stub in tests.
#[must_use]
fn missing_pid_notices(
    explicit: &[u32],
    listed: &[GpuProcessEntry],
    denied: &[u32],
    exists: impl Fn(u32) -> Option<bool>,
) -> Vec<String> {
    explicit
        .iter()
        .filter(|&&pid| !listed.iter().any(|e| e.pid == pid))
        .filter(|pid| !denied.contains(pid))
        .filter(|&&pid| exists(pid) == Some(false))
        .map(|pid| {
            format!("hmn watch: pid={pid} names no running process; its rows will read 0 MiB")
        })
        .collect()
}

/// The attach-time stderr notices for explicit PIDs the caller was refused
/// (`denied`, [`hypomnesis::GpuProcessListing::denied_pids`]): one per PID
/// in `explicit` that `denied` holds, in the order given. A denied PID is
/// still watched, but its rows read 0 MiB each interval, and the notice
/// says so once, with the remedy, so it stands alone. A readable PID gets
/// none.
#[must_use]
fn denied_pid_notices(explicit: &[u32], denied: &[u32], outside_sandbox: bool) -> Vec<String> {
    explicit
        .iter()
        .filter(|pid| denied.contains(pid))
        .map(|pid| {
            with_remedy(
                &format!("hmn watch: pid={pid} is unreadable here; its rows will read 0 MiB"),
                outside_sandbox,
            )
        })
        .collect()
}

/// The one-shot stderr notices for PIDs in `unmatchable` that this watch
/// has not announced yet, recording them in `announced` — so a process
/// `--filter` cannot judge is named exactly once rather than every
/// interval, and never passes silently.
#[must_use = "the PIDs are now marked announced; dropping the notices loses them for good"]
fn unmatchable_notices(unmatchable: &[u32], announced: &mut HashSet<u32>) -> Vec<String> {
    unmatchable
        .iter()
        .filter(|pid| announced.insert(**pid))
        .map(|pid| format!("hmn watch: pid={pid} has no resolvable name; --filter cannot match it"))
        .collect()
}

/// Build the stderr breadcrumb naming PIDs that entered or left the
/// followed set between two consecutive `--follow-new` intervals, or
/// `None` when the set didn't change. Entered-PID names come from the
/// current sample's `rows`; left-PID names come from `state`'s last
/// known name for that PID (it is already absent from `rows`, by
/// definition of having left). Purely cosmetic: doesn't affect the
/// JSONL stream shape or the closing summary.
///
/// `prev_watched` must be captured *before* the caller reassigns its
/// `watched` variable to the freshly [`Selection::select`]-computed
/// set — the two arguments have to actually differ for the diff to mean
/// anything. Call order relative to [`process_sample`] does not matter
/// on its own: `process_sample` only touches a PID present in the
/// `watched` slice it is given, so a departed PID's [`WatchState`] entry
/// is untouched that interval regardless of when this function runs
/// relative to it.
fn format_followed_set_change(
    prev_watched: &[u32],
    new_watched: &[u32],
    rows: &[GpuProcessEntry],
    state: &WatchState,
    elapsed: Duration,
) -> Option<String> {
    let entered: Vec<u32> = new_watched
        .iter()
        .copied()
        .filter(|p| !prev_watched.contains(p))
        .collect();
    let left: Vec<u32> = prev_watched
        .iter()
        .copied()
        .filter(|p| !new_watched.contains(p))
        .collect();
    if entered.is_empty() && left.is_empty() {
        return None;
    }

    let name_for = |pid: u32| -> String {
        let name = rows
            .iter()
            .find(|r| r.pid == pid)
            .and_then(|r| r.name.clone())
            .or_else(|| state.by_pid.get(&pid).and_then(|s| s.last_name.clone()));
        name.map_or_else(|| format!("pid={pid}"), |n| format!("pid={pid} ({n})"))
    };

    let mut clauses = Vec::new();
    if !entered.is_empty() {
        let list = entered
            .into_iter()
            .map(name_for)
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("entered {list}"));
    }
    if !left.is_empty() {
        let list = left
            .into_iter()
            .map(name_for)
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("left {list}"));
    }
    Some(format!(
        "hmn watch: +{:.1}s followed set changed: {}",
        elapsed.as_secs_f64(),
        clauses.join("; ")
    ))
}

/// Run the `watch` subcommand: resolve the watched PID set, sample it on
/// a timer against [`SpillTracker`] + [`gpu_process_listing`] until
/// `--duration` elapses or Ctrl+C, then print the closing summary.
///
/// Returns `2` immediately on a hard error (device unreachable, or
/// nothing to auto-select without `--follow-new`); otherwise runs to
/// completion and returns [`watch_exit_code`] of whether spill was ever
/// observed. Invalid argument combinations never reach this function:
/// [`Selection::new`] rejects them in `main`'s dispatch, before any
/// backend call.
pub fn run_watch(
    selection: &Selection,
    interval: Duration,
    duration: Option<Duration>,
    device: u32,
    json: bool,
) -> std::process::ExitCode {
    let device_name = device_info(device).ok().and_then(|d| d.name);

    // `start` (the origin every later t_ms is measured from) and
    // first_wall_clock are captured together, right before the first
    // query — not after the Selection::select/SpillTracker::new/
    // ctrlc::set_handler setup below, which on Windows includes a real
    // DXGI walk plus GPU Adapter Memory PDH enumeration and can take
    // long enough to be visible. Capturing `start` later while
    // first_wall_clock stayed early would make the very first sample's
    // wall_clock predate t_ms's own zero point — the first row
    // reconstructed as `first_wall_clock + t_ms` would land earlier
    // than it actually happened, and every later row's gap from it
    // would read larger than `--interval`.
    let start = std::time::Instant::now();
    let first_wall_clock = SystemTime::now();
    let first_listing = match gpu_process_listing(device) {
        Ok(listing) => listing,
        Err(e) => {
            eprintln!(
                "hmn: watch failed to query device {device}: {}",
                failure_detail(&e, REMEDY_OUTSIDE_SANDBOX)
            );
            return std::process::ExitCode::from(2);
        }
    };
    // The processes the caller was refused. A sandbox does not change
    // mid-run, so this set is read once, at attach, and every notice about
    // it is said once; the interval loop lists `entries` only.
    let denied = first_listing.denied_pids;
    let first_rows = first_listing.entries;
    let unreadable = remedy_clause(0, denied.len(), REMEDY_OUTSIDE_SANDBOX);
    // `hmn ps`'s one-snapshot verdict, taken once at attach: the tracker
    // measures growth from its first observation, so it cannot see a spill
    // already under way; this can, and the watch says so.
    let spilling_at_attach = snapshot_is_spilling(device);

    let mut state = WatchState::new();
    // PIDs already named as unmatchable by `--filter`, so each is announced once.
    let mut announced = HashSet::new();
    let first = selection.select(&first_rows, &state);
    let mut watched = first.pids;
    // Sized once, from the processes watched at attach, so the header and
    // every interval's rows share one NAME width.
    let name_width = watch_name_width(&first_rows, &watched);
    if watched.is_empty() {
        let top = selection.top;
        let criterion = selection.criterion();
        // Why, when processes were refused: the count and the remedy go
        // inside the parentheses, so the line says it on its own.
        let because = unreadable
            .as_deref()
            .map_or_else(String::new, |clause| format!("; {clause}"));
        if selection.follow_new {
            eprintln!(
                "hmn: watch found no GPU processes on device {device} yet (top {top} by \
                 committed{criterion}{because}); waiting for work to appear"
            );
        } else {
            eprintln!(
                "hmn: watch found no GPU processes on device {device} to auto-select \
                 (top {top}{criterion}{because}); re-run with an explicit PID once a workload is running"
            );
            return std::process::ExitCode::from(2);
        }
    }

    let mut tracker = match SpillTracker::new(device) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("hmn: watch spill tracking unavailable ({e}); showing per-PID VRAM only");
            None
        }
    };

    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = Arc::clone(&interrupted);
        if let Err(e) = ctrlc::set_handler(move || interrupted.store(true, Ordering::SeqCst)) {
            eprintln!(
                "hmn: watch failed to install Ctrl+C handler ({e}); interrupting will skip the closing summary"
            );
        }
    }

    eprintln!(
        "hmn watch: device {device}{}, interval {:.1}s, {}",
        device_name_suffix(device_name.as_deref()),
        interval.as_secs_f64(),
        selection.describe(watched.len()),
    );
    if json {
        let argv = recorded_argv(std::env::args_os());
        print!(
            "{}",
            format_watch_start_json(
                first_wall_clock,
                &argv,
                device,
                device_name.as_deref(),
                interval,
                duration,
                selection,
            )
        );
    } else {
        print!("{}", format_watch_header_text(name_width));
    }
    if let Some(notice) = spilling_at_attach_notice(device, spilling_at_attach, &first_rows) {
        eprintln!("{notice}");
    }
    for notice in denied_pid_notices(&selection.explicit, &denied, REMEDY_OUTSIDE_SANDBOX) {
        eprintln!("{notice}");
    }
    for notice in missing_pid_notices(&selection.explicit, &first_rows, &denied, process_exists) {
        eprintln!("{notice}");
    }
    for notice in unmatchable_notices(&first.unmatchable, &mut announced) {
        eprintln!("{notice}");
    }
    // `--follow-new` never follows a process it cannot read, and says how
    // many. With nothing to watch, the "found no GPU processes" line
    // already carries the count.
    if selection.follow_new
        && !watched.is_empty()
        && let Some(clause) = &unreadable
    {
        eprintln!("hmn watch: device {device}: {clause}; they are not followed");
    }

    let rows0 = process_sample(
        &first_rows,
        &mut state,
        &watched,
        Duration::ZERO,
        tracker.as_mut(),
    );
    if json {
        print!(
            "{}",
            format_watch_rows_json(Duration::ZERO, first_wall_clock, &rows0)
        );
    } else {
        print!(
            "{}",
            format_watch_rows_text(Duration::ZERO, &rows0, name_width)
        );
    }

    'watch: loop {
        if duration.is_some_and(|d| start.elapsed() >= d) || interrupted.load(Ordering::Relaxed) {
            break 'watch;
        }
        sleep_interruptibly(interval, &interrupted);
        if interrupted.load(Ordering::Relaxed) {
            break 'watch;
        }
        if duration.is_some_and(|d| start.elapsed() >= d) {
            break 'watch;
        }

        // Captured together, both right before the query, so t_ms and
        // wall_clock in the emitted sample refer to the same instant
        // rather than straddling gpu_process_listing()'s (non-zero, under
        // load) call duration.
        let elapsed = start.elapsed();
        let wall_clock = SystemTime::now();
        let rows = match gpu_process_listing(device) {
            Ok(listing) => listing.entries,
            Err(e) => {
                // Raw `{e}`, not `failure_detail`: attach said the remedy
                // once, and the sandbox does not change mid-run.
                eprintln!(
                    "hmn watch: sample failed at +{:.1}s ({e}); skipping interval",
                    elapsed.as_secs_f64()
                );
                continue 'watch;
            }
        };

        if selection.follow_new {
            let next = selection.select(&rows, &state);
            for notice in unmatchable_notices(&next.unmatchable, &mut announced) {
                eprintln!("{notice}");
            }
            if let Some(msg) =
                format_followed_set_change(&watched, &next.pids, &rows, &state, elapsed)
            {
                eprintln!("{msg}");
            }
            watched = next.pids;
        }

        let sample = process_sample(&rows, &mut state, &watched, elapsed, tracker.as_mut());
        if json {
            print!("{}", format_watch_rows_json(elapsed, wall_clock, &sample));
        } else {
            print!("{}", format_watch_rows_text(elapsed, &sample, name_width));
        }
    }

    let report = tracker.map(SpillTracker::into_report);
    // A per-PID paged verdict only where spill was measurable at all.
    let measurable = report.as_ref().is_some_and(|r| r.measurable);
    let per_pid: Vec<WatchPidSummary> = state
        .seen_order
        .iter()
        .map(|&pid| {
            let s = state.by_pid.get(&pid);
            WatchPidSummary {
                pid,
                name: s.and_then(|s| s.last_name.clone()),
                baseline_used_bytes: s.map_or(0, |s| s.baseline_used_bytes),
                peak_used_bytes: s.map_or(0, |s| s.peak_used_bytes),
                baseline_shared_bytes: s.map_or(0, |s| s.baseline_shared_bytes),
                peak_shared_bytes: s.map_or(0, |s| s.peak_shared_bytes),
                paged: measurable.then(|| s.is_some_and(|s| s.ever_paged)),
            }
        })
        .collect();

    if json {
        print!(
            "{}",
            format_watch_summary_json(report.as_ref(), spilling_at_attach, &per_pid)
        );
    } else {
        print!(
            "{}",
            format_watch_summary_text(report.as_ref(), spilling_at_attach, &per_pid)
        );
    }

    std::process::ExitCode::from(watch_exit_code(
        report.as_ref().is_some_and(SpillReport::spilled),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[cfg(feature = "test-helpers")]
    use crate::test_support::{entry, spilling_report};
    use crate::test_support::{pid_summary, row};

    // --- format_delta ---

    #[test]
    fn format_delta_zero_is_plus_zero_bytes() {
        assert_eq!(format_delta(0), "+0 B");
    }

    #[test]
    fn format_delta_positive_mib() {
        assert_eq!(format_delta(700 * 1024 * 1024), "+700 MiB");
    }

    #[test]
    fn format_delta_negative_gib() {
        let one_point_two_gib = -(1024_i64 * 1024 * 1024 + 1024 * 1024 * 1024 / 5);
        assert_eq!(format_delta(one_point_two_gib), "-1.2 GiB");
    }

    // --- signed_delta ---

    #[test]
    fn signed_delta_basic() {
        assert_eq!(signed_delta(100, 40), 60);
        assert_eq!(signed_delta(40, 100), -60);
        assert_eq!(signed_delta(0, 0), 0);
    }

    // --- watch_exit_code ---

    #[test]
    fn watch_exit_code_clean_and_spilled() {
        assert_eq!(watch_exit_code(false), 0);
        assert_eq!(watch_exit_code(true), 1);
    }

    // --- select_top_n_pids ---

    #[test]
    fn select_top_n_pids_orders_by_committed_descending() {
        let rows = vec![
            row(1, Some("a.exe"), 1_000, 0, None),
            row(2, Some("b.exe"), 5_000, 0, None),
            row(3, Some("c.exe"), 3_000, 0, None),
        ];
        assert_eq!(select_top_n_pids(&rows, 2), vec![2, 3]);
    }

    #[test]
    fn select_top_n_pids_n_larger_than_rows_returns_all() {
        let rows = vec![row(1, Some("a.exe"), 1_000, 0, None)];
        assert_eq!(select_top_n_pids(&rows, 5), vec![1]);
    }

    #[test]
    fn select_top_n_pids_empty_rows() {
        let top = select_top_n_pids(&[], 5);
        assert!(top.is_empty(), "{top:?}");
    }

    #[test]
    fn select_top_n_pids_ties_break_by_pid_ascending() {
        let rows = vec![
            row(20, Some("b.exe"), 1_000, 0, None),
            row(10, Some("a.exe"), 1_000, 0, None),
        ];
        assert_eq!(select_top_n_pids(&rows, 2), vec![10, 20]);
    }

    #[test]
    fn select_top_n_pids_ties_break_by_name_before_pid() {
        // Shares ps_row_comparator with `hmn ps`: a tie on used_bytes
        // breaks by name first, PID only as the final fallback. Name
        // and PID order deliberately *disagree* here (the lower PID, 1,
        // carries the alphabetically-later name) so this fixture can
        // actually distinguish "name-then-PID" from the old "PID-only"
        // rule: a PID-only tie-break would produce `[1, 99]`; the real
        // (name-first) comparator produces `[99, 1]`.
        let rows = vec![
            row(1, Some("z.exe"), 1_000, 0, None),
            row(99, Some("a.exe"), 1_000, 0, None),
        ];
        assert_eq!(select_top_n_pids(&rows, 2), vec![99, 1]);
    }

    // --- format_watch_rows_text / format_watch_header_text ---

    // The SPILL or PAGED cell of a row whose spill was not read: `?` on
    // Windows, where spill exists; `n/a` elsewhere, where it cannot.
    #[cfg(windows)]
    const UNKNOWN_SPILL: &str = "?";
    #[cfg(not(windows))]
    const UNKNOWN_SPILL: &str = "n/a";

    fn watch_row(
        pid: u32,
        name: Option<&str>,
        used: u64,
        used_delta: i64,
        shared: u64,
        shared_delta: i64,
        spilling: bool,
    ) -> WatchSampleRow {
        watch_row_opt(
            pid,
            name,
            used,
            used_delta,
            shared,
            shared_delta,
            Some(spilling),
        )
    }

    /// Like [`watch_row`] but with an explicit `Option<bool>` — for the
    /// "spill not measurable here" (`None`) cases `watch_row`'s plain
    /// `bool` can't express.
    fn watch_row_opt(
        pid: u32,
        name: Option<&str>,
        used: u64,
        used_delta: i64,
        shared: u64,
        shared_delta: i64,
        spilling: Option<bool>,
    ) -> WatchSampleRow {
        WatchSampleRow {
            pid,
            name: name.map(str::to_owned),
            used_bytes: used,
            used_delta,
            shared_bytes: shared,
            shared_delta,
            spilling,
            paged: paged_verdict(spilling, shared),
        }
    }

    #[test]
    fn format_watch_header_text_has_expected_columns() {
        let h = format_watch_header_text(0);
        assert!(h.contains("TIME"));
        assert!(h.contains("PID"));
        assert!(h.contains("NAME"));
        assert!(h.contains("COMMITTED"));
        assert!(h.contains("SHARED"));
        assert!(h.contains("SPILL"));
    }

    #[test]
    fn format_watch_header_text_widths() {
        // v0.2.6-v0.2.12's header with `PID` one wider, for 7-digit PIDs.
        assert_eq!(
            format_watch_header_text(0),
            "TIME      PID      NAME          COMMITTED  \u{394}COMMIT    SHARED     \u{394}SHARED    SPILL\n"
        );
        // `NAME` grows to a longer watched name, never shrinks below 12.
        assert!(
            format_watch_header_text(18)
                .starts_with("TIME      PID      NAME                COMMITTED  ")
        );
        assert_eq!(format_watch_header_text(5), format_watch_header_text(0));
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn watch_name_width_is_the_longest_watched_name() {
        let rows = [
            entry(1, Some("spillforge.exe"), 0, 0),
            entry(2, Some("msedgewebview2.exe"), 0, 0),
            entry(3, None, 0, 0),
        ];
        // Only watched PIDs count; 2 is not watched.
        assert_eq!(watch_name_width(&rows, &[1, 3]), 14);
        // Unresolved (`None`) and absent PIDs render as `?`.
        assert_eq!(watch_name_width(&rows, &[3, 99]), 1);
        assert_eq!(watch_name_width(&rows, &[]), 0);
    }

    #[test]
    fn format_watch_rows_text_lines_up_under_the_header() {
        // The askesis report's row (a name and cells narrower than the
        // header), and a name and PID wider than the old fixed widths
        // (`spillforge.exe`, a 7-digit Linux PID): both drifted off their
        // columns before v0.2.13.
        for (pid, name) in [(15534, "canvas"), (4_194_303, "spillforge.exe")] {
            let name_width = name.len();
            let header = format_watch_header_text(name_width);
            let r = watch_row(pid, Some(name), 16 * 1024 * 1024 * 1024, 0, 0, 0, false);
            let rows = format_watch_rows_text(Duration::ZERO, &[r], name_width);
            assert_columns_line_up(&header, &rows);
        }
    }

    /// Assert every column of `header` starts a word in `rows` too.
    fn assert_columns_line_up(header: &str, rows: &str) {
        // Where each word starts, in characters (the header's delta
        // columns begin with a two-byte `Δ`). Cells hold spaces of their
        // own (`16.0 GiB`), so a row has more word starts than the header;
        // every header column must start a word in the row too.
        let starts = |line: &str| -> Vec<usize> {
            line.chars()
                .enumerate()
                .zip(std::iter::once(' ').chain(line.chars()))
                .filter(|&((_, c), prev)| c != ' ' && prev == ' ')
                .map(|((i, _), _)| i)
                .collect()
        };
        let row_starts = starts(rows);
        for column in starts(header) {
            assert!(row_starts.contains(&column), "{header}{rows}");
        }
    }

    #[test]
    fn format_watch_rows_text_single_row() {
        let r = watch_row(
            12345,
            Some("python.exe"),
            8 * 1024 * 1024 * 1024,
            0,
            142 * 1024 * 1024,
            0,
            false,
        );
        let s = format_watch_rows_text(Duration::from_secs(5), &[r], 0);
        assert!(s.starts_with("+5.0s"));
        assert!(s.contains("12345"));
        assert!(s.contains("python.exe"));
        assert!(s.contains("8.0 GiB"));
        assert!(s.contains("142 MiB"));
        assert!(s.contains("+0 B"));
        assert!(s.contains("no"));
    }

    #[test]
    fn format_watch_rows_text_spilling_row() {
        let r = watch_row(
            12345,
            Some("python.exe"),
            16 * 1024 * 1024 * 1024,
            700 * 1024 * 1024,
            2 * 1024 * 1024 * 1024,
            576 * 1024 * 1024,
            true,
        );
        let s = format_watch_rows_text(Duration::from_secs(10), &[r], 0);
        assert!(s.contains("+700 MiB"));
        assert!(s.contains("+576 MiB"));
        // Since v0.2.13 the paged process reads PAGED, as in `hmn ps`.
        assert!(s.contains("PAGED"));
    }

    #[test]
    fn format_watch_rows_text_spilling_device_unpaged_row_reads_device() {
        // The device is spilling, but this process holds only a benign
        // staging baseline: not the one being paged.
        let r = watch_row(4728, Some("Zed.exe"), 0, 0, 110 * 1024 * 1024, 0, true);
        let s = format_watch_rows_text(Duration::ZERO, &[r], 0);
        assert!(s.contains("device"));
        assert!(!s.contains("PAGED"));
    }

    #[test]
    fn format_watch_rows_text_unmeasurable_spill_renders_the_unknown_glyph_not_no() {
        // None (no tracker / not measurable on this platform) must
        // render distinctly from Some(false) ("no") — same "unknown,
        // never no" convention `hmn ps`'s SPILL column uses: `?` on
        // Windows, `n/a` where spill cannot exist (`n/a` holds no "no").
        let r = watch_row_opt(1, Some("py.exe"), 0, 0, 0, 0, None);
        let s = format_watch_rows_text(Duration::ZERO, &[r], 0);
        assert!(s.contains(UNKNOWN_SPILL), "{s}");
        assert!(!s.contains("no"), "{s}");
    }

    #[test]
    fn format_watch_rows_text_missing_name_renders_question_mark() {
        let r = watch_row(99, None, 0, 0, 0, 0, false);
        let s = format_watch_rows_text(Duration::ZERO, &[r], 0);
        assert!(s.contains("99"));
        assert!(s.contains('?'));
    }

    // --- format_watch_rows_json ---

    #[test]
    fn format_watch_rows_json_shape() {
        let r = watch_row(
            7,
            Some("py.exe"),
            1_048_576,
            1_048_576,
            424_242,
            -1_000,
            true,
        );
        let s = format_watch_rows_json(Duration::from_millis(3_500), SystemTime::UNIX_EPOCH, &[r]);
        assert!(s.starts_with(
            r#"{"kind":"sample","t_ms":3500,"wall_clock":"1970-01-01T00:00:00.000Z","pid":7,"name":"py.exe","used_bytes":1048576,"used_delta_bytes":1048576,"shared_used_bytes":424242,"shared_delta_bytes":-1000,"spilling":true,"paged":false}"#
        ));
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn format_watch_rows_json_null_name() {
        let r = watch_row(7, None, 0, 0, 0, 0, false);
        let s = format_watch_rows_json(Duration::ZERO, SystemTime::UNIX_EPOCH, &[r]);
        assert!(s.contains(r#""name":null,"#));
    }

    #[test]
    fn format_watch_rows_json_unmeasurable_spilling_is_null_not_false() {
        let r = watch_row_opt(7, Some("py.exe"), 0, 0, 0, 0, None);
        let s = format_watch_rows_json(Duration::ZERO, SystemTime::UNIX_EPOCH, &[r]);
        assert!(s.contains(r#""spilling":null"#));
    }

    #[test]
    fn format_watch_rows_json_multiple_rows_multiple_lines() {
        let rows = vec![
            watch_row(1, Some("a.exe"), 0, 0, 0, 0, false),
            watch_row(2, Some("b.exe"), 0, 0, 0, 0, false),
        ];
        let s = format_watch_rows_json(Duration::ZERO, SystemTime::UNIX_EPOCH, &rows);
        assert_eq!(s.lines().count(), 2);
    }

    #[test]
    fn format_watch_rows_json_wall_clock_shared_across_rows_in_one_interval() {
        let rows = vec![
            watch_row(1, Some("a.exe"), 0, 0, 0, 0, false),
            watch_row(2, Some("b.exe"), 0, 0, 0, 0, false),
        ];
        let wall_clock = SystemTime::UNIX_EPOCH + Duration::from_millis(1_726_308_723_482);
        let s = format_watch_rows_json(Duration::ZERO, wall_clock, &rows);
        assert_eq!(
            s.matches(r#""wall_clock":"2024-09-14T10:12:03.482Z""#)
                .count(),
            2
        );
    }

    // --- process_sample ---

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_first_interval_zero_delta() {
        let mut state = WatchState::new();
        let rows = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let out = process_sample(&rows, &mut state, &[100], Duration::ZERO, None);
        assert_eq!(out.len(), 1);
        let row = out.first().unwrap();
        assert_eq!(row.used_bytes, 8_000);
        assert_eq!(row.used_delta, 0);
        assert_eq!(row.shared_delta, 0);
        // tracker: None (construction failed/not passed) means "can't
        // tell", not "measured, not spilling".
        assert_eq!(row.spilling, None);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_second_interval_computes_delta() {
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let rows1 = vec![entry(100, Some("python.exe"), 9_500, 300)];
        let out = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        let row = out.first().unwrap();
        assert_eq!(row.used_delta, 1_500);
        assert_eq!(row.shared_delta, 200);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_missing_pid_renders_zero() {
        let mut state = WatchState::new();
        let rows: Vec<GpuProcessEntry> = vec![];
        let out = process_sample(&rows, &mut state, &[42], Duration::ZERO, None);
        assert_eq!(out.len(), 1);
        let row = out.first().unwrap();
        assert_eq!(row.pid, 42);
        assert_eq!(row.used_bytes, 0);
        assert_eq!(row.name, None);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_peak_tracked_across_samples() {
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let rows1 = vec![entry(100, Some("python.exe"), 5_000, 50)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        let s = state.by_pid.get(&100).unwrap();
        assert_eq!(s.peak_used_bytes, 8_000);
        assert_eq!(s.peak_shared_bytes, 100);
        assert_eq!(s.baseline_used_bytes, 8_000);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_name_change_resets_baseline_and_peak() {
        // Simulates the OS recycling pid=100 mid-watch: python.exe ran
        // for a while (baseline/peak grow), then exits and the PID is
        // reassigned to an unrelated notepad.exe. The resolved-name
        // change must reset the row's baseline/peak to the new
        // process's reading rather than mixing the two.
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let rows1 = vec![entry(100, Some("python.exe"), 9_000, 500)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);

        let rows2 = vec![entry(100, Some("notepad.exe"), 200, 10)];
        let out = process_sample(&rows2, &mut state, &[100], Duration::from_secs(10), None);
        let row = out.first().unwrap();
        assert_eq!(row.name.as_deref(), Some("notepad.exe"));
        // Delta is 0 on the reset sample — comparing against the
        // recycled PID's own (much larger) prior reading would be
        // meaningless.
        assert_eq!(row.used_delta, 0);
        assert_eq!(row.shared_delta, 0);

        let s = state.by_pid.get(&100).unwrap();
        assert_eq!(s.baseline_used_bytes, 200);
        assert_eq!(s.baseline_shared_bytes, 10);
        assert_eq!(s.peak_used_bytes, 200);
        assert_eq!(s.peak_shared_bytes, 10);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_unresolved_name_does_not_trigger_reset() {
        // A `?` sample between two resolved samples of the SAME name
        // must not be treated as a reuse — last_name stays sticky
        // across the None sample, so the baseline is undisturbed.
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let rows1 = vec![entry(100, None, 8_500, 150)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        let rows2 = vec![entry(100, Some("python.exe"), 9_000, 200)];
        let out = process_sample(&rows2, &mut state, &[100], Duration::from_secs(10), None);
        let row = out.first().unwrap();
        // Baseline never reset: delta is against the unresolved
        // sample's prev (8_500 / 150), not a fresh 0.
        assert_eq!(row.used_delta, 500);
        assert_eq!(row.shared_delta, 50);
        assert_eq!(state.by_pid.get(&100).unwrap().baseline_used_bytes, 8_000);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_protected_bracket_flicker_does_not_trigger_reset() {
        // Same shape as the `None`-flicker test above, but for the
        // Windows-only `[protected]` synthetic bracket: a transient
        // Toolhelp32Snapshot failure on one interval must not look like
        // "the OS recycled this PID" and reset the baseline.
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let rows1 = vec![entry(100, Some("[protected]"), 8_500, 150)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        let rows2 = vec![entry(100, Some("python.exe"), 9_000, 200)];
        let out = process_sample(&rows2, &mut state, &[100], Duration::from_secs(10), None);
        let row = out.first().unwrap();
        assert_eq!(row.used_delta, 500);
        assert_eq!(row.shared_delta, 50);
        assert_eq!(state.by_pid.get(&100).unwrap().baseline_used_bytes, 8_000);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_reset_survives_a_protected_flicker_in_between() {
        // A `[protected]` flicker must not mask a GENUINE later name
        // change either: last_name should stay "python.exe" (sticky
        // across the flicker), so the real reuse at rows2 still resets.
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("python.exe"), 8_000, 100)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let rows1 = vec![entry(100, Some("[protected]"), 8_500, 150)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        let rows2 = vec![entry(100, Some("notepad.exe"), 200, 10)];
        let out = process_sample(&rows2, &mut state, &[100], Duration::from_secs(10), None);
        let row = out.first().unwrap();
        assert_eq!(row.used_delta, 0);
        assert_eq!(row.shared_delta, 0);
        let s = state.by_pid.get(&100).unwrap();
        assert_eq!(s.baseline_used_bytes, 200);
        assert_eq!(s.baseline_shared_bytes, 10);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_growth_hint_fires_for_protected_bracket() {
        // The one-shot "unresolved pid grew" stderr hint must still
        // fire for the Windows-only `[protected]` bracket, not just a
        // bare `None` — otherwise the hint would go silent on Windows
        // now that most `?` rows resolve via the Toolhelp32Snapshot
        // fallback and only genuinely-protected rows stay unresolved.
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("[protected]"), 0, 0)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let grown = UNRESOLVED_GROWTH_HINT_BYTES;
        let rows1 = vec![entry(100, Some("[protected]"), grown, 0)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        assert!(state.by_pid.get(&100).unwrap().growth_hint_fired);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn process_sample_growth_hint_does_not_fire_for_exited_bracket() {
        // `[exited]` means the process was already confirmed gone —
        // "growth" on a gone process is meaningless, so the hint (whose
        // Windows and Linux wording promises "re-run elevated to
        // identify") must not fire for it the way it does for
        // `None`/`[protected]`.
        let mut state = WatchState::new();
        let rows0 = vec![entry(100, Some("[exited]"), 0, 0)];
        let _ = process_sample(&rows0, &mut state, &[100], Duration::ZERO, None);
        let grown = UNRESOLVED_GROWTH_HINT_BYTES;
        let rows1 = vec![entry(100, Some("[exited]"), grown, 0)];
        let _ = process_sample(&rows1, &mut state, &[100], Duration::from_secs(5), None);
        assert!(!state.by_pid.get(&100).unwrap().growth_hint_fired);
    }

    #[test]
    fn unresolved_growth_hint_keeps_the_elevated_text_off_macos() {
        assert_eq!(
            unresolved_growth_hint(7, 300 * 1024 * 1024, false),
            "hmn watch: unresolved pid=7 grew +300 MiB since attach — re-run elevated to identify"
        );
    }

    #[test]
    fn unresolved_growth_hint_says_outside_the_sandbox_on_macos() {
        assert_eq!(
            unresolved_growth_hint(7, 300 * 1024 * 1024, true),
            "hmn watch: unresolved pid=7 grew +300 MiB since attach — re-run outside the sandbox to identify"
        );
    }

    // --- WatchState::track (--follow-new seen_order bookkeeping) ---

    #[test]
    fn watch_state_track_records_first_seen_order_once() {
        let mut state = WatchState::new();
        state.track(100, 1_000, 0);
        state.track(200, 2_000, 0);
        // Re-tracking an already-seen PID must not append a second
        // seen_order entry, nor reset its state.
        state.track(100, 9_000, 0);
        assert_eq!(state.seen_order, vec![100, 200]);
        assert_eq!(state.by_pid.get(&100).unwrap().baseline_used_bytes, 1_000);
    }

    #[test]
    fn watch_state_track_returns_mutable_existing_entry() {
        let mut state = WatchState::new();
        state.track(100, 1_000, 0).peak_used_bytes = 5_000;
        // The second `track` call for the same PID must see the
        // mutation the first call's caller made through the returned
        // reference, not a fresh `WatchedPidState`.
        assert_eq!(state.track(100, 1_000, 0).peak_used_bytes, 5_000);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn watch_state_seen_order_survives_pid_dropping_out_of_watched() {
        // Simulates --follow-new: pid 100 is watched for one interval,
        // genuinely excluded from `watched` the next (dropped below
        // top-N, but still alive and still present in `rows` — only
        // absent from the `watched` slice `process_sample` is given),
        // then re-enters. seen_order must record it exactly once, at
        // its first sighting, and its accumulated state must survive
        // the gap untouched rather than drifting or resetting.
        let mut state = WatchState::new();
        let rows0 = vec![
            entry(100, Some("a.exe"), 1_000, 0),
            entry(200, Some("b.exe"), 500, 0),
        ];
        let _ = process_sample(&rows0, &mut state, &[100, 200], Duration::ZERO, None);

        // pid 100 is genuinely excluded from `watched` this interval
        // even though it's still present in `rows` at a different
        // reading (9_000) — process_sample must not touch its state
        // at all while it's excluded.
        let rows1 = vec![
            entry(100, Some("a.exe"), 9_000, 0),
            entry(200, Some("b.exe"), 600, 0),
        ];
        let _ = process_sample(&rows1, &mut state, &[200], Duration::from_secs(5), None);
        assert_eq!(
            state.by_pid.get(&100).unwrap().peak_used_bytes,
            1_000,
            "excluded PID's state must be untouched while absent from `watched`"
        );

        // pid 100 re-enters `watched`; its delta is against its own
        // pre-gap prev (1_000), not the 9_000 it drifted to while
        // excluded — process_sample never saw that reading.
        let rows2 = vec![
            entry(100, Some("a.exe"), 1_200, 0),
            entry(200, Some("b.exe"), 600, 0),
        ];
        let out = process_sample(
            &rows2,
            &mut state,
            &[100, 200],
            Duration::from_secs(10),
            None,
        );
        let row100 = out.iter().find(|r| r.pid == 100).unwrap();
        assert_eq!(row100.used_delta, 200); // 1_200 - 1_000, not 1_200 - 9_000
        assert_eq!(state.by_pid.get(&100).unwrap().peak_used_bytes, 1_200);
        assert_eq!(state.seen_order, vec![100, 200]);
    }

    // --- Selection ---

    /// An auto-selection `Selection` with the given `--filter` patterns.
    fn auto(top: usize, follow_new: bool, filters: &[&str]) -> Selection {
        auto_min(top, follow_new, filters, None)
    }

    /// Like [`auto`], with a `--min` threshold too.
    fn auto_min(top: usize, follow_new: bool, filters: &[&str], min: Option<u64>) -> Selection {
        let filters: Vec<String> = filters.iter().map(|f| (*f).to_owned()).collect();
        Selection::new(&[], top, follow_new, &filters, min).unwrap()
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_explicit_passthrough_ignores_rows_and_top() {
        let rows = vec![entry(1, Some("a.exe"), 9_000, 0)];
        let sel = Selection::new(&[42, 43], 1, false, &[], None).unwrap();
        assert_eq!(sel.select(&rows, &WatchState::new()).pids, vec![42, 43]);
    }

    #[test]
    fn selection_explicit_pids_are_deduplicated_in_order() {
        let sel = Selection::new(&[7, 3, 7, 3, 9], 5, false, &[], None).unwrap();
        assert_eq!(sel.select(&[], &WatchState::new()).pids, vec![7, 3, 9]);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_auto_selects_top_n_from_rows() {
        let rows = vec![
            entry(1, Some("a.exe"), 1_000, 0),
            entry(2, Some("b.exe"), 5_000, 0),
            entry(3, Some("c.exe"), 3_000, 0),
        ];
        assert_eq!(
            auto(2, false, &[]).select(&rows, &WatchState::new()).pids,
            vec![2, 3]
        );
    }

    #[test]
    fn selection_empty_rows_and_explicit_is_empty() {
        let selected = auto(5, false, &[]).select(&[], &WatchState::new());
        assert!(selected.pids.is_empty(), "{:?}", selected.pids);
        assert!(
            selected.unmatchable.is_empty(),
            "{:?}",
            selected.unmatchable
        );
    }

    #[test]
    fn selection_rejects_follow_new_with_explicit_pids() {
        let err = Selection::new(&[42], 5, true, &[], None).err().unwrap();
        // Byte-identical to the pre-v0.2.12 message (`main` adds `hmn: `).
        assert_eq!(
            err,
            "watch --follow-new only applies to auto-selection; drop --follow-new or the \
             explicit PID list"
        );
    }

    #[test]
    fn selection_rejects_filter_with_explicit_pids() {
        let err = Selection::new(&[42], 5, false, &["train".to_owned()], None)
            .err()
            .unwrap();
        assert_eq!(
            err,
            "watch --filter only applies to auto-selection; drop --filter or the explicit PID \
             list"
        );
    }

    #[test]
    fn selection_describe_matches_the_pre_selection_header_strings() {
        // Without `--filter`, the three header clauses `run_watch` printed
        // before `Selection` existed, byte for byte.
        assert_eq!(
            auto(3, true, &[]).describe(1),
            "following top 3 by committed (re-selected every interval), 1 initially"
        );
        assert_eq!(
            auto(5, false, &[]).describe(4),
            "watching 4 PID(s) (top 5 by committed)"
        );
        assert_eq!(
            Selection::new(&[10, 11], 5, false, &[], None)
                .unwrap()
                .describe(2),
            "watching 2 PID(s)"
        );
    }

    #[test]
    fn selection_describe_announces_the_filter() {
        assert_eq!(
            auto(3, true, &["figure13"]).describe(1),
            "following top 3 by committed among names containing \"figure13\" \
             (case-insensitive) (re-selected every interval), 1 initially"
        );
        assert_eq!(
            auto(5, false, &["train", "eval run"]).describe(2),
            "watching 2 PID(s) (top 5 by committed among names containing \"train\" or \
             \"eval run\" (case-insensitive))"
        );
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_filter_narrows_before_top_n() {
        // `dwm.exe` holds the most VRAM, so rank alone would pick it; the
        // filter admits only the two `train*` processes, then top-1 picks
        // the larger of them.
        let rows = vec![
            entry(1, Some("dwm.exe"), 9_000, 0),
            entry(2, Some("train.exe"), 3_000, 0),
            entry(3, Some("Train_Eval.EXE"), 5_000, 0),
        ];
        let state = WatchState::new();
        assert_eq!(
            auto(1, true, &["TRAIN"]).select(&rows, &state).pids,
            vec![3]
        );
        assert_eq!(
            auto(5, true, &["train"]).select(&rows, &state).pids,
            vec![3, 2]
        );
        let none = auto(5, true, &["nope"]).select(&rows, &state).pids;
        assert!(none.is_empty(), "{none:?}");
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_filter_patterns_are_or_ed() {
        let rows = vec![
            entry(1, Some("python.exe"), 1_000, 0),
            entry(2, Some("train.exe"), 2_000, 0),
            entry(3, Some("dwm.exe"), 3_000, 0),
        ];
        let got = auto(5, true, &["python", "train"]).select(&rows, &WatchState::new());
        assert_eq!(got.pids, vec![2, 1]);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_filter_keeps_a_followed_pid_through_a_name_flicker() {
        // pid 9 was followed as `train.exe`; this sample its name reads
        // `[protected]`. The sticky last-resolved name keeps it matching.
        let mut state = WatchState::new();
        state.track(9, 0, 0).last_name = Some("train.exe".to_owned());
        let rows = vec![entry(9, Some("[protected]"), 4_000, 0)];
        let got = auto(3, true, &["train"]).select(&rows, &state);
        assert_eq!(got.pids, vec![9]);
        assert!(got.unmatchable.is_empty(), "{:?}", got.unmatchable);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_filter_reports_never_resolved_rows_as_unmatchable() {
        let rows = vec![
            entry(1, Some("[protected]"), 1_000, 0),
            entry(2, Some("[exited]"), 1_000, 0),
            entry(3, Some("?"), 1_000, 0),
            entry(4, None, 1_000, 0),
            entry(5, Some("train.exe"), 1_000, 0),
            entry(6, Some("dwm.exe"), 1_000, 0),
        ];
        let got = auto(9, true, &["train"]).select(&rows, &WatchState::new());
        assert_eq!(got.pids, vec![5]);
        // Only the rows the filter could not judge — `dwm.exe` was judged,
        // and rejected.
        assert_eq!(got.unmatchable, vec![1, 2, 3, 4]);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_without_filter_reports_nothing_unmatchable() {
        let rows = vec![entry(1, Some("[protected]"), 1_000, 0)];
        let got = auto(3, true, &[]).select(&rows, &WatchState::new());
        assert_eq!(got.pids, vec![1]);
        assert!(got.unmatchable.is_empty(), "{:?}", got.unmatchable);
    }

    #[test]
    fn selection_rejects_min_with_explicit_pids() {
        let err = Selection::new(&[42], 5, false, &[], Some(1)).err().unwrap();
        assert_eq!(
            err,
            "watch --min only applies to auto-selection; drop --min or the explicit PID list"
        );
    }

    #[test]
    fn selection_describe_announces_the_min_threshold() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert_eq!(
            auto_min(3, true, &[], Some(2 * GIB)).describe(1),
            "following top 3 by committed with footprint >= 2 GiB (re-selected every interval), \
             1 initially"
        );
        assert_eq!(
            auto_min(5, false, &["train"], Some(512 * 1024 * 1024)).describe(2),
            "watching 2 PID(s) (top 5 by committed among names containing \"train\" \
             (case-insensitive) with footprint >= 512 MiB)"
        );
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_min_counts_committed_plus_shared_like_ps_min() {
        // pid 1: 900 committed + 200 shared = 1,100 >= 1,000 -> kept, although
        // its committed bytes alone are below the threshold. pid 2: 999 -> cut.
        let rows = vec![
            entry(1, Some("a.exe"), 900, 200),
            entry(2, Some("b.exe"), 999, 0),
            entry(3, Some("c.exe"), 5_000, 0),
        ];
        let got = auto_min(9, true, &[], Some(1_000)).select(&rows, &WatchState::new());
        assert_eq!(got.pids, vec![3, 1]);
        // `--min 0` is a no-op, as for `hmn ps`.
        let all = auto_min(9, true, &[], Some(0)).select(&rows, &WatchState::new());
        assert_eq!(all.pids.len(), 3);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn selection_min_applies_before_the_filter() {
        // A below-threshold unresolved row is cut by `--min` first, so it is
        // not announced as unmatchable: only processes big enough to matter
        // are worth a notice.
        let rows = vec![
            entry(1, Some("[protected]"), 10, 0),
            entry(2, Some("[protected]"), 5_000, 0),
            entry(3, Some("train.exe"), 5_000, 0),
        ];
        let got = auto_min(9, true, &["train"], Some(1_000)).select(&rows, &WatchState::new());
        assert_eq!(got.pids, vec![3]);
        assert_eq!(got.unmatchable, vec![2]);
    }

    #[test]
    fn matchable_name_prefers_current_then_sticky() {
        assert_eq!(matchable_name(Some("a.exe"), Some("b.exe")), Some("a.exe"));
        assert_eq!(
            matchable_name(Some("[protected]"), Some("b.exe")),
            Some("b.exe")
        );
        assert_eq!(
            matchable_name(Some("[exited]"), Some("b.exe")),
            Some("b.exe")
        );
        assert_eq!(matchable_name(Some("?"), Some("b.exe")), Some("b.exe"));
        assert_eq!(matchable_name(None, Some("b.exe")), Some("b.exe"));
        assert_eq!(matchable_name(Some("?"), Some("?")), None);
        assert_eq!(matchable_name(None, None), None);
        // `[kernel]` is a stable, genuine name (PID 4), as for PID reuse.
        assert_eq!(matchable_name(Some("[kernel]"), None), Some("[kernel]"));
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn missing_pid_notices_name_only_pids_known_not_to_exist() {
        let listed = [entry(15534, Some("canvas"), 0, 0)];
        // 15534 is listed (never asked); 15503 exists without GPU memory;
        // 999999 does not exist; 42 cannot be judged.
        let exists = |pid: u32| match pid {
            999_999 => Some(false),
            42 => None,
            _ => Some(true),
        };
        assert_eq!(
            missing_pid_notices(&[15534, 15503, 999_999, 42], &listed, &[], exists),
            ["hmn watch: pid=999999 names no running process; its rows will read 0 MiB"]
        );
        // Auto-selection has no explicit PIDs: nothing to warn about.
        let notices = missing_pid_notices(&[], &listed, &[], |_| Some(false));
        assert!(notices.is_empty(), "{notices:?}");
    }

    // --- denied explicit PIDs and the unreadable count ---

    #[test]
    fn denied_pid_notices_name_each_denied_explicit_pid_in_the_order_given() {
        assert_eq!(
            denied_pid_notices(&[7, 3, 9], &[9, 100, 7], true),
            [
                "hmn watch: pid=7 is unreadable here; its rows will read 0 MiB — re-run outside the sandbox",
                "hmn watch: pid=9 is unreadable here; its rows will read 0 MiB — re-run outside the sandbox",
            ]
        );
        assert_eq!(
            denied_pid_notices(&[7], &[7], false),
            [
                "hmn watch: pid=7 is unreadable here; its rows will read 0 MiB — re-run elevated for names"
            ]
        );
        // A readable PID, auto-selection and no denied PID at all get none.
        for (explicit, denied) in [(&[5, 6][..], &[7, 8][..]), (&[], &[7, 8]), (&[7, 8], &[])] {
            let notices = denied_pid_notices(explicit, denied, true);
            assert!(notices.is_empty(), "{notices:?}");
        }
    }

    #[test]
    fn missing_pid_notices_skip_denied_pids() {
        // `exists` would call both PIDs gone; 999 is denied, so it is not
        // named, and 998, which is not, is.
        assert_eq!(
            missing_pid_notices(&[999, 998], &[], &[999], |_| Some(false)),
            ["hmn watch: pid=998 names no running process; its rows will read 0 MiB"]
        );
    }

    #[test]
    fn unmatchable_notices_announce_each_pid_once() {
        let mut announced = HashSet::new();
        assert_eq!(
            unmatchable_notices(&[7, 8], &mut announced),
            [
                "hmn watch: pid=7 has no resolvable name; --filter cannot match it",
                "hmn watch: pid=8 has no resolvable name; --filter cannot match it",
            ]
        );
        assert_eq!(
            unmatchable_notices(&[8, 9], &mut announced),
            ["hmn watch: pid=9 has no resolvable name; --filter cannot match it"]
        );
    }

    // --- start record ---

    #[test]
    fn start_record_describes_an_auto_selection_exactly() {
        let sel = auto_min(3, true, &["figure13"], Some(2 * 1024 * 1024 * 1024));
        let argv: Vec<String> = ["hmn", "watch", "--follow-new", "--filter", "figure13"]
            .iter()
            .map(|a| (*a).to_owned())
            .collect();
        let got = format_watch_start_json(
            SystemTime::UNIX_EPOCH,
            &argv,
            0,
            Some("NVIDIA GeForce RTX 5060 Ti"),
            Duration::from_secs(30),
            None,
            &sel,
        );
        let expected = format!(
            "{{\"kind\":\"start\",\"t_ms\":0,\"wall_clock\":\"1970-01-01T00:00:00.000Z\",\
             \"hmn_version\":\"{}\",\"argv\":[\"hmn\",\"watch\",\"--follow-new\",\"--filter\",\
             \"figure13\"],\"device\":0,\"device_name\":\"NVIDIA GeForce RTX 5060 Ti\",\
             \"interval_ms\":30000,\"duration_ms\":null,\"selection\":{{\"mode\":\"follow_new\",\
             \"pids\":[],\"top\":3,\"filters\":[\"figure13\"],\"min_bytes\":2147483648}}}}\n",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(got, expected);
    }

    #[test]
    fn start_record_explicit_mode_nulls_what_does_not_apply() {
        let sel = Selection::new(&[42, 7], 5, false, &[], None).unwrap();
        let got = format_watch_start_json(
            SystemTime::UNIX_EPOCH,
            &[],
            1,
            None,
            Duration::from_millis(500),
            Some(Duration::from_secs(60)),
            &sel,
        );
        assert!(got.ends_with(
            "\"device\":1,\"device_name\":null,\"interval_ms\":500,\"duration_ms\":60000,\
             \"selection\":{\"mode\":\"explicit\",\"pids\":[42,7],\"top\":null,\"filters\":[],\
             \"min_bytes\":null}}\n"
        ));
        // One-shot auto-selection reports its own mode.
        let mut one_shot = String::new();
        auto(4, false, &[]).write_json(&mut one_shot);
        assert!(one_shot.starts_with("{\"mode\":\"top\",\"pids\":[],\"top\":4,"));
    }

    #[test]
    fn recorded_argv_keeps_only_the_program_file_name() {
        // Built with the platform's own separator: `\` separates nothing on
        // Linux, so a literal Windows path would not exercise the stripping.
        let program: std::path::PathBuf = ["users", "someone", "target", "release", "hmn.exe"]
            .iter()
            .collect();
        let args = [
            program.into_os_string(),
            "watch".into(),
            "--filter".into(),
            "some/dir".into(),
        ];
        // Only argv[0] is reduced; a later argument that looks like a path
        // is recorded as typed.
        assert_eq!(
            recorded_argv(args),
            ["hmn.exe", "watch", "--filter", "some/dir"]
        );
        assert_eq!(recorded_argv([std::ffi::OsString::from("hmn")]), ["hmn"]);
        let recorded = recorded_argv(Vec::<std::ffi::OsString>::new());
        assert!(recorded.is_empty(), "{recorded:?}");
    }

    #[test]
    fn start_record_escapes_argv_and_patterns() {
        let sel = auto(1, true, &["a\"b"]);
        let got = format_watch_start_json(
            SystemTime::UNIX_EPOCH,
            &["C:\\hmn.exe".to_owned()],
            0,
            None,
            Duration::from_secs(1),
            None,
            &sel,
        );
        assert!(got.contains("\"argv\":[\"C:\\\\hmn.exe\"]"));
        assert!(got.contains("\"filters\":[\"a\\\"b\"]"));
    }

    // --- format_followed_set_change (--follow-new stderr breadcrumb) ---

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_followed_set_change_none_when_unchanged() {
        let rows = vec![entry(1, Some("a.exe"), 1_000, 0)];
        let state = WatchState::new();
        assert!(format_followed_set_change(&[1], &[1], &rows, &state, Duration::ZERO).is_none());
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_followed_set_change_reports_entered_with_name_from_rows() {
        let rows = vec![entry(2, Some("new.exe"), 1_000, 0)];
        let state = WatchState::new();
        let msg = format_followed_set_change(&[1], &[1, 2], &rows, &state, Duration::from_secs(10))
            .unwrap();
        assert!(msg.contains("entered pid=2 (new.exe)"), "{msg}");
        assert!(!msg.contains("left"), "{msg}");
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_followed_set_change_reports_left_with_name_from_state() {
        // The departed PID is, by construction, absent from the current
        // sample's `rows` — its name must come from `state`'s last
        // known reading instead.
        let rows: Vec<GpuProcessEntry> = vec![];
        let mut state = WatchState::new();
        state.track(1, 1_000, 0).last_name = Some("gone.exe".to_owned());
        let msg =
            format_followed_set_change(&[1], &[], &rows, &state, Duration::from_secs(10)).unwrap();
        assert!(msg.contains("left pid=1 (gone.exe)"), "{msg}");
        assert!(!msg.contains("entered"), "{msg}");
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_followed_set_change_unresolved_name_renders_bare_pid() {
        let rows = vec![entry(2, None, 1_000, 0)];
        let state = WatchState::new();
        let msg = format_followed_set_change(&[1], &[1, 2], &rows, &state, Duration::ZERO).unwrap();
        assert!(msg.contains("entered pid=2"), "{msg}");
        assert!(!msg.contains("pid=2 ("), "{msg}");
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_followed_set_change_reports_both_entered_and_left() {
        let rows = vec![entry(2, Some("new.exe"), 1_000, 0)];
        let state = WatchState::new();
        let msg = format_followed_set_change(&[1], &[2], &rows, &state, Duration::ZERO).unwrap();
        assert!(msg.contains("entered pid=2 (new.exe)"), "{msg}");
        assert!(msg.contains("left pid=1"), "{msg}");
    }

    // --- format_watch_per_pid_block / summary formatting ---

    #[test]
    fn format_watch_per_pid_block_empty_is_empty_string() {
        assert_eq!(format_watch_per_pid_block(&[]), "");
    }

    #[test]
    fn format_watch_per_pid_block_single_pid() {
        let s = format_watch_per_pid_block(&[pid_summary(
            12345,
            Some("python.exe"),
            8 * 1024 * 1024 * 1024,
            9 * 1024 * 1024 * 1024,
            100 * 1024 * 1024,
            700 * 1024 * 1024,
        )]);
        assert!(s.contains("12345"));
        assert!(s.contains("python.exe"));
        assert!(s.contains("8.0 GiB"));
        assert!(s.contains("9.0 GiB"));
        assert!(s.contains("100 MiB"));
        assert!(s.contains("700 MiB"));
    }

    #[test]
    fn format_watch_summary_text_no_tracker_notes_and_still_shows_per_pid() {
        let s = format_watch_summary_text(None, None, &[pid_summary(1, Some("a.exe"), 0, 0, 0, 0)]);
        assert!(s.contains("spill tracking unavailable"));
        assert!(s.contains("per-PID"));
        assert!(s.contains('1'));
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_watch_summary_text_unmeasurable_report_says_so_not_no_spill() {
        // The shape every Linux and macOS run produces: a tracker exists,
        // but its report is the all-zeros `measurable: false` one. It must
        // not render as a measured negative (the v0.2.6–v0.2.12 bug).
        let report = SpillReport::builder().build();
        assert!(!report.measurable);
        let s = format_watch_summary_text(
            Some(&report),
            None,
            &[pid_summary(15534, Some("canvas"), 0, 0, 0, 0)],
        );
        assert!(s.starts_with(
            "hmn watch: spill not measurable on this platform; per-PID VRAM below\n\
             hmn watch: per-PID  PID"
        ));
        assert!(s.contains("15534"));
        assert!(!s.contains("no spill observed"));
        assert!(!s.contains("peak dedicated"));
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_watch_summary_text_measurable_uses_watch_prefix() {
        let s = format_watch_summary_text(Some(&spilling_report()), None, &[]);
        assert!(s.starts_with("hmn watch: peak dedicated"));
    }

    #[test]
    fn format_watch_summary_json_unmeasurable_shape() {
        let s =
            format_watch_summary_json(None, None, &[pid_summary(1, Some("a.exe"), 10, 20, 0, 0)]);
        assert!(s.starts_with(
            r#"{"kind":"summary","measurable":false,"spilled":false,"observations":0,"#
        ));
        assert!(s.contains(r#""per_pid":[{"pid":1,"name":"a.exe","baseline_used_bytes":10,"peak_used_bytes":20,"baseline_shared_bytes":0,"peak_shared_bytes":0,"paged":null}]"#));
        assert!(s.contains(r#""spilling_at_attach":null,"per_pid":["#));
        assert!(s.ends_with("]}\n"));
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn spilling_at_attach_notice_only_when_the_device_already_spills() {
        let rows = [
            entry(1, Some("spillforge.exe"), 0, 2 * 1024 * 1024 * 1024),
            entry(2, Some("dwm.exe"), 0, 100 * 1024 * 1024),
        ];
        assert_eq!(
            spilling_at_attach_notice(0, Some(true), &rows).as_deref(),
            Some(
                "hmn watch: device 0 is already spilling at attach (2.1 GiB shared); spill is \
                 measured as growth from here, so this spill will not be counted — `hmn ps` shows it"
            )
        );
        assert_eq!(spilling_at_attach_notice(0, Some(false), &rows), None);
        assert_eq!(spilling_at_attach_notice(0, None, &rows), None);
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_watch_summary_notes_a_baseline_taken_mid_spill() {
        let text = format_watch_summary_text(Some(&spilling_report()), Some(true), &[]);
        assert!(text.contains("\nhmn watch: the device was already spilling at attach;"));
        let text = format_watch_summary_text(Some(&spilling_report()), Some(false), &[]);
        assert!(!text.contains("already spilling"));
        let json = format_watch_summary_json(Some(&spilling_report()), Some(true), &[]);
        assert!(json.contains(r#""spilling_at_attach":true,"per_pid":[]"#));
    }

    #[test]
    fn format_watch_per_pid_block_paged_column() {
        let mut paged = pid_summary(26476, Some("canvas.exe"), 0, 0, 0, 2 << 30);
        paged.paged = Some(true);
        let mut not_paged = pid_summary(22108, Some("firefox.exe"), 0, 0, 0, 0);
        not_paged.paged = Some(false);
        let unmeasured = pid_summary(15534, Some("canvas"), 0, 0, 0, 0);
        let s = format_watch_per_pid_block(&[paged, not_paged, unmeasured]);
        assert!(s.starts_with("hmn watch: per-PID  PID"));
        assert!(s.contains("PEAK SHARED  PAGED"));
        let cell = |pid: &str| {
            s.lines()
                .find(|l| l.contains(pid))
                .and_then(|l| l.split_whitespace().last())
                .map(str::to_owned)
        };
        assert_eq!(cell("26476").as_deref(), Some("yes"));
        assert_eq!(cell("22108").as_deref(), Some("no"));
        assert_eq!(cell("15534").as_deref(), Some(UNKNOWN_SPILL));
    }

    #[cfg(feature = "test-helpers")]
    #[test]
    fn format_watch_summary_json_measurable_shape() {
        let s = format_watch_summary_json(Some(&spilling_report()), None, &[]);
        assert!(s.starts_with(r#"{"kind":"summary","measurable":true,"spilled":true,"#));
        assert!(s.contains(r#""per_pid":[]"#));
    }
}
