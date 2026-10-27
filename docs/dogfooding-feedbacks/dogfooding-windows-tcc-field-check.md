# Dogfooding report (from the Windows `TCC` field check on a rented Tesla V100): `hmn` reads a `TCC` GPU through `NVML` and `nvidia-smi` alone, and v0.2.15's broken-`NVML` rule holds over a real `DXGI` zero — but the fallback's process names are full paths, and spill reads `?` where it cannot exist

**Date:** 2026-10-27
**Reporter:** maintainer field check, OVHcloud Public Cloud `t1-45` in `GRA7` (Tesla V100-PCIE-16GB in `TCC` mode, driver 582.70, CUDA 13.0, 8 vCPU), Windows Server 2022 Standard 10.0.20348, `hmn` built from `52c1608` with a static C runtime (it prints `hmn 0.2.14`, the version before the bump)
**Severity:** validation of `hmn` on a `TCC` GPU and of v0.2.15's broken-`NVML` rule + request for file names on the `nvidia-smi` fallback and for `n/a` spill on a `TCC` GPU
**Affected area:** `nvidia_smi::query_compute_apps` names on Windows, and `--filter` over them; the SPILL and `PAGED` cells and the spill-tracker messages when `PDH` finds no adapter; the `dxgi_count_stands` doc comment
**Status:** Proposed — v0.2.16 candidates (requests 1 and 2). The doc comment was corrected in v0.2.15 (`d6f5e29`).

---

## TL;DR

`hmn` had never met a GPU in `TCC` mode, which `DXGI` and `PDH` cannot see. On a rented V100 it
reads one through `NVML` and the `nvidia-smi` fallback alone, and every figure matches
`nvidia-smi` to the MiB:

- `hmn` prints `GPU 0 [Tesla V100-PCIE-16GB]: free 16234 MiB / 16384 MiB (141 MiB reserved),
  driver 582.70`, and no row for the VM's display adapters.
- `hmn ps` lists two `cudahold` processes at 2356 MiB and 1332 MiB, as `nvidia-smi` does;
  `--filter`, `--min`, `--exit-status` and `--device 1` behave as specified.
- `hmn watch` follows them with explicit PIDs, auto-selection and `--filter`, and names
  PowerShell's PID, which holds nothing on the GPU, `powershell.exe` through `process_name`.
- With the fake `NVML` beside `hmn.exe`, code 18 exits `2` ("can't tell") over the real `DXGI`
  zero, and codes 9 and 28 read as no GPU: the rule v0.2.15 added for exactly this card.
- A card disabled in Device Manager reads as no GPU, rightly; a doc comment said otherwise and is
  fixed.

Two findings: on the `nvidia-smi` fallback NAME is the full path (`C:\Users\…\holder-a.exe`), so
the table is wide and `--filter` can match a folder; and spill reads `?` ("can't measure now") with
a `PDH error` line, where it cannot exist and `n/a` is the answer, as on Linux and macOS.

## What happened — the card, through `NVML` and `nvidia-smi` only

The driver model, before anything ran:

```
index, name, pci.bus_id, driver_version, driver_model.current, driver_model.pending, memory.total [MiB], memory.used [MiB], memory.free [MiB]
0, Tesla V100-PCIE-16GB, 00000000:00:06.0, 582.70, TCC, TCC, 16384 MiB, 9 MiB, 16234 MiB
```

Windows sees three display adapters; `DXGI` lists only `WDDM` ones, and `hmn` counts the V100
alone, by `NVML`:

```
Name                 : Microsoft Basic Display Adapter
PNPDeviceID          : PCI\VEN_1013&DEV_00B8&SUBSYS_11001AF4&REV_00\3&267A616A&1&10
Name                 : Microsoft Remote Display Adapter
PNPDeviceID          : SWD\REMOTEDISPLAYENUM\RDPIDD_INDIRECTDISPLAY&SESSIONID_0002
Name                 : NVIDIA Tesla V100-PCIE-16GB
PNPDeviceID          : PCI\VEN_10DE&DEV_1DB4&SUBSYS_121410DE&REV_A1\3&267A616A&1&30
```

```
### hmn.exe
GPU 0 [Tesla V100-PCIE-16GB]: free 16234 MiB / 16384 MiB (141 MiB reserved), driver 582.70
```

The name comes from `NVML`, since `dxgi::adapter_name(0)` finds no NVIDIA adapter. With two
holders running (`cudahold 2048` and `cudahold 1024`, copied as `holder-a.exe` and
`holder-b.exe`), `nvidia-smi` and `hmn ps` side by side:

```
pid, process_name, used_gpu_memory [MiB]
4104, C:\Users\Administrator\Documents\kit\holder-a.exe, 2356 MiB
6584, C:\Users\Administrator\Documents\kit\holder-b.exe, 1332 MiB
```

```
PID   NAME                                               VRAM     SHARED  GPU  DEVICE                SPILL
4104  C:\Users\Administrator\Documents\kit\holder-a.exe  2.3 GiB  0 MiB   0    Tesla V100-PCIE-16GB  ?
6584  C:\Users\Administrator\Documents\kit\holder-b.exe  1.3 GiB  0 MiB   0    Tesla V100-PCIE-16GB  ?
hmn: 2 GPU processes found (3.6 GiB committed total).
```

The listing comes from the `nvidia-smi` fallback: `pdh::query_per_process_vram` fails on the
missing `DXGI` adapter, and the JSON's `used_bytes` (2470445056, 1396703232) are `nvidia-smi`'s
MiB exactly. Under `TCC` `nvidia-smi` reports real per-process memory, unlike under `WDDM`, where
its rows read `[N/A]` and the parser drops them. The ROADMAP item that asked for this check
expected the listing to rest on that fallback; it does, and holds.

## What happened — the broken-`NVML` rule, over a real `DXGI` zero

v0.2.15 stopped a `DXGI` count of zero from standing in for a broken `NVML`, because `DXGI` cannot
see a `TCC` GPU. CI pins it on a runner with no NVIDIA driver; here the zero is a real `TCC` box's.
The fake `NVML` (`tests/fixtures/nvml_fake.c` as `nvml.dll`) sits beside a copy of `hmn.exe`,
which Windows loads before the driver's own:

```
---------- INIT=18 (driver upgraded without a reboot): expect exit 2 ----------
### hmn.exe
exit: 2
hmn: failed to count devices: NVML error: nvmlInit_v2 returned NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)
### hmn.exe ps
exit: 2
hmn: ps failed to count devices: NVML error: nvmlInit_v2 returned NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)
hmn: ps: no device could be queried, so nothing could be listed
```

Codes 9 and 28 print `hmn: no visible GPUs.` and `hmn: 0 GPU processes found (no GPU to list).`
with exit `0`, and a fake device is named by the fake (`GPU 0 [Fake Tesla]: …, driver 999.99`),
since `DXGI` has no name to offer.

The same question with the real driver: the V100 disabled in Device Manager.

```
hmn: no visible GPUs.
exit=0
PID  NAME  VRAM  SHARED  GPU  DEVICE  SPILL
hmn: 0 GPU processes found (no GPU to list).
exit=0
NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver. [...]
exit=9
```

`nvidia-smi` exits with `NVML`'s code, 9, `NVML_ERROR_DRIVER_NOT_LOADED`; `hmn` reads that as no
GPU, which is right, since no process can run on a disabled card. Re-enabled, the card read as
before. The `dxgi_count_stands` comment said a disabled card keeps `NVML`'s error, like a card in
Code 43; it now says what was measured (`d6f5e29`).

## What was wrong — names as full paths, and spill where it cannot exist

**Names.** The `PDH` path names a process by its file name (`holder-a.exe`, as on any `WDDM` box);
the `nvidia-smi` fallback keeps the full path `nvidia-smi` prints. The table widens by the path,
and `--filter` matches the name it is given, so on this path a pattern can match a folder:
`--filter users` would select every process under `C:\Users`. That last point is read from the
code (`filterable_name` takes the name as is), not run; `--filter holder` and
`ps --filter holder-b` selected the right rows.

**Spill.** A `TCC` GPU is not managed by `WDDM`'s video memory manager, so nothing is paged to
shared memory: spill cannot exist there, as on Linux and macOS, where the SPILL cell reads `n/a`.
Here it reads `?`, the cell for "could not measure now" (a pre-`WDDM 2.0` driver, a failed `PDH`
sample), and the JSON's `spilling` is `null`. `hmn watch` and `hmn spill` say why in words that
read as a fault:

```
hmn: watch spill tracking unavailable (PDH error: no NVIDIA adapter at device_index 0 (DXGI walk)); showing per-PID VRAM only
hmn: spill tracking unavailable (PDH error: no NVIDIA adapter at device_index 0 (DXGI walk)); running command unmeasured
hmn spill: spill not measurable (no tracker)
```

## Requests

1. **Spill reads `n/a` on a `TCC` GPU, and says why in plain words** (the one that matters most:
   `?` invites a user to wait for a measurement that will never come). The signal is `NVML`'s own
   `nvmlDeviceGetDriverModel` (Windows only), which returns the current model (`WDDM`, `TCC` or
   `MCDM`); failing that, "`NVML` has the device, `DXGI` has no NVIDIA adapter at its index" is the
   same fact seen from outside. Then the SPILL and `PAGED` cells read `n/a`, the JSON stays
   `null`, and the tracker's message becomes something like
   `hmn watch: spill does not apply to device 0 (TCC mode); per-PID VRAM only`.
2. **The `nvidia-smi` fallback names a Windows process by its file name**, as the `PDH` path
   does: `pdh::name_from_pid_windows(pid)` first, so the two paths agree and a protected process
   still resolves, else the file-name part of what `nvidia-smi` printed. `--filter` then matches
   names, not folders. Whether `nvidia-smi` on Linux prints paths too was not measured; the Linux
   fallback runs only without `NVML`.

## Smaller observations

1. **SHARED reads `0 MiB` on the fallback's rows**, the value the fallback sets, as `NVML` rows do
   on Linux; under request 1 it could read `n/a` with SPILL.
2. **`hmn` does not list itself on a `TCC` GPU.** On a `WDDM` box `hmn.exe` appears at `0 MiB`
   (it opens a `DXGI` device); here it holds no context, and `nvidia-smi` does not list it.
3. **`cudahold` worked on its first real run.** `nvidia-smi` charges each holder its allocation
   plus 308 MiB, the `CUDA` context.
4. **The image comes without an NVIDIA driver**, and 582.70 installed without a reboot; the data
   center driver put the card in `TCC` by default, with `Disp.A` `Off`.
5. **Not covered:** a box mixing a `WDDM` and a `TCC` GPU, where `device_info` takes the name of
   the `DXGI` NVIDIA adapter with the same index (one GPU here); a card in Code 43; a lost GPU;
   `MCDM`; `WDDM` on this card, which NVIDIA's 582.70 notes reserve to vGPU.

## Acceptance fixtures (already run, free to regress against)

| Case | Command | Measured | Verdict |
|---|---|---|---|
| idle `TCC` V100 | `hmn` | `free 16234 MiB / 16384 MiB (141 MiB reserved), driver 582.70`, exit `0` | ✅ matches `nvidia-smi` |
| idle | `hmn ps` / `--exit-status` | header, `0 GPU processes found.`, exit `0` / `1` | ✅ |
| out of range | `hmn ps --device 1` | `device index 1 out of range (have 1 devices)`, exit `2` | ✅ |
| fits | `hmn fits 1GiB` / `64GiB` | exit `0` / `1`, `device 0 [Tesla V100-PCIE-16GB]` | ✅ |
| two holders | `hmn ps` | 2356 MiB and 1332 MiB, GPU 0, DEVICE `Tesla V100-PCIE-16GB` | ✅ figures; ❌ NAME full path, SPILL `?` |
| two holders | `hmn ps --filter holder-b`, `--min 1500MiB` | one row each, the right one | ✅ |
| two holders | `hmn watch <a> <b>`, auto-select, `--filter holder` | both followed, steady bytes | ✅ figures; ❌ `PDH error` wording |
| no GPU memory | `hmn watch <PowerShell PID>` | NAME `powershell.exe`, `0 MiB` | ✅ |
| spill | `hmn spill -- cudahold 512 5` | child exit `0`, `spill not measurable (no tracker)` | ✅ outcome; ❌ wording |
| fake `NVML`, init 18 | `hmn` / `hmn ps` | exit `2`, `NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)` | ✅ |
| fake `NVML`, init 9 and 28 | `hmn` / `hmn ps` | `no visible GPUs.` / `(no GPU to list)`, exit `0` | ✅ |
| fake `NVML`, one device | `hmn` | `GPU 0 [Fake Tesla]: free 15872 MiB / 16384 MiB (512 MiB reserved), driver 999.99` | ✅ |
| card disabled | `hmn` / `hmn ps` | `no visible GPUs.` / `(no GPU to list)`, exit `0`; `nvidia-smi` exit `9` | ✅ |

## Confidence

High for what was run: one card, one driver, one image, each output beside `nvidia-smi`'s for the
same moment, and the fake's outcomes identical to what `tests/cli_fake_nvml.rs` expects. The
`--filter`-matches-a-folder point is read from the code, not run. Nothing here covers more than
one GPU.

## References

- [`ROADMAP.md`](../../ROADMAP.md): the item *Datacenter GPUs in `TCC` mode on Windows, never run*,
  which this check answers, and the two items requests 1 and 2 became.
- [`docs/roadmap-v0.2.15.md`](../roadmap-v0.2.15.md): the broken-`NVML` rule and the Windows fake.
- [`tools/tcc-field-check`](../../tools/tcc-field-check): the procedure, the kit build and
  `run-checks.ps1`, which wrote the log quoted here; [`tools/cudahold`](../../tools/cudahold).
- [`tests/cli_fake_nvml.rs`](../../tests/cli_fake_nvml.rs) and
  [`tests/fixtures/nvml_fake.c`](../../tests/fixtures/nvml_fake.c).
- [`dogfooding-macos-sandbox-eperm-and-device-bounds.md`](dogfooding-macos-sandbox-eperm-and-device-bounds.md):
  where the SPILL cell became `n/a` where spill cannot exist.
- NVIDIA, [Data Center driver 582.70 release notes](https://docs.nvidia.com/datacenter/tesla/tesla-release-notes-582-70/index.html).
