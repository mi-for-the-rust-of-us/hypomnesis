// SPDX-License-Identifier: MIT OR Apache-2.0

//! `hmn` with no subcommand: one line — or, with `--json`, one JSON
//! object — per visible GPU, a stderr line for each device that could
//! not be read, and exit `2` when none could be read or the devices could
//! not be counted.

use std::fmt::Write as _;
use std::process::ExitCode;

use hypomnesis::{HypomnesisError, Snapshot, device_info};

use crate::format::{bytes_to_mib, device_name_suffix, json_string_or_null, json_value_or_null};

/// Whether `snaps` is non-empty but every entry's `gpu_device` is
/// `None` — devices enumerated, but `device_info` failed for each (e.g.
/// a partial driver install), so [`format_summary`] and
/// [`format_summary_json`] would print nothing, indistinguishable from
/// genuine zero-device enumeration.
fn all_gpu_devices_unreadable(snaps: &[Snapshot]) -> bool {
    !snaps.is_empty() && snaps.iter().all(|s| s.gpu_device.is_none())
}

/// The "devices enumerated but none readable" line, printed on stderr
/// before [`run_summary`] exits `2`, in text and `--json` mode alike.
fn format_none_readable_message(count: usize) -> String {
    format!("hmn: {count} GPU(s) enumerated but none readable.")
}

/// The stderr line for a device whose `device_info` failed:
/// `hmn: failed to query device <index>: <err> (skipped)`, `hmn ps`'s
/// skipped-device line without its `ps`.
#[must_use]
fn device_query_skip_line(index: u32, err: &impl std::fmt::Display) -> String {
    format!("hmn: failed to query device {index}: {err} (skipped)")
}

/// The stderr line when the devices could not be counted:
/// `hmn: failed to count devices: <err>`.
#[must_use]
fn count_failure_line(err: &impl std::fmt::Display) -> String {
    format!("hmn: failed to count devices: {err}")
}

/// Run the default subcommand: print one line per visible GPU (or, with
/// `--json`, a JSON array), and return the exit code.
///
/// - `0` when at least one device is shown, or none is visible
///   (`hmn: no visible GPUs.` on stdout, `[]` with `--json`): no count
///   source installed, or one that counts zero.
/// - A device whose `device_info` fails is skipped with
///   [`device_query_skip_line`] on stderr, and the others are shown, as
///   `hmn ps` skips one (since v0.2.15; v0.2.14 dropped it without a word).
/// - `2` with nothing on stdout, `--json` included, when no device could
///   be read ([`format_none_readable_message`]) or the devices could not
///   be counted ([`count_failure_line`]: an NVIDIA driver that is
///   installed but fails to start, `Snapshot::all`'s
///   `HypomnesisError::Nvml`), or `Snapshot::all` failed for another
///   reason (a `RAM` read; `hmn: <reason>`, which exited `1` through
///   `main`'s old `Ok`/`Err` fold). `1` is not used: `2` is `hmn ps`'s and
///   `hmn watch`'s "could not tell" too.
pub fn run_summary(json: bool) -> ExitCode {
    let mut snaps = match Snapshot::all() {
        Ok(snaps) => snaps,
        Err(e @ HypomnesisError::Nvml(_)) => {
            eprintln!("{}", count_failure_line(&e));
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("hmn: {e}");
            return ExitCode::from(2);
        }
    };
    // `Snapshot::all` keeps no error for a device whose `device_info`
    // failed, so ask once more for each such device: the second answer
    // names the failure, or shows a device that answers this time. Its
    // NVIDIA devices come first, in index order, so position is index; the
    // Windows non-NVIDIA adapters after them always carry `gpu_device`, so
    // this never asks about one.
    for (index, snap) in (0_u32..).zip(snaps.iter_mut()) {
        if snap.gpu_device.is_some() {
            continue;
        }
        match device_info(index) {
            Ok(info) => snap.gpu_device = Some(info),
            Err(e) => eprintln!("{}", device_query_skip_line(index, &e)),
        }
    }
    if all_gpu_devices_unreadable(&snaps) {
        // Nothing on stdout: an empty output or `[]` would read as a
        // machine without GPUs.
        eprintln!("{}", format_none_readable_message(snaps.len()));
        return ExitCode::from(2);
    }
    if json {
        print!("{}", format_summary_json(&snaps));
    } else if snaps.is_empty() {
        println!("hmn: no visible GPUs.");
    } else {
        print!("{}", format_summary(&snaps));
    }
    ExitCode::SUCCESS
}

/// Format the device summary, one line per snapshot that has a populated
/// `gpu_device`. Snapshots without one are devices [`run_summary`] could
/// not read and has already named on stderr; they are skipped.
fn format_summary(snaps: &[Snapshot]) -> String {
    let mut out = String::new();
    for snap in snaps {
        let Some(dev) = &snap.gpu_device else {
            continue;
        };
        let free_mib = bytes_to_mib(dev.free_bytes);
        let total_mib = bytes_to_mib(dev.total_bytes);
        let name_suffix = device_name_suffix(dev.name.as_deref());
        // Driver/firmware carve-out, when the backend surfaced it (NVML
        // R510+). It is a *subset* of `total_mib` (NVML's
        // `total = reserved + free + used`), so the parenthetical reads as
        // "of which N is reserved", not an addition on top — matching
        // `nvidia-smi -q -d MEMORY`'s separate `Total` / `Reserved` lines.
        // Elided on backends that report `None` (DXGI, nvidia-smi, Metal,
        // pre-R510).
        let reserved_suffix = dev.reserved_bytes.map_or(String::new(), |r| {
            format!(" ({} MiB reserved)", bytes_to_mib(r))
        });
        // NVIDIA driver version (NVML or nvidia-smi fallback). Elided on
        // backends that don't expose an NVIDIA driver string (DXGI,
        // Metal, non-NVIDIA adapters).
        let driver_suffix = dev
            .driver_version
            .as_deref()
            .map_or(String::new(), |v| format!(", driver {v}"));
        // `writeln!` into a String never fails — the writes-to-String
        // impl returns Ok(()). Same for every other write!/writeln! in
        // this file.
        let _ = writeln!(
            out,
            "GPU {}{name_suffix}: free {free_mib} MiB / {total_mib} MiB{reserved_suffix}{driver_suffix}",
            dev.index,
        );
    }
    out
}

/// Format the device summary as a JSON array, one object per snapshot
/// that has a populated `gpu_device` (mirrors [`format_summary`]'s
/// skip rule for snapshots without one). Hand-rolled (no `serde` dep —
/// same policy as `ps::format_ps_json`). Each object:
/// `{"index":N,"name":<string|null>,"total_bytes":N,"free_bytes":N,"used_bytes":N,"reserved_bytes":<number|null>,"driver_version":<string|null>}`.
/// String values are JSON-escaped via [`json_string_or_null`].
fn format_summary_json(snaps: &[Snapshot]) -> String {
    let mut out = String::from("[");
    let mut first = true;
    for snap in snaps {
        let Some(dev) = &snap.gpu_device else {
            continue;
        };
        if first {
            first = false;
        } else {
            out.push(',');
        }
        let name_json = json_string_or_null(dev.name.as_deref());
        let reserved_json = json_value_or_null(dev.reserved_bytes);
        let driver_json = json_string_or_null(dev.driver_version.as_deref());
        let _ = write!(
            out,
            r#"{{"index":{},"name":{name_json},"total_bytes":{},"free_bytes":{},"used_bytes":{},"reserved_bytes":{reserved_json},"driver_version":{driver_json}}}"#,
            dev.index, dev.total_bytes, dev.free_bytes, dev.used_bytes,
        );
    }
    out.push_str("]\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- format_summary ---
    //
    // `Snapshot` and `GpuDeviceInfo` are `#[non_exhaustive]` and the
    // binary is a separate crate from the library, so struct-literal
    // construction is forbidden here. We test the only case that
    // doesn't require one: an empty input.

    #[test]
    fn format_summary_empty_input() {
        assert_eq!(format_summary(&[]), "");
    }

    #[test]
    fn format_summary_json_empty_input() {
        // Unlike format_summary's "" on empty input, the JSON formatter
        // always emits a parseable array — mirrors format_ps_json's
        // empty-input behavior ("[]\n").
        assert_eq!(format_summary_json(&[]), "[]\n");
    }

    // --- the stderr lines ---

    #[test]
    fn device_query_skip_line_is_hmn_ps_s_without_ps() {
        assert_eq!(
            device_query_skip_line(1, &"boom"),
            "hmn: failed to query device 1: boom (skipped)"
        );
    }

    #[test]
    fn count_failure_line_names_the_count() {
        assert_eq!(
            count_failure_line(&"NVML error: boom"),
            "hmn: failed to count devices: NVML error: boom"
        );
    }

    #[test]
    fn format_none_readable_message_states_the_count() {
        assert_eq!(
            format_none_readable_message(2),
            "hmn: 2 GPU(s) enumerated but none readable."
        );
    }
}
