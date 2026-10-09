# hypomnesis

[![CI](https://github.com/mi-for-the-rust-of-us/hypomnesis/actions/workflows/ci.yml/badge.svg)](https://github.com/mi-for-the-rust-of-us/hypomnesis/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/hypomnesis.svg)](https://crates.io/crates/hypomnesis)
[![docs.rs](https://docs.rs/hypomnesis/badge.svg)](https://docs.rs/hypomnesis)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://www.rust-lang.org)
[![license](https://img.shields.io/crates/l/hypomnesis.svg)](https://github.com/mi-for-the-rust-of-us/hypomnesis#license)
[![unsafe: deny](https://img.shields.io/badge/unsafe-deny_(FFI_backends_only)-blue.svg)](https://github.com/rust-secure-code/safety-dance/)
[![NVIDIA](https://img.shields.io/badge/NVIDIA-NVML_%2B_DXGI-76B900.svg?logo=nvidia&logoColor=white)](#capabilities)

**ὑπόμνησις** — *External RAM and VRAM, measured.*

> 🆕 **`0.2.14` makes `hmn` honest inside a macOS sandbox.** A field check of v0.2.13 on Apple Silicon, contributed by contributor [@LittleCoinCoin](https://github.com/LittleCoinCoin), found that the sandbox, not process ownership, decides what `hmn` can read on macOS, and that a sandboxed `hmn ps` printed `0 GPU processes found.` with exit `0`. It now measures what the sandbox permits and counts the rest: `N unreadable — re-run outside the sandbox`, a new `gpu_process_listing` with the refused PIDs beside the rows, and `sysctl` enumeration where libproc is refused. On every platform, `hmn ps` states each device it skips and exits `2` when none could be queried; the SPILL cells read `n/a` where spill cannot exist; and no macOS text advises `sudo` any more. See [`CHANGELOG.md`](CHANGELOG.md) and [`docs/roadmap-v0.2.14.md`](docs/roadmap-v0.2.14.md).

> 🚀 **`0.2.13` stops `hmn watch` from reporting a spill check it never ran, and lets `hmn ps` say who is being paged.** An askesis dogfooding report found `hmn watch`'s summary saying `no spill observed` on Linux, where spill cannot be measured — a bug since v0.2.6, now fixed; validating the release also found `hmn watch` blind to a spill already under way when it attaches, which it now says at attach and in its summary. On a spilling device, `hmn ps` and `hmn watch` mark the process being paged `PAGED` and the rest `device` instead of `SPILL` on every row, and `hmn ps` states the device's verdict once on its summary line. `hmn ps` gains `--filter <PATTERN>` and `--exit-status` — `hmn ps --filter canvas --exit-status` is a one-line "is my job on the GPU?" gate — a repeatable `--pid`, and an error for a `--device` it cannot list. A new `process_exists` lets `hmn watch` warn about a PID that names no process. See [`CHANGELOG.md`](CHANGELOG.md) and [`docs/roadmap-v0.2.13.md`](docs/roadmap-v0.2.13.md).

> 🚀 **`0.2.12` teaches `hmn watch` to follow a program by name, not by VRAM rank.** A candle-mi dogfooding report found that `hmn watch --follow-new --top 3`, run alongside a research campaign, had recorded mostly the desktop: 73.9% of the rows in captures committed as research artifacts were not the workload, because `--top` ranks by committed VRAM and the other slots went to whatever else held some. `--filter <PATTERN>` (case-insensitive substring, repeatable) and `--min <SIZE>` now narrow auto-selection, the stderr header names the active criterion, and every `--json` capture opens with a `{"kind":"start",...}` record — `hmn` version, invocation, selection — so a capture describes itself, and one cut short before its summary is recognizable from the file alone. On Linux, process names longer than the kernel's 15-byte `comm` are now reported in full, so a filter can match them. The release opens with the nine fixes of a duplicate-code audit, one of them a `DXGI` robustness fix. See [the watch tutorial's Step 5](docs/tutorials/watching-a-running-job.md#step-5--follow-one-program-not-the-whole-machine---filter), [`CHANGELOG.md`](CHANGELOG.md) and [`docs/roadmap-v0.2.12.md`](docs/roadmap-v0.2.12.md).

## Table of Contents

- [Install](#install)
- [Usage](#usage)
- [Try it](#try-it)
- [Binary (`hmn`)](#binary-hmn)
- [Capabilities](#capabilities)
- [Feature Flags](#feature-flags)
- [Documentation](#documentation)
- [Used by](#used-by)
- [License](#license)
- [Development](#development)

> **New to hypomnesis?**
> - **What's eating my GPU memory right now?** → [`hmn ps`](#binary-hmn) — every process holding GPU memory, with dedicated-commit and resident-shared columns, on Windows, Linux, and macOS.
> - **Will my next job fit before I launch it?** → [`hmn fits <SIZE>`](#hmn-fits--headroom-predicate-since-v0211) — one gateable exit code, instead of hand-rolling a `jq` check per script.
> - **Is my training / inference run spilling into system RAM?** (Windows / `WDDM` only — spilling into a *separate* shared budget is a `WDDM` architectural concept; Linux gets a `CUDA` OOM instead, macOS `UMA` has nothing to spill *into*) → the [Is my run spilling?](docs/tutorials/is-my-run-spilling.md) tutorial — wrap the run with [`hmn spill`](#hmn-spill--wddm-spill-detection), read the episode pattern, react.
> - **Is a job that's already running spilling?** (Windows / `WDDM` only, same reason as above) → the [watch tutorial](docs/tutorials/watching-a-running-job.md) — [`hmn watch <pid>`](#hmn-watch--attach-to-a-running-pid) attaches directly, no restart needed; `hmn watch --follow-new --filter <name>` records only your own program, across restarts.
> - **I want to measure my own process from Rust** → [Usage](#usage) — `Snapshot::now(0)`: process RSS + device-wide + per-process GPU in one call.
> - **I want my loop to stop (or adapt) when spill starts** → `SpillTracker` on [docs.rs](https://docs.rs/hypomnesis) — `observe()` per step, a latched `has_spilled()` to early-stop, an instantaneous `is_spilling()` to adapt; portable via `is_spill_measurable()`.
> - **My numbers look wrong** — `used_bytes` above the card's total, a nonzero SHARED column, all-zeros on Linux/macOS → the [FAQ](docs/FAQ.md), most of it is measured reality, not a bug.
>
> Common questions live in the [FAQ](docs/FAQ.md); every flag is in `hmn --help`.

## Install

```toml
[dependencies]
hypomnesis = "0.2"
```

The default feature set (`nvml`, `dxgi`, `pdh`, `metal`, `nvidia-smi-fallback`, `cli`) covers process RSS, per-process / device-wide GPU memory, the foreign-process GPU listing, `WDDM` spill detection on Windows (`IDXGIAdapter3` + `PDH` + `NVML`) and Linux (`NVML`), a `nvidia-smi` subprocess fallback, and (since v0.2.8) the `hmn` CLI binary itself — see the [Feature Flags](#feature-flags) table for the per-flag breakdown. The `windows`-crate dependency behind `dxgi` / `pdh` is target-conditional — Linux users pay nothing for it.

Library-only consumers who don't want `clap`/`ctrlc` pulled in should pass `--no-default-features` and select source features explicitly:

```toml
hypomnesis = { version = "0.2", default-features = false, features = ["nvml", "dxgi", "pdh"] }
```

On macOS, the `metal` feature is in the default set. Process RSS and per-process GPU memory come from libSystem syscalls (`task_info`, `ledger`, `sysctl`). The device-wide "free" figure comes from `MTLDevice.recommendedMaxWorkingSetSize` via the `objc2-metal` binding (target-conditional, macOS-only) — no libSystem signal on Apple Silicon UMA approximates Apple's own kernel-projected GPU working-set budget within useful accuracy.

For candle-mi-compatible delta and printing helpers (`MemoryReport`, `print_delta`, `print_before_after`, `ram_mb`, `vram_mb`):

```toml
hypomnesis = { version = "0.2", features = ["report"] }
```

For a stripped-down build (process RSS only, no GPU backends):

```toml
hypomnesis = { version = "0.2", default-features = false }
```

## Usage

```rust
use hypomnesis::Snapshot;

fn main() -> Result<(), hypomnesis::HypomnesisError> {
    let snap = Snapshot::now(0)?;
    println!("RAM: {} bytes", snap.ram_bytes);

    if let Some(dev) = snap.gpu_device {
        let total_gib = dev.total_bytes as f64 / (1u64 << 30) as f64;
        let used_gib  = dev.used_bytes  as f64 / (1u64 << 30) as f64;
        println!(
            "GPU 0 [{}]: {:.1} / {:.1} GiB used",
            dev.name.as_deref().unwrap_or("unknown"),
            used_gib, total_gib,
        );
        // `total_bytes` is the full NVML framebuffer (= `nvidia-smi` Total).
        // `reserved_bytes` is the driver/firmware carve-out *within* it
        // (NVML R510+); allocation headroom is `total - reserved`, which
        // `free_bytes` already reflects.
        if let Some(reserved) = dev.reserved_bytes {
            let reserved_mib = reserved as f64 / (1u64 << 20) as f64;
            println!("  ({:.0} MiB reserved)", reserved_mib);
        }
        // NVIDIA-branded driver string (e.g. "610.88") from NVML or the
        // `nvidia-smi` fallback — not the Windows PnP driver-store form.
        if let Some(driver) = &dev.driver_version {
            println!("  driver {driver}");
        }
    }

    if let Some(proc_gpu) = snap.gpu {
        let kind = if proc_gpu.is_per_process { "per-process" } else { "device-wide" };
        let mib  = proc_gpu.used_bytes as f64 / (1u64 << 20) as f64;
        println!("This process: {:.0} MiB ({})", mib, kind);
    }

    Ok(())
}
```

Expected output (RTX 5060 Ti, Windows, idle process):

```
RAM: 142475264 bytes
GPU 0 [NVIDIA GeForce RTX 5060 Ti]: 1.8 / 15.9 GiB used
  (259 MiB reserved)
  driver 610.88
This process: 119 MiB (per-process)
```

## Try it

Real transcripts from the reference machine (Ryzen 9 5950X + RTX 5060 Ti 16 GiB, Windows 11 / `WDDM`):

```
$ hmn                                     # what's on the card?
GPU 0 [NVIDIA GeForce RTX 5060 Ti]: free 13284 MiB / 16311 MiB (259 MiB reserved), driver 610.88

$ hmn --json                              # same data, scriptable
[{"index":0,"name":"NVIDIA GeForce RTX 5060 Ti","total_bytes":17103323136,"free_bytes":14967820288,"used_bytes":2135502848,"reserved_bytes":271581184,"driver_version":"610.88"}]

$ hmn ps                                  # who's holding it? (top rows shown; SPILL since v0.2.11)
PID    NAME                         VRAM     SHARED  DEVICE                      SPILL
26796  firefox.exe                  1.5 GiB  10 MiB  NVIDIA GeForce RTX 5060 Ti  no
29092  dwm.exe                      1.2 GiB  3 MiB   NVIDIA GeForce RTX 5060 Ti  no
21324  SamsungMagician.exe          268 MiB  0 MiB   NVIDIA GeForce RTX 5060 Ti  no
9584   claude.exe                   242 MiB  0 MiB   NVIDIA GeForce RTX 5060 Ti  no
...

$ hmn spill -- .\spillforge.exe           # is this run spilling? (the release-validation
                                          # fixture: a 20 GiB working set on the 16 GiB card)
... the wrapped command runs to completion, stdout untouched ...
hmn spill: peak dedicated 14.3 GiB / 15.7 GiB
           peak shared    3.1 GiB (baseline 163 MiB)
           episodes       1 — total 13.1s, longest 13.1s, first +2.0s into run

$ hmn spill --json -- python train.py | jq -e '.measurable and (.spilled | not)'
                                          # CI gate: fail the step when the run spilled

$ hmn watch 15884 --interval 3s           # is an ALREADY-RUNNING job spilling? (same
                                          # forced-spill fixture, attached mid-run)
hmn watch: device 0 [NVIDIA GeForce RTX 5060 Ti], interval 3.0s, watching 1 PID(s)
TIME      PID    NAME            COMMITTED  ΔCOMMIT   SHARED   ΔSHARED   SPILL
+0.0s     15884  spillforge.exe  9.3 GiB    +0 B      86 MiB   +0 B      no
...
+30.1s    15884  spillforge.exe  13.3 GiB   +255 MiB  304 MiB  +0 B      no
+33.1s    15884  spillforge.exe  13.3 GiB   -4 MiB    1.3 GiB  +980 MiB  SPILL
hmn watch: peak dedicated 15.0 GiB / 15.7 GiB
           peak shared    1.4 GiB (baseline 228 MiB)
           episodes       1 — total 0.0s, longest 0.0s, first +33.1s into run
                                          # exits 1 — spill was observed

$ hmn watch --follow-new --interval 3s    # stand guard through TWO sequential
                                          # spillforge runs, back to back
hmn watch: +3.0s followed set changed: entered pid=10640 (spillforge.exe); left pid=21716 (Code.exe)
hmn watch: +18.0s followed set changed: entered pid=13004 (SamsungMagician.exe); left pid=10640 (spillforge.exe)
hmn watch: +21.1s followed set changed: entered pid=29452 (spillforge.exe); left pid=13004 (SamsungMagician.exe)
hmn watch: +36.1s followed set changed: entered pid=13004 (SamsungMagician.exe); left pid=29452 (spillforge.exe)
hmn watch: peak dedicated 15.2 GiB / 15.7 GiB
           peak shared    551 MiB (baseline 155 MiB)
           episodes       2 — total 24.1s, longest 12.0s, first +3.0s into run
                                          # per_pid[] finalizes BOTH spillforge
                                          # PIDs plus 5 desktop processes: 7 total
```

*(The `hmn watch` excerpts above predate v0.2.13, which lines rows up under the header, marks the process being paged `PAGED` and the device's other rows `device` rather than `SPILL` on every row, and adds a `PAGED` column to the closing per-PID table.)*

The VRAM column is `WDDM`'s dedicated *commit* — a big process legitimately shows more than the card holds. The SHARED column is *resident* shared-system-memory: the spill signal, and the small nonzero values above are the normal benign baseline. When those two facts surprise you, that's the [FAQ](docs/FAQ.md)'s opening entries.

## Binary (`hmn`)

`hypomnesis` ships a small CLI binary, `hmn`, behind the (since v0.2.8) default-on `cli` feature. Install it with:

```sh
cargo install hypomnesis
```

`--features cli` is still accepted but redundant on the default feature set — only needed if you've already opted out with `--no-default-features` and want the binary back.

Five subcommands:

```sh
hmn                          # device summary (free / total per GPU)
hmn --json                   # same data as a JSON array
hmn ps                       # all GPU processes — discovery command
hmn ps --pid 12345           # filter to one PID (repeatable since v0.2.13: --pid A --pid B)
hmn ps --filter canvas       # processes whose name contains "canvas", any case (v0.2.13)
hmn ps --filter canvas --exit-status   # exit 1 if nothing listed (2 if a device failed or a process the filters could match was unreadable): "is my job on the GPU?" (v0.2.13)
hmn ps --device 0            # filter to one GPU on multi-GPU rigs (exit 2 if it can't be listed; plain `hmn ps` exits 2 when every device failed)
hmn ps --json                # scriptable output
hmn ps --sort total           # order by dedicated + shared instead of dedicated alone
hmn ps --min 50MiB            # hide rows below 50 MiB total footprint (since v0.2.11)
hmn spill -- python train.py # run a command, report WDDM spill on exit
hmn spill --interval 250 --json -- ollama serve   # slower polling, JSON report
hmn watch 12345               # attach to an ALREADY-RUNNING PID, watch for spill
hmn watch --top 3 --json      # no PID: auto-select top 3 by committed VRAM, JSONL
hmn watch --follow-new --json # re-select every interval: stand guard over a machine
                               # while arbitrary short-lived work happens
hmn fits 12GiB                # headroom predicate: exit 0/1/2, gateable from a run script (since v0.2.11)
```

Example default output (single NVIDIA dGPU, the maintainer's reference machine — Ryzen 9 5950X has no iGPU, so only one adapter surfaces):

```
GPU 0 [NVIDIA GeForce RTX 5060 Ti]: free 13284 MiB / 16311 MiB (259 MiB reserved), driver 610.88
```

The `(259 MiB reserved)` parenthetical (NVML R510+) is the driver/firmware carve-out *within* the 16311 MiB total — matching `nvidia-smi -q -d MEMORY`'s `Reserved` line. It is elided on backends that don't expose it (DXGI, `nvidia-smi`, Metal, pre-R510).

The `, driver 610.88` suffix is the NVIDIA-branded driver version — from NVML (`nvmlSystemGetDriverVersion`) or the `nvidia-smi` fallback (`--query-gpu=driver_version`); elided on backends that don't expose it (DXGI, Metal, non-NVIDIA adapters). This is the same version string `nvidia-smi`, release notes, and bug reports use — **not** the Windows PnP driver-store form (e.g. `32.0.16.1088`), which NVML/`nvidia-smi` don't expose.

Apple Silicon, idle process (Apple M3 Pro, 36 GiB unified memory):

```
GPU 0 [Apple M3 Pro]: free 28753 MiB / 36864 MiB
```

The `free` figure here is `MTLDevice.recommendedMaxWorkingSetSize` — the kernel-projected GPU working-set budget on UMA — and `total` is `sysctl hw.memsize`. See the [macOS UMA semantics](#macos-uma-semantics-what-free_bytes-means) section below for what these numbers mean and why they differ from the discrete-GPU "free vs total" model.

Illustrative output on a *heterogeneous* machine (NVIDIA dGPU + Intel/AMD iGPU on Windows). Not yet verified end-to-end on real hardware — see [`docs/roadmap-v0.2.0.md`](docs/roadmap-v0.2.0.md) "Verification plan":

```
GPU 0 [NVIDIA GeForce RTX 5060 Ti]: free 13284 MiB / 16311 MiB (259 MiB reserved), driver 610.88
GPU 1 [Intel Iris Xe Graphics]: free 32768 MiB / 32768 MiB
```

(The Intel iGPU line has no reserved parenthetical or driver suffix — `DXGI` does not expose the NVML carve-out or an NVIDIA driver string, so `reserved_bytes` and `driver_version` are both `None` there.)

`hmn ps` (illustrative — empty on machines with no active CUDA workload):

```
PID    NAME              VRAM      SHARED   DEVICE                      SPILL
12345  lm-studio.exe     8.2 GiB   45 MiB   NVIDIA GeForce RTX 5060 Ti  no
67890  python.exe        1.4 GiB   0 MiB    NVIDIA GeForce RTX 5060 Ti  no
```

A one-line summary is written to **stderr** after each `hmn ps` run:

```
hmn: 2 GPU processes found (9.6 GiB committed total).
hmn: 0 GPU processes found matching pid=99 device=0.   # with filters
hmn: 1 GPU process found matching filter="canvas" (14.2 GiB committed total); device 0 spilling: 154 MiB free, 2.1 GiB shared, 1 process paged.
```

The stderr summary is printed even when the table is empty (except when every device failed, which prints no table and exits `2`), so interactive users get an unambiguous "command worked, here's the count" line without breaking stdout's scriptability. Pipelines like `hmn ps | awk 'NR>1 {print $1}'` or `hmn ps --json | jq` work as expected. Redirect `2>/dev/null` to suppress the summary.

`--sort <KEY>` (`dedicated` default, `shared`, or `total`) reorders both the text table and `--json` output — three different questions, not interchangeable: `dedicated` ("who do I kill to free VRAM?"), `shared` ("who is currently being paged out?" — a symptom, not a cause), `total` (dedicated + shared, "who is the biggest GPU-memory citizen overall?"). `dedicated` also accepts `vram` and `committed` as aliases — the words the rest of the tool's own vocabulary uses for the same quantity (the `ps` column header and `watch`'s `COMMITTED` column, respectively). Tie-breaks (name ascending, then PID ascending) are identical regardless of key. `shared`/`total` are a documented no-op ordering on Linux and macOS, where `shared_used_bytes` is always `0`.

**Limitations** (intrinsic to the underlying data sources, not bugs — longer-form answers to the recurring ones live in the [FAQ](docs/FAQ.md)):

1. **Per-platform semantics differ — compute-only on Linux, all-GPU-users on Windows.** `hmn ps` on Linux (via `NVML`'s `nvmlDeviceGetComputeRunningProcesses_v3`) enumerates only processes with an active `CUDA` context — browsers using GPU compositing, games, and pure-graphics apps do not appear. `hmn ps` on Windows (via `PDH`'s `\GPU Process Memory(*)\Dedicated Usage`) enumerates **every** process holding GPU memory — the desktop compositor (`dwm.exe`), browsers, games, and `CUDA` / compute alongside. The semantic shift reflects what each platform's kernel actually accounts for; check the `source` field on `GpuProcessEntry` if you care about the distinction.

2. **Windows `used_bytes` reflects WDDM's *dedicated commit*, not resident set.** Under `WDDM` a process can commit GPU allocations exceeding physical `VRAM` — the kernel pages them via the shared system memory budget. Numbers exceeding the device's total `VRAM` are real, not bugs: they match Task Manager's `Dedicated GPU memory` column. (Example: on a 16 GiB GPU, a heavy browser process can show 15+ GiB committed.)

3. **The SHARED column (Windows / `PDH` only) shows *resident* shared-system-memory bytes — the `WDDM` spill signal.** Matches Task Manager's `Shared GPU memory` column for the same PID. A benign baseline (staging/upload heaps, tens of MiB) is normal by design; the spill signature is this number *growing* while dedicated `VRAM` saturates — which is exactly what `hmn spill` and the library's `SpillTracker` detect. Always `0` on Linux and macOS (no shared-residency counter exists there).

4. **The SPILL column (since v0.2.11) is a single-snapshot approximation, not `hmn watch`'s temporal verdict.** `hmn ps` has no history to measure shared-memory *growth* against, so a spilling device here means "adapter dedicated commit at or above the 85% threshold AND adapter shared-resident at or above 256 MiB right now" — an absolute floor, not growth above a baseline. Since v0.2.13 the verdict is not repeated as `SPILL` on every row of the device: the process being paged (its own SHARED at or above the same 256 MiB floor) reads `PAGED`, the device's other processes read `device`, and the stderr summary states the verdict once (`; device 0 spilling: 154 MiB free, 2.1 GiB shared, 1 process paged`), counted over every process on the device before any filter. `PAGED` says who is being paged, not who caused the pressure: the memory manager pages whatever it chooses, and a desktop tenant's growth can page a trainer that did nothing new. `no` means the device is not spilling. When spill is not measured the cell reads `n/a` on Linux and macOS, where there is no shared-residency counter and so no spill to measure, and `?` (not `no`) on Windows when spill exists but cannot be read now (pre-`WDDM 2.0`, a non-NVIDIA adapter, a `PDH` hiccup, or a build without the `pdh` feature); neither is ever rendered as `no`. In `--json`, `spilling` (per device) and `paged` (per row) are `true`/`false`/`null`, and `shared_share` (per row) is a number or `null`; none collapses "can't tell" into `false`. Reach for `hmn watch`/`hmn spill` when the growth-over-baseline distinction actually matters.

5. **(Windows) `?` in the NAME column is now rare.** Before v0.2.8, any PID `OpenProcess` couldn't resolve — including plenty of ordinary foreign-user/`SYSTEM` processes like `dwm.exe` and `csrss.exe` — rendered as a bare `?`. As of v0.2.8, a `Toolhelp32Snapshot` fallback resolves those the same way `Get-Process`/Task Manager do (a system-wide process enumeration that reads names without opening a per-process handle, so it isn't subject to the same access check `OpenProcess` is), collapsing the vast majority of former `?` rows to real names non-elevated:

   ```
   PID    NAME                         VRAM     SHARED  DEVICE                      SPILL
   29092  dwm.exe                      1.2 GiB  2 MiB   NVIDIA GeForce RTX 5060 Ti  no
   19100  csrss.exe                    74 MiB   31 MiB  NVIDIA GeForce RTX 5060 Ti  no
   4      [kernel]                     4 MiB    0 MiB   NVIDIA GeForce RTX 5060 Ti  no
   ```

   *(real capture, non-elevated shell, v0.2.11 — both `dwm.exe`/`csrss.exe` rows rendered `?` before v0.2.8; SPILL added this release)*

   What remains genuinely unresolvable now renders as one of two honest brackets instead of an anonymous `?`: **`[exited]`** — the process exited between `hypomnesis`'s VRAM sample and the name lookup; elevation would not help, this is a timing race, not a permission wall. **`[protected]`** — the `Toolhelp32Snapshot` fallback itself could not be taken (very rare — resource exhaustion), so "exited" vs. "still running but unresolvable" can't be told apart. The Windows kernel itself (`PID 4`) continues to render as `[kernel]`, not `?` or `[protected]` — there is no executable image to read, so it's special-cased. This `[exited]`/`[protected]` distinction is Windows-only; Linux/macOS unresolved rows remain a bare `?` in the table (`name: None` underneath), since there is no equivalent false-wall-vs-real-wall gap to collapse there — see the [FAQ](docs/FAQ.md#what-does-a--in-the-name-column-mean--and-when-do-i-need-elevation) for the platform breakdown.

   *Security note.* A `[protected]` row (or, on Linux, a bare `?`) that does not resolve under elevation is one of: a process owned by another user, a process running as `SYSTEM` / `LOCAL SERVICE` / `NETWORK SERVICE`, a `PPL`-protected process, or (rarely, post-v0.2.8) the snapshot API itself failing. None of these are intrinsically malicious — but on a single-user desktop, an *unexpected* unresolved row holding substantial VRAM is worth investigating: a malicious local process (including a privileged-or-cross-user AI agent) using GPU resources would land in exactly this set. On macOS a bare `?` means both name lookups failed or the process is gone, and elevation does not change that; see Limitations, item 9. The `(N protected — re-run elevated for names)` parenthetical on the `hmn ps` summary line is intentionally surfaced because this distinction is security-relevant, and (as of v0.2.8) counts `[protected]`/`None` rows and the rare pre-`WDDM 2.0` `nvidia-smi` fallback's literal `?` name (limitation 6, below) — not `[exited]`, since elevation can't help a process that's already gone. On macOS the clause reads `(N protected — re-run outside the sandbox)`: a sandbox withholds a macOS name, and elevation does not lift it. `hypomnesis` is a measurement tool, not a malware scanner — but its honesty about the gap is itself a defensive primitive.

6. **Pre-`WDDM 2.0` Windows falls back to `nvidia-smi --query-compute-apps`.** Vanishingly rare in 2026 — `WDDM 2.0` shipped with Windows 10 1709 (October 2017). On the fallback path, `hmn ps` is compute-only (matching the Linux semantic) and `used_memory` may be `[N/A]` under `WDDM` (parser drops those rows). The `source` field on `GpuProcessEntry` reads `GpuQuerySource::NvidiaSmi` rather than `GpuQuerySource::Pdh` on this path.

7. **`R570`-class driver-bug filtering.** The `u64::MAX` sentinel (`R570` driver bug on `RTX 5060 Ti` and similar consumer GeForce cards) and the `used > total` corruption checks are applied per-row in `hmn ps`; affected rows are dropped rather than reported as garbage.

8. **macOS `used_bytes` reflects currently-resident GPU pages.** The kernel evicts idle Metal pages from a process's `graphics_footprint`, so the same PID may report different values across successive `hmn ps` calls when its working set has cooled. This is the same resident-bytes semantics as Windows `WorkingSetSize` and Linux `VmRSS` — not a macOS quirk, the cross-platform contract.

9. **On macOS the sandbox decides what `hmn` can read, not who owns a process.** Unsandboxed (a shell, Terminal.app, an elevated shell), `hmn ps` lists every user's processes that hold GPU memory, root's included (measured: 0 `EPERM` over 920 PIDs); no elevation is needed, and elevation does not help. Inside a sandbox that denies `process-info` (an App Sandbox app, or an agent harness's Seatbelt profile) `hmn` measures what is permitted and counts the rest: `hmn ps` lists the processes it can read and ends its summary with `N unreadable — re-run outside the sandbox`. When it can read nothing but itself, `hmn ps` prints `hmn: ps failed to query device 0: process list unreadable (…)` and exits `2` with no table, and `--exit-status` exits `2` rather than `1` when a process the filters could match was unreadable or a device failed. A name the kernel withholds from `proc_pidpath` falls back to the process's `p_comm`, cut at 16 bytes. Elevation does not help a sandboxed caller.

### `hmn spill` — WDDM spill detection

`hmn spill -- <command>` wraps a command `time(1)`-style: it spawns the command with inherited stdio, polls the spill state at a configurable interval (`--interval <MS>`, default **100 ms**), prints a `SpillReport` to **stderr** when the command exits, and **passes the wrapped command's exit code through** (so it drops into existing scripts and CI steps unchanged):

```sh
hmn spill -- python train.py
# ... train.py runs to completion, its stdout untouched ...
# SpillReport prints here (stderr):
#   hmn spill: peak dedicated 14.3 GiB / 15.7 GiB
#              peak shared    3.1 GiB (baseline 163 MiB)
#              episodes       1 — total 13.1s, longest 13.1s, first +2.0s into run
```

*(The report block is real output from the release-validation forced-spill run on the reference RTX 5060 Ti — a 20 GiB working set forced onto the 16 GiB card; `python train.py` stands in for whatever you wrap.)*

**What "spill" means here — residency, not commitment.** Under `WDDM`, a process can *commit* GPU memory far past dedicated `VRAM` with zero bytes actually paged out (see Limitation 2 above — that's every big PyTorch process, and it is *not* spill). Spill is **resident shared-system-memory growth while dedicated `VRAM` saturates** — the state where `VidMm` is actually paging GPU allocations over `PCIe` and your throughput craters. `hmn spill` flags an *episode* only when both hold: adapter dedicated-resident ≥ 85% of capacity **and** shared-resident has risen ≥ 256 MiB above its start-of-run baseline (staging heaps live in shared memory by design, so a baseline is normal). Transient spills are first-class: each contiguous spilling stretch is one episode, so *many short episodes* reads as "marginally over budget — shave the batch size" while *one sustained episode* reads as "genuinely over — rethink model / precision".

`--json` emits the report as a single JSON object on stdout (fields: `measurable`, `spilled`, `observations`, `baseline_shared_bytes`, `peak_shared_bytes`, `peak_dedicated_bytes`, `dedicated_limit_bytes`, `total_spill_duration_ms`, `episodes[]`). Check `measurable` before trusting `spilled: false` — on Linux and macOS the wrapped command still runs, but there is nothing to measure (`is_spill_measurable()` is `false`: normal `CUDA` OOMs rather than silently paging, and Apple `UMA` has nothing to spill *into*), so stderr says `spill not measurable on this platform` instead of printing a misleading all-zeros report.

Library consumers get the same primitive as [`SpillTracker`](https://docs.rs/hypomnesis) — `observe()` in their own loop, an instantaneous `is_spilling()` and a latched `has_spilled()`, and the episode history via `into_report()`. One honest limitation, shared by both: there is no background thread, so **a spill shorter than the gap between two observations is invisible** — `hmn spill`'s 100 ms default is the answer when temporal resolution matters more than in-loop integration.

### `hmn watch` — attach to a running PID

`hmn spill` only wraps a *new* command. `hmn watch [PID...]` attaches to
process(es) that are **already running** — the gap a rhyme-mdlm dogfooding
report hit three times triaging a 15-hour training campaign, hand-rolling
"two `hmn ps` samples minutes apart, diff by eye" every time
([`docs/dogfooding-feedbacks/dogfooding-spill-triage-watch-mode.md`](docs/dogfooding-feedbacks/dogfooding-spill-triage-watch-mode.md)).
Same `SpillTracker` core as `hmn spill`, on a timer instead of a wrapped
child — a `time(1)`-style scrolling sampler, **not a TUI** (same discipline
as `hmn spill`; see [Why no `hmn kill`?](#why-no-hmn-kill) for the same
scope-discipline reasoning applied to "why not a live-refresh dashboard"):

```sh
hmn watch 21844                          # attach to a known PID
hmn watch                                # no PID: auto-select top 5 by committed VRAM
hmn watch --top 3 --interval 30s --duration 10m --json   # tune interval/window, stream JSONL
hmn watch --follow-new --filter train --json             # follow one program by name (v0.2.12)
```

With no PID, `hmn watch` auto-selects the top `--top` (default 5) processes
by committed `VRAM` from the first sample and keeps that fixed set for the
run. Each interval prints one row per watched PID — committed / shared
`VRAM`, per-interval deltas, and a SPILL cell (the same adapter-wide
condition `hmn spill` uses, reused unchanged). Since v0.2.13 it names the
process being paged, as `hmn ps` does: while the adapter spills, a process
whose own SHARED is at least 256 MiB reads `PAGED` and the others `device`;
`--json` samples carry `paged`, and the closing per-PID summary says whether
each process was ever paged. Spill is measured as shared-memory growth above
the first sample, so a spill already under way at attach is not counted; since
v0.2.13 `hmn watch` says so at attach, in its closing summary, and as
`spilling_at_attach` in `--json` — `hmn ps` shows the current state. A watched PID absent from a
sample renders `0 MiB` — `hmn watch` cannot distinguish "exited" from
"currently holds no GPU memory" and does not auto-stop on this basis; use
`--duration` or Ctrl+C. At attach, though, an explicit PID that names no
running process at all — a typo — gets a one-line stderr warning (since
v0.2.13: `hmn watch: pid=999999 names no running process; its rows will read
0 MiB`), and is still watched. `--interval` / `--duration` take duration strings
(`500ms`, `30s`, `5m`, `1h`, or a bare number of seconds) rather than raw
milliseconds.

**`--follow-new`** (auto-select mode only — a hard error combined with an
explicit PID) re-runs the top-`--top` selection *every* interval instead of
once at attach, for the shape a candle-mi dogfooding report hit running
`hmn watch` alongside a 19-process sequential test suite: successive
short-lived GPU processes that are all born *after* attach, which a frozen
selection never sees. A PID entering starts with a fresh baseline; a PID
leaving (exited, or dropped below rank `--top`) stops appearing in the live
rows and is *finalized* into the closing summary's `per_pid[]` with its
peak/baseline, instead of rendering `0` forever — so the summary becomes a
roster of everyone who mattered during the watch, not just whoever was on
top at `t=0`. A stderr breadcrumb reports each change
(`entered pid=... (name); left pid=... (name)`).

**`--filter <PATTERN>` and `--min <SIZE>`** (since v0.2.12, auto-select mode
only) change *what* is selected, where `--follow-new` changes *when*. Rank
alone cannot say "follow this program": a candle-mi dogfooding report ran
`--follow-new --top 3` beside a patching campaign and found 73.9% of the rows
it committed as experimental record were the desktop
([`docs/dogfooding-feedbacks/dogfooding-watch-filter-by-identity.md`](docs/dogfooding-feedbacks/dogfooding-watch-filter-by-identity.md)).
`--filter` keeps only processes whose name contains the pattern, ignoring case
(repeat it to accept several); `--min` keeps only those whose total footprint
(`used + shared`, exactly as `hmn ps --min` measures it) reaches SIZE; the top
`--top` of what remains are watched, re-selected every interval under
`--follow-new`. The active criterion is printed on the stderr header line, so
a saved capture still says how it was selected. A process whose name cannot be
resolved cannot match a filter: a followed one whose name briefly reads
`[protected]` keeps matching on its last resolved name, and one that never
resolved is announced once on stderr rather than dropped silently. A size
floor is a proxy for identity — use it alongside `--filter`, not instead of
it. Either flag combined with an explicit PID is a hard error (exit `2`).

Runs until `--duration` elapses or Ctrl+C, then prints a closing summary
(the same `SpillReport` shape as `hmn spill`, plus a per-PID peak/baseline
table) and exits **`0`** if spill was never observed, **`1`** if it was at
least once, **`2`** on a hard error — designed for a watchdog script to check
directly. Where spill is not measurable (Linux, macOS), the summary says
`spill not measurable on this platform` in place of the report (since
v0.2.13; earlier versions printed an all-zeros report ending in `no spill
observed`) and the exit code is `0`, so a script there should read
`measurable` from `--json` before taking `0` as "no spill":

```sh
hmn watch 21844 --duration 5m
[ $? -eq 1 ] && echo "spilled in the last 5 minutes"
```

`--json` streams JSON Lines to stdout — since v0.2.12 a first
`{"kind":"start",...}` object describing the run (`hmn_version`, the
invocation, device, interval, and the `selection`: mode, explicit PIDs, `top`,
`--filter` patterns, `--min` bytes), then one `{"kind":"sample",...}` object
per PID per interval as it happens, plus a final `{"kind":"summary",...}`
object (the `SpillReport` fields plus `per_pid[]`) when the watch ends;
pipeable to `jq -c` live. A capture with a `start` record but no `summary`
was cut short. Select records by `kind` — a script that assumed line 1 is a
`sample` must skip the `start` record (`jq -c 'select(.kind == "sample")'`). Each sample carries `t_ms` (relative to attach)
and, since v0.2.11, `wall_clock` (absolute UTC ISO-8601 with millisecond
precision, e.g. `"2026-09-14T10:12:03.482Z"`) — for joining a spill trace
against a log stamped with real time, like a training driver's own run
log, without hand-converting `t_ms` offsets. Full walkthrough, including
the campaign that motivated it:
[Triage a job that's already running](docs/tutorials/watching-a-running-job.md).

### `hmn fits` — headroom predicate (since v0.2.11)

The question that actually matters before launching a job is rarely "what's on
the GPU" but "will this job fit *right now*". `hmn fits <SIZE>` answers it in
one command, gateable from a run script, instead of a hand-rolled
`hmn --json | jq` check repeated in every launcher:

```sh
$ hmn fits 1GiB
hmn: fits — 13.8 GiB free >= 1.0 GiB requested (12.75 GiB headroom; device 0 [NVIDIA GeForce RTX 5060 Ti])
$ echo $?
0

$ hmn fits 999GiB
hmn: does not fit — 13.8 GiB free < 999.0 GiB requested (short by 985.25 GiB; device 0 [NVIDIA GeForce RTX 5060 Ti])
$ echo $?
1
```

*(real captures, reference RTX 5060 Ti)*

Exits **`0`** if `SIZE` fits in the target device's current free `VRAM`
(`--device`, default `0`), **`1`** if it doesn't, **`2`** on a hard error (bad
device) — deliberately parallel to `hmn watch`'s `0`/`1`/`2` contract. The
message always states an exact headroom/shortfall margin alongside the
rounded `free`/`requested` figures, so a near-miss where both round to the
same display string (e.g. `"12.0 GiB free < 12.0 GiB requested"`) still
reads unambiguously. `free_bytes` nets out `reserved_bytes` **on the NVML
path only** (see
[Why does `used_bytes` exceed my card's total VRAM?](docs/FAQ.md#why-does-used_bytes-exceed-my-cards-total-vram));
on the Windows `DXGI`-alone fallback it can over-state true free `VRAM`
(a documented per-process lower bound on usage), and on macOS it's a
static working-set budget, not a live gauge — `hmn fits` is exact on the
common NVML/Windows path this was built and verified against, with those
two narrower platform caveats.
`SIZE` uses the same syntax as `hmn ps --min` (see [`hmn`](#binary-hmn), above):
a bare byte count, or a decimal number with
`KiB`/`MiB`/`GiB`. No `--json` — the point is a scriptable exit code, not
structured output:

```sh
hmn fits 12GiB || { echo "won't fit, skipping run"; exit 1; }
python train.py
```

### Composable workflows

`hmn ps --json` exists for scripting and survives across platforms (same JSON shape on Windows, Linux, and macOS). Two recipes that have come up in dogfooding:

**Top-5 GPU consumers** (any platform with `jq` installed):

```sh
hmn ps --json | jq 'sort_by(-.used_bytes) | .[:5]'
```

(`hmn ps --sort dedicated` — the default — now covers this natively for the whole table, in both text and `--json` form; the `jq` recipe stays handy for slicing to a specific top-N or sorting by a field `--sort` doesn't offer, like `pid` or `name`.)

(`hmn ps --min 50MiB` — since v0.2.11 — now covers "hide desktop noise below a threshold" natively, in both text and `--json` form, filtering on *total* footprint — dedicated + shared, matching `--sort total`'s definition — rather than either alone; the kill recipe below still reaches for `jq` because it filters on dedicated specifically, to decide what's actually worth killing.)

**Terminate any process holding more than 1 GiB of `VRAM`** — the JSON output composes with the platform's native kill command. Windows (PowerShell or cmd):

```sh
hmn ps --json | jq -r '.[] | select(.used_bytes > 1073741824) | .pid' | ForEach-Object { taskkill /F /PID $_ }
```

Linux / macOS:

```sh
hmn ps --json | jq -r '.[] | select(.used_bytes > 1073741824) | .pid' | xargs -r kill -TERM
```

(Use `kill -KILL` instead of `-TERM` if you want the hard variant; `-r` skips empty input.)

**Fail a CI step when a run spilled** — `hmn spill --json` composes the same way (`jq -e` sets the exit code from the expression):

```sh
hmn spill --json -- python train.py | jq -e '.measurable and (.spilled | not)'
```

**Watch a specific process's shared-residency from outside** (the `train_guarded.py`-style watchdog — key the guard off `shared_used_bytes`, never off the commit figure):

```sh
hmn ps --json | jq '.[] | select(.pid == 12345) | .shared_used_bytes'
```

#### Why no `hmn kill`?

A `hmn kill <pid>` subcommand was considered for v0.2.3 and rejected to preserve `hypomnesis`'s "measurement, not control" scope discipline. Process termination is not a *measurement* operation — it's a control operation, and one with platform-specific permission models (`taskkill` vs `kill -SIGNAL` vs `sudo kill`) that `hmn` would inevitably get wrong on at least one platform. Piping JSON to the platform's native killer is more honest about what's happening, more flexible (filter on any field, not just PID), and keeps `hypomnesis`'s API surface small.

#### Why no `hmn spill --kill` / `--throttle`?

Same discipline, same answer (recorded here so it isn't re-argued in a future PR — v0.2.5 considered and rejected both). What to *do* about a spill — kill the run, drop the batch size, switch to CPU — is the consumer's decision, wired through whatever primitive their workload already uses. `hmn spill --json` composed with `jq` and the platform's native killer covers the automation case; the library's `SpillTracker` deliberately exposes queryable state (`is_spilling()` / `has_spilled()`) and no callbacks, no background thread, and no built-in debounce for the same reason.

## Capabilities

| Metric | Windows | Linux | macOS |
|--------|---------|-------|-------|
| Process RSS | `K32GetProcessMemoryInfo` | `/proc/self/status` (no `unsafe`) | `task_info(TASK_VM_INFO_PURGEABLE).phys_footprint` |
| Device-wide GPU memory | `NVML` (`nvml.dll`) | `NVML` (`libnvidia-ml.so.1`) | `sysctl hw.memsize` (total) + `MTLDevice.recommendedMaxWorkingSetSize` (free) |
| Device reserved memory | `NVML` v2 (`nvmlDeviceGetMemoryInfo_v2`, R510+) | `NVML` v2 (R510+) | n/a (`None` — UMA has no carve-out) |
| Driver version | `NVML` (`nvmlSystemGetDriverVersion`) + `nvidia-smi` fallback (`--query-gpu=driver_version`) | same as Windows | n/a (`None` — no NVIDIA driver on Apple Silicon) |
| Per-process GPU memory | `DXGI` (`IDXGIAdapter3::QueryVideoMemoryInfo`) | `NVML` (`nvmlDeviceGetComputeRunningProcesses`) | `ledger(LEDGER_ENTRY_INFO_V2).graphics_footprint` |
| GPU-process listing (other PIDs) | `PDH` (`\GPU Process Memory(*)\Dedicated Usage` + `Shared Usage`) + `OpenProcess` / `QueryFullProcessImageNameW`; `nvidia-smi` fallback | `NVML` + `/proc/<pid>/comm`, extended past its 15-byte cut via `exe` / `argv[0]` (compute-only) | `proc_listpids` + per-PID `ledger` + `proc_pidpath`; on macOS the sandbox, not process ownership, decides what `hmn` can read; see [README Limitations, item 9](#binary-hmn) |
| Process existence (`process_exists`, since v0.2.13) | `Toolhelp32` process snapshot (`pdh` feature — the mechanism the GPU-process listing uses for names) | `/proc/<pid>/status`, whose `Tgid` must equal the PID so a thread ID is not taken for a process (no `unsafe`) | `proc_pidpath`, then `sysctl` `KERN_PROC_PID` when libproc gives no path (`metal` feature) |
| Spill detection (`SpillTracker`, `hmn spill`, `hmn watch`) | `PDH` `\GPU Adapter Memory(*)\Dedicated Usage` + `Shared Usage` (`WDDM 2.0`+) | n/a (`is_spill_measurable()` = `false` — normal `CUDA` OOMs rather than silently paging) | n/a (`false` — `UMA` has nothing to spill *into*) |
| Fallback | `nvidia-smi` subprocess | `nvidia-smi` subprocess | no second backend; enumeration, names and lookups try libproc first, then `sysctl`; what a sandbox still withholds is counted (Limitations, item 9) |

`hypomnesis` uses `IDXGIAdapter3` on Windows because `WDDM` means the kernel memory manager — not the NVIDIA driver — owns GPU allocations, so `NVML`'s per-process query returns `NOT_AVAILABLE` under Windows. `DXGI 1.4` is the only reliable per-process source. On Linux, `NVML`'s `nvmlDeviceGetComputeRunningProcesses_v3` returns true per-process figures. On Apple Silicon (M-series), the GPU shares system DRAM via unified memory architecture (UMA), so `hw.memsize` is both the system RAM total and the GPU memory pool.

The crate handles two known driver bugs out of the box:

1. **`NVML` `u64::MAX` sentinel** — some `R570`-series drivers report `0xFFFFFFFFFFFFFFFF` for every running process's memory (observed on `RTX 5060 Ti`). `hypomnesis` detects this and falls back to `nvidia-smi`.
2. **`used > total` corruption** — sanity-checks each per-process reading against the device-wide total; falls back to `nvidia-smi` on detected corruption.

### macOS UMA semantics: what `free_bytes` means

On a discrete GPU, `free_bytes` is "untaken bytes in the VRAM pool" — a hard number bounded by the card's physical memory. On Apple Silicon the GPU has no separate pool: it shares system DRAM via unified memory architecture (UMA). `hypomnesis` therefore reports `free_bytes` as `MTLDevice.recommendedMaxWorkingSetSize` — the kernel-projected GPU working-set budget that Apple's Metal driver itself computes, factoring in wired-page reserves, system memory pressure, and the kernel's known compression / eviction capability.

Two consequences worth noting:

- **The number changes slowly under load.** Apple's driver smooths it; it is a policy figure, not an instant-state reading. Expect it to shrink modestly as system memory pressure rises and recover as pressure abates.
- **Per-process `used_bytes` (from `graphics_footprint`, used by `gpu_processes()` and `process_gpu_info()`) reflects currently resident GPU pages**, matching the resident-bytes semantics of Windows `WorkingSetSize` and Linux `VmRSS`. Idle apps' Metal pages get evicted by the kernel; the same PID may report different values across calls. This is the contract Windows and Linux already exhibit, not a macOS-specific quirk.

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `nvml` | yes | `NVML` dynamic load via `libloading` (Linux + Windows-`WDDM` device-wide) |
| `dxgi` | yes | Windows per-process `VRAM` via `IDXGIAdapter3` (no-op on non-Windows) |
| `pdh` | yes | Windows foreign-process `VRAM` listing (`\GPU Process Memory(*)\Dedicated Usage` + `Shared Usage`) and the `\GPU Adapter Memory(*)` counters backing `SpillTracker`'s live path, under `WDDM 2.0`+ (no-op on non-Windows; depends on `dxgi`). `SpillTracker` itself compiles everywhere regardless — without this feature it is simply never measurable. |
| `metal` | yes | macOS device-wide GPU budget via `objc2-metal` (`MTLDevice.recommendedMaxWorkingSetSize`); no-op on non-macOS. RAM and per-process GPU paths are libSystem-only and unaffected by this flag. |
| `nvidia-smi-fallback` | yes | Subprocess fallback when `NVML` / `DXGI` / `PDH` fail or are otherwise unavailable (e.g. pre-`WDDM 2.0` Windows) |
| `report` | no | `MemoryReport` delta + `print_delta` / `print_before_after` / `ram_mb` / `vram_mb` helpers (`candle-mi` parity, candidate for `candle-mi` v0.2 migration via Cargo flag flip; figures labelled `MB` are `MiB`); `format_free` / `print_free` / `format_total` / `format_used` formatting helpers on `GpuDeviceInfo` |
| `debug-output` | no | Print raw `NVML` / `DXGI` / `PDH` / `nvidia-smi` / spill values to stderr (diagnostic) |
| `cli` | yes (since v0.2.8) | Build the `hmn` CLI binary (pulls `clap` 4 and `ctrlc` as deps — the latter backs `hmn watch`'s graceful Ctrl+C summary). Library-only consumers who don't want the extra deps use `--no-default-features` and select source features explicitly. |
| `test-helpers` | no | Expose `GpuDeviceInfoBuilder`, `GpuProcessEntryBuilder`, and `SpillReportBuilder` for downstream tests that need synthetic fixtures. Default-off, additive — production code must never enable it. |

## Documentation

| Doc | |
|-----|---|
| [FAQ](docs/FAQ.md) | Common questions — commit vs resident, `hmn spill` vs `hmn watch`, the SHARED baseline, the spill condition and its 85% threshold, per-platform zeros, `?`/`[exited]`/`[protected]` rows and elevation, threading, polling cost, upgrading |
| [Tutorial: Is my run spilling?](docs/tutorials/is-my-run-spilling.md) | Walkthrough: wrap a run with `hmn spill`, read the episode pattern, attribute per-PID, wire `SpillTracker` into your own loop |
| [Tutorial: Triage a job that's already running](docs/tutorials/watching-a-running-job.md) | Walkthrough: attach `hmn watch` to a running PID, read the live SPILL column, script the exit code — and `--follow-new` to stand guard over a machine through a suite of short-lived jobs |
| [ROADMAP](ROADMAP.md) | Status snapshot: shipped, committed, speculative, and deliberately-rejected ideas |
| [Per-release roadmaps](docs/) | The detailed plan behind each release (`docs/roadmap-vX.Y.Z.md`), including live-measured deviations |
| [The brief](docs/hypomnesis-brief.md) | Why this crate exists — Plato, the v0.1.x VRAM saga, the extraction from `candle-mi` |
| [CHANGELOG](CHANGELOG.md) | Release history |

## Used by

- [candle-mi](https://github.com/mi-for-the-rust-of-us/candle-mi) — mechanistic-interpretability toolkit for `candle`. As of **v0.1.16** it deletes its in-tree measurement FFI and delegates `src/memory.rs` to `hypomnesis` (lean feature set: `nvml`, `dxgi`, `nvidia-smi-fallback`, `metal`), flattening a `hypomnesis::Snapshot` into its own `MemorySnapshot`. Its v0.1.16 dogfooding report — live-validated on an `RTX 5060 Ti` (16 GiB, Windows / `WDDM`) — drove v0.2.4's `reserved_bytes` addition. Its `resurrect.ps1` verification pipeline's live-caught `DPC_WATCHDOG_VIOLATION` bugcheck drove v0.2.9's `driver_version` addition, so the pipeline's provenance record can stamp the GPU driver alongside the Rust toolchain. Its `scripts/resurrect.ps1` oracle suite is a load-bearing `hmn spill --json` / `hmn watch` consumer and the source of the v0.2.7 `--follow-new` / `ps --sort` dogfooding report.
- [hf-fetch-model](https://github.com/mi-for-the-rust-of-us/hf-fetch-model) — Hugging Face model weights and metadata fetcher (uses `device_info` for `inspect --check-gpu`)

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.

## Development

- Exclusively developed with [Claude Code](https://claude.com/product/claude-code) (dev)
- Git workflow managed with [Fork](https://fork.dev/)
- All code follows [CONVENTIONS.md](CONVENTIONS.md), derived from [Amphigraphic-Strict](https://github.com/PCfVW/Amphigraphic-Strict)'s [Grit](https://github.com/PCfVW/Amphigraphic-Strict/tree/master/Grit) — a strict Rust subset designed to improve AI coding accuracy.
