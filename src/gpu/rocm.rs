// SPDX-License-Identifier: MIT OR Apache-2.0

//! `ROCm` SMI backend (Linux).
//!
//! Dynamically loads `librocm_smi64.so.1` via `libloading` and measures
//! AMD GPUs the way the `NVML` backend measures NVIDIA ones, behind the
//! same crate-internal entry points used by the dispatchers in
//! `src/gpu/mod.rs`. The library is found through the dynamic loader by
//! its soname; no install path is coded.
//!
//! Device indices are the library's own enumeration, `0..n`, over the
//! devices it reports.
//!
//! # Return codes
//!
//! Every `rsmi_status_t` value is named here, so a failed call reports
//! `<call> returned <RSMI_STATUS_NAME> (<code>)` ([`call_failure`]).

#![allow(dead_code)]

/// `ROCm` SMI return code: the call succeeded.
const RSMI_STATUS_SUCCESS: u32 = 0;

/// `ROCm` SMI return code: an argument is not valid.
const RSMI_STATUS_INVALID_ARGS: u32 = 1;

/// `ROCm` SMI return code: the information or action is not available for the input, on this system.
const RSMI_STATUS_NOT_SUPPORTED: u32 = 2;

/// `ROCm` SMI return code: a file could not be accessed, possibly because the kernel does not support the operation.
const RSMI_STATUS_FILE_ERROR: u32 = 3;

/// `ROCm` SMI return code: permission denied; many calls need root.
const RSMI_STATUS_PERMISSION: u32 = 4;

/// `ROCm` SMI return code: memory or another resource could not be acquired.
const RSMI_STATUS_OUT_OF_RESOURCES: u32 = 5;

/// `ROCm` SMI return code: the library caught an internal exception.
const RSMI_STATUS_INTERNAL_EXCEPTION: u32 = 6;

/// `ROCm` SMI return code: the input is outside the allowable range.
const RSMI_STATUS_INPUT_OUT_OF_BOUNDS: u32 = 7;

/// `ROCm` SMI return code: an error occurred while the library initialized its internal data structures (also spelled `RSMI_INITIALIZATION_ERROR`).
const RSMI_STATUS_INIT_ERROR: u32 = 8;

/// `ROCm` SMI return code: the function is not yet implemented for the current devices.
const RSMI_STATUS_NOT_YET_IMPLEMENTED: u32 = 9;

/// `ROCm` SMI return code: an item was searched for and not found.
const RSMI_STATUS_NOT_FOUND: u32 = 10;

/// `ROCm` SMI return code: not enough resources or buffer space for the operation.
const RSMI_STATUS_INSUFFICIENT_SIZE: u32 = 11;

/// `ROCm` SMI return code: an interrupt occurred during the call.
const RSMI_STATUS_INTERRUPT: u32 = 12;

/// `ROCm` SMI return code: an unexpected amount of data was read.
const RSMI_STATUS_UNEXPECTED_SIZE: u32 = 13;

/// `ROCm` SMI return code: no data was found for the input.
const RSMI_STATUS_NO_DATA: u32 = 14;

/// `ROCm` SMI return code: the data read or provided is not what was expected.
const RSMI_STATUS_UNEXPECTED_DATA: u32 = 15;

/// `ROCm` SMI return code: a resource or mutex could not be acquired because it is in use.
const RSMI_STATUS_BUSY: u32 = 16;

/// `ROCm` SMI return code: an internal reference counter exceeded `INT32_MAX`.
const RSMI_STATUS_REFCOUNT_OVERFLOW: u32 = 17;

/// `ROCm` SMI return code: the requested setting is unavailable for the device.
const RSMI_STATUS_SETTING_UNAVAILABLE: u32 = 18;

/// `ROCm` SMI return code: the `amdgpu` driver could not be restarted.
const RSMI_STATUS_AMDGPU_RESTART_ERR: u32 = 19;

/// `ROCm` SMI return code: a `libdrm` call failed.
const RSMI_STATUS_DRM_ERROR: u32 = 20;

/// `ROCm` SMI return code: a library failed to load.
const RSMI_STATUS_FAIL_LOAD_MODULE: u32 = 21;

/// `ROCm` SMI return code: a symbol failed to load.
const RSMI_STATUS_FAIL_LOAD_SYMBOL: u32 = 22;

/// `ROCm` SMI return code: an unknown error occurred.
const RSMI_STATUS_UNKNOWN_ERROR: u32 = 0xFFFF_FFFF;

/// The `RSMI_STATUS_*` name of a `ROCm` SMI return code. Transcribed from
/// `rsmi_status_t` in `rocm_smi.h` (`ROCm` 7.2.4). A code this table does not
/// know, such as a newer library's new one, is named the generic
/// `RSMI_STATUS`; [`call_failure`] still prints its number.
#[must_use]
const fn return_code_name(code: u32) -> &'static str {
    match code {
        RSMI_STATUS_SUCCESS => "RSMI_STATUS_SUCCESS",
        RSMI_STATUS_INVALID_ARGS => "RSMI_STATUS_INVALID_ARGS",
        RSMI_STATUS_NOT_SUPPORTED => "RSMI_STATUS_NOT_SUPPORTED",
        RSMI_STATUS_FILE_ERROR => "RSMI_STATUS_FILE_ERROR",
        RSMI_STATUS_PERMISSION => "RSMI_STATUS_PERMISSION",
        RSMI_STATUS_OUT_OF_RESOURCES => "RSMI_STATUS_OUT_OF_RESOURCES",
        RSMI_STATUS_INTERNAL_EXCEPTION => "RSMI_STATUS_INTERNAL_EXCEPTION",
        RSMI_STATUS_INPUT_OUT_OF_BOUNDS => "RSMI_STATUS_INPUT_OUT_OF_BOUNDS",
        RSMI_STATUS_INIT_ERROR => "RSMI_STATUS_INIT_ERROR",
        RSMI_STATUS_NOT_YET_IMPLEMENTED => "RSMI_STATUS_NOT_YET_IMPLEMENTED",
        RSMI_STATUS_NOT_FOUND => "RSMI_STATUS_NOT_FOUND",
        RSMI_STATUS_INSUFFICIENT_SIZE => "RSMI_STATUS_INSUFFICIENT_SIZE",
        RSMI_STATUS_INTERRUPT => "RSMI_STATUS_INTERRUPT",
        RSMI_STATUS_UNEXPECTED_SIZE => "RSMI_STATUS_UNEXPECTED_SIZE",
        RSMI_STATUS_NO_DATA => "RSMI_STATUS_NO_DATA",
        RSMI_STATUS_UNEXPECTED_DATA => "RSMI_STATUS_UNEXPECTED_DATA",
        RSMI_STATUS_BUSY => "RSMI_STATUS_BUSY",
        RSMI_STATUS_REFCOUNT_OVERFLOW => "RSMI_STATUS_REFCOUNT_OVERFLOW",
        RSMI_STATUS_SETTING_UNAVAILABLE => "RSMI_STATUS_SETTING_UNAVAILABLE",
        RSMI_STATUS_AMDGPU_RESTART_ERR => "RSMI_STATUS_AMDGPU_RESTART_ERR",
        RSMI_STATUS_DRM_ERROR => "RSMI_STATUS_DRM_ERROR",
        RSMI_STATUS_FAIL_LOAD_MODULE => "RSMI_STATUS_FAIL_LOAD_MODULE",
        RSMI_STATUS_FAIL_LOAD_SYMBOL => "RSMI_STATUS_FAIL_LOAD_SYMBOL",
        RSMI_STATUS_UNKNOWN_ERROR => "RSMI_STATUS_UNKNOWN_ERROR",
        _ => "RSMI_STATUS",
    }
}

/// The detail of a `ROCm` SMI call that returned `code`:
/// `<call> returned <RSMI_STATUS_NAME> (<code>)`, or
/// `<call> returned RSMI_STATUS (<code>)` for a code [`return_code_name`]
/// does not know.
#[must_use]
fn call_failure(call: &str, code: u32) -> String {
    let name = return_code_name(code);
    format!("{call} returned {name} ({code})")
}

/// Per-process information returned by `ROCm` SMI.
///
/// Matches the C struct `rsmi_process_info_t` (32 bytes, 8-byte aligned)
/// used by `rsmi_compute_process_info_get` and
/// `rsmi_compute_process_info_by_device_get`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RsmiProcessInfo {
    /// Process ID.
    process_id: u32,
    /// `PASID` of the process.
    pasid: u32,
    /// `VRAM` used by the process in bytes. Zero in every row of
    /// `rsmi_compute_process_info_get`; `rsmi_compute_process_info_by_device_get`
    /// fills it per device.
    vram_usage: u64,
    /// `SDMA` usage in microseconds.
    sdma_usage: u64,
    /// Compute unit usage in percent.
    cu_occupancy: u32,
}

// -----------------------------------------------------------------------
// Tests (pure, no FFI)
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_code_name_names_every_status() {
        for (constant, code, name) in [
            (RSMI_STATUS_SUCCESS, 0, "RSMI_STATUS_SUCCESS"),
            (RSMI_STATUS_INVALID_ARGS, 1, "RSMI_STATUS_INVALID_ARGS"),
            (RSMI_STATUS_NOT_SUPPORTED, 2, "RSMI_STATUS_NOT_SUPPORTED"),
            (RSMI_STATUS_FILE_ERROR, 3, "RSMI_STATUS_FILE_ERROR"),
            (RSMI_STATUS_PERMISSION, 4, "RSMI_STATUS_PERMISSION"),
            (
                RSMI_STATUS_OUT_OF_RESOURCES,
                5,
                "RSMI_STATUS_OUT_OF_RESOURCES",
            ),
            (
                RSMI_STATUS_INTERNAL_EXCEPTION,
                6,
                "RSMI_STATUS_INTERNAL_EXCEPTION",
            ),
            (
                RSMI_STATUS_INPUT_OUT_OF_BOUNDS,
                7,
                "RSMI_STATUS_INPUT_OUT_OF_BOUNDS",
            ),
            (RSMI_STATUS_INIT_ERROR, 8, "RSMI_STATUS_INIT_ERROR"),
            (
                RSMI_STATUS_NOT_YET_IMPLEMENTED,
                9,
                "RSMI_STATUS_NOT_YET_IMPLEMENTED",
            ),
            (RSMI_STATUS_NOT_FOUND, 10, "RSMI_STATUS_NOT_FOUND"),
            (
                RSMI_STATUS_INSUFFICIENT_SIZE,
                11,
                "RSMI_STATUS_INSUFFICIENT_SIZE",
            ),
            (RSMI_STATUS_INTERRUPT, 12, "RSMI_STATUS_INTERRUPT"),
            (
                RSMI_STATUS_UNEXPECTED_SIZE,
                13,
                "RSMI_STATUS_UNEXPECTED_SIZE",
            ),
            (RSMI_STATUS_NO_DATA, 14, "RSMI_STATUS_NO_DATA"),
            (
                RSMI_STATUS_UNEXPECTED_DATA,
                15,
                "RSMI_STATUS_UNEXPECTED_DATA",
            ),
            (RSMI_STATUS_BUSY, 16, "RSMI_STATUS_BUSY"),
            (
                RSMI_STATUS_REFCOUNT_OVERFLOW,
                17,
                "RSMI_STATUS_REFCOUNT_OVERFLOW",
            ),
            (
                RSMI_STATUS_SETTING_UNAVAILABLE,
                18,
                "RSMI_STATUS_SETTING_UNAVAILABLE",
            ),
            (
                RSMI_STATUS_AMDGPU_RESTART_ERR,
                19,
                "RSMI_STATUS_AMDGPU_RESTART_ERR",
            ),
            (RSMI_STATUS_DRM_ERROR, 20, "RSMI_STATUS_DRM_ERROR"),
            (
                RSMI_STATUS_FAIL_LOAD_MODULE,
                21,
                "RSMI_STATUS_FAIL_LOAD_MODULE",
            ),
            (
                RSMI_STATUS_FAIL_LOAD_SYMBOL,
                22,
                "RSMI_STATUS_FAIL_LOAD_SYMBOL",
            ),
            (
                RSMI_STATUS_UNKNOWN_ERROR,
                0xFFFF_FFFF,
                "RSMI_STATUS_UNKNOWN_ERROR",
            ),
        ] {
            assert_eq!(constant, code, "{name}");
            assert_eq!(return_code_name(code), name, "code {code}");
        }
        assert_eq!(return_code_name(23), "RSMI_STATUS");
        assert_eq!(return_code_name(0xFFFF_FFFE), "RSMI_STATUS");
    }

    #[test]
    fn call_failure_names_the_call_and_the_code() {
        assert_eq!(
            call_failure("rsmi_init", 8),
            "rsmi_init returned RSMI_STATUS_INIT_ERROR (8)"
        );
        assert_eq!(
            call_failure("rsmi_num_monitor_devices", 42),
            "rsmi_num_monitor_devices returned RSMI_STATUS (42)"
        );
    }

    #[test]
    fn rsmi_process_info_layout_matches_the_header() {
        // C `sizeof` / `offsetof` of `rsmi_process_info_t` in `rocm_smi.h`.
        assert_eq!(std::mem::size_of::<RsmiProcessInfo>(), 32);
        assert_eq!(std::mem::align_of::<RsmiProcessInfo>(), 8);
        assert_eq!(std::mem::offset_of!(RsmiProcessInfo, process_id), 0);
        assert_eq!(std::mem::offset_of!(RsmiProcessInfo, pasid), 4);
        assert_eq!(std::mem::offset_of!(RsmiProcessInfo, vram_usage), 8);
        assert_eq!(std::mem::offset_of!(RsmiProcessInfo, sdma_usage), 16);
        assert_eq!(std::mem::offset_of!(RsmiProcessInfo, cu_occupancy), 24);
    }
}
