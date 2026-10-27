//! `cudahold` — holds GPU memory through the `CUDA` driver API, so
//! hypomnesis has a compute process to list. Not part of the published
//! crate.
//!
//! It loads the driver library every NVIDIA driver installs
//! (`nvcuda.dll` on Windows, `libcuda.so.1` on Linux) and needs no `CUDA`
//! toolkit, which is what a rented field-check box lacks. It retains the
//! device's primary context, allocates `MIB` MiB, writes it once so the
//! memory is really committed, prints its PID, and holds until Enter (or
//! for `HOLD_SECS` seconds). Unlike `spillforge`, which goes through
//! `D3D11`, it runs on a datacenter GPU in `TCC` mode, where there is no
//! `D3D` device.
//!
//! ```sh
//! cargo build --release --manifest-path tools/cudahold/Cargo.toml
//! tools/cudahold/target/release/cudahold [MIB] [HOLD_SECS] [DEVICE]
//! ```
//!
//! `MIB` defaults to 1024, `HOLD_SECS` to 0 (hold until Enter), `DEVICE`
//! to 0. Copy the binary under another name to tell two holders apart in
//! a listing.

use std::ffi::c_void;
use std::io::BufRead;
use std::process::ExitCode;
use std::time::Duration;

use libloading::{Library, Symbol};

#[cfg(windows)]
const CUDA_LIBRARY: &str = "nvcuda.dll";
#[cfg(not(windows))]
const CUDA_LIBRARY: &str = "libcuda.so.1";

const MIB: usize = 1024 * 1024;

type CuInit = unsafe extern "C" fn(u32) -> i32;
type CuDeviceGet = unsafe extern "C" fn(*mut i32, i32) -> i32;
type CuPrimaryCtxRetain = unsafe extern "C" fn(*mut *mut c_void, i32) -> i32;
type CuCtxSetCurrent = unsafe extern "C" fn(*mut c_void) -> i32;
type CuMemAlloc = unsafe extern "C" fn(*mut u64, usize) -> i32;
type CuMemsetD8 = unsafe extern "C" fn(u64, u8, usize) -> i32;
type CuCtxSynchronize = unsafe extern "C" fn() -> i32;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mib: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(1024);
    let hold_secs: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    let device: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);

    match hold(mib, hold_secs, device) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cudahold: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Allocate `mib` MiB on `device` and hold it; the memory is released when
/// the process exits.
fn hold(mib: usize, hold_secs: u64, device: i32) -> Result<(), String> {
    let bytes = mib
        .checked_mul(MIB)
        .ok_or_else(|| format!("{mib} MiB does not fit in usize"))?;

    // SAFETY: loading the CUDA driver runs its initialisers, as any CUDA
    // program does at start-up.
    let lib = unsafe { Library::new(CUDA_LIBRARY) }
        .map_err(|e| format!("cannot load {CUDA_LIBRARY} (no NVIDIA driver?): {e}"))?;

    // SAFETY: each symbol is resolved with the signature of its CUDA driver
    // API declaration (cuda.h); `lib` outlives every call below.
    unsafe {
        let cu_init: Symbol<'_, CuInit> = symbol(&lib, b"cuInit\0")?;
        let cu_device_get: Symbol<'_, CuDeviceGet> = symbol(&lib, b"cuDeviceGet\0")?;
        let cu_primary_ctx_retain: Symbol<'_, CuPrimaryCtxRetain> =
            symbol(&lib, b"cuDevicePrimaryCtxRetain\0")?;
        let cu_ctx_set_current: Symbol<'_, CuCtxSetCurrent> = symbol(&lib, b"cuCtxSetCurrent\0")?;
        let cu_mem_alloc: Symbol<'_, CuMemAlloc> = symbol(&lib, b"cuMemAlloc_v2\0")?;
        let cu_memset_d8: Symbol<'_, CuMemsetD8> = symbol(&lib, b"cuMemsetD8_v2\0")?;
        let cu_ctx_synchronize: Symbol<'_, CuCtxSynchronize> = symbol(&lib, b"cuCtxSynchronize\0")?;

        check("cuInit", cu_init(0))?;
        let mut dev: i32 = 0;
        check("cuDeviceGet", cu_device_get(&raw mut dev, device))?;
        let mut ctx: *mut c_void = std::ptr::null_mut();
        check(
            "cuDevicePrimaryCtxRetain",
            cu_primary_ctx_retain(&raw mut ctx, dev),
        )?;
        check("cuCtxSetCurrent", cu_ctx_set_current(ctx))?;
        let mut ptr: u64 = 0;
        check("cuMemAlloc_v2", cu_mem_alloc(&raw mut ptr, bytes))?;
        check("cuMemsetD8_v2", cu_memset_d8(ptr, 0xA5, bytes))?;
        check("cuCtxSynchronize", cu_ctx_synchronize())?;
    }

    let pid = std::process::id();
    if hold_secs == 0 {
        println!("cudahold: pid {pid} holds {mib} MiB on device {device}; press Enter to release");
        let mut line = String::new();
        // EXPLICIT: any outcome of the read (a line, EOF, an error) ends the hold.
        let _ = std::io::stdin().lock().read_line(&mut line);
    } else {
        println!("cudahold: pid {pid} holds {mib} MiB on device {device} for {hold_secs} s");
        std::thread::sleep(Duration::from_secs(hold_secs));
    }
    Ok(())
}

/// Resolve `name` (NUL-terminated) in `lib`.
///
/// # Safety
///
/// `T` must be the symbol's real function type.
unsafe fn symbol<'lib, T>(lib: &'lib Library, name: &[u8]) -> Result<Symbol<'lib, T>, String> {
    // SAFETY: forwarded to the caller.
    unsafe { lib.get(name) }.map_err(|e| {
        format!(
            "{CUDA_LIBRARY} has no {}: {e}",
            String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name))
        )
    })
}

/// `Ok` for `CUDA_SUCCESS` (0), else the call's name and its `CUresult`.
fn check(call: &str, code: i32) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{call} returned CUresult {code}"))
    }
}
