// SPDX-License-Identifier: MIT OR Apache-2.0

//! macOS GPU backend — per-process Metal memory on Apple Silicon UMA.
//!
//! Reads `graphics_footprint` from the BSD kernel ledger for any PID
//! the caller's sandbox lets it read, via
//! `ledger(LEDGER_ENTRY_INFO_V2, pid, …)`. On a
//! unified-memory-architecture (`UMA`) Apple Silicon `SoC` the GPU and
//! CPU share the same physical pages, so device-wide `total_bytes` is
//! `sysctl hw.memsize` and the adapter name is the CPU brand string
//! (`machdep.cpu.brand_string`). Process ownership plays no part.
//!
//! Enumeration is `proc_listpids`; when the caller's sandbox refuses it
//! (`EPERM`), `sysctl(KERN_PROC_ALL)`. A ledger read the sandbox refuses
//! is counted as denied, not dropped, and a name comes from
//! `proc_pidpath`, or from the kernel's `p_comm` where only that call is
//! refused (see README Limitations, item 9).
//!
//! Source map: `ledger` (per-process graphics resident bytes),
//! `sysctlbyname` (device totals, name), `proc_listpids` +
//! `proc_pidpath` (enumeration, names), `sysctl(KERN_PROC)` (the process
//! table when libproc is refused; one process's record for its `p_comm`
//! and for `process_exists`). No Mach `task_for_pid` is used and no
//! Apple-framework dependency is required.
//!
//! Semantic equivalence: `graphics_footprint` is **resident** bytes,
//! mirroring Windows `WorkingSetSize` (DXGI `CurrentUsage`) and Linux
//! `VmRSS` (NVML `used`). This is the choice forced by the
//! cross-platform contract — `MTLDevice.currentAllocatedSize` would be
//! allocator-tracked (virtual) and is therefore not used.
//!
//! The `graphics_footprint` ledger entry index is **discovered by name
//! at first call** via `LEDGER_TEMPLATE_INFO`, then cached in a
//! `OnceLock<i32>`. The index observed on macOS 26.x happens to be 36
//! but must never be hardcoded as a literal expression — the entry
//! ordering is not part of any stable ABI guarantee.

use core::ffi::{c_char, c_void};
use std::sync::OnceLock;

use super::kinfo::{
    self, CTL_KERN, KERN_PROC, KERN_PROC_ALL, KERN_PROC_PID, KINFO_PROC_SIZE, KernProcAllAttempt,
    KinfoRecord, PathLookup, PidLookup, classify_kern_proc_all, classify_kern_proc_pid,
    decide_exists, parse_kinfo_records,
};

/// libSystem FFI declarations for the macOS GPU backend.
///
/// Every entry below (`getpid`, `ledger`, `sysctl`, `sysctlbyname`,
/// `proc_listpids`, `proc_pidpath`) is a stable libSystem syscall
/// available on every macOS install since at least 10.15. No header from
/// `Kernel.framework` is shipped in user space for `ledger()`, so the
/// signature is declared inline against the kernel's documented ABI.
mod libsystem_ffi {
    use core::ffi::{c_char, c_int, c_void};

    // SAFETY: These are stable libSystem entry points with documented
    // C ABI. `getpid` is POSIX. `sysctl` and `sysctlbyname` are BSD,
    // declared in `<sys/sysctl.h>`. `proc_listpids` and `proc_pidpath`
    // are declared in `<libproc.h>`.
    // `ledger` has no user-space header but its ABI is fixed
    // (`SYS_ledger = 373`).
    // Each call's safety contract is upheld at its call site.
    #[allow(unsafe_code)]
    unsafe extern "C" {
        /// Returns the calling process's PID. Cannot fail.
        ///
        /// See: `<unistd.h>`. Marked `safe` per Rust 2024 idiom — the
        /// kernel guarantees a valid PID is always returned. Declared
        /// for ABI completeness of the libSystem surface; the calling
        /// PID is read via `std::process::id` (safe stdlib) elsewhere.
        #[allow(dead_code)]
        pub(super) safe fn getpid() -> i32;

        /// BSD kernel ledger syscall (no user-space header ships this).
        ///
        /// `cmd` selects the operation: [`super::LEDGER_INFO`],
        /// [`super::LEDGER_TEMPLATE_INFO`], [`super::LEDGER_ENTRY_INFO_V2`].
        /// `arg1`/`arg2`/`arg3` semantics depend on `cmd`. All three
        /// have C type `caddr_t` (`char *`); for `LEDGER_ENTRY_INFO_V2`
        /// arg1 is the target PID reinterpreted as a pointer-sized
        /// integer (kernel convention — see `osfmk/kern/ledger.c`).
        /// Returns `0` on success, `-1` with `errno` set on failure
        /// (`EPERM` for a read refused by the caller's sandbox, `ESRCH`
        /// for a PID that exited).
        pub(super) unsafe fn ledger(
            cmd: i32,
            arg1: *mut c_void,
            arg2: *mut c_void,
            arg3: *mut c_void,
        ) -> i32;

        /// `sysctlbyname` — read a kernel state variable by name.
        ///
        /// See: `<sys/sysctl.h>`. `name` is a NUL-terminated C string.
        /// `oldp`/`oldlenp` form the standard in/out buffer pair;
        /// `newp`/`newlen` are zero/null for read-only queries.
        pub(super) unsafe fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> i32;

        /// `sysctl` — read a kernel state variable by MIB.
        ///
        /// See: `<sys/sysctl.h>`. The MIB form, distinct from
        /// `sysctlbyname`: `name` points at `namelen` integers (for a
        /// process record, `CTL_KERN, KERN_PROC, KERN_PROC_PID, pid`; for
        /// the process table, `CTL_KERN, KERN_PROC, KERN_PROC_ALL`).
        /// `oldp`/`oldlenp` form the standard in/out buffer pair;
        /// `newp`/`newlen` are null/zero for read-only queries. Returns
        /// `0` on success, `-1` with `errno` set on failure.
        pub(super) unsafe fn sysctl(
            name: *mut c_int,
            namelen: u32,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> i32;

        /// `proc_listpids` — enumerate process IDs by type.
        ///
        /// See: `<libproc.h>`. With `type_ = PROC_ALL_PIDS` and
        /// `typeinfo = 0`, fills `buffer` with `i32` PIDs and returns
        /// the number of bytes written. Calling with
        /// `buffer = NULL, buffersize = 0` returns the buffer size in
        /// bytes the kernel suggests: `(nprocs + 20) * sizeof(int)`, the
        /// process count plus 20 spare slots (XNU `bsd/kern/proc_info.c`);
        /// the fill returns the bytes it wrote, `4 * pid_count`. Returns `0`
        /// with `errno` `EPERM` when the caller's sandbox refuses it.
        pub(super) unsafe fn proc_listpids(
            type_: u32,
            typeinfo: u32,
            buffer: *mut c_void,
            buffersize: i32,
        ) -> i32;

        /// `proc_pidpath` — resolve a PID's executable path.
        ///
        /// See: `<libproc.h>`. Writes a NUL-terminated path into
        /// `buffer`. Returns the path length on success (excluding
        /// NUL), or `0` on failure with `errno` set: `ESRCH` for a PID
        /// that names no process, `EPERM` for a call the caller's sandbox
        /// refuses. The kernel looks the PID up before it checks the
        /// sandbox (XNU `bsd/kern/proc_info.c`), so a missing PID reads
        /// `ESRCH` even inside a sandbox.
        pub(super) unsafe fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }
}

/// `LEDGER_INFO` command — query the per-PID ledger metadata
/// (`li_entries` = number of entries, `li_name` = task name).
///
/// Value from XNU `osfmk/kern/ledger.h`. Not currently issued by this
/// module (only [`LEDGER_TEMPLATE_INFO`] and [`LEDGER_ENTRY_INFO_V2`]
/// are) — kept as a zero-cost `const` documenting the complete 3-command
/// family this FFI surface is built against, so a future reader sees the
/// full picture rather than two commands with no visible sibling.
#[allow(dead_code)]
const LEDGER_INFO: i32 = 0;

/// `LEDGER_TEMPLATE_INFO` command — fetch the array of
/// [`LedgerTemplateInfo`] rows describing every ledger entry by name.
///
/// Used at init to discover the `graphics_footprint` entry index by
/// name. Value from XNU `osfmk/kern/ledger.h`.
const LEDGER_TEMPLATE_INFO: i32 = 2;

/// `LEDGER_ENTRY_INFO_V2` command — fetch the per-PID
/// [`LedgerEntryInfo`] rows. Each entry's `lei_balance` is the current
/// resident-bytes count for that ledger category.
///
/// Value from XNU `osfmk/kern/ledger.h`.
const LEDGER_ENTRY_INFO_V2: i32 = 4;

/// `proc_listpids` selector — enumerate every PID on the system.
///
/// Value from XNU `bsd/sys/proc_info.h`.
const PROC_ALL_PIDS: u32 = 1;

/// `PROC_PIDPATHINFO_MAXSIZE` — maximum path length returned by
/// `proc_pidpath`. `4 * MAXPATHLEN` from `<sys/proc_info.h>`.
const PROC_PIDPATHINFO_MAXSIZE: usize = 4096;

/// `ledger_template_info` from XNU `osfmk/kern/ledger.h`.
///
/// One row per ledger entry, returned in an array by
/// `ledger(LEDGER_TEMPLATE_INFO, …)`. Fields are 32-byte
/// NUL-terminated C strings.
///
/// See: <https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/ledger.h>
#[repr(C)]
#[allow(clippy::struct_field_names)] // `lti_*` prefix is the XNU kernel ABI field naming
struct LedgerTemplateInfo {
    /// Entry name (e.g. `"graphics_footprint"`). NUL-terminated.
    lti_name: [c_char; 32],
    /// Group name (e.g. `"phys"`). NUL-terminated.
    lti_group: [c_char; 32],
    /// Units (e.g. `"bytes"`). NUL-terminated.
    lti_units: [c_char; 32],
}

/// `ledger_entry_info_v2` from XNU `osfmk/kern/ledger.h`.
///
/// One row per ledger entry, returned in an array by
/// `ledger(LEDGER_ENTRY_INFO_V2, pid, …)`. Layout is the V2 ABI
/// (sizeof = 88 bytes) — this is **not** the V1 `ledger_entry_info`
/// shape.
///
/// See: <https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/ledger.h>
#[repr(C)]
#[allow(clippy::struct_field_names)] // `lei_*` prefix is the XNU kernel ABI field naming
struct LedgerEntryInfo {
    /// Current ledger balance in entry units (for `graphics_footprint`:
    /// resident GPU-attributed bytes).
    lei_balance: i64,
    /// Credit total (bytes ever credited to this entry; monotonic).
    lei_credit: i64,
    /// Debit total (bytes ever debited from this entry; monotonic).
    lei_debit: i64,
    /// Limit in entry units (`-1` = no limit).
    lei_limit: u64,
    /// Refill period in absolute-time units (`0` = no refill).
    lei_refill_period: u64,
    /// Last refill timestamp in absolute-time units.
    lei_last_refill: u64,
    /// Lifetime maximum value of `lei_balance` (peak).
    lei_lifetime_max: i64,
    /// Reserved for future ABI growth. Kernel writes zero.
    lei_reserved: [u64; 4],
}

/// Combined result of a single Metal device-wide query.
///
/// Shape mirrors `super::dxgi::DxgiQueryResult` in spirit: the device
/// total, the device-wide working-set budget, and the adapter name.
/// Unlike `DxgiQueryResult`, there is no per-process figure here — the
/// per-process path is [`process_gpu_info`], which reads
/// `graphics_footprint` independently rather than through this struct.
/// Returned by [`query`].
pub(super) struct MetalQueryResult {
    /// Total physical memory in bytes — `sysctl hw.memsize`. On UMA
    /// this is the system DRAM size, which is also the GPU's address
    /// space ceiling.
    pub dedicated_video_memory: u64,
    /// Apple-driver-recommended GPU working-set budget in bytes —
    /// `MTLDevice.recommendedMaxWorkingSetSize`. The value the kernel
    /// projects as "memory the GPU can hold resident with good
    /// performance," factoring in compression + system reserves. Used
    /// as the macOS analogue of `free_bytes` on a discrete GPU.
    pub recommended_max_working_set: u64,
    /// Adapter name — the CPU brand string
    /// (`machdep.cpu.brand_string`, e.g. `"Apple M3 Pro"`). On Apple
    /// Silicon the CPU and GPU share the same die, so the CPU brand
    /// identifies the GPU.
    pub adapter_name: String,
}

/// Cached index of the `graphics_footprint` entry in the per-PID
/// ledger entry array. Resolved by name on first read via
/// [`resolve_graphics_footprint_index`]; observed value on macOS 26.x
/// is 36, but the literal must never appear as a Rust expression value
/// — the kernel's entry ordering is not part of any stable ABI.
static GRAPHICS_FOOTPRINT_INDEX: OnceLock<i32> = OnceLock::new();

/// Maximum ledger-entry count probed at init.
///
/// XNU defines ~70 entries on macOS 26.x; this cap bounds the
/// `LEDGER_TEMPLATE_INFO` buffer growth. Set well above the observed
/// count to absorb future kernel additions without truncation.
const LEDGER_TEMPLATE_BUF_CAP: usize = 128;

/// Cached system-default Metal device handle.
///
/// Acquired on first call to [`recommended_max_working_set_size`] via
/// `MTLCreateSystemDefaultDevice`; never re-acquired thereafter. The
/// driver's per-device cost of acquiring a handle is ~100-200 µs the
/// first time and unmeasurable thereafter (the property read on a
/// cached handle is a synchronised getter on the device object).
///
/// `Option<...>` because the call may legitimately return `nil` on a
/// system without a usable Metal device (extremely rare on Apple
/// Silicon; possible in very locked-down environments).
static METAL_DEVICE: OnceLock<
    Option<objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>>>,
> = OnceLock::new();

/// Compile-time guard: if a future `objc2-metal` version drops the
/// `Send + Sync` supertraits on `MTLDevice`, this fails to compile and
/// the implementation must switch to a newtype with explicit
/// `unsafe impl Send + Sync` to remain safe in the static `OnceLock`.
const _: fn() = || {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>>,
    >();
};

/// Read `MTLDevice.recommendedMaxWorkingSetSize` for the system-default
/// device, caching the device handle for the life of the process.
///
/// Returns `None` if `MTLCreateSystemDefaultDevice` returns `nil`. The
/// caller (in [`query`]) falls back to the total physical DRAM in that
/// case — that's the conservative upper bound on what the GPU can hold.
///
/// `redundant_closure` is a false positive on the `get_or_init` call
/// below: `MTLCreateSystemDefaultDevice` is declared `extern "C-unwind"`,
/// which does not implement `FnOnce()` — passing the bare function item
/// (clippy's suggested simplification) fails to compile with E0277
/// ("expected a `FnOnce()` closure, found `extern "C-unwind" fn()...`"),
/// confirmed live against the real `aarch64-apple-darwin` target. The
/// closure is required, not redundant.
#[allow(clippy::redundant_closure)]
fn recommended_max_working_set_size() -> Option<u64> {
    // objc2-metal's bindings expose this property as a safe method; no
    // `unsafe` block is required at the call site. The trait import is
    // necessary because the method is a trait method, not an inherent.
    use objc2_metal::MTLDevice;
    METAL_DEVICE
        .get_or_init(|| objc2_metal::MTLCreateSystemDefaultDevice())
        .as_ref()
        .map(|d| d.recommendedMaxWorkingSetSize())
}

/// Read a `u64` `sysctlbyname` value (e.g. `b"hw.memsize\0"`).
///
/// `name` must be a NUL-terminated byte slice. Returns `None` if the
/// syscall fails or returns a non-8-byte value.
#[allow(unsafe_code)]
fn read_sysctl_u64(name: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    let mut len: usize = size_of::<u64>();
    // CAST: &u64 → *mut c_void via `&raw mut value` then explicit cast;
    // the sysctl ABI treats the out-buffer as an opaque byte region.
    #[allow(clippy::as_conversions, clippy::ptr_as_ptr)]
    let out_ptr = (&raw mut value).cast::<c_void>();
    // SAFETY: `name` is caller-provided as a NUL-terminated byte slice;
    // its `as_ptr()` is valid for `name.len()` bytes including the
    // terminator the kernel scans for. `out_ptr` points to an 8-byte
    // stack-resident `u64`. `&raw mut len` is a live `usize` whose
    // initial value matches the buffer capacity. `newp`/`newlen` are
    // null/zero — read-only query.
    let rc = unsafe {
        // INDEX: `name[0]` is the first byte of the NUL-terminated
        // name; passing a zero-length slice would yield a dangling
        // pointer, so reject empty names up-front via the slice's
        // own bounds check on `as_ptr`.
        libsystem_ffi::sysctlbyname(
            name.as_ptr().cast::<c_char>(),
            out_ptr,
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 && len == size_of::<u64>() {
        Some(value)
    } else {
        None
    }
}

/// Read a UTF-8 `sysctlbyname` string (e.g. `b"machdep.cpu.brand_string\0"`).
///
/// Two-call probe: query the buffer length first with a null `oldp`,
/// then allocate and re-query. Returns `None` if either call fails or
/// the result is not valid UTF-8 after trimming the trailing NUL.
#[allow(unsafe_code)]
fn read_sysctl_string(name: &[u8]) -> Option<String> {
    let mut len: usize = 0;
    // SAFETY: `name.as_ptr()` is a NUL-terminated C string; `oldp =
    // null` instructs the kernel to write the required buffer size
    // into `*oldlenp`. `newp`/`newlen` are null/zero (read-only).
    let rc = unsafe {
        libsystem_ffi::sysctlbyname(
            name.as_ptr().cast::<c_char>(),
            core::ptr::null_mut(),
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || len == 0 {
        return None;
    }
    let mut buf: Vec<u8> = vec![0_u8; len];
    // SAFETY: `buf` is a freshly allocated Vec<u8> with `len` bytes
    // capacity AND length; `as_mut_ptr` is valid for `buf.len()`
    // bytes. `&raw mut len` still holds the kernel-reported size.
    let rc2 = unsafe {
        libsystem_ffi::sysctlbyname(
            name.as_ptr().cast::<c_char>(),
            buf.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    if rc2 != 0 {
        return None;
    }
    // Trim trailing NUL byte(s) the kernel includes in the count.
    while buf.last() == Some(&0) {
        buf.pop();
    }
    // BORROW: `String::from_utf8(buf).ok()` — sysctl strings are ASCII
    // by convention; UTF-8 decode failure surfaces as `None`.
    String::from_utf8(buf).ok()
}

/// Discover the `graphics_footprint` ledger entry index by name.
///
/// Calls `ledger(LEDGER_TEMPLATE_INFO, buf, &count, NULL)` and
/// linear-scans the returned [`LedgerTemplateInfo`] rows for an
/// `lti_name` whose decoded prefix equals `"graphics_footprint"`.
/// Returns `None` if the syscall fails or the entry is absent on this
/// kernel.
#[allow(unsafe_code)]
fn resolve_graphics_footprint_index() -> Option<i32> {
    // SAFETY: `LedgerTemplateInfo` is `#[repr(C)]` with only `c_char`
    // array fields (POD). All-zero is a valid bit pattern; we
    // initialise via `core::mem::zeroed` per element so the resulting
    // `Vec` is fully initialised before the FFI call sees its buffer.
    let mut buf: Vec<LedgerTemplateInfo> = (0..LEDGER_TEMPLATE_BUF_CAP)
        .map(|_| unsafe { core::mem::zeroed::<LedgerTemplateInfo>() })
        .collect();
    // CAST: usize → i32, count is bounded by `LEDGER_TEMPLATE_BUF_CAP`
    // (128); fits trivially in i32.
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    let mut count: i32 = LEDGER_TEMPLATE_BUF_CAP as i32;
    // SAFETY: arg1 is the template buffer (`caddr_t`); arg2 is the
    // count in/out pointer (`caddr_t` aliasing an `i32`); arg3 is
    // NULL. The kernel writes at most `count` rows and updates
    // `*count` with the number actually written.
    let rc = unsafe {
        libsystem_ffi::ledger(
            LEDGER_TEMPLATE_INFO,
            buf.as_mut_ptr().cast::<c_void>(),
            (&raw mut count).cast::<c_void>(),
            core::ptr::null_mut(),
        )
    };
    if rc != 0 || count <= 0 {
        return None;
    }
    // CAST: i32 → usize, count was just checked > 0 and is bounded
    // by `LEDGER_TEMPLATE_BUF_CAP`.
    #[allow(clippy::as_conversions, clippy::cast_sign_loss)]
    let returned = (count as usize).min(LEDGER_TEMPLATE_BUF_CAP);
    let target = b"graphics_footprint";
    for (i, row) in buf.iter().take(returned).enumerate() {
        // The XNU entry name is a 32-byte NUL-terminated ASCII field.
        // Decode by iterating until the first NUL; produce a
        // bounded-length `Vec<u8>` that lets us compare with `target`
        // without raw slice indexing.
        // CAST: c_char → u8, the kernel writes ASCII bytes; both have
        // identical wire representation.
        #[allow(clippy::as_conversions, clippy::cast_sign_loss)]
        let name_bytes: Vec<u8> = row
            .lti_name
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        if name_bytes == target {
            // CAST: usize → i32, `i < returned <= LEDGER_TEMPLATE_BUF_CAP`
            // (128); fits in i32.
            #[allow(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap
            )]
            return Some(i as i32);
        }
    }
    None
}

/// What one ledger read of a PID's `graphics_footprint` said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FootprintRead {
    /// The balance, in bytes (zero included).
    Bytes(u64),
    /// The caller's sandbox refused the read (`EPERM`).
    Denied,
    /// The process exited (`ESRCH`); it holds nothing that can be listed.
    Gone,
    /// The read failed, and the cause is neither a refusal nor the process
    /// exiting: any other `errno` or none, an empty entry array, an index
    /// past it, or a negative balance.
    Failed,
    /// The `graphics_footprint` entry index did not resolve, so no PID can
    /// be read.
    Unavailable,
}

/// Classify the `errno` of a failed ledger read.
///
/// Only a refusal (`EPERM`) is `Denied` and only `ESRCH` (the process
/// exited) is `Gone`. Every other `errno` and a missing one are `Failed`, so
/// a PID is never counted as protected, or as gone, for any other reason.
const fn footprint_from_errno(errno: Option<i32>) -> FootprintRead {
    match errno {
        Some(kinfo::EPERM) => FootprintRead::Denied,
        Some(kinfo::ESRCH) => FootprintRead::Gone,
        _ => FootprintRead::Failed,
    }
}

/// Read `graphics_footprint` (resident GPU-attributed bytes) for `pid`.
///
/// Resolves the entry index once, then calls
/// `ledger(LEDGER_ENTRY_INFO_V2, pid, buf, &count)` and reads the
/// `lei_balance` of that entry. `Unavailable` when the index did not
/// resolve, with no syscall. A failed call goes through
/// [`footprint_from_errno`]: `EPERM`, a read refused by the caller's
/// sandbox, is `Denied`; `ESRCH`, the process having exited, is `Gone`;
/// any other `errno` is `Failed`. A success with an empty entry array, an
/// index past it and a negative balance (not physically meaningful for a
/// bytes-unit entry) are `Failed` too.
///
/// `graphics_footprint` tracks resident Metal-written pages on Apple
/// Silicon UMA: writing every byte of a 256 MiB `MTLBuffer` increases
/// the entry by exactly 256 MiB (resident-bytes semantics, the macOS
/// analogue of Windows `WorkingSetSize` and Linux `VmRSS`).
#[allow(unsafe_code)]
fn read_graphics_footprint(pid: i32) -> FootprintRead {
    let idx =
        *GRAPHICS_FOOTPRINT_INDEX.get_or_init(|| resolve_graphics_footprint_index().unwrap_or(-1));
    let Ok(idx_usize) = usize::try_from(idx) else {
        return FootprintRead::Unavailable;
    };

    // Allocate one row per kernel entry. The kernel writes
    // `LEDGER_TEMPLATE_BUF_CAP` rows max; we size the buffer the same
    // way the template probe did so indexing into it is in-range.
    // SAFETY: `LedgerEntryInfo` is `#[repr(C)]` with only integer
    // fields (POD). All-zero is a valid bit pattern; per-element
    // `zeroed` initialises the whole `Vec` before the FFI sees it.
    let mut buf: Vec<LedgerEntryInfo> = (0..LEDGER_TEMPLATE_BUF_CAP)
        .map(|_| unsafe { core::mem::zeroed::<LedgerEntryInfo>() })
        .collect();
    // CAST: usize → i32, bounded by `LEDGER_TEMPLATE_BUF_CAP` (128).
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap
    )]
    let mut count: i32 = LEDGER_TEMPLATE_BUF_CAP as i32;
    // CAST: i32 PID → *mut c_void. The `ledger` syscall reinterprets
    // arg1's pointer-sized bits as the target PID (kernel convention,
    // mirroring `caddr_t(bitPattern: Int(pid))` on the Swift side).
    // PIDs are non-negative on macOS, so the sign-loss step is benign.
    #[allow(clippy::as_conversions, clippy::cast_sign_loss)]
    let pid_as_ptr = pid as usize as *mut c_void;
    // SAFETY: arg1 is the PID encoded as a pointer; arg2 is the
    // entry-array buffer; arg3 is the count in/out. A read refused by
    // the caller's sandbox returns -1 with `EPERM`; a PID that exited
    // returns -1 with `ESRCH`. `errno` is read below, before anything
    // else runs.
    let rc = unsafe {
        libsystem_ffi::ledger(
            LEDGER_ENTRY_INFO_V2,
            pid_as_ptr,
            buf.as_mut_ptr().cast::<c_void>(),
            (&raw mut count).cast::<c_void>(),
        )
    };
    if rc != 0 {
        return footprint_from_errno(last_errno());
    }
    if count <= 0 {
        return FootprintRead::Failed;
    }
    // CAST: i32 → usize, `count > 0` just checked.
    #[allow(clippy::as_conversions, clippy::cast_sign_loss)]
    let returned = (count as usize).min(LEDGER_TEMPLATE_BUF_CAP);
    if idx_usize >= returned {
        return FootprintRead::Failed;
    }
    // Use `.get()` to avoid panic-prone indexing; `idx_usize < returned`
    // already checked, so the `else` is defensive only.
    let Some(entry) = buf.get(idx_usize) else {
        return FootprintRead::Failed;
    };
    u64::try_from(entry.lei_balance).map_or(FootprintRead::Failed, FootprintRead::Bytes)
}

/// Run a single Metal device query for `idx`.
///
/// On Apple Silicon there is a single integrated GPU; this returns
/// `None` for any `idx != 0`. The non-zero case yields a
/// [`MetalQueryResult`] populated from `sysctl hw.memsize` for the
/// total and `sysctl machdep.cpu.brand_string` for the name. (Per-process
/// usage is not part of this device-wide query — see [`process_gpu_info`],
/// which reads `graphics_footprint` independently.)
pub(super) fn query(idx: u32) -> Option<MetalQueryResult> {
    if idx != 0 {
        return None;
    }
    let dedicated_video_memory = read_sysctl_u64(b"hw.memsize\0")?;
    let adapter_name =
        read_sysctl_string(b"machdep.cpu.brand_string\0").unwrap_or_else(|| "Apple GPU".into());
    // Falls back to total DRAM if the Metal driver cannot be loaded
    // (extremely rare on Apple Silicon; possible on very locked-down
    // environments). `dedicated_video_memory` is the conservative
    // upper bound on what `free_bytes` can be.
    let recommended_max_working_set =
        recommended_max_working_set_size().unwrap_or(dedicated_video_memory);
    Some(MetalQueryResult {
        dedicated_video_memory,
        recommended_max_working_set,
        adapter_name,
    })
}

/// Number of Metal devices visible — `Some(1)` on Apple Silicon,
/// `None` elsewhere (Intel Macs are out of scope for v0.2.2).
///
/// Detects Apple Silicon by reading `machdep.cpu.brand_string` and
/// looking for "Apple"; the alternative `hw.optional.arm64` sysctl
/// returns a 32-bit `int` and so cannot be read through the
/// [`read_sysctl_u64`] helper without a separate u32 variant.
pub(super) fn device_count() -> Option<u32> {
    let brand = read_sysctl_string(b"machdep.cpu.brand_string\0")?;
    if brand.contains("Apple") {
        Some(1)
    } else {
        None
    }
}

/// Calling-process PID via `std::process::id` — safe-stdlib path that
/// avoids the libSystem `getpid` FFI for the trivial self-PID case.
///
/// Kept as a separate fn so Step 3 / Step 4 share the same casting
/// annotation site.
fn process_self_pid() -> i32 {
    // CAST: u32 → i32, POSIX PIDs are non-negative i32; `std::process::id`
    // returns the same bit pattern as `getpid()` would.
    #[allow(clippy::as_conversions, clippy::cast_possible_wrap)]
    {
        std::process::id() as i32
    }
}

/// Per-process GPU memory usage for the calling PID on `device_index`.
///
/// Returns `None` for any `device_index != 0`. Otherwise reads
/// `graphics_footprint` from the BSD ledger for the calling PID and
/// wraps it in a [`crate::ProcessGpuInfo`] with [`crate::GpuQuerySource::Metal`]
/// as the source tag. The `GpuQuerySource::Metal` variant is added by
/// the `gpu_dispatcher_wiring` leaf; `cargo check` for this module
/// will fail until that leaf lands. The `cfg(all(target_os = "macos",
/// feature = "metal"))` gate on `mod metal;` ensures the macOS build
/// only succeeds once the variant exists.
pub(super) fn process_gpu_info(device_index: u32) -> Option<crate::ProcessGpuInfo> {
    if device_index != 0 {
        return None;
    }
    let self_pid = process_self_pid();
    let used_bytes = match read_graphics_footprint(self_pid) {
        FootprintRead::Bytes(bytes) => bytes,
        FootprintRead::Denied
        | FootprintRead::Gone
        | FootprintRead::Failed
        | FootprintRead::Unavailable => return None,
    };
    Some(crate::ProcessGpuInfo {
        used_bytes,
        is_per_process: true,
        source: crate::GpuQuerySource::Metal,
    })
}

/// Whether `pid` names a running process: `proc_pidpath` first (a path
/// means yes), then, when it gives no path, `sysctl` `KERN_PROC_PID`
/// (one record means yes, none means no). When `sysctl` is refused too,
/// `proc_pidpath`'s `ESRCH` means no and anything else (e.g. `EPERM`) is
/// `None`, "can't tell"; an unusable record is `None` as well. The rule
/// is [`super::kinfo::decide_exists`]. `None` too for a `pid` past
/// `i32::MAX`, which no macOS PID reaches. Backs
/// [`crate::gpu::process_exists`] on macOS.
pub(super) fn process_exists(pid: u32) -> Option<bool> {
    let pid = i32::try_from(pid).ok()?;
    decide_exists(proc_pidpath_lookup(pid), || kern_proc_pid_lookup(pid))
}

/// What `proc_pidpath` says about `pid`: `Found` when it returns a path,
/// else `Failed` with the `errno` it left.
#[allow(unsafe_code)]
fn proc_pidpath_lookup(pid: i32) -> PathLookup {
    let mut buf: [u8; PROC_PIDPATHINFO_MAXSIZE] = [0; PROC_PIDPATHINFO_MAXSIZE];
    // CAST: usize → u32, `PROC_PIDPATHINFO_MAXSIZE` is 4096; fits.
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let cap_u32 = PROC_PIDPATHINFO_MAXSIZE as u32;
    // SAFETY: `buf.as_mut_ptr` is valid for `PROC_PIDPATHINFO_MAXSIZE`
    // bytes (its declared length), and `cap_u32` tells the kernel so. It
    // writes at most that many bytes and returns the length, or 0 with
    // `errno` set; PID validity is the kernel's to judge.
    let len =
        unsafe { libsystem_ffi::proc_pidpath(pid, buf.as_mut_ptr().cast::<c_void>(), cap_u32) };
    if len > 0 {
        PathLookup::Found
    } else {
        PathLookup::Failed {
            errno: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        }
    }
}

/// The raw result of one `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID,
/// pid)` call: `(rc, errno, buffer, len)`, with `errno` 0 when `rc` is.
///
/// A buffer of one record is enough for a PID query; a record that
/// would not fit fails with `ENOMEM`, which classifies as `Unusable`
/// ("can't tell"). Kept apart from [`kern_proc_pid_lookup`] so the live
/// layout test can check the raw record.
#[allow(unsafe_code)]
fn kern_proc_pid_raw(pid: i32) -> (i32, i32, [u8; KINFO_PROC_SIZE], usize) {
    let mut mib = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid];
    let mut buf = [0_u8; KINFO_PROC_SIZE];
    let mut len = KINFO_PROC_SIZE;
    // CAST: usize → u32, a 4-element MIB; fits.
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let namelen = mib.len() as u32;
    // SAFETY: `mib` holds the `namelen` ints; `buf` is valid for
    // `len` bytes, and `len` is in/out (the kernel writes back how many
    // bytes it stored, never more than it was given); `newp`/`newlen`
    // are null/zero, a read-only query.
    let rc = unsafe {
        libsystem_ffi::sysctl(
            mib.as_mut_ptr(),
            namelen,
            buf.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    // Read `errno` before any other call can clobber it.
    (rc, errno_after(rc), buf, len)
}

/// What `sysctl` `KERN_PROC_PID` says about `pid`.
fn kern_proc_pid_lookup(pid: i32) -> PidLookup {
    let (rc, errno, buf, len) = kern_proc_pid_raw(pid);
    classify_kern_proc_pid(rc, errno, &buf, len, pid)
}

/// Resolve `pid`'s executable basename via `proc_pidpath`.
///
/// The basename is the final `/`-separated component of the full
/// executable path. `Err` carries the `errno` a failed call left, or 0 for
/// a path that is not UTF-8 or has an empty basename.
#[allow(unsafe_code)]
fn read_proc_pidpath_basename(pid: i32) -> Result<String, i32> {
    let mut buf: [u8; PROC_PIDPATHINFO_MAXSIZE] = [0; PROC_PIDPATHINFO_MAXSIZE];
    // CAST: usize → u32, `PROC_PIDPATHINFO_MAXSIZE` is 4096; fits.
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let cap_u32 = PROC_PIDPATHINFO_MAXSIZE as u32;
    // SAFETY: `buf.as_mut_ptr` is valid for `PROC_PIDPATHINFO_MAXSIZE`
    // bytes (its declared length). The kernel writes a NUL-terminated
    // path of at most `cap_u32` bytes and returns the length excluding
    // the NUL, or 0 with `errno` set (`ESRCH` for a stale PID, `EPERM`
    // for one refused by the caller's sandbox). PID validity is the
    // kernel's to judge.
    let len =
        unsafe { libsystem_ffi::proc_pidpath(pid, buf.as_mut_ptr().cast::<c_void>(), cap_u32) };
    if len <= 0 {
        return Err(last_errno().unwrap_or(0));
    }
    // CAST: i32 → usize, `len > 0` just checked and bounded by
    // `cap_u32`.
    #[allow(clippy::as_conversions, clippy::cast_sign_loss)]
    let len_usize = (len as usize).min(PROC_PIDPATHINFO_MAXSIZE);
    // Take only the bytes the kernel wrote (excludes terminator).
    let path_bytes = buf.get(..len_usize).ok_or(0)?;
    let path_str = core::str::from_utf8(path_bytes).map_err(|_| 0)?;
    // Extract basename — the substring after the final '/'.
    let basename = path_str.rsplit('/').next().unwrap_or(path_str);
    if basename.is_empty() {
        Err(0)
    } else {
        // BORROW: `to_owned` — `basename` is borrowed from the stack
        // buffer, which is dropped at function return.
        Ok(basename.to_owned())
    }
}

/// A process's name: `proc_pidpath`'s basename when it answered, else
/// `comm()` when the caller's sandbox refused it (`EPERM`).
///
/// `comm` is never called after a path, and never for a process that is
/// gone or whose failure is not a refusal. A name therefore comes from
/// `p_comm` only where `proc_pidpath` is refused, so it does not flip
/// between the two sources from one sample to the next.
fn name_after_pidpath(
    read: Result<String, i32>,
    comm: impl FnOnce() -> Option<String>,
) -> Option<String> {
    match read {
        Ok(name) => Some(name),
        Err(kinfo::EPERM) => comm(),
        Err(_) => None,
    }
}

/// The name held in a `p_comm` buffer: the bytes before the first NUL.
///
/// `None` when that is empty or not UTF-8. The kernel cuts `p_comm` at 16
/// bytes, which can fall inside a multibyte character; the valid prefix
/// before the cut is the name then. Only a name of exactly 16 bytes can
/// have been cut, so a shorter one that ends in an incomplete character is
/// not a name. A name that is valid UTF-8 is returned whole, including a
/// full 16-byte one.
fn comm_to_name(comm: &[u8]) -> Option<String> {
    let bytes = comm.split(|&byte| byte == 0).next().unwrap_or(comm);
    let name = match core::str::from_utf8(bytes) {
        Ok(name) => name,
        // An incomplete last sequence at 16 bytes is the kernel's cut; any
        // other invalid input is not a name.
        Err(e) if e.error_len().is_none() && bytes.len() == kinfo::P_COMM_SIZE - 1 => {
            core::str::from_utf8(bytes.get(..e.valid_up_to())?).ok()?
        }
        Err(_) => return None,
    };
    // BORROW: `to_owned` copies the name out of the caller's buffer.
    (!name.is_empty()).then(|| name.to_owned())
}

/// What `proc_listpids` said.
#[derive(Debug, PartialEq, Eq)]
enum LibprocPids {
    /// The PIDs it listed, at least one.
    Pids(Vec<i32>),
    /// The caller's sandbox refused the call (`EPERM`).
    Refused,
    /// The call failed for any other reason, or listed no PID.
    Failed,
}

/// Classify one `proc_listpids` result: the byte count it `written`
/// (`<= 0` on failure), the `errno` it left and the `pids` it listed.
///
/// Only `EPERM` is a refusal: `ESRCH`, `ENOMEM` and the rest are a
/// failure, so a transient libproc error never reads as a sandbox. A
/// success that lists no PID is a failure too, not an empty process table.
fn libproc_outcome(written: i32, errno: Option<i32>, pids: Vec<i32>) -> LibprocPids {
    if written <= 0 {
        return if errno == Some(kinfo::EPERM) {
            LibprocPids::Refused
        } else {
            LibprocPids::Failed
        };
    }
    if pids.is_empty() {
        LibprocPids::Failed
    } else {
        LibprocPids::Pids(pids)
    }
}

/// The `errno` of the failed call just made.
///
/// Call it first after the failing FFI call, before anything else can
/// overwrite `errno`.
fn last_errno() -> Option<i32> {
    std::io::Error::last_os_error().raw_os_error()
}

/// The `errno` for `classify_*`, which takes it as an `i32`: `0` when the
/// call just made returned `rc == 0`, else the `errno` it left, `0` when it
/// left none.
///
/// Call it first after the `sysctl`, before anything else can overwrite
/// `errno`.
fn errno_after(rc: i32) -> i32 {
    if rc == 0 {
        0
    } else {
        last_errno().unwrap_or(0)
    }
}

/// List every PID with `proc_listpids`.
///
/// Two phases: query the buffer size first, then fill. The PID count may
/// grow between the two calls; the iteration is capped at the buffer's
/// filled length. `errno` is read right after each failed call and goes
/// through [`libproc_outcome`].
#[allow(unsafe_code)]
fn list_libproc_pids() -> LibprocPids {
    // Phase 1 — size probe: `buffer = NULL, buffersize = 0` returns
    // the byte count the kernel would write.
    // SAFETY: `PROC_ALL_PIDS` is a documented selector; `typeinfo = 0`
    // means "any predicate"; `buffer = NULL`/`buffersize = 0` is the
    // documented size-probe convention.
    let size_bytes =
        unsafe { libsystem_ffi::proc_listpids(PROC_ALL_PIDS, 0, core::ptr::null_mut(), 0) };
    if size_bytes <= 0 {
        return libproc_outcome(size_bytes, last_errno(), Vec::new());
    }
    let Ok(size_usize) = usize::try_from(size_bytes) else {
        return LibprocPids::Failed;
    };
    let pid_count = size_usize / size_of::<i32>();
    if pid_count == 0 {
        return LibprocPids::Failed;
    }

    // Phase 2 — fill. Allocate `pid_count` i32 slots; the kernel may
    // see a slightly larger live PID count by the time it runs but
    // will not exceed the byte budget we pass.
    let mut pids: Vec<i32> = vec![0_i32; pid_count];
    // `pid_count * 4` is at most `size_bytes`, the kernel's own i32.
    let Ok(cap_bytes) = i32::try_from(pid_count * size_of::<i32>()) else {
        return LibprocPids::Failed;
    };
    // SAFETY: `pids.as_mut_ptr` is valid for `pid_count *
    // size_of::<i32>()` bytes (matches `cap_bytes`). The kernel
    // writes up to `cap_bytes` bytes worth of PIDs and returns the
    // actual byte count written.
    let written_bytes = unsafe {
        libsystem_ffi::proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr().cast::<c_void>(),
            cap_bytes,
        )
    };
    if written_bytes <= 0 {
        return libproc_outcome(written_bytes, last_errno(), Vec::new());
    }
    let Ok(written_usize) = usize::try_from(written_bytes) else {
        return LibprocPids::Failed;
    };
    let written_usize = written_usize.min(size_usize);
    // Keep only the slots the kernel filled.
    pids.truncate(written_usize / size_of::<i32>());
    libproc_outcome(written_bytes, None, pids)
}

/// How many probe-and-fill attempts [`list_kern_proc_all`] makes before it
/// gives up on a process table that keeps outgrowing its buffer.
const KERN_PROC_ALL_MAX_ATTEMPTS: usize = 4;

/// The divisor of the slack [`kern_proc_all_buffer_len`] adds to the probed
/// length: one `KERN_PROC_ALL_SLACK_DIVISOR`th of it.
const KERN_PROC_ALL_SLACK_DIVISOR: usize = 8;

/// The buffer length for the `KERN_PROC_ALL` fill, given the `probed`
/// length: `probed` plus `probed / KERN_PROC_ALL_SLACK_DIVISOR` slack,
/// rounded up to a whole number of `KINFO_PROC_SIZE` records and never
/// zero.
///
/// The kernel's probe already counts five spare records; the slack covers
/// a table that grows faster than that between the probe and the fill.
fn kern_proc_all_buffer_len(probed: usize) -> usize {
    probed
        .saturating_add(probed / KERN_PROC_ALL_SLACK_DIVISOR)
        .div_ceil(KINFO_PROC_SIZE)
        .max(1)
        .saturating_mul(KINFO_PROC_SIZE)
}

/// List every process with `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_ALL)`.
///
/// The records come through [`parse_kinfo_records`], the parser
/// `KERN_PROC_PID` uses. Each attempt is a probe with a null buffer, which
/// gives the length, then a fill into a buffer of [`kern_proc_all_buffer_len`]
/// bytes, judged by [`classify_kern_proc_all`]. A fill it calls `Retry` is
/// tried again, up to `KERN_PROC_ALL_MAX_ATTEMPTS` attempts in all; one it
/// calls `Failed`, a refusal included, is `None`.
#[allow(unsafe_code)]
fn list_kern_proc_all() -> Option<Vec<KinfoRecord>> {
    let mut mib = [CTL_KERN, KERN_PROC, KERN_PROC_ALL];
    // CAST: usize → u32, a 3-element MIB; fits.
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
    let namelen = mib.len() as u32;
    for _ in 0..KERN_PROC_ALL_MAX_ATTEMPTS {
        let mut probed: usize = 0;
        // SAFETY: `mib` holds the `namelen` ints; `oldp` is null, which
        // asks the kernel to write the length it would return into
        // `probed`, a live `usize`, and to copy nothing; `newp`/`newlen`
        // are null/zero, a read-only query.
        let rc = unsafe {
            libsystem_ffi::sysctl(
                mib.as_mut_ptr(),
                namelen,
                core::ptr::null_mut(),
                &raw mut probed,
                core::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        let mut buf = vec![0_u8; kern_proc_all_buffer_len(probed)];
        let mut len = buf.len();
        // SAFETY: `mib` holds the `namelen` ints; `buf` is valid for
        // `len` bytes, and `len` is in/out (the kernel writes back how
        // many bytes it stored, never more than it was given);
        // `newp`/`newlen` are null/zero, a read-only query.
        let rc = unsafe {
            libsystem_ffi::sysctl(
                mib.as_mut_ptr(),
                namelen,
                buf.as_mut_ptr().cast::<c_void>(),
                &raw mut len,
                core::ptr::null_mut(),
                0,
            )
        };
        // Read `errno` before any other call can clobber it.
        match classify_kern_proc_all(rc, errno_after(rc), &buf, len) {
            KernProcAllAttempt::Records(records) => return Some(records),
            // EXPLICIT: the table outgrew the buffer; probe and fill again.
            KernProcAllAttempt::Retry => {}
            KernProcAllAttempt::Failed => return None,
        }
    }
    None
}

/// Trust a `KERN_PROC_ALL` listing only when `self_lookup` shows the
/// kernel's `kinfo_proc` is 648 bytes.
///
/// A kernel with another record size answers the probe-sized fill with
/// whole records of its own size and `rc` 0, and `parse_kinfo_records`
/// rejects that only when the total is not a multiple of 648. The caller's
/// own `KERN_PROC_PID` lookup is `PidLookup::Record` only for exactly one
/// 648-byte record that names it, so any other answer distrusts the
/// listing. A missing or empty listing is `None` without asking
/// `self_lookup`, since the caller is always in the table.
fn trust_kinfo_listing(
    listing: Option<Vec<KinfoRecord>>,
    self_lookup: impl FnOnce() -> PidLookup,
) -> Option<Vec<KinfoRecord>> {
    let records = listing.filter(|records| !records.is_empty())?;
    matches!(self_lookup(), PidLookup::Record).then_some(records)
}

/// Enumerate PIDs: `proc_listpids` first; `sysctl(KERN_PROC_ALL)` only when
/// libproc says the caller's sandbox refused it. `None` when no enumeration
/// worked.
fn list_pids() -> Option<Vec<i32>> {
    let self_lookup = || kern_proc_pid_lookup(process_self_pid());
    match list_libproc_pids() {
        LibprocPids::Pids(pids) => Some(pids),
        LibprocPids::Refused => {
            let records = trust_kinfo_listing(list_kern_proc_all(), self_lookup)?;
            Some(records.into_iter().map(|record| record.pid).collect())
        }
        LibprocPids::Failed => None,
    }
}

/// The `p_comm` bytes of `pid`, from its `KERN_PROC_PID` record.
///
/// The source of a name where `proc_pidpath` was refused. `None` unless
/// [`classify_kern_proc_pid`] says the call is a record for `pid`.
fn kern_proc_pid_comm(pid: i32) -> Option<Vec<u8>> {
    let (rc, errno, buf, len) = kern_proc_pid_raw(pid);
    if classify_kern_proc_pid(rc, errno, &buf, len, pid) != PidLookup::Record {
        return None;
    }
    let record = parse_kinfo_records(buf.get(..len)?)?.into_iter().next()?;
    Some(record.comm)
}

/// What reading every listed PID's ledger found.
#[derive(Debug, PartialEq, Eq)]
struct ReadTally {
    /// `(pid, bytes)` for every PID holding a non-zero balance, the
    /// caller's own included, in enumeration order.
    found: Vec<(i32, u64)>,
    /// The PIDs whose read the sandbox refused, in enumeration order. Not
    /// the caller and not a PID that is gone.
    denied: Vec<u32>,
    /// How many PIDs other than the caller were read, zero balances
    /// included.
    others_read: usize,
}

/// Fold per-PID reads into a [`ReadTally`]. `None` when a read is
/// `Unavailable`: nothing can be read at all. `None` too when no other
/// process was read, none was refused and at least one read, the caller's
/// own included, `Failed`: nothing says why, so the list is not an answer.
///
/// The caller's own read adds its entry but never counts as another process
/// read, and its own denial is not listed.
fn tally_reads(
    self_pid: i32,
    reads: impl Iterator<Item = (i32, FootprintRead)>,
) -> Option<ReadTally> {
    let mut tally = ReadTally {
        found: Vec::new(),
        denied: Vec::new(),
        others_read: 0,
    };
    let mut failed = 0_usize;
    // EXPLICIT: a stateful fold, not an iterator chain: the caller's own read
    // counts differently from the others, and `Unavailable`, like the failed
    // count, abandons the tally.
    for (pid, read) in reads {
        match read {
            FootprintRead::Unavailable => return None,
            FootprintRead::Bytes(bytes) => {
                if pid != self_pid {
                    tally.others_read += 1;
                }
                if bytes > 0 {
                    tally.found.push((pid, bytes));
                }
            }
            FootprintRead::Denied => {
                if pid != self_pid {
                    tally.denied.extend(u32::try_from(pid).ok());
                }
            }
            // EXPLICIT: the process is gone; it is neither read nor denied.
            FootprintRead::Gone => {}
            FootprintRead::Failed => failed += 1,
        }
    }
    if tally.others_read == 0 && tally.denied.is_empty() && failed > 0 {
        return None;
    }
    Some(tally)
}

/// The processes holding GPU memory, and what the caller's sandbox kept
/// from it.
pub(super) struct MetalProcessList {
    /// One row per PID holding GPU memory, the caller's own included.
    pub(super) entries: Vec<crate::GpuProcessEntry>,
    /// The PIDs whose ledger read the sandbox refused, in enumeration
    /// order, excluding the caller, gone PIDs and PIDs that are not
    /// positive.
    pub(super) denied_pids: Vec<u32>,
    /// How many PIDs other than the caller were read, zero balances
    /// included.
    pub(super) others_read: usize,
}

/// Enumerate every process on `device_index` and read each one's
/// `graphics_footprint`: every process the caller's sandbox lets it read.
///
/// `None` when:
/// - `device_index` is not 0;
/// - `proc_listpids` fails without being refused: an `errno` other than
///   `EPERM`, no `errno`, a size too small for one PID, or a success that
///   lists no PID (`sysctl` is not tried);
/// - `proc_listpids` is refused and `sysctl(KERN_PROC_ALL)` is refused,
///   fails, or gives a listing the record-size guard distrusts;
/// - the ledger entry index did not resolve;
/// - no other process was read, none was refused and at least one failed.
///
/// A PID whose read is refused is in `denied_pids`; a PID that is gone, or
/// holds a zero balance, makes no row (mirrors NVML's per-process filter on
/// Linux); a PID that is not positive is skipped before its read. Names
/// come from [`name_after_pidpath`], with `p_comm` as the fallback. The
/// caller decides from `others_read` and `denied_pids` whether the list is
/// readable at all.
pub(super) fn list_processes(device_index: u32) -> Option<MetalProcessList> {
    if device_index != 0 {
        return None;
    }
    let pids = list_pids()?;
    let tally = tally_reads(
        process_self_pid(),
        pids.into_iter()
            .filter(|&pid| pid > 0)
            .map(|pid| (pid, read_graphics_footprint(pid))),
    )?;
    let mut entries: Vec<crate::GpuProcessEntry> = Vec::new();
    for &(pid, used_bytes) in &tally.found {
        let Ok(pid_u32) = u32::try_from(pid) else {
            continue;
        };
        let name = name_after_pidpath(read_proc_pidpath_basename(pid), || {
            kern_proc_pid_comm(pid).and_then(|comm| comm_to_name(&comm))
        });
        entries.push(crate::GpuProcessEntry {
            pid: pid_u32,
            name,
            used_bytes,
            // Apple Silicon UMA is a single physical pool — there is
            // no separate shared budget to spill into (see
            // GpuProcessEntry docs).
            shared_used_bytes: 0,
            source: crate::GpuQuerySource::Metal,
        });
    }
    Some(MetalProcessList {
        entries,
        denied_pids: tally.denied,
        others_read: tally.others_read,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::gpu::kinfo::parse_kinfo_records;
    use std::io::Write as _;
    use std::os::unix::ffi::OsStrExt;

    /// This process's PID.
    fn me() -> i32 {
        i32::try_from(std::process::id()).unwrap()
    }

    /// The first 16 bytes (`MAXCOMLEN`) of a path's file name.
    fn comm_of(path: &std::path::Path) -> Vec<u8> {
        // BORROW: `as_bytes` views the file name's bytes in place.
        let name = path.file_name().unwrap().as_bytes();
        // BORROW: `to_vec` copies the name out of `path`.
        name.get(..name.len().min(kinfo::P_COMM_SIZE - 1))
            .unwrap()
            .to_vec()
    }

    /// Assert `comm` is this process's `p_comm`: the name `execve` was
    /// given, which is the executable's file name or `argv[0]`'s basename.
    fn assert_is_my_comm(comm: &[u8]) {
        let exe = comm_of(&std::env::current_exe().unwrap());
        let argv0 = std::env::args_os()
            .next()
            .map(|a| comm_of(std::path::Path::new(&a)))
            .unwrap_or_default();
        assert!(
            comm == exe || comm == argv0,
            "comm {:?}, exe {:?}, argv[0] {:?}",
            String::from_utf8_lossy(comm),
            String::from_utf8_lossy(&exe),
            String::from_utf8_lossy(&argv0)
        );
    }

    /// Anchors the 648-byte layout on the running kernel: synthetic
    /// buffers in `kinfo::tests` prove only that the parser agrees with
    /// itself. Run natively and under Rosetta 2
    /// (`--target x86_64-apple-darwin`).
    #[test]
    fn kern_proc_pid_record_matches_the_kernel_for_this_process() {
        let me = std::process::id();
        let (rc, errno, buf, len) = kern_proc_pid_raw(i32::try_from(me).unwrap());
        assert_eq!(rc, 0, "KERN_PROC_PID refused, errno {errno}");
        assert_eq!(len, 648);
        let records = parse_kinfo_records(buf.get(..len).unwrap()).unwrap();
        assert_eq!(records.len(), 1, "{records:?}");
        let record = records.first().unwrap();
        assert_eq!(u32::try_from(record.pid).ok(), Some(me));
        assert_is_my_comm(&record.comm);
    }

    #[test]
    fn list_pids_holds_this_process() {
        let pids = list_pids().unwrap();
        assert!(
            pids.contains(&me()),
            "{} PIDs, none for this process",
            pids.len()
        );
    }

    #[test]
    fn footprint_from_errno_only_eperm_is_denied() {
        // Literals, so a wrong `kinfo::EPERM` or `kinfo::ESRCH` fails here.
        // `ESRCH` is `Gone`; `EINVAL`, `ENOMEM` and no `errno` are `Failed`,
        // never `Denied`.
        assert_eq!(footprint_from_errno(Some(1)), FootprintRead::Denied);
        assert_eq!(footprint_from_errno(Some(3)), FootprintRead::Gone);
        for errno in [Some(22), Some(12), None] {
            assert_eq!(
                footprint_from_errno(errno),
                FootprintRead::Failed,
                "errno {errno:?}"
            );
        }
    }

    #[test]
    fn tally_reads_counts_every_read_pid_and_lists_a_row_for_a_non_zero_balance() {
        let tally = tally_reads(
            me(),
            [
                (1, FootprintRead::Gone),
                (3, FootprintRead::Bytes(0)),
                (4, FootprintRead::Bytes(7)),
            ]
            .into_iter(),
        )
        .unwrap();
        // A gone PID is neither read nor denied, and a zero balance is read
        // but makes no row.
        assert_eq!(tally.found, vec![(4, 7)]);
        assert_eq!(tally.others_read, 2);
        assert!(tally.denied.is_empty(), "{:?}", tally.denied);
    }

    #[test]
    fn tally_reads_does_not_count_the_caller_as_denied_or_as_another_process() {
        let tally = tally_reads(
            me(),
            [
                (me(), FootprintRead::Bytes(16384)),
                (7, FootprintRead::Denied),
                (me(), FootprintRead::Denied),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(tally.found, vec![(me(), 16384)]);
        assert_eq!(tally.denied, vec![7]);
        assert_eq!(tally.others_read, 0);
    }

    #[test]
    fn tally_reads_is_none_when_nothing_was_read_nothing_denied_and_a_read_failed() {
        let failed = || (me(), FootprintRead::Failed);
        // Only failures, the caller's own included, say nothing.
        let only_failed = tally_reads(
            me(),
            [
                failed(),
                (7, FootprintRead::Failed),
                (8, FootprintRead::Gone),
            ]
            .into_iter(),
        );
        assert_eq!(only_failed, None);
        // The caller's own failure counts, and so do failures beside a read
        // of the caller alone: that is the empty list again.
        let own_failed = tally_reads(me(), [failed(), (8, FootprintRead::Gone)].into_iter());
        assert_eq!(own_failed, None);
        let own_read = tally_reads(
            me(),
            [(me(), FootprintRead::Bytes(5)), (7, FootprintRead::Failed)].into_iter(),
        );
        assert_eq!(own_read, None);
        // One refusal, or one other read, among the failures is an answer.
        let with_denied = tally_reads(me(), [failed(), (7, FootprintRead::Denied)].into_iter());
        assert_eq!(with_denied.map(|tally| tally.denied), Some(vec![7]));
        let with_read = tally_reads(me(), [failed(), (7, FootprintRead::Bytes(0))].into_iter());
        assert_eq!(with_read.map(|tally| tally.others_read), Some(1));
    }

    #[test]
    fn tally_reads_is_none_when_the_index_is_unavailable() {
        let tally = tally_reads(
            me(),
            [
                (1, FootprintRead::Bytes(5)),
                (2, FootprintRead::Unavailable),
            ]
            .into_iter(),
        );
        assert_eq!(tally, None);
    }

    #[test]
    fn name_after_pidpath_uses_comm_only_when_eperm() {
        // Literals, so a wrong `kinfo::EPERM` fails here. A path answers
        // first; `ESRCH` and any other `errno` give no name.
        // BORROW: `to_owned` builds the `String` from a literal.
        let comm = || Some("p_comm".to_owned());
        assert_eq!(name_after_pidpath(Err(1), comm), comm());
        assert_eq!(
            // BORROW: `to_owned` builds the `String`s from literals.
            name_after_pidpath(Ok("Safari".to_owned()), comm),
            Some("Safari".to_owned())
        );
        assert_eq!(name_after_pidpath(Err(3), comm), None);
        assert_eq!(name_after_pidpath(Err(0), comm), None);
    }

    #[test]
    fn comm_to_name_cuts_at_nul_and_rejects_what_is_not_a_name() {
        // BORROW: `to_owned` builds the expected `String` from a literal.
        assert_eq!(comm_to_name(b"abc\0def"), Some("abc".to_owned()));
        // A full 16-byte name comes back whole.
        assert_eq!(
            comm_to_name(b"abcdefghijklmnop"),
            // BORROW: `to_owned` builds the expected `String` from a literal.
            Some("abcdefghijklmnop".to_owned())
        );
        assert_eq!(comm_to_name(b""), None);
        assert_eq!(comm_to_name(&[0_u8; 17]), None);
        assert_eq!(comm_to_name(b"ab\xffcd"), None);
    }

    #[test]
    fn comm_to_name_keeps_the_valid_prefix_of_a_cut_char() {
        // Each character is 3 bytes; the kernel's 16-byte cut falls inside
        // the sixth.
        // BORROW: `as_bytes` views the literal's bytes in place.
        let full = "日本語プロセス名テスト".as_bytes();
        assert_eq!(
            comm_to_name(full.get(..16).unwrap()),
            // BORROW: `to_owned` builds the expected `String` from a literal.
            Some("日本語プロ".to_owned())
        );
        // A lead byte with nothing after it is a cut only at 16 bytes; a
        // continuation byte after `ab` is invalid.
        assert_eq!(comm_to_name(b"ab\xe3"), None);
        assert_eq!(comm_to_name(b"ab\x80"), None);
    }

    #[test]
    fn libproc_outcome_only_eperm_is_refused() {
        // Literals, so a wrong `kinfo::EPERM` fails here.
        assert_eq!(libproc_outcome(0, Some(1), vec![]), LibprocPids::Refused);
        // `ESRCH`, `ENOMEM`, `EINVAL`, none: a failure, never a sandbox.
        for errno in [Some(3), Some(12), Some(22), Some(0), None] {
            assert_eq!(
                libproc_outcome(0, errno, vec![]),
                LibprocPids::Failed,
                "errno {errno:?}"
            );
        }
        // `errno` counts only when the call failed.
        assert_eq!(
            libproc_outcome(4, Some(1), vec![7]),
            LibprocPids::Pids(vec![7])
        );
    }

    #[test]
    fn libproc_outcome_an_empty_list_is_failed() {
        assert_eq!(libproc_outcome(8, None, vec![]), LibprocPids::Failed);
    }

    #[test]
    fn kern_proc_pid_comm_returns_this_process_name() {
        assert_is_my_comm(&kern_proc_pid_comm(me()).unwrap());
    }

    #[test]
    fn kern_proc_pid_comm_is_none_for_a_dead_pid() {
        assert_eq!(kern_proc_pid_comm(2_147_483_647), None);
    }

    #[test]
    fn trust_kinfo_listing_keeps_records_only_when_self_is_one_648_byte_record() {
        let listing = || {
            Some(vec![KinfoRecord {
                pid: 5,
                comm: b"abc".to_vec(),
            }])
        };
        // Positive control: the caller's own lookup is one record.
        assert_eq!(
            trust_kinfo_listing(listing(), || PidLookup::Record),
            listing()
        );
        // A kernel with a larger record answers `ENOMEM` to `KERN_PROC_PID`.
        assert_eq!(
            trust_kinfo_listing(listing(), || {
                classify_kern_proc_pid(-1, 12, &[0_u8; 648], 0, me())
            }),
            None
        );
        // No listing, or an empty one, is not trusted either.
        assert_eq!(trust_kinfo_listing(None, || PidLookup::Record), None);
        assert_eq!(
            trust_kinfo_listing(Some(Vec::new()), || PidLookup::Record),
            None
        );
    }

    #[test]
    fn list_kern_proc_all_holds_this_process() {
        let listing = list_kern_proc_all();
        if listing.is_none() && last_errno() == Some(kinfo::EPERM) {
            let _ = writeln!(
                std::io::stderr().lock(),
                "metal tests: kern.proc.all refused (EPERM), skipping"
            );
            return;
        }
        let records = listing.unwrap_or_default();
        let mine = records.iter().find(|record| record.pid == me());
        assert!(
            mine.is_some(),
            "{} records, none for this process",
            records.len()
        );
        assert_is_my_comm(&mine.map(|record| record.comm.clone()).unwrap_or_default());
    }
}
