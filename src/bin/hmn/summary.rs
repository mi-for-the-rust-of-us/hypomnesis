// SPDX-License-Identifier: MIT OR Apache-2.0

//! `hmn` with no subcommand: one line — or, with `--json`, one JSON
//! object — per visible GPU, plus the "enumerated but none readable"
//! notice for a half-broken driver stack.

use std::fmt::Write as _;

use hypomnesis::{Result, Snapshot};

use crate::format::{bytes_to_mib, device_name_suffix, json_string_or_null, json_value_or_null};

/// Whether `snaps` is non-empty but every entry's `gpu_device` is
/// `None` — devices enumerated, but `device_info` failed for each (e.g.
/// a partial driver install). Shared by both [`run_summary`] paths:
/// [`format_summary`] and [`format_summary_json`] both silently skip
/// exactly these entries, so `[]`/no-output is otherwise indistinguishable
/// from genuine zero-device enumeration.
fn all_gpu_devices_unreadable(snaps: &[Snapshot]) -> bool {
    !snaps.is_empty() && snaps.iter().all(|s| s.gpu_device.is_none())
}

/// The "devices enumerated but none readable" diagnostic line, shared by
/// both [`run_summary`] call sites (stdout in text mode, stderr in
/// `--json` mode) so the wording can't drift between the two.
fn format_none_readable_message(count: usize) -> String {
    format!("hmn: {count} GPU(s) enumerated but none readable.")
}

/// Run the default subcommand: print one line per visible GPU (or, with
/// `--json`, a JSON array).
///
/// An NVIDIA driver that is installed but fails to start is
/// `Snapshot::all`'s `HypomnesisError::Nvml` (since v0.2.15), returned
/// here before anything is printed, so `main` reports it and exits `1`
/// rather than this printing `no visible GPUs.`, which is kept for a
/// machine with no count source installed or one that counts zero.
pub fn run_summary(json: bool) -> Result<()> {
    let snaps = Snapshot::all()?;
    if json {
        // The `[]` shape is identical whether zero devices were visible
        // or every visible device failed to read — distinguish the two
        // on stderr (mirrors `hmn ps`'s always-on stderr summary line)
        // without changing the documented JSON array shape on stdout.
        if all_gpu_devices_unreadable(&snaps) {
            eprintln!("{}", format_none_readable_message(snaps.len()));
        }
        print!("{}", format_summary_json(&snaps));
        return Ok(());
    }
    if snaps.is_empty() {
        println!("hmn: no visible GPUs.");
        return Ok(());
    }
    if all_gpu_devices_unreadable(&snaps) {
        // Printing nothing here would exit `0` with empty stdout,
        // indistinguishable from a working "nothing to show" success —
        // say so explicitly instead.
        println!("{}", format_none_readable_message(snaps.len()));
        return Ok(());
    }
    print!("{}", format_summary(&snaps));
    Ok(())
}

/// Format the device summary, one line per snapshot that has a populated
/// `gpu_device`. Snapshots without a `gpu_device` (e.g. RAM-only entries)
/// are skipped.
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
}
