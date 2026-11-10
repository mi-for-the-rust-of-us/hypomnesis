# Dogfooding report (from an 8×MI300X Linux node with ROCm 7.2.4): `hmn` reads AMD GPUs as no GPU — it prints `no visible GPUs.` and exits 0 beside a 4 GiB `hipMalloc` that `rocm-smi` and `amd-smi` both read to the byte

**Date:** 2026-11-09
**Reporter:** field check on one MI300X allocated from an 8×MI300X Linux node (ROCm 7.2.4, `amd-smi` 26.2.2, `rocm-smi`, Slurm with device cgroups), `hmn` 0.2.15
**Severity:** request for an AMD backend (a misreported absence, not a crash) + request that its parity with the NVIDIA path be written down
**Affected area:** `gpu::device_count` and every dispatcher built on it; `hmn` and `hmn ps` on a node whose GPUs are AMD
**Status:** ✅ **Resolved in v0.2.16** (ROCm backend through `librocm_smi64`)

---

## TL;DR

`hmn` has no AMD backend, and it does not say so. On a node with an MI300X it behaves as on a node
with no GPU, while the vendor's own tools read the card without trouble:

- `hmn` prints `hmn: no visible GPUs.` and exits `0`; `hmn ps` prints an empty table and
  `0 GPU processes found (no GPU to list).`, exit `0`. A human reads "no GPU on this node"; a
  script reads success.
- One process held a 4 GiB `hipMalloc` (4294967296 B) for 30 s. `rocm-smi --showpids` and
  `amd-smi process --json` both report 4445999104 B for it, the same figure to the byte.
- The device-wide delta from `rocm-smi --showmeminfo vram` (4446081024 B) differs from that figure
  by 81920 B (80 KiB, 0.002 %), so the per-process number is corroborated by a second signal.
- Inside the job the scheduler calls the GPU 4 (`SLURM_JOB_GPUS=4`) and ROCm calls it 0
  (`ROCR_VISIBLE_DEVICES=0`). ROCm's index is the one a backend must use.
- `librocm_smi64.so.1` and `libamd_smi.so.26` are already in `ldconfig` on the node, so reading the
  card needs no install.

Request 1 is the one that matters: a ROCm backend. Request 2 is the page that says what is the same
as on NVIDIA and what is not.

## What happened — `hmn` prints `no visible GPUs.` and exits 0 beside a 4 GiB allocation

In the job: `ROCR_VISIBLE_DEVICES=0`, `SLURM_GPUS_ON_NODE=1`, `SLURM_JOB_GPUS=4`. One process held a
4 GiB `hipMalloc` plus `hipMemset` for 30 s, and every tool was run from inside it:

| Tool | Result | Exit |
|---|---|---|
| `hmn` | `hmn: no visible GPUs.` | 0 |
| `hmn ps` | empty table; stderr `0 GPU processes found (no GPU to list).` | 0 |
| `amd-smi list` | 1 GPU | 0 |
| `rocm-smi --showmeminfo vram` | device total and used | 0 |
| `rocm-smi --showpids` | KFD processes, per-process VRAM | 0 |
| `amd-smi process --json` | 1 entry (the caller's); 0 foreign | 0 |
| `/sys/class/kfd/kfd/proc` | 1 per-process directory | 0 |

The two `hmn` lines are the ones that would be correct on a CPU-only box. Here they sit above a
card that three other tools list. The wording is the harm: "no visible GPUs" says the node has none,
and exit `0` tells a script there is nothing to act on.

## What the vendor tools agree on — 4445999104 B, within 80 KiB of the device delta

| Signal | Idle | While holding 4 GiB | Delta |
|---|---|---|---|
| `rocm-smi` device VRAM used (B) | 297771008 | 4743852032 | 4446081024 |
| `rocm-smi --showpids` VRAM USED for the process (B) | — | 4445999104 | — |
| Device total VRAM (B) | 206141652992 (192 GiB) | same | — |

`amd-smi process --json` for the same process:

| Field | Value (B) |
|---|---|
| `vram_mem` | 4341876000 |
| `gtt_mem` | 8232000 |
| `cpu_mem` | 0 |
| `mem_usage` | 4445999104 |

`mem_usage` equals `rocm-smi`'s VRAM USED exactly, and it is the figure to report. `vram_mem` and
`gtt_mem` are not a split of it: their sum, 4350108000 B, falls 95891104 B (about 91 MiB) short, and
nothing here explains the gap. A backend that summed the two fields would be wrong by that much.

The allocation costs 4445999104 − 4294967296 = 151031808 B (144 MiB) of overhead on MI300X. On an
H200 a 4 GiB allocation reads 4614 MiB, about 518 MiB of `CUDA` context. The overhead differs by
vendor, so a fixed "add 0.5 GiB" rule of thumb must not be carried over from NVIDIA.

## What the scheduler calls the GPU is not what ROCm calls it — scheduler 4, ROCm 0

The job's `SLURM_JOB_GPUS` is `4`; `ROCR_VISIBLE_DEVICES` is `0`, and `amd-smi` and `rocm-smi` both
index the card as 0. `gres.conf` maps the node's MI300X to `/dev/dri/renderD128` through `184`, and
`cgroup.conf` sets `ConstrainDevices=yes`, so the job sees one device node and the runtime numbers
from zero. In-job, ROCm's index is canonical: the `GPU` column of `hmn ps` should read 0 here, and
`hmn` should not refer to the scheduler's 4.

## Requests

1. **A ROCm backend through `librocm_smi64`** (the one that matters most: without it every AMD node
   reads as no GPU, and nothing else on this page helps). It is a source in the existing
   dispatchers, loaded at runtime like `NVML`, so a node without the library is not an error:
   - device count and per-device facts (name, total, used) from the library, with ROCm's device
     index as the canonical `GPU` value;
   - per-process VRAM from the `mem_usage` figure, which is what `rocm-smi --showpids` prints, not
     `vram_mem` and not `vram_mem + gtt_mem`;
   - a library that is present but broken exits `2` ("can't tell") for both `hmn` and `hmn ps`, as
     v0.2.15's rule does for `NVML`, and is not read as absence.
2. **The parity with NVIDIA is documented**, in the README and in `hmn --help`'s own wording: what
   `hmn` reports on an AMD node (device free and total, per-process VRAM), what it takes from the
   library, that the runtime overhead differs (144 MiB on MI300X against about 518 MiB on H200), and
   that the device index is ROCm's, not the scheduler's.

## Smaller observations

1. **The device-wide figure is the check, not the tool's own number.** The per-process figure was
   trusted only because `rocm-smi --showmeminfo vram` moved by the same amount to within 80 KiB;
   an acceptance run should keep that cross-check beside `hmn`'s row.
2. **Per-process listing looked job-scoped, and that is not proof.** `amd-smi process --json` showed
   the caller's process and no foreign one, but no other GPU job was running on the node then. An
   acceptance run on a two-MI300X allocation listed no row for another PID, and
   `rocm-smi --showpids` listed none either, so how a foreign process appears under
   `ConstrainDevices=yes` was not measured.
3. **The KFD directory lists processes, not their VRAM.** `/sys/class/kfd/kfd/proc` holds one
   directory per process; whether VRAM figures sit under it was not read.
4. **Several GPUs were measured on a two-MI300X allocation.** `hmn ps` with one process on device 0
   and a second on devices 0 and 1 shows one row per device, each with that device's own figure, and
   the second process's per-device figures sum to the `rocm-smi --showpids` figure to the byte.
   **Not covered:** two jobs sharing a node, a ROCm release other than 7.2.4, spill, and Intel GPUs.

## Acceptance fixtures (already run, free to regress against)

Each row is produced by the backend's acceptance run on an MI300X allocation: one process holding a
4 GiB `hipMalloc`, with `rocm-smi --showmeminfo vram` read before and during as the independent
signal. The result cell holds the command's captured stdout, stderr and exit status.

| Case | Command | Must hold | Result |
|---|---|---|---|
| one MI300X, one holder | `hmn` | names the GPU, free and total in MiB, exit `0` | {{acceptance:summary}} |
| one MI300X, one holder | `hmn ps` | one row, VRAM within 1 MiB of `rocm-smi --showpids`, `GPU` 0 | {{acceptance:ps}} |
| one MI300X, one holder | `hmn ps --json` | the row's bytes equal `mem_usage` | {{acceptance:ps_json}} |
| library present but broken | `hmn ps` | exit `2`, an error line, not an empty table | {{acceptance:broken_ps}} |
| library present but broken | `hmn` | exit `2`, an error line, not `no visible GPUs.` | {{acceptance:broken_summary}} |
| no GPU of any vendor | `hmn` | `no visible GPUs.`, exit `0`, unchanged | {{acceptance:plain}} |
| two devices visible | `hmn ps` | `GPU` column carries ROCm's indexes, one per device | {{acceptance:two_device}} |

## Confidence

High that `hmn` 0.2.15 reads no AMD GPU and says so as absence: one node, one card, the outputs
copied from the job. High on the figure: `rocm-smi --showpids` and `amd-smi process --json` agree to
the byte, and the device-wide delta from `rocm-smi --showmeminfo vram` corroborates it to 80 KiB.
Lower on isolation and on anything beyond one GPU and one ROCm release (see the last observation).

## References

- [`ROADMAP.md`](../../ROADMAP.md): the deferred AMD item this report answers.
- [`dogfooding-windows-tcc-field-check.md`](dogfooding-windows-tcc-field-check.md): v0.2.15's
  broken-library rule (exit `2`, not absence) that request 1 extends to ROCm.
- [`dogfooding-driver-version-provenance.md`](dogfooding-driver-version-provenance.md): a field the
  backend's device facts sit beside.
- AMD, [ROCm SMI library](https://github.com/ROCm/rocm_smi_lib) (`librocm_smi64`) and
  [AMD SMI](https://github.com/ROCm/amdsmi) (`amd-smi`).
