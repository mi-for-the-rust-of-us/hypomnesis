// SPDX-License-Identifier: MIT OR Apache-2.0

//! Formatting and parsing primitives shared across subcommands: byte
//! units (`format_vram`, `format_vram_precise`, `parse_size_bytes`),
//! durations and timestamps, the SPILL-cell glyphs, the column-table
//! renderer (`Table`), and JSON string escaping.

use std::fmt::Write as _;
use std::time::{Duration, SystemTime};

use hypomnesis::HypomnesisError;

/// Binary byte-size ladder, shared by [`bytes_to_mib`], [`format_vram`],
/// [`format_vram_precise`], and `parse_size_bytes`'s unit table — one
/// definition of `KiB`/`MiB`/`GiB` instead of several independently
/// spelled copies, so `hmn`'s display units and the units `--min`/`fits`
/// accept can't quietly drift apart.
const KIB: u64 = 1024;
/// See [`KIB`].
const MIB: u64 = KIB * 1024;
/// See [`KIB`].
pub const GIB: u64 = MIB * 1024;

/// `MiB` (`bytes / 1_048_576`), rounded down. Used by the device-summary
/// formatter where `MiB` precision is sufficient.
pub const fn bytes_to_mib(bytes: u64) -> u64 {
    bytes / MIB
}

/// Human-readable VRAM string. Renders `MiB` below 1 `GiB`, else `GiB`
/// to one decimal place. Tuned for glanceable table columns — small
/// values can round to a display figure that doesn't round-trip
/// exactly; where exact size matters (a size copied back into
/// `--min`/`fits`, or a comparison two figures are drawn side by
/// side for) use [`format_vram_precise`] instead.
pub fn format_vram(bytes: u64) -> String {
    if bytes >= GIB {
        // CAST: u64 → f64, byte count and constant; fits in f64 mantissa
        // for any realistic VRAM size (< 2^53 bytes ≈ 8 PiB).
        #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
        let g = (bytes as f64) / (GIB as f64);
        format!("{g:.1} GiB")
    } else {
        let mib = bytes / MIB;
        format!("{mib} MiB")
    }
}

/// Precise size string for contexts where [`format_vram`]'s rounding
/// would mislead — the `hmn ps --min` summary echo and `hmn fits`'s
/// margin, both of which state or imply an exact comparison.
/// `format_vram(524_288)` prints `"0 MiB"` (a nonzero value read as
/// the documented `--min 0` no-op); `format_vram_precise` picks the
/// largest unit the value actually clears and prints up to two
/// decimal places, trimming trailing zeros, so `524_288` reads
/// `"512 KiB"` and nothing nonzero ever displays as `"0"`.
pub fn format_vram_precise(bytes: u64) -> String {
    let (divisor, unit) = [(GIB, "GiB"), (MIB, "MiB"), (KIB, "KiB")]
        .into_iter()
        .find(|&(d, _)| bytes >= d)
        .unwrap_or((1, "B"));
    if divisor == 1 {
        return format!("{bytes} B");
    }
    // CAST: u64 → f64, byte count and constant; fits in f64 mantissa
    // for any realistic VRAM size (< 2^53 bytes ≈ 8 PiB).
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let value = (bytes as f64) / (divisor as f64);
    let formatted = format!("{value:.2}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    format!("{trimmed} {unit}")
}

/// The ` [name]` suffix rendered after a device index in several
/// stderr/summary lines (`hmn watch: device 0 [RTX 5060 Ti], ...`,
/// `hmn: fits ... (device 0 [RTX 5060 Ti])`) — empty string when no
/// name is available, so callers can splice it in unconditionally.
pub fn device_name_suffix(name: Option<&str>) -> String {
    name.map_or_else(String::new, |n| format!(" [{n}]"))
}

/// SPILL-column cell for one row, from its device's verdict (`spilling`)
/// and whether this process is being paged (`paged`, `ps::paged_verdict`):
/// `PAGED` when the device is spilling and this process is paged,
/// `device` when the device is spilling but this process is not paged,
/// `no` when the device is not spilling, and, when the verdict is `None`,
/// `n/a` on Linux and macOS, where spill cannot exist (no shared-residency
/// counter), or `?` on Windows, where spill exists but cannot be read now
/// (pre-`WDDM 2.0`, a non-NVIDIA adapter, a `PDH` hiccup, or a build
/// without the `pdh` feature). Never `no` for `None`: the v0.2.11 honesty
/// contract, so an operator never mistakes "not measurable here" for
/// "measured, not spilling". The platform is [`SPILL_CANNOT_EXIST`], fixed
/// at compile time ([`spill_cell_for`] is the testable core). Shared by
/// `hmn ps`'s table and `hmn watch`'s rows so the two surfaces cannot
/// render the contract differently.
#[must_use]
pub const fn spill_cell(spilling: Option<bool>, paged: Option<bool>) -> &'static str {
    spill_cell_for(spilling, paged, SPILL_CANNOT_EXIST)
}

/// Whether this build's platform has no spill to measure: `true` on Linux
/// and macOS, which have no shared-residency counter, `false` on Windows,
/// where spill exists even when it cannot be read now. A compile-time
/// fact, never the runtime `PDH` probe, which also folds pre-`WDDM 2.0`
/// and a `PDH` hiccup into "not measurable".
pub const SPILL_CANNOT_EXIST: bool = !cfg!(windows);

/// The pure core of [`spill_cell`], with the platform passed in as
/// `spill_cannot_exist` so tests exercise both platforms on any OS.
/// `PAGED`, `device` and `no` do not depend on the flag; `None` renders as
/// the unknown glyph, `n/a` where spill cannot exist and `?` otherwise.
#[must_use]
pub const fn spill_cell_for(
    spilling: Option<bool>,
    paged: Option<bool>,
    spill_cannot_exist: bool,
) -> &'static str {
    match (spilling, paged) {
        (Some(true), Some(true)) => "PAGED",
        (Some(true), _) => "device",
        (Some(false), _) => "no",
        (None, _) => unknown_cell(spill_cannot_exist),
    }
}

/// `hmn watch`'s per-PID `PAGED` cell: `yes`, `no`, or the platform's
/// unknown glyph for `None` (see [`paged_cell_for`]).
#[must_use]
pub const fn paged_cell(paged: Option<bool>) -> &'static str {
    paged_cell_for(paged, SPILL_CANNOT_EXIST)
}

/// The pure core of [`paged_cell`], with the platform passed in as
/// `spill_cannot_exist`: `yes` for `Some(true)`, `no` for `Some(false)`,
/// and for `None` `n/a` where spill cannot exist and `?` otherwise.
#[must_use]
pub const fn paged_cell_for(paged: Option<bool>, spill_cannot_exist: bool) -> &'static str {
    match paged {
        Some(true) => "yes",
        Some(false) => "no",
        None => unknown_cell(spill_cannot_exist),
    }
}

/// The glyph of a SPILL or `PAGED` cell whose verdict is `None`: `n/a`
/// where spill cannot exist, `?` where it exists but cannot be read now.
/// Never `no`. One place, so the two cells cannot diverge.
const fn unknown_cell(spill_cannot_exist: bool) -> &'static str {
    if spill_cannot_exist { "n/a" } else { "?" }
}

/// Whether this build's remedy for an unresolved process is "re-run
/// outside the sandbox" (macOS) rather than "re-run elevated" (Windows,
/// Linux). Selected at compile time because the reason is a platform
/// fact, not a runtime one: on macOS only a sandbox withholds another
/// process's name and bytes from `hmn`, and elevation does not lift a
/// sandbox, so advising it there is wrong on every run. A `bool` rather
/// than a hidden branch, so callers pass it to [`remedy_text`] and tests
/// pin both texts on any OS.
pub const REMEDY_OUTSIDE_SANDBOX: bool = cfg!(target_os = "macos");

/// What a remedy clause asks the user to re-run `hmn` for; [`remedy_text`]
/// words it.
///
/// Binary-internal dispatch enum, not a library type — matched
/// exhaustively by [`remedy_text`], the sole place that interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemedyPurpose {
    /// The names of protected rows and of the processes the caller was
    /// refused, on `hmn ps`'s summary line, the denial line and
    /// `hmn watch`'s denied-PID notice (`for names`).
    Names,
    /// An unresolved PID whose memory grew, in `hmn watch`'s growth hint
    /// (`to identify`).
    Identify,
}

/// The remedy clause `hmn ps` and `hmn watch` print for a process they
/// could not resolve: `re-run elevated` when `outside_sandbox` is false
/// and `re-run outside the sandbox` when it is true, followed by the
/// words of `purpose` (`for names`, `to identify`), with one exception.
/// On macOS the purpose `RemedyPurpose::Names` yields the bare `re-run outside the sandbox`.
/// Pure, so both platform texts are tested on any OS, the way
/// [`spill_cell`] is; production callers pass [`REMEDY_OUTSIDE_SANDBOX`].
#[must_use]
pub fn remedy_text(outside_sandbox: bool, purpose: RemedyPurpose) -> String {
    let text = match (outside_sandbox, purpose) {
        (false, RemedyPurpose::Names) => "re-run elevated for names",
        (false, RemedyPurpose::Identify) => "re-run elevated to identify",
        (true, RemedyPurpose::Names) => "re-run outside the sandbox",
        (true, RemedyPurpose::Identify) => "re-run outside the sandbox to identify",
    };
    // BORROW: explicit to_owned — the caller owns the remedy text.
    text.to_owned()
}

/// `text`, then ` — ` and the remedy for [`RemedyPurpose::Names`]: the one
/// join every line that ends in a remedy goes through, so the separator,
/// the purpose and the order are spelled once.
#[must_use]
pub fn with_remedy(text: &str, outside_sandbox: bool) -> String {
    format!(
        "{text} — {}",
        remedy_text(outside_sandbox, RemedyPurpose::Names)
    )
}

/// The text `hmn ps` and `hmn watch` print for a failed device query: the
/// error's `Display`, with ` — ` and [`remedy_text`] for
/// [`RemedyPurpose::Names`] appended when the error is
/// [`HypomnesisError::ProcessListDenied`], the one failure a sandbox
/// causes and the user can remedy. Every other error is its `Display`.
/// One function, so the skip line, the `--device` line and the attach
/// error of `hmn watch` cannot word the denial differently.
#[must_use]
pub fn failure_detail(e: &HypomnesisError, outside_sandbox: bool) -> String {
    if matches!(e, HypomnesisError::ProcessListDenied { .. }) {
        with_remedy(&e.to_string(), outside_sandbox)
    } else {
        e.to_string()
    }
}

/// Compute the width of a table column as `max(header.len(),
/// max(cell.len()))`.
pub fn column_width<'a>(header: &str, cells: impl IntoIterator<Item = &'a str>) -> usize {
    cells
        .into_iter()
        .map(str::len)
        .chain(std::iter::once(header.len()))
        .max()
        .unwrap_or(0)
}

/// A column-aligned text table — the one renderer behind `hmn ps`'s
/// listing and `hmn watch`'s interval rows and closing per-PID block.
///
/// Each column is as wide as its widest cell or its header ([`column_width`],
/// so widths are measured in bytes, as they always have been), and at least
/// its minimum width when one was set ([`Self::with_min_widths`]); cells are
/// left-aligned, separated by two spaces, and the last column is padded too,
/// so every line of a table has the same shape.
pub struct Table {
    /// Column headers, in display order. Their count fixes the column count.
    headers: Vec<&'static str>,
    /// Data rows, one cell per header. A short row renders its missing
    /// cells as empty; extra cells are ignored.
    rows: Vec<Vec<String>>,
    /// Minimum width of each column, in header order; a column without an
    /// entry has none. Empty unless [`Self::with_min_widths`] set it.
    min_widths: Vec<usize>,
}

impl Table {
    /// An empty table with the given column headers.
    #[must_use]
    pub fn new(headers: &[&'static str]) -> Self {
        Self {
            headers: headers.to_vec(),
            rows: Vec::new(),
            min_widths: Vec::new(),
        }
    }

    /// Give each column a minimum width, in header order, so tables
    /// rendered separately share their columns whenever their cells fit —
    /// `hmn watch` renders its header once and each interval's rows as a
    /// table of their own. A cell wider than its minimum still widens its
    /// column.
    #[must_use]
    pub fn with_min_widths(mut self, min_widths: &[usize]) -> Self {
        self.min_widths = min_widths.to_vec();
        self
    }

    /// Append one data row.
    pub fn push_row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    /// Render the table. With `header_prefix: Some(p)`, a header line —
    /// prefixed by `p` — comes first; with `None`, no header line is
    /// printed, but the headers still count toward the column widths. Every
    /// data row is prefixed by `row_prefix`. Each line ends in `\n`.
    #[must_use]
    pub fn render(&self, header_prefix: Option<&str>, row_prefix: &str) -> String {
        let widths: Vec<usize> = self
            .headers
            .iter()
            .enumerate()
            .map(|(col, header)| {
                column_width(header, self.rows.iter().map(|r| cell(r, col)))
                    .max(self.min_widths.get(col).copied().unwrap_or(0))
            })
            .collect();
        let mut out = String::new();
        if let Some(prefix) = header_prefix {
            write_table_line(&mut out, prefix, self.headers.iter().copied(), &widths);
        }
        for row in &self.rows {
            write_table_line(
                &mut out,
                row_prefix,
                (0..widths.len()).map(|col| cell(row, col)),
                &widths,
            );
        }
        out
    }
}

/// Cell `col` of `row`, or `""` when the row is short.
fn cell(row: &[String], col: usize) -> &str {
    row.get(col).map_or("", String::as_str)
}

/// Write one `prefix`-led table line: each cell left-aligned to its
/// column's width, two spaces between cells, newline-terminated.
fn write_table_line<'a>(
    out: &mut String,
    prefix: &str,
    cells: impl Iterator<Item = &'a str>,
    widths: &[usize],
) {
    out.push_str(prefix);
    for (col, (text, &width)) in cells.zip(widths).enumerate() {
        if col > 0 {
            out.push_str("  ");
        }
        let _ = write!(out, "{text:<width$}");
    }
    out.push('\n');
}

/// A JSON string value for `s` — quoted and escaped via [`json_escape`] —
/// or `null` for `None`. The one spelling of every optional-string field
/// in `hmn`'s `--json` output (process and device names, driver version,
/// episode labels, the `start` record's `device_name`).
#[must_use]
pub fn json_string_or_null(s: Option<&str>) -> String {
    s.map_or_else(|| String::from("null"), json_string)
}

/// A JSON string value for `s`: quoted, and escaped via [`json_escape`].
/// For strings that are always present, such as the `start` record's
/// `argv` entries and `--filter` patterns; see [`json_string_or_null`] for
/// optional ones.
#[must_use]
pub fn json_string(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

/// A JSON number or boolean for `v` — its `Display` form, which is already
/// valid JSON for integers and `bool` — or `null` for `None`. The one
/// spelling of every optional numeric or boolean field in `hmn`'s
/// `--json` output (`spilling`, `paged`, `shared_share`, `reserved_bytes`,
/// the `start` record's `duration_ms` / `top` / `min_bytes`). Callers pass
/// integers, `bool`, or a float already formatted as a JSON number
/// (`shared_share`'s `format!("{f:.4}")`): a float's own `Display` (`NaN`,
/// `inf`) would not be valid JSON.
#[must_use]
pub fn json_value_or_null<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| String::from("null"), |v| v.to_string())
}

/// Escape a string for JSON output. Hand-rolled to avoid pulling in
/// `serde_json` for the CLI feature.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                // CAST: char → u32, valid scalar values fit (≤ 0x10FFFF).
                #[allow(clippy::as_conversions)]
                let code = c as u32;
                let _ = write!(out, "\\u{code:04x}");
            }
            c => out.push(c),
        }
    }
    out
}

/// Whole milliseconds of a [`Duration`] for JSON output (`u128`
/// clamped into `u64` — saturates at `u64::MAX`, unreachable for real
/// run lengths).
pub fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Seconds with one decimal place (`"3.8s"`) — the human-facing
/// duration rendering in the spill report.
pub fn format_secs(d: Duration) -> String {
    format!("{:.1}s", d.as_secs_f64())
}

/// Format `t` as UTC ISO-8601 with millisecond precision
/// (`YYYY-MM-DDThh:mm:ss.mmmZ`) — the `wall_clock` field on
/// `hmn watch --json` samples. No date/time dependency: pure
/// proleptic-Gregorian civil-from-days arithmetic via
/// [`civil_from_days`] (Unix time has no leap seconds, so integer
/// day/second arithmetic is exact). A `SystemTime` predating the Unix
/// epoch — vanishingly unlikely on any real system clock — renders as
/// the epoch itself rather than panicking.
pub fn iso8601_utc_millis(t: SystemTime) -> String {
    let since_epoch = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    // CAST: u128 → i64, millis-since-epoch fits comfortably (i64 spans
    // ~292 million years either side of 1970; any real SystemTime is
    // absurdly far inside that range).
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let total_ms = since_epoch.as_millis() as i64;
    let ms = total_ms.rem_euclid(1000);
    let total_secs = total_ms.div_euclid(1000);
    let secs_of_day = total_secs.rem_euclid(86_400);
    let days = total_secs.div_euclid(86_400);

    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{ms:03}Z")
}

/// Proleptic-Gregorian civil date `(year, month, day)` from a day count
/// relative to the Unix epoch (`1970-01-01` = day `0`).
///
/// Howard Hinnant's `civil_from_days` algorithm
/// (<https://howardhinnant.github.io/date_algorithms.html#civil_from_days>,
/// public domain) — pure integer arithmetic, no floating point, exact
/// over a domain far wider than the narrow range
/// [`iso8601_utc_millis`] actually feeds it (any `z` reachable from a
/// real `SystemTime`, `unwrap_or_default`-clamped to `>= 0`). Not
/// exact over the *entire* `i64` domain: `z + 719_468` overflows (and
/// therefore panics in a debug build) for `z` within `719_468` of
/// `i64::MAX`, a range this `const fn`'s signature doesn't itself rule
/// out for a hypothetical wider caller.
// EXPLICIT: `doe` (day-of-era) and `doy` (day-of-year) are Hinnant's own
// algorithm variable names, kept verbatim from the reference for
// traceability against the linked writeup — renaming them to appease
// `similar_names` would make this harder to audit against its source,
// not easier to read.
#[allow(clippy::similar_names)]
const fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    // CAST: i64 → u64, `doe` is in [0, 146096] by construction (`era`'s
    // division above pins `z - era*146097` into exactly that range).
    #[allow(clippy::as_conversions, clippy::cast_sign_loss)]
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    // CAST: u64 → i64, `yoe` is in [0, 399] by construction.
    #[allow(clippy::as_conversions, clippy::cast_possible_wrap)]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    // CAST: u64 → u32, day-of-month derived from `doy`/`mp` is in [1, 31].
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    // CAST: u64 → u32, `mp` is in [0, 11], so month is in [1, 12].
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Parse a `--interval` / `--duration` value: digits followed by an
/// optional unit (`ms`, `s`, `m`, `h`); bare digits mean seconds. Used
/// as a clap `value_parser`, so a parse failure surfaces as a normal
/// `--help`-style clap usage error (`String` satisfies clap's error
/// bound via the standard library's `impl From<String> for Box<dyn
/// Error + Send + Sync>`).
pub fn parse_duration(s: &str) -> std::result::Result<Duration, String> {
    let trimmed = s.trim();
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (digits, unit) = trimmed.split_at(split_at);
    if digits.is_empty() {
        return Err(format!(
            "invalid duration {s:?}: expected digits followed by an optional unit (ms, s, m, h)"
        ));
    }
    let value: u64 = digits
        .parse()
        .map_err(|_| format!("invalid duration {s:?}: {digits:?} is not a whole number"))?;
    let millis = match unit {
        "" | "s" => value.saturating_mul(1_000),
        "ms" => value,
        "m" => value.saturating_mul(60_000),
        "h" => value.saturating_mul(3_600_000),
        other => {
            return Err(format!(
                "invalid duration {s:?}: unknown unit {other:?} (expected ms, s, m, or h)"
            ));
        }
    };
    if millis == 0 {
        return Err(format!("invalid duration {s:?}: must be greater than zero"));
    }
    Ok(Duration::from_millis(millis))
}

/// Parse a `--min <SIZE>` (`hmn ps`) / `fits <SIZE>` (`hmn fits`) value:
/// digits (optionally with a decimal point and more digits) followed
/// by an optional unit (`KiB`, `MiB`, `GiB`) — the exact spellings
/// [`format_vram`]
/// prints, so what the tool shows is always what it accepts back,
/// including the space `format_vram` always puts before the unit
/// (`"512 MiB"`, `"8.0 GiB"`) — a `--min "$(hmn ps ... )"`-style copy
/// from `hmn`'s own output round-trips, not just the no-space form. A
/// bare number means bytes. Used as a clap `value_parser`, so a parse
/// failure surfaces as a normal `--help`-style clap usage error
/// (`String` satisfies clap's error bound). Unlike [`parse_duration`],
/// `0` is accepted — `--min 0` is a valid (if useless) no-op filter.
pub fn parse_size_bytes(s: &str) -> std::result::Result<u64, String> {
    let trimmed = s.trim();
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split_at);
    if number.is_empty() {
        return Err(format!(
            "invalid size {s:?}: expected digits (optionally with a decimal point and more digits) followed by an optional unit (KiB, MiB, GiB)"
        ));
    }
    // format_vram always separates the number from the unit with a
    // space ("512 MiB", "8.0 GiB") — trim it so hmn's own VRAM/SHARED
    // output round-trips straight back into --min/fits, not just the
    // no-space form.
    let unit = unit.trim_start();
    // Only ASCII digits and `.` ever reach `number` (the split above
    // excludes everything else), so this can't parse as the *literal*
    // `inf`/`NaN`/negative — but a long enough digit string still
    // overflows to `f64::INFINITY` (`f64::from_str` saturates rather
    // than erroring on magnitude overflow), so `is_finite` below is
    // load-bearing, not defensive filler: without it a fat-fingered
    // extra digit silently becomes `u64::MAX` instead of a usage error.
    let value: f64 = number
        .parse()
        .map_err(|_| format!("invalid size {s:?}: {number:?} is not a number"))?;
    if !value.is_finite() {
        return Err(format!(
            "invalid size {s:?}: {number:?} is too large to represent"
        ));
    }
    let multiplier: u64 = match unit {
        "" => 1,
        "KiB" => KIB,
        "MiB" => MIB,
        "GiB" => GIB,
        other => {
            return Err(format!(
                "invalid size {s:?}: unknown unit {other:?} (expected KiB, MiB, or GiB)"
            ));
        }
    };
    // CAST: f64 → u64, `value * multiplier` is bounded by realistic
    // VRAM/RAM sizes, far below f64's exact-integer ceiling (2^53
    // bytes ≈ 8 PiB) — `.round()` first so the cast never truncates a
    // near-integer float down by one from floating-point rounding.
    #[allow(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    let bytes = (value * multiplier as f64).round() as u64;
    Ok(bytes)
}

/// Parse one `--filter` pattern: any non-blank string, kept as typed. A
/// blank pattern would match every name — almost certainly a scripting
/// mistake (an unset variable), so it is rejected rather than accepted as
/// a silent no-op.
///
/// # Errors
///
/// Returns an error message when `s` is empty or whitespace only.
pub fn parse_filter_pattern(s: &str) -> std::result::Result<String, String> {
    if s.trim().is_empty() {
        return Err(format!(
            "invalid filter {s:?}: expected a non-blank name pattern"
        ));
    }
    // BORROW: explicit to_owned — clap stores the parsed value.
    Ok(s.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // --- format_vram ---

    #[test]
    fn format_vram_sub_gib() {
        assert_eq!(format_vram(0), "0 MiB");
        assert_eq!(format_vram(1024 * 1024), "1 MiB");
        assert_eq!(format_vram(512 * 1024 * 1024), "512 MiB");
    }

    #[test]
    fn format_vram_gib_one_decimal() {
        let one_gib = 1024_u64 * 1024 * 1024;
        assert_eq!(format_vram(one_gib), "1.0 GiB");
        // 1.5 GiB
        assert_eq!(format_vram(one_gib + one_gib / 2), "1.5 GiB");
        // ≈ 8.2 GiB (8 * 1024^3 + 200 * 1024^2 = 8 * GiB + 200 MiB).
        // 200 / 1024 = 0.1953... → renders as 8.2 GiB after one-decimal
        // rounding (matches the roadmap example output).
        let bytes_8_2_gib = 8 * one_gib + 200 * 1024 * 1024;
        assert_eq!(format_vram(bytes_8_2_gib), "8.2 GiB");
    }

    // --- format_vram_precise ---

    #[test]
    fn format_vram_precise_never_rounds_a_nonzero_value_to_zero() {
        // The exact bug format_vram has: 512 KiB is well under 1 MiB,
        // so format_vram(524_288) == "0 MiB" — indistinguishable from
        // the documented --min 0 no-op.
        assert_eq!(format_vram_precise(524_288), "512 KiB");
        assert_eq!(format_vram_precise(0), "0 B");
        assert_eq!(format_vram_precise(1), "1 B");
        assert_eq!(format_vram_precise(1023), "1023 B");
    }

    #[test]
    fn format_vram_precise_exact_tier_values_trim_trailing_zeros() {
        assert_eq!(format_vram_precise(50 * MIB), "50 MiB");
        assert_eq!(format_vram_precise(12 * GIB), "12 GiB");
        assert_eq!(format_vram_precise(KIB), "1 KiB");
    }

    #[test]
    fn format_vram_precise_disambiguates_a_format_vram_near_miss() {
        // format_vram(12_873_164_472) == format_vram(12 * GIB) ==
        // "12.0 GiB" for both — the exact hmn-fits self-contradiction
        // finding. format_vram_precise must show them as different.
        let a = 12 * GIB;
        let b = 12_873_164_472_u64;
        assert_eq!(format_vram(a), format_vram(b));
        assert_ne!(format_vram_precise(a), format_vram_precise(b));
    }

    // --- device_name_suffix ---

    #[test]
    fn device_name_suffix_some_and_none() {
        assert_eq!(device_name_suffix(Some("RTX 5060 Ti")), " [RTX 5060 Ti]");
        assert_eq!(device_name_suffix(None), "");
    }

    // --- bytes_to_mib ---

    #[test]
    fn bytes_to_mib_basic() {
        assert_eq!(bytes_to_mib(0), 0);
        assert_eq!(bytes_to_mib(1_048_576), 1);
        assert_eq!(bytes_to_mib(16_384 * 1_048_576), 16_384);
    }

    // --- spill and PAGED cells ---

    #[test]
    fn spill_cell_for_renders_n_a_where_spill_cannot_exist() {
        // The unknown glyph follows the platform flag...
        assert_eq!(spill_cell_for(None, None, true), "n/a");
        assert_eq!(spill_cell_for(None, None, false), "?");
        // ...and the measured verdicts do not depend on it.
        for flag in [true, false] {
            assert_eq!(spill_cell_for(Some(true), Some(true), flag), "PAGED");
            assert_eq!(spill_cell_for(Some(true), Some(false), flag), "device");
            assert_eq!(spill_cell_for(Some(false), Some(false), flag), "no");
        }
    }

    #[test]
    fn paged_cell_for_renders_n_a_where_spill_cannot_exist() {
        assert_eq!(paged_cell_for(None, true), "n/a");
        assert_eq!(paged_cell_for(None, false), "?");
        for flag in [true, false] {
            assert_eq!(paged_cell_for(Some(true), flag), "yes");
            assert_eq!(paged_cell_for(Some(false), flag), "no");
        }
    }

    #[test]
    fn spill_cell_wrapper_follows_the_compile_time_platform() {
        // Literal per-platform expectations, not computed from
        // SPILL_CANNOT_EXIST (that would be circular). "Can't tell" never
        // renders as "measured, not spilling" on either platform.
        #[cfg(windows)]
        {
            assert_eq!(spill_cell(None, None), "?");
            assert_eq!(paged_cell(None), "?");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(spill_cell(None, None), "n/a");
            assert_eq!(paged_cell(None), "n/a");
        }
    }

    // --- remedy_text ---

    #[test]
    fn remedy_text_outside_sandbox_keeps_to_identify() {
        assert_eq!(
            remedy_text(true, RemedyPurpose::Identify),
            "re-run outside the sandbox to identify"
        );
    }

    #[test]
    fn remedy_text_elevated_keeps_the_v0_2_13_wording() {
        assert_eq!(
            remedy_text(false, RemedyPurpose::Names),
            "re-run elevated for names"
        );
        assert_eq!(
            remedy_text(false, RemedyPurpose::Identify),
            "re-run elevated to identify"
        );
    }

    #[test]
    fn remedy_outside_sandbox_for_names_is_the_bare_macos_remedy() {
        assert_eq!(
            remedy_text(true, RemedyPurpose::Names),
            "re-run outside the sandbox"
        );
    }

    // --- failure_detail ---

    #[test]
    fn failure_detail_appends_the_remedy_to_a_denial_only() {
        assert_eq!(
            failure_detail(&HypomnesisError::ProcessListDenied { denied: 908 }, true),
            "process list unreadable (908 refused, none other than the caller's could be read) \
             — re-run outside the sandbox"
        );
        assert_eq!(
            failure_detail(
                &HypomnesisError::DeviceIndexOutOfRange { index: 3, count: 1 },
                true
            ),
            "device index 3 out of range (have 1 devices)"
        );
        // The error a macos-latest VM gives: no remedy either.
        let no_source = failure_detail(&HypomnesisError::NoGpuSource, true);
        assert!(
            no_source.starts_with("no GPU measurement source available")
                && !no_source.contains("re-run"),
            "{no_source}"
        );
    }

    // --- column_width ---

    #[test]
    fn table_min_widths_pad_narrow_columns_but_never_truncate() {
        let mut table = Table::new(&["A", "B"]).with_min_widths(&[4, 2]);
        table.push_row(vec!["x".to_owned(), "long".to_owned()]);
        assert_eq!(table.render(Some(""), ""), "A     B   \nx     long\n");
    }

    #[test]
    fn column_width_picks_max() {
        assert_eq!(column_width("PID", ["1", "12345"]), 5);
        assert_eq!(column_width("HEADER", ["a", "bc"]), 6);
        assert_eq!(column_width("PID", std::iter::empty::<&str>()), 3);
    }

    // --- Table ---

    #[test]
    fn table_render_pads_every_column_including_the_last() {
        let mut t = Table::new(&["A", "LONG"]);
        t.push_row(vec!["xyz".to_owned(), "1".to_owned()]);
        assert_eq!(t.render(Some("> "), ". "), "> A    LONG\n. xyz  1   \n");
    }

    #[test]
    fn table_render_without_header_still_sizes_columns_to_headers() {
        // `hmn watch` rows print no header line, yet stay as wide as the
        // header its caller printed once up front.
        let mut t = Table::new(&["WIDE_HEADER", "B"]);
        t.push_row(vec!["x".to_owned()]); // short row: missing cell renders empty
        // Column 0 is `WIDE_HEADER`-wide (11); column 1 is `B`-wide (1) and
        // its missing cell still pads to that width.
        assert_eq!(t.render(None, ""), format!("{:<11}  {:<1}\n", "x", ""));
        assert_eq!(Table::new(&["A"]).render(None, ""), "");
    }

    // --- json_string / json_string_or_null / json_value_or_null ---

    #[test]
    fn json_string_quotes_and_escapes() {
        assert_eq!(json_string(""), "\"\"");
        assert_eq!(json_string("a.exe"), "\"a.exe\"");
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn json_string_or_null_quotes_escapes_or_nulls() {
        assert_eq!(json_string_or_null(None), "null");
        assert_eq!(json_string_or_null(Some("a.exe")), "\"a.exe\"");
        assert_eq!(json_string_or_null(Some("a\"b")), "\"a\\\"b\"");
    }

    #[test]
    fn json_value_or_null_renders_bools_and_integers() {
        assert_eq!(json_value_or_null(Some(true)), "true");
        assert_eq!(json_value_or_null(Some(false)), "false");
        assert_eq!(json_value_or_null(None::<bool>), "null");
        assert_eq!(json_value_or_null(Some(42_u64)), "42");
    }

    // --- json_escape ---

    #[test]
    fn json_escape_passthrough() {
        assert_eq!(json_escape("python.exe"), "python.exe");
    }

    #[test]
    fn json_escape_quotes_and_backslash() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
    }

    #[test]
    fn json_escape_control_chars() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        // 0x01 is a control char without a short escape — 
        assert_eq!(json_escape("\u{0001}"), "\\u0001");
    }

    // --- duration helpers ---

    #[test]
    fn duration_ms_and_format_secs() {
        assert_eq!(duration_ms(Duration::from_millis(3_800)), 3_800);
        assert_eq!(duration_ms(Duration::ZERO), 0);
        assert_eq!(format_secs(Duration::from_millis(3_800)), "3.8s");
        assert_eq!(format_secs(Duration::ZERO), "0.0s");
    }

    // --- parse_duration ---

    #[test]
    fn parse_duration_bare_number_is_seconds() {
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn parse_duration_seconds_suffix() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn parse_duration_ms_suffix() {
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    }

    #[test]
    fn parse_duration_minutes_suffix() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
    }

    #[test]
    fn parse_duration_hours_suffix() {
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3_600));
    }

    #[test]
    fn parse_duration_rejects_empty() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("s").is_err());
    }

    #[test]
    fn parse_duration_rejects_unknown_unit() {
        assert!(parse_duration("30x").is_err());
    }

    #[test]
    fn parse_duration_rejects_zero() {
        assert!(parse_duration("0").is_err());
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("0ms").is_err());
    }

    #[test]
    fn parse_duration_trims_whitespace() {
        assert_eq!(parse_duration(" 30s ").unwrap(), Duration::from_secs(30));
    }

    // --- parse_filter_pattern ---

    #[test]
    fn parse_filter_pattern_rejects_blank() {
        assert_eq!(parse_filter_pattern("train").unwrap(), "train");
        assert!(parse_filter_pattern("").is_err());
        assert!(parse_filter_pattern("   ").is_err());
    }

    // --- parse_size_bytes ---

    #[test]
    fn parse_size_bytes_bare_number_is_bytes() {
        assert_eq!(parse_size_bytes("1048576").unwrap(), 1_048_576);
    }

    #[test]
    fn parse_size_bytes_kib_mib_gib_units() {
        assert_eq!(parse_size_bytes("50KiB").unwrap(), 50 * 1024);
        assert_eq!(parse_size_bytes("50MiB").unwrap(), 50 * 1024 * 1024);
        assert_eq!(parse_size_bytes("12GiB").unwrap(), 12 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_bytes_decimal_value() {
        assert_eq!(
            parse_size_bytes("1.5GiB").unwrap(),
            1024 * 1024 * 1024 + 512 * 1024 * 1024
        );
    }

    #[test]
    fn parse_size_bytes_zero_is_accepted() {
        // Unlike parse_duration, 0 is a valid (if useless) no-op filter.
        assert_eq!(parse_size_bytes("0").unwrap(), 0);
        assert_eq!(parse_size_bytes("0MiB").unwrap(), 0);
    }

    #[test]
    fn parse_size_bytes_rejects_unknown_unit() {
        assert!(parse_size_bytes("50TiB").is_err());
        assert!(parse_size_bytes("50mib").is_err()); // case-sensitive, matches format_vram's own casing
    }

    #[test]
    fn parse_size_bytes_rejects_empty_and_non_numeric() {
        assert!(parse_size_bytes("").is_err());
        assert!(parse_size_bytes("GiB").is_err());
        assert!(parse_size_bytes("-5MiB").is_err());
        assert!(parse_size_bytes("1.2.3MiB").is_err());
    }

    #[test]
    fn parse_size_bytes_trims_whitespace() {
        assert_eq!(parse_size_bytes(" 50MiB ").unwrap(), 50 * 1024 * 1024);
    }

    #[test]
    fn parse_size_bytes_round_trips_format_vram_output() {
        // format_vram always puts a space before the unit ("512 MiB",
        // "8.0 GiB") — a value copied straight from hmn's own VRAM/
        // SHARED column must parse back, not just the no-space form.
        assert_eq!(parse_size_bytes("512 MiB").unwrap(), 512 * 1024 * 1024);
        assert_eq!(parse_size_bytes("8.0 GiB").unwrap(), 8 * 1024 * 1024 * 1024);
        assert_eq!(format_vram(parse_size_bytes("512 MiB").unwrap()), "512 MiB");
    }

    // --- iso8601_utc_millis / civil_from_days ---

    #[test]
    fn iso8601_utc_millis_epoch() {
        assert_eq!(
            iso8601_utc_millis(SystemTime::UNIX_EPOCH),
            "1970-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn iso8601_utc_millis_known_recent_date() {
        // 2024-09-14T10:12:03.482Z, cross-checked against
        // `[DateTimeOffset]::FromUnixTimeMilliseconds(1726308723482).UtcDateTime`.
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_726_308_723_482);
        assert_eq!(iso8601_utc_millis(t), "2024-09-14T10:12:03.482Z");
    }

    #[test]
    fn iso8601_utc_millis_leap_day() {
        // 2024-02-29T00:00:00Z = 19782 days since epoch.
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(19_782 * 86_400);
        assert_eq!(iso8601_utc_millis(t), "2024-02-29T00:00:00.000Z");
    }

    #[test]
    fn iso8601_utc_millis_century_non_leap_year_boundary() {
        // 2000 IS a leap year (divisible by 400) — 2000-02-29 exists;
        // 1900 is NOT (divisible by 100, not 400) — no 1900-02-29. This
        // pins civil_from_days to the Gregorian rule, not a naive
        // "divisible by 4" leap check. 2000-02-29T00:00:00Z = 11016 days.
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(11_016 * 86_400);
        assert_eq!(iso8601_utc_millis(t), "2000-02-29T00:00:00.000Z");
        // One day later rolls over to March, confirming the leap day
        // was actually inserted rather than skipped.
        let t_next = SystemTime::UNIX_EPOCH + Duration::from_secs(11_017 * 86_400);
        assert_eq!(iso8601_utc_millis(t_next), "2000-03-01T00:00:00.000Z");
    }

    #[test]
    fn iso8601_utc_millis_end_of_year_rollover() {
        // 2023-12-31T23:59:59Z, one second before 2024-01-01T00:00:00Z
        // (day 19723, verified independently via PowerShell's
        // [DateTimeOffset]/Get-Date arithmetic).
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(19_723 * 86_400 - 1);
        assert_eq!(iso8601_utc_millis(t), "2023-12-31T23:59:59.000Z");
    }

    #[test]
    fn iso8601_utc_millis_millisecond_zero_padding() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(5);
        assert_eq!(iso8601_utc_millis(t), "1970-01-01T00:00:00.005Z");
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(50);
        assert_eq!(iso8601_utc_millis(t), "1970-01-01T00:00:00.050Z");
    }
}
