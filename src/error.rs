// SPDX-License-Identifier: MIT OR Apache-2.0

//! Error types for `hypomnesis`.

/// Errors that can occur during a `hypomnesis` measurement.
///
/// `#[non_exhaustive]`: new variants will be added as new backends are introduced
/// (e.g., AMD `ROCm` SMI). Patch-release-safe.
///
/// # `Display` vs structured fields
///
/// `HypomnesisError`'s `Display` impl is the **default English one-liner** —
/// suitable for logs, library-tier error reporting, and `?`-propagation where
/// the consumer is content with the default rendering. Structured fields
/// ([`Self::DeviceIndexOutOfRange`]'s `index` / `count`,
/// [`Self::ProcessListDenied`]'s `denied`, the inner `String` of
/// [`Self::Nvml`] / [`Self::Dxgi`] / [`Self::Pdh`] / [`Self::NvidiaSmi`]) are
/// the **canonical source** for any consumer that wants to:
///
/// - Localize the message to a non-English language.
/// - Restyle for a CLI / GUI / JSON output (column-aligned tables,
///   wrap-aware formatting, JSON keys for the structured pieces).
/// - Apply singular / plural agreement, custom punctuation, or richer
///   formatting (`"have 1 device"` vs the default `"have 1 devices"`,
///   for instance).
///
/// This contract makes `Display` stable for the common case while leaving
/// custom-render consumers free to assemble their own strings without
/// fighting the default. Consumers writing user-facing tools should prefer
/// `match err { HypomnesisError::DeviceIndexOutOfRange { index, count } => ... }`
/// over `format!("{err}")`. Future `Display`-string improvements will avoid
/// adding structural information that the structured fields already expose
/// (so consumers that hand-format from the fields cannot end up
/// double-rendering the count).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum HypomnesisError {
    /// Process `RSS` query failed (platform API error).
    #[error("RAM query failed: {0}")]
    Ram(String),

    /// `NVML` query failed (library load, symbol lookup, FFI call,
    /// or driver-reported error code).
    #[error("NVML error: {0}")]
    Nvml(String),

    /// `DXGI` query failed (factory creation, adapter enumeration,
    /// `IDXGIAdapter3` cast, or interface call).
    #[error("DXGI error: {0}")]
    Dxgi(String),

    /// `PDH` (Windows Performance Data Helper) query failed (counter
    /// enumeration, counter add, value collection, or instance parsing
    /// for the `GPU Process Memory` or `GPU Adapter Memory` counter
    /// sets — the latter backs the v0.2.5 spill-detection path).
    /// Includes the case where the counter set is unregistered on
    /// pre-`WDDM 2.0` systems.
    #[error("PDH error: {0}")]
    Pdh(String),

    /// `nvidia-smi` subprocess invocation failed or produced unparseable output.
    #[error("nvidia-smi error: {0}")]
    NvidiaSmi(String),

    /// Requested device index is past the number of available GPUs.
    #[error("device index {index} out of range (have {count} devices)")]
    DeviceIndexOutOfRange {
        /// The requested zero-based index.
        index: u32,
        /// The number of available devices.
        count: u32,
    },

    /// No GPU measurement source was usable.
    ///
    /// Returned when `NVML`, `DXGI`, `PDH`, and `nvidia-smi` all failed
    /// (or were disabled by feature flags) for a single query. On macOS,
    /// `Metal` replaces `DXGI` and `PDH`, and the message names `Metal`,
    /// `NVML`, and `nvidia-smi`. On macOS, [`crate::gpu_processes`] and
    /// [`crate::gpu_process_listing`] also return it when the caller's
    /// sandbox refuses `proc_listpids` and no `sysctl` listing can stand in,
    /// for example when `KERN_PROC_ALL` is refused, or is allowed while
    /// `KERN_PROC_PID`, which vouches for its record size, is refused; the
    /// sandbox is then the cause, though the message names the backends.
    #[cfg_attr(
        target_os = "macos",
        error(
            "no GPU measurement source available (Metal, NVML, and nvidia-smi all failed or are disabled)"
        )
    )]
    #[cfg_attr(
        not(target_os = "macos"),
        error(
            "no GPU measurement source available (NVML, DXGI, PDH, and nvidia-smi all failed or are disabled)"
        )
    )]
    NoGpuSource,

    /// The processes were enumerated but none other than the caller's
    /// could be read, so the list says nothing about the machine.
    ///
    /// Returned on macOS, by [`crate::gpu_process_listing`] and
    /// [`crate::gpu_processes`], when at least one process was refused and
    /// no process other than the caller's could be read: the caller's
    /// sandbox refuses the ledger read of every other process, and under a
    /// profile that refuses it for every process, the caller's own too.
    /// `denied` is the number of processes refused, the caller excluded,
    /// saturating at `u32::MAX`.
    #[error(
        "process list unreadable ({denied} refused, none other than the caller's could be read)"
    )]
    ProcessListDenied {
        /// How many processes' GPU memory the caller was refused.
        denied: u32,
    },

    /// Generic I/O error.
    ///
    /// Reserved for a possible future I/O-based backend — this crate
    /// does not currently construct this variant itself: the one
    /// existing filesystem read (`/proc/self/status` on Linux) is
    /// deliberately wrapped into [`Self::Ram`] instead (see
    /// [`crate::Snapshot::now`]'s `# Errors` section), so downstream
    /// code should not expect to observe `Io` from this crate's public
    /// API today.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result alias for `hypomnesis` operations.
pub type Result<T> = std::result::Result<T, HypomnesisError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gpu_source_display_names_the_backends_of_the_platform() {
        let text = HypomnesisError::NoGpuSource.to_string();
        // Windows and Linux keep the v0.2.13 text byte for byte.
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            text,
            "no GPU measurement source available (NVML, DXGI, PDH, and nvidia-smi all failed or are disabled)"
        );
        // macOS names Metal and none of the Windows-only backends.
        #[cfg(target_os = "macos")]
        assert_eq!(
            text,
            "no GPU measurement source available (Metal, NVML, and nvidia-smi all failed or are disabled)"
        );
    }

    #[test]
    fn process_list_denied_display_states_the_count_and_no_remedy() {
        let text = HypomnesisError::ProcessListDenied { denied: 908 }.to_string();
        assert_eq!(
            text,
            "process list unreadable (908 refused, none other than the caller's could be read)"
        );
    }
}
