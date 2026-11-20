// SPDX-License-Identifier: MIT OR Apache-2.0

//! # hypomnesis
//!
//! External measurement of a Rust process's `RAM` and `VRAM` state, on Windows, Linux, and macOS.
//!
//! `hypomnesis` reports what's currently in a process's memory — process `RSS`,
//! device-wide GPU memory, and per-process GPU `VRAM` — without depending on
//! `candle`, `cudarc`, `sysinfo`, or any inference framework.
//!
//! ## Capabilities
//!
//! | Metric | Windows | Linux | macOS |
//! |--------|---------|-------|-------|
//! | Process `RSS` | `K32GetProcessMemoryInfo` | `/proc/self/status` | `task_info(TASK_VM_INFO_PURGEABLE).phys_footprint` |
//! | Device-wide GPU memory | `NVML` (`nvml.dll`) | `NVML` (`libnvidia-ml.so.1`); AMD: `ROCm SMI` (`librocm_smi64.so.1`) | `sysctl hw.memsize` (total) + `MTLDevice.recommendedMaxWorkingSetSize` (free) |
//! | Device reserved memory | `NVML` v2 (`nvmlDeviceGetMemoryInfo_v2`, R510+) | `NVML` v2 (R510+); AMD: `ROCm SMI` (n/a, `None`) | n/a (`None` — UMA has no carve-out) |
//! | Per-process GPU memory | `DXGI` (`IDXGIAdapter3::QueryVideoMemoryInfo`) | `NVML` (`nvmlDeviceGetComputeRunningProcesses`); AMD: `ROCm SMI` (`rsmi_compute_process_info_by_device_get`) | `ledger(LEDGER_ENTRY_INFO_V2).graphics_footprint` |
//! | GPU-process listing (other PIDs) | `PDH` (`\GPU Process Memory(*)\Dedicated Usage` + `Shared Usage`) + `OpenProcess`/`QueryFullProcessImageNameW`, with a `CreateToolhelp32Snapshot` fallback (v0.2.8) for names `OpenProcess` denies even non-elevated; `nvidia-smi --query-compute-apps` fallback (NB: Windows `PDH` is **not** compute-only — it surfaces every GPU memory holder, including the compositor and browsers) | `NVML` + `/proc/<pid>/comm` (compute-only); AMD: `ROCm SMI` (`rsmi_compute_process_info_get` + `rsmi_compute_process_gpus_get`, compute-only) | `proc_listpids` (`sysctl KERN_PROC_ALL` where libproc is refused) + per-PID `ledger` + `proc_pidpath`; on macOS the sandbox, not process ownership, decides what `hmn` can read; see [README Limitations, item 9](https://github.com/mi-for-the-rust-of-us/hypomnesis#binary-hmn) |
//! | Process existence (`process_exists`) | `Toolhelp32` process snapshot (`pdh`) | `/proc/<pid>/status`, whose `Tgid` must equal the PID | `proc_pidpath`, then `sysctl KERN_PROC_PID` |
//! | Process name (`process_name`, since v0.2.15) | `OpenProcess` / `QueryFullProcessImageNameW`, then the `Toolhelp32` snapshot (`pdh`) | `/proc/<pid>/comm`, extended past its 15-byte cut via `exe` / `argv[0]` | `proc_pidpath`, then `p_comm` where a sandbox refuses the path |
//! | Spill detection (`SpillTracker`) | `PDH` `\GPU Adapter Memory(*)\Dedicated Usage` + `Shared Usage` (`WDDM 2.0`+) | n/a (`is_spill_measurable()` returns `false` — normal `CUDA` OOMs rather than silently paging) | n/a (`false` — `UMA` has nothing to spill *into*) |
//! | Fallback | `nvidia-smi` subprocess | `nvidia-smi` subprocess; AMD: `ROCm SMI` (no fallback) | no second backend; enumeration, names and lookups try libproc first, then `sysctl`; what a sandbox still withholds is counted (Limitations, item 9) |
//!
//! On Linux the backends are tried in the order `NVML`, `ROCm` SMI, `nvidia-smi`.
//! `ROCm` SMI is asked for the device count only when `NVML`'s count is not
//! positive, and a device index is the library's own enumeration order over the
//! devices visible to the process, as `NVML`'s is the `NVML`-canonical order;
//! scheduler GPU numbers are never read. `ROCm` SMI reports no reserved memory
//! (`reserved_bytes` is `None`), spill is not measurable on Linux for either
//! vendor, and `ROCm` SMI has no subprocess fallback. See
//! [README, AMD GPUs on Linux](https://github.com/mi-for-the-rust-of-us/hypomnesis#amd-gpus-on-linux-parity-with-nvidia).
//!
//! ## Quick start
//!
//! ```no_run
//! let snap = hypomnesis::Snapshot::now(0)?;
//! println!("RAM: {} bytes", snap.ram_bytes);
//! if let Some(dev) = snap.gpu_device {
//!     println!("GPU 0: {:?}, free {} of {} bytes",
//!              dev.name, dev.free_bytes, dev.total_bytes);
//! }
//! # Ok::<(), hypomnesis::HypomnesisError>(())
//! ```
//!
//! ## Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `nvml` | yes | `NVML` dynamic load via `libloading` (Linux + Windows-`WDDM` device-wide) |
//! | `rocm` | yes | `ROCm` SMI dynamic load via `libloading` (Linux, AMD GPUs; no-op on other platforms) |
//! | `dxgi` | yes | Windows per-process `VRAM` via `IDXGIAdapter3` (no-op on non-Windows) |
//! | `pdh` | yes | Windows foreign-process `VRAM` listing via `PDH` `\GPU Process Memory(*)\Dedicated Usage` + `Shared Usage`, and the `\GPU Adapter Memory(*)` counters backing `SpillTracker`'s live path, under `WDDM 2.0`+ (no-op on non-Windows; depends on `dxgi` for adapter `LUID` / capacity lookup). `SpillTracker` itself compiles on every platform regardless — without this feature it is simply never measurable |
//! | `metal` | yes | macOS device-wide GPU budget via `objc2-metal` (`MTLDevice.recommendedMaxWorkingSetSize`) (no-op on non-macOS) |
//! | `nvidia-smi-fallback` | yes | Subprocess fallback when `NVML` / `ROCm` SMI / `DXGI` / `PDH` fail or are otherwise unavailable (e.g. pre-`WDDM 2.0` Windows) |
//! | `report` | no | `MemoryReport` delta + `print_delta` / `print_before_after` / `ram_mb` / `vram_mb` helpers (`candle-mi` parity); `format_free` / `print_free` / `format_total` / `format_used` formatting helpers on `GpuDeviceInfo` |
//! | `debug-output` | no | Print raw values from the `NVML` / `ROCm` SMI / `DXGI` / `PDH` / `nvidia-smi` / spill paths to stderr (diagnostic) |
//! | `cli` | yes (since v0.2.8) | Build the `hmn` CLI binary (pulls `clap` 4 and `ctrlc` as deps). Library-only consumers who don't want these use `--no-default-features` and select source features explicitly |
//! | `test-helpers` | no | Expose `GpuDeviceInfoBuilder`, `GpuProcessEntryBuilder`, and `SpillReportBuilder` for downstream tests that need synthetic fixtures. Default-off, additive — production code must never enable it. |

#![deny(unsafe_code)]
#![allow(unknown_lints)]

pub mod error;
pub mod gpu;
pub mod ram;
pub mod snapshot;
pub mod spill;

#[cfg(feature = "report")]
pub mod report;

pub use error::{HypomnesisError, Result};
pub use gpu::{
    device_count, device_info, gpu_process_listing, gpu_processes, process_exists,
    process_gpu_info, process_name,
};
pub use ram::process_rss;
pub use snapshot::{
    GpuDeviceInfo, GpuProcessEntry, GpuProcessListing, GpuQuerySource, ProcessGpuInfo, Snapshot,
};
pub use spill::{
    SpillEpisode, SpillReport, SpillTracker, is_spill_measurable, snapshot_is_spilling,
};

#[cfg(feature = "report")]
pub use report::MemoryReport;

#[cfg(feature = "test-helpers")]
pub use snapshot::{GpuDeviceInfoBuilder, GpuProcessEntryBuilder};

#[cfg(feature = "test-helpers")]
pub use spill::SpillReportBuilder;
