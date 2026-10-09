// SPDX-License-Identifier: MIT OR Apache-2.0

//! GPU memory measurement dispatchers and backend modules.
//!
//! Each backend (`nvml`, `dxgi`, `nvidia_smi`) is gated by a Cargo
//! feature; the dispatchers below try them in priority order and surface
//! the first success. Backend modules are crate-private — public access
//! is via the five dispatchers ([`device_count`], [`device_info`],
//! [`process_gpu_info`], [`gpu_processes`], [`gpu_process_listing`]), plus
//! [`process_exists`] (since v0.2.13), which reuses the Windows and macOS
//! backends' process lookups to answer whether a PID names a running process
//! at all.

use crate::{
    GpuDeviceInfo, GpuProcessEntry, GpuProcessListing, HypomnesisError, ProcessGpuInfo, Result,
};

#[cfg(any(
    feature = "nvml",
    all(windows, feature = "dxgi"),
    all(windows, feature = "pdh"),
    feature = "nvidia-smi-fallback"
))]
use crate::GpuQuerySource;

#[cfg(feature = "nvml")]
mod nvml;

#[cfg(all(windows, feature = "dxgi"))]
mod dxgi;

// `pub(crate)` (not private like the other backends): `crate::spill`'s
// Windows arm sits on this module's adapter-wide query. Still invisible
// outside the crate.
#[cfg(all(windows, feature = "pdh"))]
pub(crate) mod pdh;

#[cfg(feature = "nvidia-smi-fallback")]
mod nvidia_smi;

// Linux names for `NVML`'s process rows; see its module docs for why
// `comm` alone is not enough.
#[cfg(all(target_os = "linux", feature = "nvml"))]
mod proc_name;

#[cfg(all(target_os = "macos", feature = "metal"))]
mod metal;

// `kinfo_proc` records and the macOS `process_exists` lookup rule. Pure
// byte parsing, so it also builds in every test build, where the Linux
// and Windows CI jobs run its offset and length checks.
#[cfg(any(all(target_os = "macos", feature = "metal"), test))]
mod kinfo;

/// Number of NVIDIA GPUs visible to `NVML` (`NVML`-canonical ordering).
///
/// On Windows the count uses `NVML`; if `NVML` is unavailable, the
/// `DXGI` fallback counts NVIDIA adapters with non-zero dedicated `VRAM`.
///
/// # Errors
///
/// Returns [`HypomnesisError::NoGpuSource`] if no enumeration backend
/// is enabled, or if every enabled backend failed to report a count.
#[allow(clippy::missing_const_for_fn)] // const only when no features are enabled (body collapses)
pub fn device_count() -> Result<u32> {
    #[cfg(all(target_os = "macos", feature = "metal"))]
    if let Some(count) = metal::device_count() {
        return Ok(count);
    }

    #[cfg(feature = "nvml")]
    if let Some(count) = nvml::device_count() {
        return Ok(count);
    }

    #[cfg(all(windows, feature = "dxgi"))]
    if let Some(count) = dxgi::device_count() {
        return Ok(count);
    }

    // nvidia-smi fallback for device_count is intentionally not wired —
    // counting via `nvidia-smi -L` is more brittle than NVML/DXGI and
    // adds a subprocess invocation for what's typically a metadata call.

    Err(HypomnesisError::NoGpuSource)
}

/// Device-wide info for a specific GPU index (`NVML`-canonical ordering).
///
/// Source priority:
/// 1. `NVML` for `total` / `free` / `used` numerics, augmented with
///    `DXGI`'s `Description`-derived `name` on Windows when available.
/// 2. `DXGI` alone (Windows only): falls back when `NVML` is
///    unavailable. **Imprecision note:** in this path `used_bytes`
///    is set to DXGI's `CurrentUsage`, which is per-process — not the
///    device-wide sum. Treat it as a lower bound. This path is rare
///    (it requires `NVML` to fail while `DXGI` works, e.g. partial
///    driver installs).
/// 3. `nvidia-smi` subprocess fallback (Phase B+1) — device-wide proper.
///
/// iGPUs and the Microsoft Basic Render Driver are skipped during the
/// `DXGI` adapter walk (filtered by NVIDIA vendor ID `0x10DE` and
/// non-zero dedicated `VRAM`).
///
/// # Errors
///
/// Returns [`HypomnesisError::DeviceIndexOutOfRange`] if `index` is past
/// the device count reported by `NVML`, `DXGI` or `Metal`.
/// Returns [`HypomnesisError::NoGpuSource`] if no backend can satisfy
/// the query.
#[allow(unused_variables)] // `index` unused when no GPU backend feature is enabled
#[allow(clippy::missing_const_for_fn)] // const only when no features are enabled (body collapses)
pub fn device_info(index: u32) -> Result<GpuDeviceInfo> {
    #[cfg(all(target_os = "macos", feature = "metal"))]
    if let Some(d) = metal::query(index) {
        // On Apple Silicon UMA the discrete-GPU "free vs total" mental
        // model doesn't apply: `hw.memsize` is the physical DRAM ceiling
        // (acts as `total`), and Apple's own
        // `MTLDevice.recommendedMaxWorkingSetSize` is the kernel-projected
        // soft cap on what the GPU can hold resident with good
        // performance (acts as `free`). `used = total - free` is the
        // implied non-GPU reserve.
        return Ok(GpuDeviceInfo {
            index,
            name: Some(d.adapter_name),
            total_bytes: d.dedicated_video_memory,
            free_bytes: d.recommended_max_working_set,
            used_bytes: d
                .dedicated_video_memory
                .saturating_sub(d.recommended_max_working_set),
            // Apple UMA exposes no driver/firmware carve-out figure.
            reserved_bytes: None,
            // macOS has no NVIDIA driver.
            driver_version: None,
        });
    }

    #[cfg(feature = "nvml")]
    if let Some(snap) = nvml::query(index) {
        #[cfg(all(windows, feature = "dxgi"))]
        let name = dxgi::adapter_name(index).or(snap.device_name);
        #[cfg(not(all(windows, feature = "dxgi")))]
        let name = snap.device_name;

        return Ok(GpuDeviceInfo {
            index,
            name,
            total_bytes: snap.device_total,
            free_bytes: snap.device_free,
            used_bytes: snap.device_used,
            // Best-effort v2 carve-out; `None` on pre-R510 drivers. The
            // total/free/used above stay the v1, `nvidia-smi`-consistent
            // figures regardless.
            reserved_bytes: snap.reserved_bytes,
            // From nvmlSystemGetDriverVersion, read in the same NVML session.
            driver_version: snap.driver_version,
        });
    }

    // DXGI-alone fallback (Windows only). Loose semantics: CurrentUsage
    // is per-process; treated here as a lower bound on device-wide used.
    #[cfg(all(windows, feature = "dxgi"))]
    if let Some(d) = dxgi::query(index) {
        return Ok(GpuDeviceInfo {
            index,
            name: d.adapter_name,
            total_bytes: d.dedicated_video_memory,
            free_bytes: d.dedicated_video_memory.saturating_sub(d.current_usage),
            used_bytes: d.current_usage,
            // DXGI does not expose the NVML driver/firmware reservation.
            reserved_bytes: None,
            // DXGI exposes no NVIDIA-branded driver version string.
            driver_version: None,
        });
    }

    // nvidia-smi fallback (device-wide proper, no name).
    #[cfg(feature = "nvidia-smi-fallback")]
    if let Some(result) = nvidia_smi::query(index) {
        return Ok(GpuDeviceInfo {
            index,
            name: None,
            total_bytes: result.total_bytes,
            free_bytes: result.total_bytes.saturating_sub(result.used_bytes),
            used_bytes: result.used_bytes,
            // `nvidia-smi --query-gpu=memory.total` already reports the
            // usable figure; it has no separate reserved column.
            reserved_bytes: None,
            // Unlike reserved_bytes, nvidia-smi CAN supply this (a
            // `driver_version` column on the same --query-gpu call).
            driver_version: result.driver_version,
        });
    }

    bounds_check(index)?;
    Err(HypomnesisError::NoGpuSource)
}

/// Per-process GPU memory used by the calling process on the given device.
///
/// Source priority:
/// 1. `DXGI` on Windows — the only WDDM-aware per-process source.
/// 2. `NVML` (Linux primary; on Windows it returns `NVML_VALUE_NOT_AVAILABLE`
///    for compute processes under WDDM, so this path is effectively Linux-only).
/// 3. `nvidia-smi` device-wide fallback (Phase B+1) — sets
///    `is_per_process = false` because `nvidia-smi` cannot break the
///    figure down per process.
///
/// # Errors
///
/// Returns [`HypomnesisError::DeviceIndexOutOfRange`] if `device_index`
/// is past the device count reported by `NVML`, `DXGI` or `Metal`.
/// Returns [`HypomnesisError::NoGpuSource`] if every available backend fails.
#[allow(unused_variables)] // `device_index` unused when no GPU backend feature is enabled
#[allow(clippy::missing_const_for_fn)] // const only when no features are enabled (body collapses)
pub fn process_gpu_info(device_index: u32) -> Result<ProcessGpuInfo> {
    #[cfg(all(target_os = "macos", feature = "metal"))]
    if let Some(info) = metal::process_gpu_info(device_index) {
        return Ok(info);
    }

    #[cfg(all(windows, feature = "dxgi"))]
    if let Some(d) = dxgi::query(device_index) {
        return Ok(ProcessGpuInfo {
            used_bytes: d.current_usage,
            is_per_process: true,
            source: GpuQuerySource::Dxgi,
        });
    }

    #[cfg(feature = "nvml")]
    if let Some(snap) = nvml::query(device_index)
        && let Some(used) = snap.process_used_bytes
    {
        return Ok(ProcessGpuInfo {
            used_bytes: used,
            is_per_process: true,
            source: GpuQuerySource::Nvml,
        });
    }

    // nvidia-smi fallback — device-wide reading (`is_per_process = false`).
    #[cfg(feature = "nvidia-smi-fallback")]
    if let Some(result) = nvidia_smi::query(device_index) {
        return Ok(ProcessGpuInfo {
            used_bytes: result.used_bytes,
            is_per_process: false,
            source: GpuQuerySource::NvidiaSmi,
        });
    }

    bounds_check(device_index)?;
    Err(HypomnesisError::NoGpuSource)
}

/// Convert each non-NVIDIA `DXGI` adapter into a `(GpuDeviceInfo, ProcessGpuInfo)`
/// pair, ready to be wrapped in a `Snapshot` by [`crate::Snapshot::all`].
///
/// Indices are assigned sequentially starting at `starting_index` so that
/// the NVIDIA portion of `Snapshot::all()` (`NVML`-canonical 0..N-1) and
/// the non-NVIDIA portion (N, N+1, …) form a contiguous index space.
///
/// `total_bytes` is the adapter's `DedicatedVideoMemory` when non-zero
/// (matches what dGPUs and UMA-allocated iGPUs expose), otherwise
/// `SharedSystemMemory` (`WDDM` shared budget — the right number for
/// iGPUs without UMA). The semantics of `total_bytes` therefore differ
/// subtly between dGPUs and iGPUs; the `Snapshot::all` rustdoc flags
/// this for callers.
///
/// `is_per_process` is `true` because `DXGI`'s `CurrentUsage` is
/// `WDDM`-aware and reports the calling process's own usage, not a
/// device-wide sum.
#[cfg(all(windows, feature = "dxgi"))]
#[must_use]
pub(crate) fn dxgi_non_nvidia_devices(starting_index: u32) -> Vec<(GpuDeviceInfo, ProcessGpuInfo)> {
    dxgi::enumerate_non_nvidia()
        .into_iter()
        .enumerate()
        .map(|(offset, entry)| {
            // CAST: usize → u32, offset is bounded by the DXGI adapter
            // count (handfuls in practice); never approaches u32::MAX.
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
            let index = starting_index.saturating_add(offset as u32);

            let total_bytes = if entry.dedicated_video_memory > 0 {
                entry.dedicated_video_memory
            } else {
                entry.shared_system_memory
            };
            let used_bytes = entry.current_usage;
            let free_bytes = total_bytes.saturating_sub(used_bytes);

            (
                GpuDeviceInfo {
                    index,
                    name: entry.adapter_name,
                    total_bytes,
                    free_bytes,
                    used_bytes,
                    // Non-NVIDIA DXGI adapters have no NVML reserved figure.
                    reserved_bytes: None,
                    // Not an NVIDIA adapter — no NVIDIA driver version.
                    driver_version: None,
                },
                ProcessGpuInfo {
                    used_bytes,
                    is_per_process: true,
                    source: GpuQuerySource::Dxgi,
                },
            )
        })
        .collect()
}

/// List every process holding GPU memory on the given device.
///
/// Returns one [`GpuProcessEntry`] per running process visible to the
/// active backend. Empty `Vec` when the device exists but no processes
/// are using it. [`gpu_process_listing`] returns the same rows together
/// with the PIDs the platform refused to let the caller measure.
///
/// # Source priority
///
/// 1. `Metal` (macOS primary). `proc_listpids`, or `sysctl`
///    `KERN_PROC_ALL` when libproc is refused, enumerates the processes,
///    and one `ledger` read per PID gives its `graphics_footprint`: every
///    process the caller's sandbox lets it read, whatever its owner. A list
///    that was enumerated but not readable is an error, not an empty list;
///    see [`gpu_process_listing`] for that case and its table.
/// 2. `NVML` (Linux primary). `nvmlDeviceGetComputeRunningProcesses_v3`
///    yields `(pid, used_bytes)`; `/proc/<pid>/comm` supplies names on
///    Linux, extended past the kernel's 15-byte cut from the `exe`
///    link or `argv[0]` when either shows the full name. Capped at 64
///    processes per device — the existing `NVML` stack-buffer size. Per-row sentinel and `used > total` checks
///    mirror the library's other `NVML` consumers; offending rows are
///    dropped rather than reported as garbage. Returns compute-only
///    processes (active `CUDA` context).
/// 3. `PDH` (Windows primary, consumer `WDDM`). Reads
///    `\GPU Process Memory(<instance>)\Dedicated Usage` (→
///    `used_bytes`, dedicated commit) and its `Shared Usage` sibling
///    (→ `shared_used_bytes`, resident shared — the `WDDM` spill
///    signal) via Performance Data Helper; names come from `Win32`'s
///    `OpenProcess` + `QueryFullProcessImageNameW` (cross-platform
///    consistent with the Linux `/proc/<pid>/comm` and macOS
///    `proc_pidpath` patterns). Returns **every** process holding GPU
///    memory — compositor, browsers, games, compute alike — because
///    `VidMm`'s accounting is not compute-only. See
///    [`GpuQuerySource::Pdh`] doc-comment for the semantics shift.
/// 4. `nvidia-smi` (fallback) — subprocess
///    `nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv,noheader,nounits --id=N`.
///    Reached on `Linux` when `NVML` is missing, or on `Windows` when
///    the `PDH` `GPU Process Memory` counter set is unregistered (e.g.
///    pre-`WDDM 2.0` drivers — vanishingly rare in 2026). Compute-only
///    semantics; under `WDDM` typically returns rows with `[N/A]`
///    memory that the parser drops, so the list often appears empty.
/// 5. `DXGI` is **not** used — `IDXGIAdapter3::QueryVideoMemoryInfo`
///    only answers for the calling process and cannot enumerate other
///    PIDs.
///
/// # Limitations
///
/// **Per-backend compute-only semantics.** `NVML` and `nvidia-smi`
/// rows are compute-only — browsers using GPU compositing, games, and
/// pure-graphics apps do not appear. `PDH` rows are **not**
/// compute-only — they surface every GPU user. Callers comparing
/// across platforms should check `source` before assuming.
///
/// **Windows process names are resolved via a two-stage fallback (`PDH`
/// path).** `OpenProcess` + `QueryFullProcessImageNameW` is tried
/// first; PIDs it can't resolve (foreign-user, `SYSTEM`,
/// `PPL`-protected) fall through to a `CreateToolhelp32Snapshot` scan
/// (v0.2.8) that resolves most of them non-elevated — so `name: None`
/// essentially never reaches callers on the `PDH` path. What remains
/// renders as `Some("[kernel]")` (`PID 4`), `Some("[exited]")` (the
/// process exited between the `VRAM` sample and the name lookup —
/// elevation would not help), or `Some("[protected]")` (the snapshot
/// itself could not be taken — very rare). On the `nvidia-smi` fallback
/// path, a literal `Some("?")` is produced for protected processes
/// instead (no snapshot fallback exists there). Calling user's own
/// processes always have names available on either path.
///
/// **On macOS the caller's sandbox decides what is listed, not who owns a
/// process.** A process whose `ledger` read the sandbox refuses is not in
/// the `Vec`; [`gpu_process_listing`] reports it in `denied_pids`. A name
/// that `proc_pidpath` refuses is the kernel's `p_comm`, cut at 16 bytes.
///
/// # Errors
///
/// Returns [`HypomnesisError::DeviceIndexOutOfRange`] if `device_index`
/// is past the device count reported by `NVML`, `DXGI` or `Metal`.
/// Returns [`HypomnesisError::NoGpuSource`] if every available backend
/// fails (or no backend is enabled by features).
/// Returns [`HypomnesisError::ProcessListDenied`] on macOS when the
/// process list was enumerated but not read, as that variant defines.
pub fn gpu_processes(device_index: u32) -> Result<Vec<GpuProcessEntry>> {
    gpu_process_listing(device_index).map(|listing| listing.entries)
}

/// List every process holding GPU memory on the given device, and the
/// PIDs whose GPU memory the caller was refused.
///
/// The listing's `entries` are what [`gpu_processes`] returns, and its
/// backends and limitations apply to them. `denied_pids` names the
/// processes the platform would not let the caller measure. Report the
/// count, `denied_pids.len()` or `denied` in
/// [`HypomnesisError::ProcessListDenied`]: inside an App Sandbox there is
/// no "outside" to re-run in, so what to do about a refusal is the
/// application's to say. `hmn` words its advice as a CLI constant.
///
/// | Platform | `denied_pids` | [`HypomnesisError::ProcessListDenied`] |
/// |---|---|---|
/// | Linux | always empty | never returned |
/// | Windows | always empty: only names can be refused, which `[protected]` states | never returned |
/// | macOS | the PIDs whose ledger read the caller's sandbox refused (`EPERM`), not the caller's own, a gone one or a non-positive one | when the list was enumerated but not read, as the variant defines; a machine with nothing to refuse is `Ok` |
///
/// On macOS an `Ok` listing with a non-empty `denied_pids` is a partial
/// one: `entries` holds the processes the caller could read, and no
/// entries means those hold no GPU memory. `entries` can hold the caller's
/// own row, since a process that has initialised Metal holds a few KiB.
/// Whether it appears depends on the caller, and it is not returned when
/// the result is [`HypomnesisError::ProcessListDenied`].
///
/// # Errors
///
/// Returns [`HypomnesisError::DeviceIndexOutOfRange`] if `device_index`
/// is past the device count reported by `NVML`, `DXGI` or `Metal`.
/// Returns [`HypomnesisError::NoGpuSource`] if every available backend
/// fails (or no backend is enabled by features).
/// Returns [`HypomnesisError::ProcessListDenied`] on macOS when the
/// process list was enumerated but not read, as that variant defines.
pub fn gpu_process_listing(device_index: u32) -> Result<GpuProcessListing> {
    // Metal is the macOS primary source: per-PID ledger reads of
    // `graphics_footprint` over `proc_listpids`, or over
    // `sysctl(KERN_PROC_ALL)` when libproc is refused. `None` (the index is not
    // 0, no enumeration gave a trusted list, the ledger entry index did not
    // resolve, or no other process was read, none was refused and at least one
    // failed) falls through to the other arms and `NoGpuSource`, as for every
    // backend; a list that was enumerated but not readable is
    // `ProcessListDenied`.
    #[cfg(all(target_os = "macos", feature = "metal"))]
    if let Some(list) = metal::list_processes(device_index) {
        return decide_listing(list.entries, list.denied_pids, list.others_read);
    }

    // NVML is the primary source on Linux: it answers cleanly there
    // (compute-only, per-process bytes from `nvmlDeviceGetComputeRunningProcesses_v3`,
    // names via `/proc/<pid>/comm`, extended past its 15-byte cut —
    // see `proc_name`). On Windows under `WDDM`, NVML's
    // per-process query returns rows with the `u64::MAX` sentinel for
    // every row (R570-driver-class bug); the sentinel filter then
    // produces `Some(vec![])` — an "I succeeded, here's nothing"
    // response that would block PDH from running. Gating NVML's
    // compute-process branch to Linux side-steps that — PDH owns the
    // Windows primary path below.
    #[cfg(all(target_os = "linux", feature = "nvml"))]
    if let Some(rows) = nvml::list_compute_processes(device_index) {
        let mut entries: Vec<GpuProcessEntry> = rows
            .into_iter()
            .map(|(pid, used_bytes)| {
                let name = proc_name::read_proc_name(pid);
                GpuProcessEntry {
                    pid,
                    name,
                    used_bytes,
                    // NVML has no shared-residency counter; spill is a
                    // WDDM concept (see GpuProcessEntry docs).
                    shared_used_bytes: 0,
                    source: GpuQuerySource::Nvml,
                }
            })
            .collect();
        sort_by_pid(&mut entries);
        return Ok(listing_without_denials(entries));
    }

    // PDH primary path on Windows / WDDM 2.0+. Reads VidMm-tracked
    // bytes via `\GPU Process Memory(*)\Dedicated Usage`; the only
    // path that gives real per-process numbers on consumer Windows.
    // Falls through to nvidia-smi on any PDH error — including the
    // pre-WDDM-2.0 case where the GPU Process Memory counter set
    // isn't registered.
    #[cfg(all(windows, feature = "pdh"))]
    if let Ok(rows) = pdh::query_per_process_vram(device_index) {
        let mut entries: Vec<GpuProcessEntry> = rows
            .into_iter()
            .map(|row| GpuProcessEntry {
                pid: row.pid,
                name: pdh::name_from_pid_windows(row.pid),
                used_bytes: row.dedicated_committed_bytes,
                shared_used_bytes: row.shared_used_bytes,
                source: GpuQuerySource::Pdh,
            })
            .collect();
        sort_by_pid(&mut entries);
        resolve_unresolved_windows_names(&mut entries);
        return Ok(listing_without_denials(entries));
    }

    #[cfg(feature = "nvidia-smi-fallback")]
    if let Some(rows) = nvidia_smi::query_compute_apps(device_index) {
        let mut entries: Vec<GpuProcessEntry> = rows
            .into_iter()
            .map(|app| GpuProcessEntry {
                pid: app.pid,
                name: app.name,
                used_bytes: app.used_bytes,
                // nvidia-smi exposes no shared-residency figure; spill
                // is a WDDM concept (see GpuProcessEntry docs).
                shared_used_bytes: 0,
                source: GpuQuerySource::NvidiaSmi,
            })
            .collect();
        sort_by_pid(&mut entries);
        return Ok(listing_without_denials(entries));
    }

    bounds_check(device_index)?;
    Err(HypomnesisError::NoGpuSource)
}

/// The decision on a macOS listing: [`HypomnesisError::ProcessListDenied`]
/// when the list was enumerated but not read, otherwise the rows and the
/// denied PIDs, each sorted by `pid`.
///
/// The list was not read as [`HypomnesisError::ProcessListDenied`] defines it
/// (`others_read`, the processes other than the caller's that were read, is
/// 0; a zero balance counts as read). A sandboxed caller's own row is not
/// returned on its own: `hmn` lists itself at 16 KiB, so where the sandbox
/// refuses every other process `entries` is exactly the caller's row, and
/// returning it as `Ok` would be the empty list again. Nothing denied is not
/// a refusal: an idle machine has an empty list.
#[cfg(any(all(target_os = "macos", feature = "metal"), test))]
fn decide_listing(
    mut entries: Vec<GpuProcessEntry>,
    mut denied_pids: Vec<u32>,
    others_read: usize,
) -> Result<GpuProcessListing> {
    denied_pids.sort_unstable();
    denied_pids.dedup();
    if others_read == 0 && !denied_pids.is_empty() {
        return Err(HypomnesisError::ProcessListDenied {
            denied: u32::try_from(denied_pids.len()).unwrap_or(u32::MAX),
        });
    }
    sort_by_pid(&mut entries);
    Ok(GpuProcessListing {
        entries,
        denied_pids,
    })
}

/// A listing with no denied PIDs: what a backend that cannot refuse a
/// process returns.
#[cfg(any(
    all(target_os = "linux", feature = "nvml"),
    all(windows, feature = "pdh"),
    feature = "nvidia-smi-fallback"
))]
const fn listing_without_denials(entries: Vec<GpuProcessEntry>) -> GpuProcessListing {
    GpuProcessListing {
        entries,
        denied_pids: Vec::new(),
    }
}

/// Whether a process with this PID exists right now, as far as this
/// platform can tell the caller.
///
/// `Some(true)` or `Some(false)` when the platform can answer; `None` when
/// it cannot, which a caller must treat as "don't know", never as "no".
/// The answer is a snapshot: a process can start or exit, and a PID can
/// be reused, the moment after it is taken. Never errors.
///
/// | Platform | Source | `None` when |
/// |---|---|---|
/// | Linux | `/proc/<pid>/status`, whose `Tgid` must equal `pid` (so a thread ID is not taken for a process) | the file exists but cannot be read |
/// | Windows (`pdh` feature) | a `Toolhelp32` process snapshot, the mechanism `gpu_processes` uses to name processes `OpenProcess` cannot (a process whose snapshot name is empty — none is known — would read as absent) | the snapshot cannot be taken |
/// | macOS (`metal` feature) | `proc_pidpath`, then `sysctl` `KERN_PROC_PID` when libproc gives no path (`kernel_task` has none, and a sandbox can refuse libproc and still allow `sysctl`) | both are refused and `proc_pidpath` did not say `ESRCH`, or the `kinfo_proc` record does not fit `sysctl`'s buffer (`ENOMEM`), or `pid` exceeds `i32::MAX` |
/// | anything else | — | always |
///
/// On Linux a process hidden from the caller (a `/proc` mounted with
/// `hidepid`) reads as `Some(false)`, indistinguishable from one that does
/// not exist. On macOS a sandbox that answered `KERN_PROC_PID` with no
/// record, rather than refusing it, would make a live PID read
/// `Some(false)` the same way.
///
/// On macOS PID 0 (`kernel_task`) reads `Some(true)`: it has no executable
/// path, so `proc_pidpath` says `ESRCH`, and `KERN_PROC_PID` finds it (since
/// v0.2.14). When libproc and `kern.proc` are both refused, the answer is
/// `None` unless `proc_pidpath` said `ESRCH`, which reads `Some(false)`.
/// A zombie (exited, not yet reaped) reads `Some(true)` on macOS as on
/// Linux: the kernel keeps its record until the parent reaps it.
///
/// Added in v0.2.13 for `hmn watch`, which warns when a PID given on its
/// command line names no process, rather than watching it silently as
/// `0` bytes.
#[must_use]
#[allow(unused_variables)] // `pid` unused where no process-lookup source is compiled in
#[allow(clippy::missing_const_for_fn)] // const only on platforms whose arm is `None`
pub fn process_exists(pid: u32) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string(format!("/proc/{pid}/status")) {
            // A status without a parsable `Tgid` is still a process.
            Ok(status) => Some(status_tgid(&status).is_none_or(|tgid| tgid == pid)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(false),
            Err(_) => None,
        }
    }
    #[cfg(all(windows, feature = "pdh"))]
    {
        pdh::resolve_names_via_snapshot(&[pid]).map(|found| !found.is_empty())
    }
    #[cfg(all(target_os = "macos", feature = "metal"))]
    {
        metal::process_exists(pid)
    }
    #[cfg(not(any(
        target_os = "linux",
        all(windows, feature = "pdh"),
        all(target_os = "macos", feature = "metal")
    )))]
    {
        // No process-lookup source on this platform / feature set:
        // "don't know", never "no".
        None
    }
}

/// The `Tgid` (thread-group ID, the process's PID) field of a Linux
/// `/proc/<pid>/status` text; `None` when absent or malformed. For a
/// thread's `/proc/<tid>/status` it names the owning process, which
/// differs from `tid`.
#[cfg(any(target_os = "linux", test))]
#[must_use]
fn status_tgid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Tgid:"))
        .and_then(|rest| rest.trim().parse().ok())
}

/// Sort [`gpu_processes`] output by `pid` ascending.
///
/// Deterministic across calls — the same input state produces the
/// same output order, regardless of whether the underlying backend
/// (`NVML`, `PDH`, `nvidia-smi`, or macOS `Metal` via the ledger
/// syscall) emits its rows in PID order, allocation order, or
/// hash-iteration order. Matches the convention every other
/// process-listing API uses (Unix `ps`, `top -p`, `NVML`'s
/// `nvmlDeviceGetComputeRunningProcesses`). Library consumers can
/// re-sort as they please without fighting an opinionated default —
/// the CLI's `hmn ps`, for instance, re-sorts by `used_bytes`
/// descending for human-facing display.
#[cfg(any(
    all(target_os = "linux", feature = "nvml"),
    all(windows, feature = "pdh"),
    all(target_os = "macos", feature = "metal"),
    feature = "nvidia-smi-fallback",
    test
))]
fn sort_by_pid(entries: &mut [GpuProcessEntry]) {
    entries.sort_by_key(|e| e.pid);
}

/// Fix up `entries` whose `name` is still `None` after
/// [`pdh::name_from_pid_windows`]'s `OpenProcess`-based fast path, using
/// [`pdh::resolve_names_via_snapshot`]'s batched `Toolhelp32` fallback.
///
/// Collapses `?` rows to real names wherever the snapshot found the PID
/// (the common case for foreign-user / `SYSTEM` processes like
/// `dwm.exe`/`csrss.exe`), `"[exited]"` when the PID had already exited
/// by the time of the snapshot, or `"[protected]"` when the snapshot
/// itself could not be taken at all (so "exited" vs. "still running but
/// unresolvable" can't be told apart). Skips the snapshot call entirely
/// when every row already resolved via the fast path — the common case
/// on a normal desktop, where this function costs nothing.
#[cfg(all(windows, feature = "pdh"))]
fn resolve_unresolved_windows_names(entries: &mut [GpuProcessEntry]) {
    let unresolved_pids: Vec<u32> = entries
        .iter()
        .filter(|e| e.name.is_none())
        .map(|e| e.pid)
        .collect();
    if unresolved_pids.is_empty() {
        return;
    }

    match pdh::resolve_names_via_snapshot(&unresolved_pids) {
        Some(resolved) => {
            for entry in entries.iter_mut().filter(|e| e.name.is_none()) {
                let name = resolved
                    .iter()
                    .find(|(pid, _)| *pid == entry.pid)
                    .map_or_else(|| "[exited]".to_owned(), |(_, name)| name.clone());
                entry.name = Some(name);
            }
        }
        None => {
            for entry in entries.iter_mut().filter(|e| e.name.is_none()) {
                entry.name = Some("[protected]".to_owned());
            }
        }
    }
}

/// Bounds-check `index` against whatever count source is available.
///
/// On macOS, tries `Metal` first (the same order as [`device_count`],
/// so the bound reported matches the count `hmn` shows). Then tries
/// `NVML`; on Windows, falls back to `DXGI` if `NVML` is unavailable.
/// Returns `Ok(())` when no count source is available (caller will
/// surface its own error, typically `NoGpuSource`).
///
/// # Errors
///
/// Returns [`HypomnesisError::DeviceIndexOutOfRange`] when a count
/// source reports a count and `index >= count`.
#[allow(unused_variables)] // unused when no backend feature is enabled
#[allow(clippy::missing_const_for_fn)] // const only when no features are enabled
#[allow(clippy::unnecessary_wraps)] // Result is necessary only when metal, nvml or dxgi feature returns Err
fn bounds_check(index: u32) -> Result<()> {
    #[cfg(all(target_os = "macos", feature = "metal"))]
    if let Some(count) = metal::device_count() {
        return if index >= count {
            Err(HypomnesisError::DeviceIndexOutOfRange { index, count })
        } else {
            Ok(())
        };
    }

    #[cfg(feature = "nvml")]
    if let Some(count) = nvml::device_count() {
        return if index >= count {
            Err(HypomnesisError::DeviceIndexOutOfRange { index, count })
        } else {
            Ok(())
        };
    }

    #[cfg(all(windows, feature = "dxgi"))]
    if let Some(count) = dxgi::device_count() {
        return if index >= count {
            Err(HypomnesisError::DeviceIndexOutOfRange { index, count })
        } else {
            Ok(())
        };
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_tgid_reads_the_field() {
        let status = "Name:\tcanvas\nUmask:\t0022\nState:\tR (running)\nTgid:\t15534\nNgid:\t0\nPid:\t15534\n";
        assert_eq!(status_tgid(status), Some(15534));
        assert_eq!(status_tgid("Name:\tx\nPid:\t1\n"), None);
        assert_eq!(status_tgid("Tgid:\tbogus\n"), None);
    }

    #[test]
    fn process_exists_finds_this_process() {
        // `None` is allowed only where the platform cannot answer at all.
        let me = std::process::id();
        if let Some(exists) = process_exists(me) {
            assert!(exists);
        }
        #[cfg(any(target_os = "linux", all(windows, feature = "pdh")))]
        assert_eq!(process_exists(me), Some(true));
        #[cfg(all(target_os = "macos", feature = "metal"))]
        assert_eq!(process_exists(me), Some(true));
    }

    #[test]
    fn process_exists_does_not_find_an_impossible_pid() {
        // Above every platform's PID ceiling: Linux's `pid_max` is at most
        // 2^22, Windows PIDs are multiples of 4 well below 2^32, and on
        // macOS `u32::MAX` exceeds `i32::MAX` (so `None` there).
        let impossible = u32::MAX - 2;
        assert_ne!(process_exists(impossible), Some(true));
        #[cfg(any(target_os = "linux", all(windows, feature = "pdh")))]
        assert_eq!(process_exists(impossible), Some(false));
        #[cfg(all(target_os = "macos", feature = "metal"))]
        assert_eq!(process_exists(impossible), None);
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    #[test]
    fn process_exists_finds_kernel_task_on_macos() {
        // PID 0 is `kernel_task`: it has no executable path, so
        // `proc_pidpath` says `ESRCH`, and only `sysctl` `KERN_PROC_PID`
        // finds it.
        assert_eq!(process_exists(0), Some(true));
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    #[test]
    fn process_exists_says_false_for_a_dead_pid_on_macos() {
        // `i32::MAX`: a valid `pid_t` no process holds (macOS PIDs stop
        // at 99999).
        assert_eq!(process_exists(2_147_483_647), Some(false));
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    #[test]
    fn bounds_check_metal_arm_admits_index_0_and_rejects_index_1() {
        // Apple Silicon reports one Metal device; an Intel Mac (no count)
        // skips. Unsandboxed, the dispatchers answer index 0 from Metal
        // before `bounds_check` runs, so only this test sees its Metal arm.
        let Some(1) = metal::device_count() else {
            return;
        };
        assert!(bounds_check(0).is_ok(), "{:?}", bounds_check(0));
        assert!(
            matches!(
                bounds_check(1),
                Err(HypomnesisError::DeviceIndexOutOfRange { index: 1, count: 1 })
            ),
            "{:?}",
            bounds_check(1)
        );
    }

    /// A GPU process row for the decision tests.
    fn row(pid: u32) -> GpuProcessEntry {
        GpuProcessEntry {
            pid,
            name: None,
            used_bytes: 16_384,
            shared_used_bytes: 0,
            source: crate::GpuQuerySource::Metal,
        }
    }

    #[cfg(any(
        all(target_os = "linux", feature = "nvml"),
        all(windows, feature = "pdh"),
        feature = "nvidia-smi-fallback"
    ))]
    #[test]
    fn listing_without_denials_has_no_denied_pids() {
        let listing = listing_without_denials(vec![row(7)]);
        assert!(listing.denied_pids.is_empty(), "{:?}", listing.denied_pids);
        assert_eq!(listing.entries.len(), 1);
    }

    #[test]
    fn decide_listing_is_denied_when_only_the_callers_own_row_was_read() {
        // A sandboxed caller's own row is not returned on its own: under
        // profiles P and S0 `entries` is exactly the caller's row, with no
        // other process read.
        let result = decide_listing(vec![row(4242)], vec![1, 2, 3], 0);
        assert!(
            matches!(
                result,
                Err(HypomnesisError::ProcessListDenied { denied: 3 })
            ),
            "{result:?}"
        );
    }

    #[test]
    fn decide_listing_sorts_the_rows_and_the_denied_pids_without_duplicates() {
        let listing = decide_listing(vec![row(9), row(3), row(5)], vec![20, 7, 20], 1).ok();
        let pids = listing
            .as_ref()
            .map(|l| l.entries.iter().map(|e| e.pid).collect::<Vec<u32>>());
        assert_eq!(pids, Some(vec![3, 5, 9]));
        assert_eq!(listing.map(|l| l.denied_pids), Some(vec![7, 20]));
        // A PID listed twice is one refusal, so the count agrees with the list.
        let result = decide_listing(Vec::new(), vec![5, 5], 0);
        assert!(
            matches!(
                result,
                Err(HypomnesisError::ProcessListDenied { denied: 1 })
            ),
            "{result:?}"
        );
    }

    #[test]
    fn decide_listing_with_nothing_denied_is_ok_even_when_empty() {
        let listing = decide_listing(Vec::new(), Vec::new(), 0).ok();
        let empty = listing.as_ref().map(|l| l.entries.is_empty());
        assert_eq!(empty, Some(true));
        let denied = listing.map(|l| l.denied_pids);
        assert_eq!(denied, Some(Vec::new()));
    }
}
