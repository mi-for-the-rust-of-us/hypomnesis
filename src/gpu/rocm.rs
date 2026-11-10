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

/// Path to the `ROCm` SMI shared library: the soname, resolved by the
/// dynamic loader, so no install path is coded.
const ROCM_LIB_PATH: &str = "librocm_smi64.so.1";

/// Function signature: `rsmi_init(init_flags)`.
type RsmiInitFn = unsafe extern "C" fn(u64) -> u32;

/// Function signature: `rsmi_shut_down()`.
type RsmiShutdownFn = unsafe extern "C" fn() -> u32;

/// Function signature: `rsmi_num_monitor_devices(*num_devices)`.
type RsmiNumMonitorDevicesFn = unsafe extern "C" fn(*mut u32) -> u32;

/// What a failed `rsmi_init` says about the GPUs, read from its return
/// code by [`classify_init_failure`]. The question is whether GPU processes
/// could exist that `ROCm` SMI cannot show: if not, the answer is "no
/// usable `ROCm` SMI"; if so, "can't tell".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitFailure {
    /// `RSMI_STATUS_INIT_ERROR`: the library found no AMD driver to
    /// initialize against (a host with the `ROCm` userland installed and no
    /// `amdgpu` module), so no process can be on its GPUs; read as no usable
    /// `ROCm` SMI, like a library that does not load.
    NoDriver,
    /// Any other code: GPU processes may exist that `ROCm` SMI cannot show.
    /// Can't tell.
    CannotTell,
}

/// The [`InitFailure`] an `rsmi_init` return `code` means: one code says no
/// GPU process can exist, and every other one, unknown codes included, is
/// "can't tell", the safe answer.
#[must_use]
const fn classify_init_failure(code: u32) -> InitFailure {
    match code {
        RSMI_STATUS_INIT_ERROR => InitFailure::NoDriver,
        _ => InitFailure::CannotTell,
    }
}

/// Why [`RocmSession::start`] gave no session.
enum StartFailure {
    /// No usable `ROCm` SMI: the library did not load (no `ROCm` userland
    /// here, or one whose library cannot be loaded), or `rsmi_init` said
    /// there is no AMD driver ([`InitFailure::NoDriver`]). Either way no GPU
    /// process can be measured, nor exist, through it.
    Absent,
    /// The library loaded but could not start, and GPUs may be there,
    /// unreadable: `rsmi_init` or `rsmi_shut_down` is missing, or
    /// `rsmi_init` returned a [`InitFailure::CannotTell`] code. Holds the
    /// detail: the call and its return code ([`call_failure`]), or the
    /// symbol that failed to resolve.
    Broken(String),
}

/// What [`device_count`] found: a count, no usable `ROCm` SMI, or a library
/// that loaded but could not start or count.
pub(super) enum RocmCount {
    /// `rsmi_num_monitor_devices` answered.
    Counted(u32),
    /// No usable `ROCm` SMI ([`StartFailure::Absent`]).
    Absent,
    /// The library loaded but could not start or count; holds the detail
    /// that becomes [`crate::HypomnesisError::Rocm`]'s text.
    Broken(String),
}

/// An initialized `ROCm` SMI session: the loaded library, plus the
/// `rsmi_shut_down` entry point its [`Drop`] calls.
///
/// [`Self::start`] returns a session only if `rsmi_init` succeeded, and
/// dropping it calls `rsmi_shut_down` exactly once, on early returns, `?`
/// and unwinding alike. Symbols resolved from [`Self::lib`] borrow the
/// session, so an explicit `drop(session)` also makes the borrow checker
/// reject any `ROCm` SMI call placed after it. Each entry point opens its
/// own session.
struct RocmSession {
    /// `rsmi_shut_down`, copied out of its `Symbol` so [`Drop`] can call it
    /// without re-borrowing `lib`. Valid while `lib` stays loaded, which
    /// `Drop::drop` guarantees: it runs before any field is dropped.
    shutdown: RsmiShutdownFn,
    /// The loaded `ROCm` SMI shared library; unloaded when the session
    /// drops, after `shutdown` has run.
    lib: libloading::Library,
}

impl RocmSession {
    /// Load `ROCm` SMI, resolve `rsmi_init` and `rsmi_shut_down`, and
    /// initialize. Tells apart the two ways to get no session: the library
    /// did not load ([`StartFailure::Absent`]), or it loaded and could not
    /// start ([`StartFailure::Broken`]). In either case there is nothing to
    /// shut down.
    #[allow(unsafe_code)]
    fn start() -> std::result::Result<Self, StartFailure> {
        // SAFETY: libloading::Library::new dynamically loads a shared library.
        // ROCm SMI is a stable AMD component with a well-defined C ABI; the
        // library is reference-counted by the OS and unloaded when the
        // session (which owns `lib`) is dropped.
        let lib =
            unsafe { libloading::Library::new(ROCM_LIB_PATH) }.map_err(|_| StartFailure::Absent)?;
        // SAFETY: symbol names match the ROCm SMI C API exactly, and the type
        // aliases match the header definitions. The copied-out function
        // pointers are only called while `lib` is loaded: `init` right here,
        // `shutdown` from Drop (see the struct doc).
        let init: RsmiInitFn = *unsafe { lib.get::<RsmiInitFn>(b"rsmi_init\0") }
            .map_err(|e| StartFailure::Broken(format!("failed to resolve rsmi_init: {e}")))?;
        // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
        let shutdown: RsmiShutdownFn = *unsafe { lib.get::<RsmiShutdownFn>(b"rsmi_shut_down\0") }
            .map_err(|e| {
            StartFailure::Broken(format!("failed to resolve rsmi_shut_down: {e}"))
        })?;

        // SAFETY: rsmi_init initializes the library's internal state; flags 0
        // is the plain initialization. RSMI_STATUS_SUCCESS (0) is the success
        // return code.
        let ret = unsafe { init(0) };
        if ret != RSMI_STATUS_SUCCESS {
            #[cfg(feature = "debug-output")]
            eprintln!("[ROCm debug] rsmi_init returned {ret}");
            return Err(match classify_init_failure(ret) {
                InitFailure::NoDriver => StartFailure::Absent,
                InitFailure::CannotTell => StartFailure::Broken(call_failure("rsmi_init", ret)),
            });
        }
        Ok(Self { shutdown, lib })
    }

    /// The loaded library, for resolving an entry point's own symbols.
    const fn lib(&self) -> &libloading::Library {
        &self.lib
    }
}

impl Drop for RocmSession {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: a session exists only if rsmi_init returned
        // RSMI_STATUS_SUCCESS in `start`, so this balances exactly one
        // successful init. `self.shutdown` came from `self.lib`, which is
        // still loaded here: Drop::drop runs before any field is dropped. The
        // return code is discarded, there is nothing useful to do on a failed
        // teardown.
        unsafe { (self.shutdown)() };
    }
}

/// Number of AMD GPUs the `ROCm` SMI library reports: [`RocmCount::Counted`],
/// or why there is none, [`RocmCount::Absent`] when the library does not
/// load or `rsmi_init` says there is no AMD driver, [`RocmCount::Broken`]
/// when it loads but cannot start for any other reason
/// ([`classify_init_failure`]), or `rsmi_num_monitor_devices` is missing or
/// fails. Used by the `device_count()` dispatcher and for bounds-checking an
/// index in the other dispatchers.
#[allow(unsafe_code)]
pub(super) fn device_count() -> RocmCount {
    let session = match RocmSession::start() {
        Ok(session) => session,
        Err(StartFailure::Absent) => return RocmCount::Absent,
        Err(StartFailure::Broken(detail)) => return RocmCount::Broken(detail),
    };

    // SAFETY: symbol name matches the ROCm SMI C API.
    let get_count: libloading::Symbol<'_, RsmiNumMonitorDevicesFn> = match unsafe {
        session.lib().get(b"rsmi_num_monitor_devices\0")
    } {
        Ok(symbol) => symbol,
        Err(e) => {
            return RocmCount::Broken(format!("failed to resolve rsmi_num_monitor_devices: {e}"));
        }
    };

    let mut count: u32 = 0;
    // SAFETY: rsmi_num_monitor_devices writes one u32 to the caller-provided
    // pointer.
    let ret = unsafe { get_count(&raw mut count) };

    // The only ROCm SMI call is done: end the session (rsmi_shut_down).
    drop(session);

    if ret == RSMI_STATUS_SUCCESS {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] device_count = {count}");
        RocmCount::Counted(count)
    } else {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_num_monitor_devices returned {ret}");
        RocmCount::Broken(call_failure("rsmi_num_monitor_devices", ret))
    }
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

    #[test]
    fn classify_init_failure_maps_the_measured_codes() {
        // No AMD driver (measured on a host with the userland installed):
        // no GPU process can exist.
        assert_eq!(
            classify_init_failure(RSMI_STATUS_INIT_ERROR),
            InitFailure::NoDriver
        );
        // Every other code, unknown and future ones included: GPU processes
        // may exist that ROCm SMI cannot show.
        for code in [
            RSMI_STATUS_INVALID_ARGS,
            RSMI_STATUS_PERMISSION,
            RSMI_STATUS_FAIL_LOAD_MODULE,
            RSMI_STATUS_UNKNOWN_ERROR,
            23,
        ] {
            assert_eq!(
                classify_init_failure(code),
                InitFailure::CannotTell,
                "code {code}"
            );
        }
    }
}
