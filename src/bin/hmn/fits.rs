// SPDX-License-Identifier: MIT OR Apache-2.0

//! `hmn fits <SIZE>`: "will this job fit right now" as a single gateable
//! exit code.

use hypomnesis::device_info;

use crate::format::{device_name_suffix, format_vram, format_vram_precise};

/// Run the `fits` subcommand: compare `size` against `device`'s current
/// free `VRAM` and print one line to stderr either way. `free_bytes`
/// nets out `reserved_bytes` **on the NVML path only** — `DXGI`-alone,
/// `nvidia-smi`, and Metal backends all leave `reserved_bytes: None`,
/// and on Windows without NVML `free_bytes` is derived from a
/// documented per-process *lower bound* on usage (see
/// [`hypomnesis::device_info`]'s "Imprecision note"), so it can
/// over-state true free `VRAM` there; on macOS it is `MTLDevice`'s static working-set
/// budget, not a live gauge. Returns its own exit code, as every
/// subcommand does: the code conveys the answer, not `hmn`'s own
/// success/failure.
pub fn run_fits(size: u64, device: u32) -> std::process::ExitCode {
    let info = match device_info(device) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("hmn: fits failed to query device {device}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let fits = size <= info.free_bytes;
    eprintln!(
        "{}",
        format_fits_message(fits, info.free_bytes, size, device, info.name.as_deref())
    );
    std::process::ExitCode::from(fits_exit_code(fits))
}

/// Build `run_fits`'s stderr line. A separate, testable function
/// because the two verdict-dependent pieces (the leading word and the
/// comparison glyph) must never desync — inlined `if fits {...}`
/// ternaries in an `eprintln!` argument list can drift apart with no
/// compiler or test catching it. Also states the exact margin via
/// [`format_vram_precise`], since [`format_vram`]'s one-decimal-place
/// `free`/`size` figures can independently round to the *same*
/// displayed string on a near-miss — printing `"12.0 GiB free < 12.0
/// GiB requested"` reads as self-contradictory even though the
/// underlying byte counts genuinely differ.
#[must_use]
fn format_fits_message(
    fits: bool,
    free_bytes: u64,
    size: u64,
    device: u32,
    name: Option<&str>,
) -> String {
    let (verdict, cmp) = if fits {
        ("fits", ">=")
    } else {
        ("does not fit", "<")
    };
    let margin = if fits {
        format!(
            "{} headroom",
            format_vram_precise(free_bytes.saturating_sub(size))
        )
    } else {
        format!(
            "short by {}",
            format_vram_precise(size.saturating_sub(free_bytes))
        )
    };
    format!(
        "hmn: {verdict} — {} free {cmp} {} requested ({margin}; device {device}{})",
        format_vram(free_bytes),
        format_vram(size),
        device_name_suffix(name),
    )
}

/// Map a `fits` boolean to `hmn`'s exit-code contract: `0` if it fits,
/// `1` if it doesn't. The `2` (hard error) case is returned directly
/// from [`run_fits`], bypassing this mapping — same split
/// `watch::watch_exit_code` uses.
const fn fits_exit_code(fits: bool) -> u8 {
    if fits { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::GIB;

    // --- format_fits_message ---

    #[test]
    fn format_fits_message_fits_shows_headroom() {
        let msg = format_fits_message(true, 14 * GIB, 12 * GIB, 0, Some("RTX 5060 Ti"));
        assert_eq!(
            msg,
            "hmn: fits — 14.0 GiB free >= 12.0 GiB requested (2 GiB headroom; device 0 [RTX 5060 Ti])"
        );
    }

    #[test]
    fn format_fits_message_does_not_fit_shows_shortfall() {
        let msg = format_fits_message(false, 12 * GIB, 14 * GIB, 1, None);
        assert_eq!(
            msg,
            "hmn: does not fit — 12.0 GiB free < 14.0 GiB requested (short by 2 GiB; device 1)"
        );
    }

    #[test]
    fn format_fits_message_near_miss_is_never_self_contradictory() {
        // The live-reproduced bug: free and size round to the same
        // GiB-1-decimal string, so the plain comparison alone would
        // print e.g. "12.0 GiB free < 12.0 GiB requested". The margin
        // clause must still make it unambiguous.
        let free = 12 * GIB;
        let size = 12_873_164_472_u64; // rounds to the same "12.0 GiB" via format_vram
        assert_eq!(format_vram(free), format_vram(size));
        let msg = format_fits_message(false, free, size, 0, None);
        assert!(
            !msg.contains("short by 0 GiB") && !msg.contains("short by 0 MiB"),
            "margin must disambiguate the near-miss: {msg}"
        );
    }

    #[test]
    fn format_fits_message_verdict_and_glyph_never_desync() {
        // A structural guard against exactly the "if fits {...}" /
        // "if fits {...}" desync finding: the verdict word and the
        // comparison glyph are read from the SAME function output, so
        // the source of truth is single, not two independent ternaries.
        for fits in [true, false] {
            let msg = format_fits_message(fits, 10, 10, 0, None);
            let is_fits_verdict = msg.starts_with("hmn: fits");
            let has_ge_glyph = msg.contains(">=");
            assert_eq!(is_fits_verdict, fits);
            assert_eq!(has_ge_glyph, fits);
        }
    }

    // --- fits_exit_code ---

    #[test]
    fn fits_exit_code_fits_and_does_not_fit() {
        assert_eq!(fits_exit_code(true), 0);
        assert_eq!(fits_exit_code(false), 1);
    }
}
