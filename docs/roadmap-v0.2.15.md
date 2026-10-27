# `hypomnesis` v0.2.15 — roadmap

> *Say which GPU, say which device failed, and stop reading a missing name as an unreadable one.*

**Status: ✅ shipped 2026-10-27.** The maintainer settled the scope on 2026-10-26 (see *Decisions
taken*); it landed on `main` one commit per item, after the broken-`NVML` fix `43bd592`, and a
Windows `TCC` field check on a rented Tesla V100 confirmed the rule its audit added.

---

## Why v0.2.15 (and not v0.3.0)

Every item is additive under the crate's own rule (`ROADMAP.md`, Principle 2: *"New variants and
fields land in patch releases. Type-shape changes … are minor bumps, never patches."*):

- one new public function, `hypomnesis::process_name`, a sibling of `process_exists`;
- `HypomnesisError::Nvml`, which no code constructed until now, for an NVIDIA stack that loads but
  cannot start; `NoGpuSource` keeps its meaning, "no count source installed";
- a third platform arm on the `NoGpuSource` text, and one sentence in the `Snapshot::all` docs that
  states the order the crate already returns;
- wording and layout changes to human-readable output: a `GPU` column in the `hmn ps` table,
  `(no GPU to list)` on its summary line, stderr lines for the devices `hmn` skips, and notices
  from `hmn watch` that name the device holding the work.

Six behaviour changes are deliberate. The first five each turn a silent wrong answer into a stated
one, and the sixth follows from them:

- an NVIDIA driver that is installed but fails to start (`nvmlInit_v2` returning
  `NVML_ERROR_LIB_RM_VERSION_MISMATCH` after a driver upgrade without a reboot) is
  `HypomnesisError::Nvml`, not `NoGpuSource`: `device_count`, `Snapshot::all` and the per-device
  functions return it, and `hmn ps` exits `2` where v0.2.14 printed `0 GPU processes found.` and
  exited `0` (`1` under `--exit-status`);
- `hmn` exits `2` with nothing on stdout, `--json` included, when no device can be read or none
  can be counted, where v0.2.14 printed `no visible GPUs.` or `enumerated but none readable.` and
  exited `0`, and printed `[]` with `--json`;
- `hmn` states each device it skips, where v0.2.14 dropped it without a word and exited `0`;
- the DEVICE cell of `hmn ps` reads `?` when the model name is unknown, where it read `GPU N`:
  the index has its own column now;
- `hmn`'s other failures (a `RAM` read, say) exit `2` where they exited `1`: with every
  subcommand returning its own exit code, `main`'s `Ok`/`Err` fold and its generic `1` went;
- `hmn watch` names a watched PID that is alive but absent from the watched device's listing, where
  v0.2.14 printed `?`, which elsewhere means "the name could not be read".

Three candidates are **not** in this release. The unified `spill_condition` core changes library
semantics, so it gets its own release. Names for the PIDs a macOS sandbox refuses wait for a
consumer that gates on `--exit-status` inside a sandbox. Hardening the `nvidia-smi` fallback against
a GPU that is really lost waits for real output (see *Design decisions*). All three stay in
`ROADMAP.md`.

---

## Origin — a multi-GPU Linux field check, and what reviewing it found

[The field check](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/4#issuecomment-6319767457)
(the issue #4 field check, 2026-10-21/22, two multi-GPU Linux nodes: 8× V100 driver 580.173.02 and
2× H100 NVL driver 610.57.04 under Slurm), by contributor
[@LittleCoinCoin](https://github.com/LittleCoinCoin). Every check of
[mi-for-the-rust-of-us/hypomnesis#4](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/4)
passed except the lost-GPU check, which no real device triggered; a stand-in `libnvidia-ml.so.1`
simulated it. The report's notes, in its order:

1. `hmn watch` watches GPU 0 unless `--device` says otherwise. With `--filter` and the job on
   another GPU it exits `2`, and its line reads as though nothing were running, though device 0
   held another user's process.
2. `hmn watch` shows `?` in NAME for a live process whose name is readable, when that process is
   absent from the watched device's listing (another GPU, or PID 1).
3. `hmn ps` rows carry the model name but not the index, so on a node whose GPUs are all one model
   two rows for one PID on two GPUs read the same.
4. The out-of-range line and the skipped-device line share a prefix.
5. `hmn` drops a GPU that fails to answer without saying so; with every GPU failing it exits `0`.
6. Inside a GPU-isolated Slurm job, device indices are the job's own; in a job given no GPU, `hmn`
   says `no visible GPUs` while `hmn ps` says only `0 GPU processes found`.

[The review](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/4#issuecomment-6383417351)
corrected two of the report's conclusions, notes 4 and the "stronger form" of the processes-you-
don't-own check, and added two findings:

- **finding 5:** the `NoGpuSource` text names DXGI and PDH on Linux, which has neither;
- **finding 6:** an NVIDIA stack that loads but cannot start reads as "no GPU": `device_count`
  fails, `hmn ps` has no device to try and prints `0 GPU processes found`, and `hmn` says
  `no visible GPUs.` Fixed in `43bd592`, before this roadmap;
  [the follow-up](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/4#issuecomment-6384431641)
  says no field check is needed for it.

Note 4 needs no code: the `(skipped)` suffix tells a skipped device from a fatal `--device`
failure, and the text after the colon tells an out-of-range index from a failing device. The FAQ
says so (item 12).

---

## Design decisions taken before starting

- **`NoGpuSource` means "no count source installed"; a broken one is `Nvml`** (item 1, done in
  `43bd592`). `NvmlSession::start` (`src/gpu/nvml.rs`) tells a library that does not load from one
  that loads and cannot start; the detail names the call and the code from `nvml.h`:
  `nvmlInit_v2 returned NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)`. A machine without an NVIDIA
  driver behaves as before. `Snapshot::now` keeps mapping failures to `None`. The audit after the
  last item refined the line (see *Decisions taken*): two init codes mean no GPU, and on Windows a
  `DXGI` zero no longer hides a broken `NVML`.
- **Link a target that is always compiled by its crate path** (item 2). The link to
  `GpuQuerySource::Pdh` in `gpu_processes`'s docs (`src/gpu/mod.rs:392`) resolves only through a
  `use` that is compiled when a backend feature is on, so any `--no-default-features` doc build
  fails, on every OS. The enum is never gated, so a full path keeps the link:
  ``[`Pdh`](crate::GpuQuerySource::Pdh)``, under a label that does not resolve on its own, since
  ``[`GpuQuerySource::Pdh`](crate::GpuQuerySource::Pdh)`` trips rustdoc's
  `redundant_explicit_links` wherever the `use` is compiled. `CONVENTIONS.md` gains that rule.
  (This is the form as implemented; the plan named that second form, see *Decisions taken*.)
- **Cross-document from Ubuntu** (item 3). The doc job becomes a matrix over the three docs.rs
  targets × {`--all-features`, `--no-default-features`}, all on `ubuntu-latest`, as docs.rs builds
  them; no macOS runner minutes.
- **A fake NVML, not a GPU, for the multi-GPU paths** (item 4). This machine has one GPU, and
  WSL2's NVML lists no processes, so the issue #4 paths cannot run here for real. A C stand-in
  `libnvidia-ml.so.1`, configured by environment variables (devices, lost devices, process rows,
  init and count codes), exports the nine symbols the crate resolves, with structs pinned by
  `_Static_assert` to `nvml.rs`'s `repr(C)` layouts. Loaded through `LD_LIBRARY_PATH`, with no
  `nvidia-smi` on `PATH`, it touches no GPU, so the tests run on every Linux host and on CI. It
  replaces `43bd592`'s single-purpose `nvml_broken.c` (see *Decisions taken* for its history).
  Without a C compiler the tests skip locally and fail when `CI` is set.
- **Each platform's `NoGpuSource` names its own backends** (item 5). Windows keeps
  `NVML, DXGI, PDH, and nvidia-smi` byte for byte and macOS keeps its v0.2.14 list; elsewhere the
  text reads `no GPU measurement source available (NVML and nvidia-smi both failed or are
  disabled)`.
- **A `GPU` column before DEVICE** (item 6). `hmn ps` prints
  `PID NAME VRAM SHARED GPU DEVICE SPILL`; an unknown model name reads `?` in DEVICE, the glyph
  `hmn` uses for anything it cannot tell, since `GPU N` there would repeat the new column. JSON
  already carries `device_index` and is unchanged.
- **`hmn ps` says when there is no GPU to list** (item 7). With no `--device` and no device to try
  (`device_count` counted zero, or found no count source), the summary reads
  `0 GPU processes found (no GPU to list).` The words avoid "no visible GPUs", which `hmn` would
  contradict on a Windows PC whose only GPU is an iGPU listed through DXGI. Exit codes stay: `0`,
  and `1` under `--exit-status`, since such a job is not on a GPU.
- **`hmn` states skipped devices and exits `2` when it can tell nothing** (item 8).
  `Snapshot::all` keeps no error for a device whose `device_info` failed, so `run_summary`
  (`src/bin/hmn/summary.rs`) asks `device_info` again for each such device, which costs one call
  only where a device already failed, and prints `hmn: failed to query device N: <err> (skipped)`.
  The NVIDIA devices come first in `Snapshot::all`, in index order, which its docs now state. With
  no device readable, `hmn: N GPU(s) enumerated but none readable.` moves to stderr and `hmn` exits
  `2` with nothing on stdout, `--json` included; an NVIDIA stack that cannot start prints
  `hmn: failed to count devices: NVML error: …` and exits `2`, where `43bd592` exited `1`. Every
  subcommand now returns its own exit code, so `main`'s Ok/Err fold goes.
- **A direct name lookup for watched PIDs the listing lacks** (item 9). `process_name(pid)` uses
  the resolvers the listings use, so a name is spelled as in `gpu_processes`: `/proc/<pid>/comm`
  extended past its 15-byte cut on Linux (behind the same thread-ID guard as `process_exists`),
  `OpenProcess` then the `Toolhelp32` snapshot on Windows, `proc_pidpath` then `p_comm` on macOS.
  `hmn watch` looks up every watched PID missing from an interval's listing; the looked-up name
  fills the NAME cell and the closing summary, never `last_name` or the PID-reuse reset, so a
  spelling that differs (the `nvidia-smi` fallback's) cannot fake a reuse. A PID that names no
  process keeps `?` and its attach warning.
- **`hmn watch` stays single-device and says where the work is** (item 10). When nothing is
  selected, or an explicit PID is missing from the watched device, it lists the other devices once,
  at attach, and applies the same `Selection::select`:
  - with processes on the watched device but none matching:
    `hmn: watch found 1 GPU process on device 0, none to auto-select (top 5 among names containing "fieldcheck" (case-insensitive)); device 5 has a match (pid 2860458): re-run with --device 5`
    (exit `2`); with none at all, the v0.2.14 opening is unchanged, and so is the whole line
    unless another device has a match, whose hint replaces the re-run advice;
  - under `--follow-new`, the same opening ending `none to follow yet (…); waiting for work to
    appear`, and the hint on a line of its own;
  - an explicit PID found elsewhere:
    `hmn watch: pid=1415320 holds 10.4 GiB on device 1, not device 0; its rows will read 0 MiB — re-run with --device 1`,
    never also reported as naming no running process.
- **`--follow-new` reads the refused set once: say so** (item 11). A sentence in `--help`, the
  README and the tutorial; a per-interval count stays in `ROADMAP.md`.
- **The `nvidia-smi` fallback stays as it is.** `run_smi` takes exit `0` as success whatever it
  printed (`src/gpu/nvidia_smi.rs:58`), so a really lost GPU for which `nvidia-smi` exits `0` with
  no parsable row would list as empty rather than skipped. Hardening it without its real output
  could trip on normal output nobody here can produce; the lost-GPU check of issue #4 stays open
  for that output, and `ROADMAP.md` gains the item.

---

## Scope

| # | Item | Kind | Status |
|---|---|---|---|
| 1 | `device_count` tells a broken NVIDIA stack from no GPU (finding 6) | **fix** | ✅ `43bd592` |
| 2 | The `GpuQuerySource::Pdh` link resolves under every feature set | docs | ✅ |
| 3 | CI doc legs: three targets × all / no default features | build | ✅ |
| 4 | A fake NVML stand-in for Linux CI | test only | ✅ |
| 5 | `NoGpuSource` names only the backends of the platform (finding 5) | fix | ✅ |
| 6 | `hmn ps` GPU column (note 3) | feature | ✅ |
| 7 | `hmn ps` says there is no GPU to list (note 6) | feature | ✅ |
| 8 | `hmn` states skipped devices, exits `2` when it can tell nothing (note 5) | **fix** | ✅ |
| 9 | `process_name`; `hmn watch` names PIDs the listing lacks (note 2) | **fix** | ✅ |
| 10 | `hmn watch` says which device holds the work (note 1) | feature | ✅ |
| 11 | `--follow-new` reads the refused set once, at attach: say so | docs | ✅ |
| 12 | README, FAQ, tutorials, `CHANGELOG.md`, `ROADMAP.md` | docs | ✅ |

Every item updates the `CHANGELOG.md`, `--help`, README and FAQ text it makes stale, as in v0.2.13
and v0.2.14, and adds its tests to the fake NVML where a GPU would be needed. Item 12 covers what
remains.

---

## Verification

The tree at `ffd3ba8`, on 2026-10-26, on the reference machine (Windows 11, RTX 5060 Ti) and in
its Ubuntu WSL2, whose NVML lists no processes, so the multi-GPU paths ran against the fake
NVML (item 4) and the single-GPU ones live on Windows. The gate set, the doc matrix and the
discrimination check were run again on the tree after the consistency pass (`e5c7a3e`), all
passing, with 15 fake-NVML tests.

| What had to be true | How it was shown |
|---|---|
| The gate set passes on both CI toolchains and both host OSes | `cargo fmt --check`, both `--no-default-features` checks, clippy with default and all features, `cargo test --all-features`, on Windows (1.99.1, 1.88) and WSL (1.99.0, 1.88); WSL's `--no-default-features` library tests too |
| The docs build everywhere CI and docs.rs build them | `RUSTDOCFLAGS=-D warnings cargo doc` for linux-gnu, windows-msvc and aarch64-apple-darwin, each with all features and none (from Windows and, as CI runs it, cross-documented from WSL), and `--document-private-items`; macOS clippy via `--target aarch64-apple-darwin` |
| The fake NVML is sound | built under `-std=c11 -Wall -Wextra -Werror -pedantic` by gcc and clang; its nine symbols checked with `nm`; the tests build it with `-Werror` |
| The new tests discriminate | the same fake scenarios run against v0.2.14's `hmn` reproduce the field check: two identical rows for one process on two H100s, a lost GPU dropped silently with exit `0`, `?` for PID 1 in `hmn watch`; and the audit dropped the new test file into an export of v0.2.14, where 13 of its then 14 tests failed, only the control passing |
| `process_name` refuses a thread | the thread reads its ID from `/proc/thread-self`; 400 runs, none failing (a first version found the thread by its name and raced the kernel's naming) |
| The single-GPU paths are unchanged live | `hmn` and `hmn --json` on the RTX 5060 Ti as before; `hmn ps` re-captured with the GPU column for the README; `hmn watch` names a SYSTEM `svchost.exe` holding no GPU memory, `?` before, non-elevated; an empty `--filter` counts the device's 24 processes and exits `2` |
| The links hold | every relative link and anchor in the touched docs resolves (a GitHub-style slug check) |

Each item's commit ran its own subset of these before it landed.

---

## Consistency pass

After the last item, three fresh agents that had not seen the work audited `020d6c0..296cb7d`,
which includes `43bd592`: one against `CONVENTIONS.md`, one adversarially for correctness and test
quality, one for drift between the docs and the code. They reported findings only; none was a
blocker. Adopted, in three commits:

- **Two correctness gaps on the "no GPU" / "can't tell" line**, fixed in `e14473b` and recorded
  under *Decisions taken*: `nvmlInit_v2` codes 9 and 28 read as no GPU, and on Windows a `DXGI`
  zero no longer hides a broken `NVML`.
- **Code, tests and CI** (`71ff411`):
  - neither watch test depends on what PID 1 is (a container's `sleep infinity`, or a long
    entrypoint name, broke them);
  - the GPU-column JSON check matches one object per GPU;
  - the fake is built with `-Werror`, counts a device's rows only up to what it holds (its `free`
    wrapped to about 16 EiB), and documents its limits;
  - the `process_name` smoke test requires a name where a lookup source exists;
  - a test pins the empty-device watch line with a hint;
  - each doc leg keeps its own `rust-cache` key;
  - a `BORROW` annotation, two test doc comments and a doc wrap.
- **Docs** (`e5c7a3e`):
  - the empty-device watch line is unchanged only without a hint (CHANGELOG, this roadmap, the
    code doc);
  - `hmn`'s other failures exit `2`, where they exited `1` (a sixth behaviour change);
  - the FAQ's exit-`1` sentence, log-scan advice, Slurm provenance and watch `?` paragraph;
  - the tutorial's process count and its NAME and PID-reuse caveats;
  - the README's "by its bytes";
  - the `--json` doc and the CHANGELOG's details and item numbers;
  - the `lib.rs` capability table;
  - and the broken `# Errors` anchor in `CONVENTIONS.md`.

Left as they are:

- `hmn: N GPU(s) enumerated but none readable.` keeps its trailing period, byte for byte, like
  `hmn: no visible GPUs.`: it reads as a statement, not a reason line.
- The other-device probe runs only when the watched device lists. A lost watched device exits `2`
  with its reason and no pointer elsewhere, and a PID the watched device holds is not looked for
  on others. Both are by design: the probe answers "the work is not here".
- Two commit bodies (`ffd3ba8` without a "Key changes:" heading, `6e0760f`'s "contributed by
  contributor") stay as committed rather than rewriting history.
- The fake cannot drive the retry past 64 process rows, and no test covers a device that answers
  only when `hmn` asks a second time; the fake is stateless.
- `hf-fetch-model`'s `friendly_error` will print `NVML backend reported: NVML error: …`, naming
  `NVML` twice; that is for its own next release.

---

## Decisions taken

Raised with the maintainer on 2026-10-26, before the first commit:

- **Scope:** issue #4 notes 1, 2, 3, 5 and 6, finding 5, the doc link with its CI legs, and the
  `--follow-new` sentence. Deferred: unified `spill_condition` (its own release), names for denied
  PIDs (macOS, no consumer yet), `nvidia-smi` hardening (waits for real output).
- **A separate `GPU` column**, not the index inside DEVICE.
- **`hmn` exits `2` when it can tell nothing**, as `hmn ps` does, so `43bd592`'s exit `1` became
  `2` before any release.
- **`hmn watch` stays single-device** and says where the work is, rather than switching device
  on its own; under `--follow-new` too, with the hint on a line of its own.
- **Every missing watched PID is looked up**, not only one never named: a process that was listed
  and went idle shows its name too.
- **`(no GPU to list)`**, not `(no visible GPUs)`, which `hmn` would contradict on an iGPU-only
  Windows PC.
- **A fake NVML for Linux CI**, so the issue #4 paths are tested on every push.

Taken while implementing:

- **The doc link names its full path under a label that does not resolve on its own.** The plain
  path link the plan proposed trips `rustdoc::redundant_explicit_links` wherever the `use` is
  compiled; `CONVENTIONS.md` records the rule.
- **The fake replaces `nvml_broken.c` rather than sitting beside it.** The file is renamed, but
  the rewrite leaves too little in common for git to follow the rename: history is under the old
  name.

Raised by the audit after the last item, decided by the maintainer on 2026-10-26: the line between
"no GPU" and "can't tell" is whether GPU processes could exist that no backend can show.

- **Two `nvmlInit_v2` codes mean no GPU.** `NVML_ERROR_DRIVER_NOT_LOADED` (9) reads as no usable
  `NVML`, like a library that does not load, and `NVML_ERROR_GPU_NOT_FOUND` (28) as a count of
  zero: no process can be on a GPU there. Every other code, unknown ones included, stays "can't
  tell". Before this, a CPU cloud instance on a GPU image, or a node whose module Secure Boot
  refused, made `hmn` exit `2`. The reason a driver is not loaded is lost from `hmn`'s output
  (`NoGpuSource` is a unit variant); `debug-output` builds still print it.
- **On Windows, a `DXGI` count of zero no longer stands in for a broken `NVML`.** `DXGI` sees only
  `WDDM` adapters, so a datacenter GPU in `TCC` mode is invisible to it, and a broken `NVML` over
  it read as "no GPU" while its jobs ran. A positive `DXGI` count still answers. A machine with one
  `WDDM` and one `TCC` GPU, where `DXGI` counts one, is left as it was.

Decided on 2026-10-27, after the consistency pass:

- **The fake NVML runs on Windows too, so CI tests the `TCC` rule end to end.** Built as `nvml.dll`
  with `clang` (it finds the MSVC headers without a developer shell) and placed beside a copy of
  `hmn.exe`, since Windows looks for a DLL in the executable's folder first. On a host without an
  NVIDIA driver, the windows CI runner included, `DXGI` counts zero NVIDIA adapters, which is how
  it sees a `TCC` GPU: the broken-`NVML` tests then pin the rule above, and a fake device is a
  `TCC` GPU as `hmn` sees it. A host with a real driver skips them, because `DXGI` sees its
  adapters and `nvidia-smi.exe` in `System32` is reachable whatever `PATH` says. The tests that
  need `NVML`'s process listing, read on Linux only, stay Linux-only. Checked locally with a build
  that has no `DXGI`, `PDH` or `nvidia-smi` (so no GPU is touched): the copy loads the fake, not
  the driver's `nvml.dll`, and every scenario prints what the tests expect; the runner's own
  `DXGI` zero is first exercised by CI. What a real `TCC` GPU would show beyond that is a
  `ROADMAP.md` item for a rented field check.
- **That field check ran the same day**, on an OVHcloud Tesla V100 in `TCC` mode
  ([report](dogfooding-feedbacks/dogfooding-windows-tcc-field-check.md), kit in
  `tools/tcc-field-check`). The rule holds over the real `DXGI` zero: the fake `NVML` at code 18
  exits `2`, codes 9 and 28 read as no GPU. A card disabled in Device Manager reads as no GPU too
  (`NVML` says its driver is not loaded), so the `dxgi_count_stands` comment, which said it keeps
  `NVML`'s error, was corrected (`d6f5e29`). Its two findings, full-path names on the
  `nvidia-smi` fallback and spill reading `?` where it cannot exist, change output and wait for
  v0.2.16 in `ROADMAP.md`.

---

## At release

- `Cargo.toml` bumped to `0.2.15`; this roadmap's status flipped.
- The README's "what's new" banner rotated: 🆕 `0.2.15`, `0.2.14` to 🚀, `0.2.12` dropped.
- `CHANGELOG.md`: `[Unreleased]` becomes `[0.2.15]` with its date and compare link.
- `ROADMAP.md`: current state, per-release index and revision note rotated.

---

## References

- [Issue #4](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/4) — the field check
  this release answers, the review and its follow-up.
- [`docs/roadmap-v0.2.14.md`](roadmap-v0.2.14.md) — the skipped-device line and exit `2` that
  `hmn` now follows.
- [`docs/roadmap-v0.2.13.md`](roadmap-v0.2.13.md) — `process_exists`, whose shape
  `process_name` follows, and `--exit-status`.
