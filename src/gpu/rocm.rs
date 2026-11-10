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
#[derive(Debug, Clone, Copy, Default)]
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

/// Hard ceiling on any buffer sized from a count the `ROCm` SMI library
/// reports: the host's process count from `rsmi_compute_process_info_get` and
/// a process's device count from `rsmi_compute_process_gpus_get`. A defensive
/// bound against a corrupted report claiming an implausible count, so it
/// cannot drive an attempted multi-gigabyte heap allocation. Real systems
/// never come close, so it never triggers in practice; above it the listing is
/// `None`, never clamped to a partial list.
const ROCM_MAX_REPORTED_COUNT: usize = 65_536;

/// Function signature: `rsmi_dev_name_get(dv_ind, *name, len)`. `len` is a
/// `size_t`.
type RsmiDevNameGetFn = unsafe extern "C" fn(u32, *mut std::ffi::c_char, usize) -> u32;

/// Function signature: `rsmi_dev_memory_total_get(dv_ind, mem_type, *total)`,
/// and, with the same shape, `rsmi_dev_memory_usage_get(dv_ind, mem_type,
/// *used)`. `mem_type` is an `rsmi_memory_type_t`, a 4-byte enum.
type RsmiDevMemoryGetFn = unsafe extern "C" fn(u32, u32, *mut u64) -> u32;

/// Function signature: `rsmi_version_str_get(component, *ver_str, len)`.
/// System-level: no device index. `component` is an `rsmi_sw_component_t`, a
/// 4-byte enum; `len` is a `uint32_t`.
type RsmiVersionStrGetFn = unsafe extern "C" fn(u32, *mut std::ffi::c_char, u32) -> u32;

/// Function signature: `rsmi_compute_process_info_get(*procs, *num_items)`.
type RsmiComputeProcessInfoGetFn = unsafe extern "C" fn(*mut RsmiProcessInfo, *mut u32) -> u32;

/// Function signature: `rsmi_compute_process_gpus_get(pid, *dv_indices,
/// *num_devices)`.
type RsmiComputeProcessGpusGetFn = unsafe extern "C" fn(u32, *mut u32, *mut u32) -> u32;

/// Function signature: `rsmi_compute_process_info_by_device_get(pid, dv_ind,
/// *proc)`.
type RsmiComputeProcessInfoByDeviceGetFn =
    unsafe extern "C" fn(u32, u32, *mut RsmiProcessInfo) -> u32;

/// `rsmi_memory_type_t` value `RSMI_MEM_TYPE_VRAM`.
const RSMI_MEM_TYPE_VRAM: u32 = 0;

/// `rsmi_sw_component_t` value `RSMI_SW_COMP_DRIVER`.
const RSMI_SW_COMP_DRIVER: u32 = 0;

/// Buffer size for `rsmi_dev_name_get`. The header names no length; a name
/// longer than the buffer is cut and the call returns
/// `RSMI_STATUS_INSUFFICIENT_SIZE`.
const ROCM_DEVICE_NAME_BUFFER_SIZE: usize = 256;

/// Buffer size for `rsmi_version_str_get`. The header names no length; a
/// version longer than the buffer is cut and the call returns
/// `RSMI_STATUS_INSUFFICIENT_SIZE`.
const ROCM_VERSION_BUFFER_SIZE: usize = 80;

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

/// Combined result of a single `ROCm` SMI query for a given device index.
///
/// Returned by [`query`].
pub(super) struct RocmQueryResult {
    /// Per-process `VRAM` in bytes for the calling process on this device,
    /// from `rsmi_compute_process_info_by_device_get`.
    ///
    /// `None` when that call fails.
    pub process_used_bytes: Option<u64>,
    /// Device-wide total `VRAM` in bytes.
    pub device_total: u64,
    /// Device-wide free `VRAM` in bytes: total minus used, saturating.
    pub device_free: u64,
    /// Device-wide used `VRAM` in bytes.
    pub device_used: u64,
    /// `ROCm` SMI reports no driver or firmware carve-out; always `None`.
    pub reserved_bytes: Option<u64>,
    /// Adapter name as reported by `rsmi_dev_name_get`,
    /// e.g. `"AMD Instinct MI300X OAM"`.
    /// `None` when the name query fails.
    pub device_name: Option<String>,
    /// Driver version string from `rsmi_version_str_get` for
    /// `RSMI_SW_COMP_DRIVER`. System-level, not device-scoped, the same value
    /// regardless of `idx`. `None` when the query fails.
    pub driver_version: Option<String>,
}

/// Run a single `ROCm` SMI query session for the given device index.
///
/// Loads `ROCm` SMI, runs init, reads the device's `VRAM` total and used, its
/// name, the driver version, and the calling process's `VRAM` on the device,
/// then shuts `ROCm` SMI down before returning. Name, driver version and
/// per-process failures are tolerated (`None`) since the device-wide figures
/// are still useful. If the library load, init, or the `VRAM` total or used
/// query fails, the function returns `None`.
#[allow(unsafe_code)]
pub(super) fn query(idx: u32) -> Option<RocmQueryResult> {
    let session = RocmSession::start().ok()?;
    let lib = session.lib();

    // SAFETY: Loading function symbols from the ROCm SMI library. Each name
    // matches the ROCm SMI C API exactly. The function signatures (type
    // aliases above) match the header definitions.
    let get_name: libloading::Symbol<'_, RsmiDevNameGetFn> =
        unsafe { lib.get(b"rsmi_dev_name_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_total: libloading::Symbol<'_, RsmiDevMemoryGetFn> =
        unsafe { lib.get(b"rsmi_dev_memory_total_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_used: libloading::Symbol<'_, RsmiDevMemoryGetFn> =
        unsafe { lib.get(b"rsmi_dev_memory_usage_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_version: libloading::Symbol<'_, RsmiVersionStrGetFn> =
        unsafe { lib.get(b"rsmi_version_str_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_by_device: libloading::Symbol<'_, RsmiComputeProcessInfoByDeviceGetFn> =
        unsafe { lib.get(b"rsmi_compute_process_info_by_device_get\0") }.ok()?;

    let mut device_total: u64 = 0;
    // SAFETY: rsmi_dev_memory_total_get writes one u64 to the caller-provided
    // pointer; RSMI_MEM_TYPE_VRAM is a valid rsmi_memory_type_t.
    let ret = unsafe { get_total(idx, RSMI_MEM_TYPE_VRAM, &raw mut device_total) };
    if ret != RSMI_STATUS_SUCCESS {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_dev_memory_total_get(idx={idx}) returned {ret}");
        return None;
    }

    let mut device_used: u64 = 0;
    // SAFETY: rsmi_dev_memory_usage_get writes one u64 to the caller-provided
    // pointer; RSMI_MEM_TYPE_VRAM is a valid rsmi_memory_type_t.
    let ret = unsafe { get_used(idx, RSMI_MEM_TYPE_VRAM, &raw mut device_used) };
    if ret != RSMI_STATUS_SUCCESS {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_dev_memory_usage_get(idx={idx}) returned {ret}");
        return None;
    }

    // Adapter name (best-effort; failure is non-fatal for the rest of the result).
    let mut name_buf = [0_u8; ROCM_DEVICE_NAME_BUFFER_SIZE];
    // SAFETY: rsmi_dev_name_get writes a null-terminated C string into
    // name_buf, up to `len` bytes, `len` being a size_t. The buffer is
    // stack-allocated and ROCM_DEVICE_NAME_BUFFER_SIZE long.
    let ret = unsafe {
        get_name(
            idx,
            name_buf.as_mut_ptr().cast::<std::ffi::c_char>(),
            ROCM_DEVICE_NAME_BUFFER_SIZE,
        )
    };
    let device_name = if ret == RSMI_STATUS_SUCCESS {
        let nul_pos = name_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_buf.len());
        // BORROW: explicit String::from_utf8_lossy + into_owned — name_buf
        // is stack-local; we need an owned String to return.
        name_buf
            .get(..nul_pos)
            .map(|slice| String::from_utf8_lossy(slice).into_owned())
            .filter(|s| !s.is_empty())
    } else {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_dev_name_get returned {ret}");
        None
    };

    // Driver version (best-effort, system-level).
    let mut version_buf = [0_u8; ROCM_VERSION_BUFFER_SIZE];
    // CAST: usize → u32, ROCM_VERSION_BUFFER_SIZE = 80 fits in u32
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let version_len = ROCM_VERSION_BUFFER_SIZE as u32;
    // SAFETY: rsmi_version_str_get writes a null-terminated C string into
    // version_buf, up to `version_len` bytes, a uint32_t. The buffer is
    // stack-allocated and ROCM_VERSION_BUFFER_SIZE long.
    let ret = unsafe {
        get_version(
            RSMI_SW_COMP_DRIVER,
            version_buf.as_mut_ptr().cast::<std::ffi::c_char>(),
            version_len,
        )
    };
    let driver_version = if ret == RSMI_STATUS_SUCCESS {
        let nul_pos = version_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(version_buf.len());
        // BORROW: explicit String::from_utf8_lossy + into_owned — version_buf
        // is stack-local; we need an owned String to return.
        version_buf
            .get(..nul_pos)
            .map(|slice| String::from_utf8_lossy(slice).into_owned())
            .filter(|s| !s.is_empty())
    } else {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_version_str_get returned {ret}");
        None
    };

    // The calling process's VRAM on this device (best-effort).
    let mut process_info = RsmiProcessInfo::default();
    // SAFETY: rsmi_compute_process_info_by_device_get writes one
    // rsmi_process_info_t to the caller-provided pointer; RsmiProcessInfo
    // matches its layout (see the layout test).
    let ret = unsafe { get_by_device(std::process::id(), idx, &raw mut process_info) };
    let process_used_bytes = if ret == RSMI_STATUS_SUCCESS {
        Some(process_info.vram_usage)
    } else {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_compute_process_info_by_device_get returned {ret}");
        None
    };

    // Every ROCm SMI call is done: end the session (rsmi_shut_down) here,
    // before the result is assembled.
    drop(session);

    let device_free = device_total.saturating_sub(device_used);

    #[cfg(feature = "debug-output")]
    eprintln!(
        "[ROCm debug] device {idx}: total={device_total} free={device_free} used={device_used} \
         per_process={process_used_bytes:?} name={device_name:?} driver={driver_version:?}"
    );

    Some(RocmQueryResult {
        process_used_bytes,
        device_total,
        device_free,
        device_used,
        reserved_bytes: None,
        device_name,
        driver_version,
    })
}

/// The rows of one device's process listing: `(pid, used_bytes)` for each
/// process whose device list (`rsmi_compute_process_gpus_get`) includes
/// `idx`, with the bytes `by_device` reports for that PID on the device
/// (`rsmi_compute_process_info_by_device_get`).
///
/// The list call's own `vram_usage` is zero and the per-PID total sums every
/// device, so neither is a figure for one device. Every PID is checked,
/// whoever owns it. `by_device` is asked only for PIDs on the device; `None`
/// when it fails for any of them, so a failed call is never an empty listing.
fn rows_on_device(
    idx: u32,
    procs: &[(u32, Vec<u32>)],
    by_device: impl Fn(u32) -> Option<u64>,
) -> Option<Vec<(u32, u64)>> {
    procs
        .iter()
        .filter(|(_, devices)| devices.contains(&idx))
        .map(|&(pid, _)| by_device(pid).map(|bytes| (pid, bytes)))
        .collect()
}

/// Enumerate every compute process on the given device, returning
/// `(pid, used_bytes)` for each process that uses it.
///
/// `rsmi_compute_process_info_get` lists every `KFD` process on the host,
/// whatever its owner or device, with `vram_usage` zero. For each PID,
/// `rsmi_compute_process_gpus_get` says which devices it uses, and
/// `rsmi_compute_process_info_by_device_get` gives its bytes on `idx`
/// ([`rows_on_device`]).
///
/// Both list calls follow the library's two-call size protocol: a null buffer
/// returns the count, then a buffer of that many entries is filled.
///
/// Returns `None` when `ROCm` SMI cannot be loaded or started, a symbol does
/// not resolve, any call fails (the device count's included), or `idx` is not
/// below the device count; never an empty `Vec` for a failed call.
#[allow(unsafe_code)]
#[must_use]
pub(super) fn list_compute_processes(idx: u32) -> Option<Vec<(u32, u64)>> {
    let session = RocmSession::start().ok()?;
    let lib = session.lib();

    // SAFETY: symbol names match the ROCm SMI C API.
    let get_processes: libloading::Symbol<'_, RsmiComputeProcessInfoGetFn> =
        unsafe { lib.get(b"rsmi_compute_process_info_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_gpus: libloading::Symbol<'_, RsmiComputeProcessGpusGetFn> =
        unsafe { lib.get(b"rsmi_compute_process_gpus_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_by_device: libloading::Symbol<'_, RsmiComputeProcessInfoByDeviceGetFn> =
        unsafe { lib.get(b"rsmi_compute_process_info_by_device_get\0") }.ok()?;
    // SAFETY: as above, a ROCm SMI symbol; the alias matches the header.
    let get_count: libloading::Symbol<'_, RsmiNumMonitorDevicesFn> =
        unsafe { lib.get(b"rsmi_num_monitor_devices\0") }.ok()?;

    // The process list is host-wide, so an index past the devices would read
    // as an empty device: it is `None`, as NVML's listing is when its handle
    // lookup fails.
    let mut devices_present: u32 = 0;
    // SAFETY: rsmi_num_monitor_devices writes one u32 to the caller-provided
    // pointer.
    let ret = unsafe { get_count(&raw mut devices_present) };
    if ret != RSMI_STATUS_SUCCESS {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_num_monitor_devices returned {ret}");
        return None;
    }
    if idx >= devices_present {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] device {idx} is not below the {devices_present} devices");
        return None;
    }

    // ---- Every PID on the host: size call, then the fill call -------------
    let mut count: u32 = 0;
    // SAFETY: with a null buffer, rsmi_compute_process_info_get writes only
    // the number of processes to `count`.
    let ret = unsafe { get_processes(std::ptr::null_mut(), &raw mut count) };
    if ret != RSMI_STATUS_SUCCESS {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_compute_process_info_get(size) returned {ret}");
        return None;
    }
    if count == 0 {
        return Some(Vec::new());
    }
    // CAST: u32 → usize, ROCm SMI process count; usize >= 32 bits everywhere
    #[allow(clippy::as_conversions)]
    let needed = count as usize;
    if needed > ROCM_MAX_REPORTED_COUNT {
        #[cfg(feature = "debug-output")]
        eprintln!(
            "[ROCm debug] rsmi_compute_process_info_get reported {needed} processes, \
             above {ROCM_MAX_REPORTED_COUNT}"
        );
        return None;
    }
    let mut infos = vec![RsmiProcessInfo::default(); needed];
    // SAFETY: rsmi_compute_process_info_get fills `infos` with up to `count`
    // entries and updates `count` to the number written. The buffer holds
    // exactly the `count` entries the size call reported.
    let ret = unsafe { get_processes(infos.as_mut_ptr(), &raw mut count) };
    if ret != RSMI_STATUS_SUCCESS {
        #[cfg(feature = "debug-output")]
        eprintln!("[ROCm debug] rsmi_compute_process_info_get returned {ret}");
        return None;
    }
    // CAST: u32 → usize, bounded by the buffer's own length below.
    #[allow(clippy::as_conversions)]
    let written = (count as usize).min(infos.len());

    // ---- The devices each PID uses: size call, then the fill call ---------
    let procs = infos
        .get(..written)?
        .iter()
        .map(|info| {
            let pid = info.process_id;
            let mut num_devices: u32 = 0;
            // SAFETY: with a null buffer, rsmi_compute_process_gpus_get
            // writes only the number of devices to `num_devices`.
            let ret = unsafe { get_gpus(pid, std::ptr::null_mut(), &raw mut num_devices) };
            if ret != RSMI_STATUS_SUCCESS {
                #[cfg(feature = "debug-output")]
                eprintln!(
                    "[ROCm debug] rsmi_compute_process_gpus_get(size, pid={pid}) returned {ret}"
                );
                return None;
            }
            if num_devices == 0 {
                return Some((pid, Vec::new()));
            }
            // CAST: u32 → usize, ROCm SMI device count; usize >= 32 bits everywhere
            #[allow(clippy::as_conversions)]
            let needed_devices = num_devices as usize;
            if needed_devices > ROCM_MAX_REPORTED_COUNT {
                #[cfg(feature = "debug-output")]
                eprintln!(
                    "[ROCm debug] rsmi_compute_process_gpus_get(pid={pid}) reported \
                     {needed_devices} devices, above {ROCM_MAX_REPORTED_COUNT}"
                );
                return None;
            }
            let mut devices = vec![0_u32; needed_devices];
            // SAFETY: rsmi_compute_process_gpus_get fills `devices` with up
            // to `num_devices` indices and updates `num_devices` to the
            // number written. The buffer holds exactly the count the size
            // call reported.
            let ret = unsafe { get_gpus(pid, devices.as_mut_ptr(), &raw mut num_devices) };
            if ret != RSMI_STATUS_SUCCESS {
                #[cfg(feature = "debug-output")]
                eprintln!("[ROCm debug] rsmi_compute_process_gpus_get(pid={pid}) returned {ret}");
                return None;
            }
            // CAST: u32 → usize, bounded by the buffer's own length via truncate.
            #[allow(clippy::as_conversions)]
            devices.truncate(num_devices as usize);
            Some((pid, devices))
        })
        .collect::<Option<Vec<(u32, Vec<u32>)>>>()?;

    // ---- Each PID's bytes on this device ----------------------------------
    let rows = rows_on_device(idx, &procs, |pid| {
        let mut process_info = RsmiProcessInfo::default();
        // SAFETY: rsmi_compute_process_info_by_device_get writes one
        // rsmi_process_info_t to the caller-provided pointer; RsmiProcessInfo
        // matches its layout (see the layout test).
        let ret = unsafe { get_by_device(pid, idx, &raw mut process_info) };
        if ret == RSMI_STATUS_SUCCESS {
            Some(process_info.vram_usage)
        } else {
            #[cfg(feature = "debug-output")]
            eprintln!(
                "[ROCm debug] rsmi_compute_process_info_by_device_get(pid={pid}, idx={idx}) \
                 returned {ret}"
            );
            None
        }
    });

    // End the session (rsmi_shut_down) only now that every call is done.
    drop(session);

    #[cfg(feature = "debug-output")]
    eprintln!(
        "[ROCm debug] list_compute_processes(idx={idx}): {} PID(s) on the host, {} on the device",
        procs.len(),
        rows.as_ref().map_or(0, Vec::len)
    );

    rows
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

    #[test]
    fn rows_on_device_reports_the_device_figure() {
        // A process on devices 0 and 1; the row carries the by-device figure.
        let procs = [(4242, vec![0, 1])];
        let rows = rows_on_device(1, &procs, |pid| (pid == 4242).then_some(2_298_515_456));
        assert_eq!(rows, Some(vec![(4242, 2_298_515_456)]));
    }

    #[test]
    fn rows_on_device_excludes_a_process_on_another_device() {
        // One process on device 1, one whose device list lacks it: only the
        // first is kept, whoever owns either.
        let procs = [(4242, vec![0, 1]), (4343, vec![0])];
        let rows = rows_on_device(1, &procs, |_| Some(2_298_515_456));
        assert_eq!(rows, Some(vec![(4242, 2_298_515_456)]));
    }

    #[test]
    fn rows_on_device_is_none_when_a_device_read_fails() {
        // A failed read for a process on the device is a failed listing, not
        // a listing without that process.
        let procs = [(4242, vec![1]), (4343, vec![1])];
        let rows = rows_on_device(1, &procs, |pid| (pid == 4242).then_some(2_298_515_456));
        assert_eq!(rows, None);
    }
}
