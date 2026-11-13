# `hypomnesis` v0.2.16 — roadmap

> *Measure the AMD GPU the way `hmn` measures the NVIDIA one, and say where the two differ.*

**Status: in progress.** The scope was settled on 2026-11-09 (see *Decisions taken*); no version
bump until release.

---

## Why v0.2.16 (and not v0.3.0)

Every item is additive under the crate's own rule (`ROADMAP.md`, Principle 2: *"New variants and
fields land in patch releases. Type-shape changes … are minor bumps, never patches."*):

- one new Cargo feature, `rocm`, default-on and Linux-only, over the `libloading` dependency that
  `nvml` already pulls in;
- `GpuQuerySource::Rocm` and `HypomnesisError::Rocm`, new variants of two `#[non_exhaustive]`
  enums, the second for a ROCm SMI library that loads but cannot start;
- a Linux arm on the `NoGpuSource` text that names ROCm, and one more source in the dispatchers,
  tried after `NVML` and before `nvidia-smi`;
- no new public function and no new field: a ROCm device is a `GpuDeviceInfo` whose
  `reserved_bytes` is `None`, as for any source that lacks the figure.

Three behaviour changes are deliberate:

- a Linux host with an AMD GPU and the ROCm SMI library, which v0.2.15 read as no GPU (`hmn`
  printed `no visible GPUs.` and exited `0`), is measured: `hmn` names the device, `hmn ps` lists
  the processes with VRAM on it, and the library's `GpuQuerySource` on the per-process and listing
  results reads `Rocm` where it reads `Nvml` today;
- a ROCm SMI library that loads but cannot start is `HypomnesisError::Rocm`, not absence: `hmn`
  and `hmn ps` both exit `2` with the reason, the rule v0.2.15 set for `NVML`;
- on Linux the `NoGpuSource` text reads
  `no GPU measurement source available (NVML, ROCm, and nvidia-smi all failed or are disabled)`,
  where it read `(NVML and nvidia-smi both failed or are disabled)`.

Four candidates are **not** in this release. AMD GPUs on Windows (read through `DXGI` and `PDH`,
which are vendor-agnostic) and on macOS are neither changed nor claimed. An AMD iGPU is not
claimed: the backend is verified on a discrete MI300X, and `ROADMAP.md` keeps the row. A host
where the `amdgpu` driver is loaded and the ROCm userland is not stays a documented limitation, the
counterpart of an NVIDIA driver without `NVML`. Mixed NVIDIA and AMD nodes are documented as
unsupported. All stay in `ROADMAP.md` or the README's limitations.

---

## Origin — a dogfooding report from an 8×MI300X node

[The report](dogfooding-feedbacks/dogfooding-amd-gpu-reads-as-no-gpu.md) (2026-11-09, one MI300X
allocated from an 8×MI300X Linux node, ROCm 7.2.4, Slurm with device cgroups, `hmn` 0.2.15). A
process held a 4 GiB `hipMalloc`; `hmn` printed `hmn: no visible GPUs.` and exited `0`, `hmn ps`
printed an empty table and `0 GPU processes found (no GPU to list).`, while `rocm-smi --showpids`
and `amd-smi process --json` both read 4445999104 B for the process, within 80 KiB of the device's
own used-VRAM delta. The report asks for two things:

1. A ROCm backend through `librocm_smi64`, loaded at runtime like `NVML`, with ROCm's device index
   as the canonical one, and a library that is present but broken exiting `2` for `hmn` and
   `hmn ps`.
2. The parity with NVIDIA written down: what `hmn` reports on an AMD node, what it takes from the
   library, and that the device index is ROCm's, not the scheduler's.

The report resolves the deferred `ROCm` row of `ROADMAP.md`, which waited for hardware access or a
contributor PR with maintainer hardware coverage.

Reading the library on the same node settled the questions the report left open:

- `rsmi_compute_process_info_get` lists every KFD process on the node, other jobs' included, and
  returns `vram_usage` 0 for each; the bytes come from
  `rsmi_compute_process_info_by_device_get`, per device;
- for a process on two devices the per-device figures are 1224773632 B and 2298515456 B, and
  `rsmi_compute_process_info_by_pid_get` returns their sum, the figure `rocm-smi --showpids` prints;
- a job given two GPUs by the scheduler sees ROCm indices 0 and 1, in PCI order; a job given one
  sees index 0;
- `rsmi_init` returns `RSMI_STATUS_INIT_ERROR` (8) on a host with the ROCm userland and neither
  `/dev/kfd` nor the `amdgpu` module.

---

## Design decisions taken before starting

- **The source is the library, through `libloading`** (`src/gpu/rocm.rs`, feature `rocm`).
  `librocm_smi64.so.1` is found by its soname through the dynamic loader, as `NVML` is; no install
  path is coded, and nothing reads a scheduler variable, a cluster path or `ROCR_VISIBLE_DEVICES`.
  Its per-process bytes equal `rocm-smi`'s to the byte and sit within 1 MiB of the device delta
  (-77824, -94208 and -98304 B on the node above). Neither alternative was taken: KFD `sysfs` would
  expose other jobs' per-process files, which `NVML` does not, and `amd-smi process` filters to the
  caller. `rsmi_process_info_t` is pinned at 32 bytes (`process_id` at 0, `pasid` 4, `vram_usage`
  8, `sdma_usage` 16, `cu_occupancy` 24) by a layout test; 200 init, shutdown and `dlclose` cycles
  ran without a failure.
- **ROCm goes after `NVML` and before `nvidia-smi`, in every dispatcher.** A per-index call
  reaches ROCm whenever `NVML`'s call returned `None`, the fall-through chain `DXGI` already uses,
  with no new guard. The Windows-only `DXGI` and `PDH` arms are untouched, and the cfg lists that
  name Linux backends gain `rocm`.
- **The count rule: a positive count stands, `NVML`'s first; otherwise the first broken detail,
  `NVML`'s first; otherwise a zero; otherwise `NoGpuSource`.** In v0.2.15 `NvmlCount::Counted(0)`
  answered at once, so on a host with an NVIDIA userland and only AMD GPUs an `NVML` zero masked
  every AMD device. As in the `DXGI` rule (`dxgi_count_stands`), a zero never stands over a
  broken library. ROCm is asked only when `NVML`'s count is not positive. The rule is a pure
  function with its own unit tests. A broken ROCm reads as
  `Rocm(..)`, not `Nvml(..)`: the error carried out of the count is the backend's own.
- **Absent, no GPU and broken follow the `NVML` classifier.** A library that does not load is
  absence. `RSMI_STATUS_SUCCESS` counts. `RSMI_STATUS_INIT_ERROR` (8) means no GPU here, the
  `NoDriver` arm of `InitFailure`, so a CPU-only host with the ROCm userland installed still prints
  `no visible GPUs.` and exits `0`. Every other code, unknown ones included, is `CannotTell`, which
  is `HypomnesisError::Rocm` naming the call and the code from `rocm_smi.h`. The library describes
  `INIT_ERROR` as a failure in initialization, monitor discovery or set-up, so a broken stack on a
  host that has AMD GPUs can return it too and read as no GPU; no such host was observed. The
  mapping follows the line v0.2.15 drew: a CPU-only host must not exit `2`.
- **The index is the library's enumeration index over the devices the process can see, `0..n`.**
  Scheduler numbers are never read, as for `NVML`: scoping a job to its devices comes from the OS.
- **Processes are listed per device, like `NVML`.** The list call is host-wide and carries no bytes;
  membership and bytes are per device. A device's rows are the PIDs whose
  `rsmi_compute_process_gpus_get` includes it, each with its
  `rsmi_compute_process_info_by_device_get` figure, whoever owns the process. A device outside the
  process's view contributes no row.
- **No reserved bytes, no spill measurement.** `reserved_bytes` is `None`; the driver version is
  `RSMI_SW_COMP_DRIVER` through `rsmi_version_str_get`; the process name comes from `/proc`, the path
  `NVML` rows take. Spill is not measurable on Linux for either vendor, so `hmn watch` and
  `hmn spill` say so as they do for NVIDIA.
- **There is no fallback.** `nvidia-smi` is NVIDIA's, and the CLI of the AMD stack (`amd-smi`) is
  not a source, so a ROCm failure has one answer and the parity table says so beside NVIDIA's
  `nvidia-smi` row.
- **A fake ROCm SMI, not a GPU, for CI** (`tests/cli_fake_rocm.rs`, `tests/fixtures/rocm_smi_fake.c`).
  The same shape as the fake `NVML`: a C stand-in `librocm_smi64.so.1` configured by `FAKE_RSMI_*`
  environment variables, with `rsmi_process_info_t` pinned by `_Static_assert`, loaded through
  `LD_LIBRARY_PATH`, with no `nvidia-smi` on `PATH`. It covers a named subset of the fake `NVML`'s
  cases. Without a C compiler the tests skip locally and fail when `CI` is set, and they skip with a
  note when the other vendor's real stack answers on the host, since "absent" means only "did not
  load". The fake `NVML` tests skip the same way when a real ROCm stack answers.
- **Each platform's `NoGpuSource` names its own backends.** Windows and macOS keep their text;
  the Linux arm names ROCm; the residual arm keeps "NVML and nvidia-smi both". The arm lands with
  the wiring, so the text never names a backend the tree does not dispatch to.
- **The tested claim is narrow.** MI300X with ROCm 7.2.4 on an 8×MI300X node. Other AMD GPUs and
  other ROCm releases are expected to work through the same ABI and are not claimed.

---

## Scope

| # | Item | Kind | Status |
|---|---|---|---|
| 1 | `HypomnesisError::Rocm`, `GpuQuerySource::Rocm`, the backend error carried out of the device count | seams | ✅ `bb40e73` |
| 2 | The dogfooding report | docs | ✅ |
| 3 | `src/gpu/rocm.rs` and the `rocm` feature | **feature** | ⬜ |
| 4 | The backend in every dispatcher, the count rule, the `NoGpuSource` Linux arm, the `hmn` count-failure arm | **feature** | ⬜ |
| 5 | The fake ROCm SMI tests and the ignored live ROCm tests | test only | ⬜ |
| 6 | `hmn --help`, CLI strings and rustdoc name the AMD case | docs | ⬜ |
| 7 | README parity subsection, `CHANGELOG.md`, `CONVENTIONS.md`, `ROADMAP.md`, FAQ, brief, this roadmap | docs | ⬜ |

Every item updates the `CHANGELOG.md`, `--help`, README and FAQ text it makes stale, as in v0.2.14
and v0.2.15, and adds its tests to the fake ROCm SMI where a GPU would be needed. Item 7 covers what
remains.

---

## Parity with NVIDIA

The README holds the statement, in the subsection
[AMD GPUs on Linux: parity with NVIDIA](../README.md#amd-gpus-on-linux-parity-with-nvidia): the
capability table for NVIDIA and AMD on Linux, row by row, with an AMD value or an explicit `n/a`
for each, and the CLI table, command by command. Parity is stated per vendor and per platform.
It covers Linux, the only platform the backend runs on. AMD on Windows and macOS is not claimed,
and no text assumes one GPU per job or a scheduler that scopes devices.

---

## Verification

Each row is a predicate the acceptance run on an 8×MI300X node (ROCm 7.2.4, Slurm with device
cgroups) ticks, or, for a library-level row, the measurement that was taken there.

| What has to be true | How it is shown |
|---|---|
| The library's per-process bytes are the tool's | `rsmi_compute_process_info_by_device_get` equals `rocm-smi --showpids` to the byte for one device; for two devices the per-device figures sum to its figure (measured: 2298515456 B; 1224773632 B and 2298515456 B) |
| The bytes track the device | within 1 MiB of the device's used-VRAM delta (measured: -77824, -94208 and -98304 B) |
| The ABI is the header's | `rsmi_process_info_t` is 32 bytes at the offsets above, and `rsmi_status_t`, `rsmi_memory_type_t` and `rsmi_sw_component_t` are 4; 200 init, shutdown and `dlclose` cycles ran with no non-zero return (measured, in C) |
| A host with the ROCm userland and no AMD driver is not broken | `rsmi_init` returns `RSMI_STATUS_INIT_ERROR` (8) there (measured); `hmn` prints `no visible GPUs.` and exits `0` |
| A job granted no GPU on an AMD node | `hmn` prints `no visible GPUs.` and exits `0` |
| `hmn`, `hmn --json` | names the GPU, total, used and free, and the driver version; shows no reserved parenthetical; exit `0` |
| `hmn ps`, `--json`, `--pid`, `--device`, `--min`, `--filter`, `--sort` | one row per process with VRAM on each device, VRAM within 1 MiB of `rocm-smi --showpids` and of the device-wide delta, NAME from `/proc`, `GPU` column carrying ROCm's index; a process on two devices shows each device's own figure |
| `hmn watch` (PID, `--follow-new`, `--filter`, `--min`) | the same rows sampled over time; the spill line reads "not measurable" |
| `hmn spill -- <cmd>` | runs the command and prints the "not measurable on this platform" note |
| `hmn fits <size>` | exits `0`, `1` or `2` on the device's free VRAM |
| A library that loads and cannot start | `hmn` and `hmn ps` both exit `2` with the reason, `hmn` as `hmn: failed to count devices: <e>`, not an empty table and not `no visible GPUs.` |
| The count rule holds with both userlands | the fake libraries cover an `NVML` zero over ROCm devices; no host with both userlands was available |
| The fake ROCm SMI is sound | built under `-std=c11 -Wall -Wextra -Werror -pedantic` by gcc and clang; its symbols checked with `nm`; the tests build it with `-Werror` |
| The gate set passes on both CI toolchains | `cargo fmt --check`, both `--no-default-features` checks, clippy with default and all features, `cargo test --all-features`, and the doc matrix of v0.2.15 |
| The links hold | every relative link and anchor in the touched docs resolves |

Not claimed, because no run covered it: another user's process on a GPU the caller can see,
NVIDIA's per-device listing under device cgroups, other AMD GPUs, other ROCm releases, and an AMD
iGPU.

---

## Consistency pass

Run by a reviewer with no part in the work, who read the range from v0.2.15 to the tip against
the plan, the spec delta, the roadmap leaves and the acceptance evidence. The question was
consistency and whether each AMD claim in the shipped text is backed by the code or by an
acceptance row. Both Capabilities tables agree on the shared rows, AMD cells included; each of the
ten README Capabilities rows has a cell in the AMD subsection; the AMD samples in the README equal
the captured acceptance output; a broken library makes `hmn` and `hmn ps` exit `2`; the per-device
figures, the within-1-MiB claim and the per-process `NoGpuSource` cell match the measurements.

Adopted:

- The `GpuQuerySource::Rocm` entry of the CHANGELOG does not mention `--json`, whose output
  carries no `source`.
- The dogfooding report's smaller observations state what the two-MI300X acceptance run
  measured and what it did not.
- The README's AMD subsection states the measured per-process overhead, 144 MiB on an MI300X
  against about 518 MiB of `CUDA` context on an H200.
- Limitation 7 of the README carries its AMD counterpart, none known.
- The FAQ gives the Linux `NoGpuSource` text and names the missing `ROCm` library under
  `no visible GPUs.`
- The README's Feature Flags rows for `nvidia-smi-fallback` and `debug-output` name `ROCm` SMI,
  as `src/lib.rs` does.
- The driver-suffix sentence, its elision list, the brief's adapter-name line and the FAQ's
  device-order sentences name `ROCm` SMI where they speak of Linux in general.
- The count rule states the v0.2.15 behaviour it replaces, and this section and the
  decisions below are in final-state wording.

Not adopted:

- `ProcessGpuInfo::source` and `GpuProcessEntry::source` read `GpuQuerySource::Rocm` because
  `rocm::query` and `rocm::list_compute_processes` set it. No hardware test pins the value, since a
  test binary holds no compute context.

---

## Decisions taken

Decided on 2026-11-09, before the first commit:

- **The backend is built.** The deferred row's hardware condition is met by the node in the report.
- **The `NoGpuSource` text names ROCm on Linux**, as it names `DXGI` and `PDH` on Windows.
- **Both fake-library test files skip when the other vendor's real stack answers**, the existing
  idiom for a missing C compiler.
- **Parity is documented on one page, the README's AMD subsection**, not scattered; the two
  Capabilities tables gain their AMD cells on the shared rows.

Taken from the measurements:

- **The library, not `sysfs` or `amd-smi`**, for the reasons under *Design decisions*.
- **`RSMI_STATUS_INIT_ERROR` reads as no GPU**; the alternative, reading it as broken, would exit
  `2` on every CPU-only host with the ROCm userland and is the maintainer's call.
- **The version number is the maintainer's to confirm.** This roadmap and `ROADMAP.md` carry
  v0.2.16 until the maintainer says otherwise.

---

## At release

- `Cargo.toml` bumped to `0.2.16`; this roadmap's status flipped.
- The README's "what's new" banner rotated.
- `CHANGELOG.md`: `[Unreleased]` becomes `[0.2.16]` with its date and compare link.
- `ROADMAP.md`: current state, per-release index and revision note rotated.

---

## References

- [`dogfooding-feedbacks/dogfooding-amd-gpu-reads-as-no-gpu.md`](dogfooding-feedbacks/dogfooding-amd-gpu-reads-as-no-gpu.md)
  — the report this release answers.
- [`docs/roadmap-v0.2.15.md`](roadmap-v0.2.15.md) — the `NVML` classifier, the exit-`2` rule and
  the fake `NVML` this release mirrors.
- [`docs/roadmap-v0.2.14.md`](roadmap-v0.2.14.md) — the skipped-device line and `n/a` where spill
  cannot exist.
- AMD, [ROCm SMI library](https://github.com/ROCm/rocm_smi_lib) (`librocm_smi64`).
