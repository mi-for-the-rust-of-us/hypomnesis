# `hypomnesis` v0.2.14 — roadmap

> *Measure what a sandbox allows, say what it forbids, and stop reporting an unreadable list as
> an empty one.*

**Status: accepted — shipping as three PRs.** The maintainer approved implementation on
2026-10-02 ([issue #3 comment](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/3#issuecomment-5947395182));
PR A, this documentation, goes first (see *PR split*), and there is no version bump until release.

---

## Why v0.2.14 (and not v0.3.0)

Every item is additive under the crate's own rule (`ROADMAP.md`, Principle 2: *"New variants and
fields land in patch releases. Type-shape changes … are minor bumps, never patches."*):

- one new public function, `hypomnesis::gpu_process_listing`, and its `#[non_exhaustive]`
  result struct; `gpu_processes` keeps its signature and becomes a thin wrapper;
- one new `HypomnesisError` variant on the `#[non_exhaustive]` enum, as `Pdh` once was;
- a Metal arm in the private `bounds_check`, and macOS-only `sysctl` fallbacks inside the private
  Metal backend;
- wording changes to human-readable output: `n/a` instead of `?` in the SPILL and `PAGED` cells
  where spill cannot exist, an `unreadable` part on the `hmn ps` summary line, and a
  platform-correct remedy in place of "re-run elevated" on macOS.

Five behaviour changes are deliberate. Each turns a silent wrong answer into a stated one:

- `gpu_processes` returns an error, not an empty list, when the process list was enumerated but
  no process other than the caller's could be read;
- `hmn ps` exits `2` when every device it tried failed, where it now prints an empty table and
  exits `0`;
- `hmn ps --exit-status` exits `2` ("can't tell") rather than `1` ("nothing matched") when nothing
  is listed and some processes could not be read;
- `hmn ps --exit-status` also exits `2`, not `1`, when nothing is listed and a
  tried device failed;
- `gpu_processes` returns `NoGpuSource`, not an empty list, when the `graphics_footprint`
  template index does not resolve, so `hmn ps` exits `2` on such a host, unsandboxed included,
  where v0.2.13 prints `0 GPU processes found.` and exits `0`.

One request is **not** in this release: making the JSON `spilled` field `null` when spill is not
measurable. It changes a `bool` into a `bool` or `null` on the wire, a type-shape change, so it
waits for v0.3.0 (see *Design decisions*).

---

## Origin — a field check on Apple Silicon, and what checking it found

[`docs/dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md`](dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md)
(the issue #3 field check, 2026-10-01, Apple M3 Pro, macOS 26.6.2). All eight checks of
[mi-for-the-rust-of-us/hypomnesis#3](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/3)
passed as worded. The premise of check 3 did not. The crate says that macOS cross-user PIDs are
skipped because `ledger` returns `EPERM`, and that `sudo hmn ps` lists them. Measured, ownership
plays no part:

- **Unsandboxed:** other users' processes, root's included, read fine (0 `EPERM` over 920 PIDs).
- **Sandboxed:** under a sandbox that denies `process-info*`, even the caller's own Safari gets
  `EPERM`, and `hmn ps` prints `0 GPU processes found.` with exit `0`.

Requests, in the report's order:

1. make an unreadable list loud;
2. restate the macOS limitation;
3. give `bounds_check` a Metal arm;
4. `n/a` rather than `?` where spill does not exist, and a `null` `spilled`;
5. PID 0 on macOS.

Checking the report against the code and the kernel confirmed it, and went further in five places.

- **The cause is in the kernel source.** XNU's `ledger()` (`bsd/kern/sys_generic.c`) returns
  `ESRCH` from `proc_find`, then consults only `mac_proc_check_ledger`, the sandbox's hook. There
  is no uid check. The crate's own first macOS probe (May 2026) had already read WindowServer's ledger unprivileged. "Cross-user needs
  root" came from `task_for_pid` and was never measured for `ledger`.
- **The report's mechanism for the silent zero is wrong for its own profile.** Under
  `(deny process-info*)(allow process-info* (target self))` it is **`proc_listpids`** that fails,
  not the per-PID reads. `list_compute_processes` returns `None`, so `gpu_processes(0)` returns
  `NoGpuSource`, whose text names four backends macOS doesn't have. `hmn ps --device 0` then
  exits `2`. Plain `hmn ps` exits `0` with an empty table, because `run_ps` drops a failing device
  without a word (`src/bin/hmn/ps.rs:403`), **on every platform**. The per-PID silent skip the
  report describes is real, but only under a profile that denies `process-info-ledger` alone.
- **A real App Sandbox behaves the same.** Built with an embedded `Info.plist` and ad-hoc signed
  with `com.apple.security.app-sandbox`, `hmn ps` printed `0 found` (exit `0`), `--device 0`
  exited `2` and `hmn watch` exited `2` with the `NoGpuSource` text.
- **The doc claim sits at about sixteen sites, not seven.** The README capability table even says
  "libSystem syscalls always succeed", and `ROADMAP.md` Principle 4 cites "macOS cross-user
  `EPERM`".
- **The sandbox still lets through more than `hmn` uses.** `sysctl(KERN_PROC…)`, the call `ps(1)`
  is built on, enumerates every process, names it and tells a live PID from a dead one under the
  report's profile. `ledger` reads of the caller's own sandbox (an agent's training job) succeed.
  `hmn` measures neither today, because it stops at the refused `proc_listpids`.

**When it bites.** Every earlier macOS test and benchmark ran in an unsandboxed shell, where nothing
is refused. A `(deny default)` profile does not refuse `process-info` either. Under OpenAI Codex's
Seatbelt policy, which allows `process-info*` only for `same-sandbox` targets, `hmn ps` lists all
20 processes correctly; Chromium's `common.sb` keeps a TODO to deny it explicitly. Claude Code's
own Bash sandbox (2.1.273, measured 2026-10-03) refuses none of the calls `hmn` makes, and `hmn`
0.2.13 lists the GPU processes there with their bytes; `ps(1)`, a setuid binary, could not run
(evidence: `field_check_v0213/evidence/claude_code_sandbox.md` at `f03298a7bb`). It takes an
explicit `(deny process-info…)`, or the App Sandbox:

| Caller | `proc_listpids` | others' `ledger` | `proc_pidpath` | `sysctl kern.proc` | `hmn` 0.2.13 | `hmn` 0.2.14 |
|---|---|---|---|---|---|---|
| unsandboxed | ok | ok | ok | ok | correct | unchanged: lists every user's processes |
| Codex Seatbelt policy | ok | ok | ok | `kern.proc.all` denied, `kern.proc.pid` ok | correct | unchanged: lists the processes it can read |
| App Sandbox | `EPERM` | `EPERM` (self ok) | ok | ok (823 processes, `kernel_task` named) | `0 found`, exit `0` | blind: exit `2`, `process list unreadable (N refused, none other than the caller's could be read) — re-run outside the sandbox` |
| explicit `deny process-info*` (the report's profile; agent sandboxes that deny it to stop argv leaks) | `EPERM` | `EPERM` | `EPERM` | ok (969 processes) | `0 found`, exit `0` | blind: exit `2` with the same denial line and the count |
| the same, with `same-sandbox` allowed | `EPERM` | ok for the sandbox's own jobs | ok for them | ok | `0 found`, exit `0`, though the job is readable | partial: the job with its 256 MiB, plus `N unreadable — re-run outside the sandbox` |
| `process-info-pidinfo` denied outside the sandbox (`agent-safehouse` v0.12) | ok | ok | `EPERM` | ok | right numbers, names `?`, "re-run elevated" | listed: every row named, from `p_comm` where `proc_pidpath` is refused, with no `?` and no remedy |
| Claude Code's Bash sandbox (macOS Seatbelt, `/sandbox`) | ok | ok | ok | ok (1108 processes, `kernel_task` named) | correct | unchanged: lists every GPU process (28), exit `0` |

With PR B, the Claude Code sandbox still refuses nothing `hmn` needs: measured 2026-10-04 in one sandboxed session (Claude Code 2.1.273), the PR B build lists the 18 GPU processes normally and exits `0`, as the 0.2.13 build does (evidence: `v0214_part1/claude_code_sandbox/pr_b.md` at `f03298a7bb`). With PR C, measured 2026-10-08 in one sandboxed session of the same version, it lists all 28 GPU processes and reports nothing unreadable.

**Why Windows never showed it.** PDH on Windows, and NVML on Linux, return every process's VRAM
from one system-wide query, with no permission check per process. Only names can be refused there,
and v0.2.8's `Toolhelp32` fallback and `[protected]` bracket handle that. macOS is the one platform
that reads each PID separately, so it is the one where some rows can go missing.

---

## Design decisions taken before starting

- **Measure everything the sandbox permits, with one lookup rule.** On macOS every process
  lookup asks libproc first, and asks `sysctl kern.proc` only when libproc refuses or answers
  "no". The rule covers:
  - enumeration: `proc_listpids`, then `KERN_PROC_ALL`;
  - names: `proc_pidpath`, then `p_comm`;
  - existence: `proc_pidpath`, then `KERN_PROC_PID`.

  Neither source alone covers every caller: Codex's policy refuses `kern.proc.all` and allows
  libproc, while an explicit deny does the reverse. Because libproc answers first, every case that
  works today, unsandboxed and under Codex, keeps byte-identical output, except on a host whose
  ledger template lacks the `graphics_footprint` entry, which the fifth deliberate change in
  *Why v0.2.14* covers.

  One private helper reads `kinfo_proc` records as `[u8; 648]` with named offsets. It needs no
  `libc` dependency, checks that the length is a whole number of records, and takes `p_comm` from
  the record. The enumeration fallback keeps only each record's PID; a name is read per row, from
  `proc_pidpath` and, where that is refused, from the `p_comm` of the PID's own `KERN_PROC_PID`
  record. Record parsing and errno
  classification are pure functions with unit tests, the way `proc_name.rs` tests its own; the
  sandbox paths cannot be unit-tested any other way.
- **The 648-byte `kinfo_proc` layout, and where it was checked.** Measured 2026-10-02 on the M3
  Pro (macOS 26.6.2, SDK 26.2) and re-run 2026-10-03; the programs and their verbatim output are
  in `field_check_v0213/evidence/kinfo_proc_layout.md` at `f03298a7bb`.
  - arm64, natively: `sizeof(struct kinfo_proc)` is 648, `p_pid` sits at offset 40 and `p_comm`
    at offset 243 (17 bytes with the NUL). A live `KERN_PROC_PID` read of PID 1 returns one
    648-byte record named `launchd`, and `KERN_PROC_ALL` returns a whole number of records.
  - x86_64, against the SDK header: the same `sizeof`/`offsetof` program compiled with
    `clang -arch x86_64` prints 648, 40 and 243. The same live reads, run as an x86_64 process
    under Rosetta 2, give the same lengths.
  - x86_64, by test: the x86_64 unit-test run passed on 2026-10-04, under Rosetta 2:
    `cargo test --target x86_64-apple-darwin --lib`, with `--locked --all-features`, ran 94
    library tests, the parser's unit tests and one live read among them, and all 94 passed.
  - Not verified: a native Intel Mac.
    Rosetta 2 runs x86_64 userland on the arm64 kernel, so it cannot show what an
    Intel kernel returns, and `ROADMAP.md` lists Apple Metal on Intel Macs as untested hardware
    (Principle 3, no Intel-Mac test hardware). This release changes neither, so three guards stand
    against a layout that differs: the whole-records length check and the PID cross-check, as
    before, and a check of the caller's own `KERN_PROC_PID` record before a `KERN_PROC_ALL`
    listing is trusted. A record larger than 648 bytes does not fit `KERN_PROC_PID`'s
    one-record buffer, so `sysctl` fails with `ENOMEM`, which reads as "can't tell". A
    probe-sized `KERN_PROC_ALL` buffer takes whole records of any size, so the whole-records
    check alone would pass such a listing whenever its total is a multiple of 648.
- **A per-PID read has five outcomes, not two.** `read_graphics_footprint` stops folding
  everything into `None`. It returns bytes; *denied* (`EPERM`); *gone* (`ESRCH`); *failed*, for
  any other errno or a reply it cannot use; or *unavailable*, when the `graphics_footprint`
  template index did not resolve. *Unavailable*, both enumerations refused, or a listing where no
  other process was read, none was refused and at least one failed, makes the backend return
  `None`. The dispatcher then falls through to `NoGpuSource`, as for every other backend, instead
  of today's silent empty list.
- **Names follow the Linux rule literally.** The `p_comm` fallback is used only on `EPERM`, never
  on `ESRCH`, so a PID's name cannot flip between sources and trigger `hmn watch`'s PID-reuse reset
  (`src/bin/hmn/watch.rs:316-327`). A `p_comm` shorter than 16 bytes is exact; one of 16 may be cut
  and is returned as is. That is what `proc_name.rs` does with Linux's 15-byte `comm` when nothing
  extends it. The `GpuProcessEntry::name` docs, the FAQ and the `--filter` help each say so in one
  sentence: a name `--filter` cannot match past the cut.
- **`process_exists` follows the same rule**, which also fixes PID 0:
  - `proc_pidpath` gives a path → `Some(true)`;
  - otherwise `KERN_PROC_PID` decides: a record → `Some(true)`, no record → `Some(false)`;
  - if that call is refused too: `ESRCH` → `Some(false)`, anything else → `None`.

  kernel_task has no executable path, so `proc_pidpath(0)` says `ESRCH`, but `KERN_PROC_PID`
  finds it, with no special case. A sandboxed caller also gets an answer where it used to get
  `None`.
- **State what is forbidden, by count, with one dispatcher.**
  - `gpu_process_listing(device_index) -> Result<GpuProcessListing>` is the one dispatcher; it
    carries `entries` and `denied_pids: Vec<u32>`. `gpu_processes` maps it to its entries.
  - `denied_pids` is a list, not a count, so `hmn ps --pid` and `hmn watch` can ask about a given
    PID. It is always empty on Linux and Windows, as a per-platform doc table says.
  - `HypomnesisError::ProcessListDenied { denied: u32 }` is returned only when the list was
    enumerated but no process other than the caller's could be read, counting zero-balance reads
    as read. Its `Display` describes what happened, like its siblings.
  - The remedy lives in one CLI constant, not in `Display`, so there are never two phrasings to
    drift apart.
  - No `test-helpers` builder is added: nothing outside the crate builds a listing, and the
    formatters take slices.
- **The remedy copies the Windows one.** Since v0.2.2 the `hmn ps` summary line has ended with
  `(N protected — re-run elevated for names)`. Since v0.2.6 `hmn watch` has said
  `re-run elevated to identify`. macOS gets the same shape, `N unreadable — re-run outside the
  sandbox`, in the same parenthetical and positions.

  | | Windows | macOS |
  |---|---|---|
  | What is withheld | names only; "measurement itself never needs elevation" (FAQ) | the bytes of processes outside the caller's sandbox |
  | Remedy | re-run elevated | re-run outside the sandbox: an agent harness's escalation or unsandboxed mode; there is none inside an App Sandbox app |

  The remedy is one `cfg!`-selected constant in `format.rs`. The summary, the skipped-device line,
  `watch`'s growth hint and its notices all use it. On macOS, when both counts are non-zero, it is
  said once. Windows and Linux output stays byte-identical. No `sudo` advice remains for macOS:
  unsandboxed it is never needed, and inside a sandbox it is not known to help.
- **`hmn ps` states a skipped device, and fails when every device failed.** Skipping a failing
  device without `--device` stays, as v0.2.13 decided, so one broken device does not hide the
  others. It now gets a stderr line. When every device tried has failed, `hmn ps` exits `2`. When
  no device was tried at all (`device_count` failing on a GPU-less runner), it keeps today's empty
  table and exit `0`. With `--exit-status`, nothing listed and a
  tried device failed is also `2` (see *Decisions taken*).
- **`hmn ps` and `hmn watch` report a partial denial.**
  - `ps`: `SummaryNotes` gains `unreadable`. `--pid` applies to denied PIDs the way `judge`
    applies it to rows (`ps.rs:260`), so `hmn ps --pid N` does not report hundreds of unrelated
    unreadable processes. `--exit-status` exits `2` when nothing is listed and a relevant PID was
    denied.
  - `watch`: one notice per denied explicit PID ("unreadable here; its rows will read 0 MiB"),
    and denied PIDs are kept out of `missing_pid_notices`. The "found no GPU processes" message
    (`watch.rs:1119-1124`) says why when processes were denied. A sandbox does not change
    mid-run, so notices come once at attach; `--follow-new` gets a count; the per-interval error
    path does not repeat the remedy.
  - JSON stdout keeps its shape; the notices go to stderr, as v0.2.10 did for the no-subcommand
    path.
- **`n/a` where spill cannot exist, `?` where it can't be read now.**
  - `n/a` on Linux and macOS, decided at compile time: the README capability table already writes
    `n/a` there.
  - `?` stays on Windows: a PDH hiccup, pre-`WDDM 2.0`, or a build without `pdh`.
  - `is_spill_measurable()` is not used: it is a runtime PDH probe that also folds in pre-`WDDM 2.0`,
    which is the `?` case.
  - A pure core takes the platform answer as a parameter, so the tests run on any OS.
    `format::spill_cell` wraps it, and `hmn watch`'s per-PID `PAGED` renderer (`watch.rs:521-525`)
    shares it, so the two surfaces cannot diverge.
  - JSON stays `null`. The FAQ adds that CUDA managed-memory oversubscription is not measured
    either.
- **`spilled: null` waits for v0.3.0.** `write_spill_report_fields` feeds both `hmn spill --json`
  and `hmn watch --json`; turning `false` into `null` is a wire type change. It is written up as
  `field_check_v0213/02-notice_spilled_null_v0.md` at `f03298a7bb`
  and logged under `ROADMAP.md` "Speculative: v0.3.0". The FAQ's "check `measurable` first" stays.
- **The doc fix is one canonical statement.** The README Limitations bullet says the sandbox
  decides; that unsandboxed, every user's processes are listed; and that inside, `hmn` measures
  what is permitted and counts the rest. Every other site says it in one line and points there,
  so no site restates the mechanism and drifts again. One FAQ line covers the self-denying profile
  that crashes any Foundation program inside Apple's `libdispatch`, before `hmn` runs.
- **Logged, not built:**
  - naming the GPU clients a sandbox hides: IORegistry `AGXDeviceUserClient` survives the sandbox
    with pid, name and GPU time but no bytes, and needs an IOKit binding;
  - display width for CJK and emoji names: column widths are bytes and padding is chars, which
    holds wherever one char is one column, and the fix needs East Asian Width data.

  In `ROADMAP.md`, the display-width item extends the existing *Text-table widths in characters,
  not bytes* entry under *Speculative: v0.3.0*, and the IORegistry item is a *Carried forward*
  row.

---

## Scope

| # | Item | Kind | Status |
|---|---|---|---|
| 1 | Metal arm in `bounds_check`; Metal named in `NoGpuSource` and the `# Errors` docs (request 3) | **fix** | ✅ |
| 2 | `process_exists` through the lookup rule: PID 0, sandboxed callers (request 5) | **fix** | ✅ |
| 3 | `sysctl kern.proc` enumeration and `p_comm` names when libproc is refused; five-outcome ledger read (request 1) | feature | ✅ |
| 4 | `gpu_process_listing`, `denied_pids`, `ProcessListDenied` (request 1) | feature | ✅ |
| 5 | `hmn ps` states a skipped device; exits `2` when every device failed, and under `--exit-status` when one failed and nothing is listed (request 1) | **fix** | ✅ |
| 6 | `unreadable` counts, `--exit-status` `2`, `watch` notices (request 1); the platform remedy ships in PR B | feature | ✅ |
| 7 | `n/a` vs `?` in SPILL and `PAGED` cells (request 4, cells) | fix | ✅ |
| 8 | The macOS limitation restated from evidence, one canonical statement (request 2) | docs | ✅ |
| 9 | `spilled: null` notice and `ROADMAP.md` v0.3.0 entries (request 4, JSON) | docs | ✅ |
| 10 | Correct the field report's F5 mechanism and site list, before the issue comment | docs | ✅ |
| 11 | README, FAQ, tutorials, `CHANGELOG.md`, `ROADMAP.md` | docs | ✅ |

Items 3, 4 and 6 get an adversarial review before they merge. They change what the instrument
reports and the exit codes scripts gate on. Every item updates the `CHANGELOG.md`, `--help`,
README and FAQ text it makes stale, as in v0.2.13. Item 11 covers what remains.

---

## PR split

| PR | Contents | Scope items | Notes |
|---|---|---|---|
| A | docs only: the dogfooding report, this roadmap, the `ROADMAP.md` entries, `field_check_v0213/` at `f03298a7bb` | 9, 10, the planning half of 11 | the development reports directory is dropped at release, as `f3c6010` did |
| B | cross-platform fixes and docs, plus the macOS remedy `N protected — re-run outside the sandbox`, pulled forward from item 6 | 1, 2, 5, 7, 8 and their share of 11 | the `sudo` advice and the cross-user claim removed; item 5's silent wrong answer (a failing device dropped without a word, on every platform) stated; `--exit-status` `2` on a partial device failure |
| C | measuring inside a sandbox | 3, 4, 6 and the rest of 11 | new FFI and new public API, so it gets the adversarial review *Scope* asks for; the remedy text itself is already in B |

Each part can ship alone as its own release. Part 1 (PR B) goes first. Part 2 (PR C) reuses part
1's `kinfo_proc` parser (item 2), so it merges second, and whichever release ships alone must have
docs that describe only its own behaviour.

- **CI.** Item 5's "every device failed → exit 2" may change what `tests/cli_ps.rs` sees on
  GitHub's `macos-latest` runners, which are VMs. The denial line differs by PR:
  - PR B's test accepts exit `2` only together with a device line ending `(skipped)`, that is
    `hmn: ps failed to query device N: <err> (skipped)`;
  - PR C's accepts it only when that skip line carries `process list unreadable (`, or, on a VM
    whose ledger template does not resolve, the `NoGpuSource` text, under its own label in the
    test output.

  The change is not relied on until `gh pr checks` shows both `macos-latest` jobs passing on the
  PR's own CI, as the maintainer asked on issue #3.

---

## Verification

The fixtures, re-run on the M3 Pro (macOS 26.6.2) on 2026-10-04 against default-feature
release builds of c1810a5 and PR B, with `sandbox-exec` and the profiles of the campaign's
harness (`v0214_part1/harness/`). Each part 1 result is a row of `v0214_part1/00-findings_v0.md`;
the PR C rows were run on 2026-10-08 the same way, against PR C's release build. The evidence
files named below were dropped from the tree with the development reports directory and are
in the tree at commit `f03298a7bb`.

| Fixture | PR | Result | Evidence |
|---|---|---|---|
| the report's profile, PR B's form | B | `hmn ps` exits `2` with `hmn: ps failed to query device 0: … (skipped)` and no table; `--exit-status` exits `2`; `--json` prints nothing | `v0214_part1/00-findings_v0.md` (4)–(6) |
| the report's profile, PR C's form: exit `2` with the count and the remedy | C | `hmn ps` exits `2` with no table; `--device 0` and `hmn watch 1` exit `2` with the same denial | `hmn: ps failed to query device 0: process list unreadable (954 refused, none other than the caller's could be read) — re-run outside the sandbox (skipped)` |
| `same-sandbox` allowed: the job listed **with its bytes**, plus the unreadable count | C | `hmn ps` exits `0` and lists the job at 256 MiB beside `hmn`'s own 16 KiB row; `ps --pid <job> --exit-status` exits `0` | `hmn: 2 GPU processes found (256 MiB committed total; 948 unreadable — re-run outside the sandbox).` |
| pidinfo denied: names, not `?` | C | `hmn ps --json` lists 28 rows, each with a name, `hmn`'s own row included; exit `0`, no `protected` and no `re-run elevated` in stderr (PR B: 25 rows, 24 nameless) | `hmn: 28 GPU processes found (802 MiB committed total).` |
| pidinfo denied, the remedy | B | `N protected — re-run outside the sandbox` | `v0214_part1/00-findings_v0.md` (12) |
| unsandboxed: output unchanged | B | `compare.py` identical apart from SPILL/`PAGED` `?` → `n/a` | `v0214_part1/c1810a5/none/`, `v0214_part1/pr_b/none/` |
| the Codex policy: output unchanged | B | `compare.py` identical apart from SPILL/`PAGED` `?` → `n/a` | `v0214_part1/harness/CODEX_PIN`, `v0214_part1/pr_b/C/` |
| the App Sandbox build, PR B's form | B | `hmn ps` exits `2` with the skip line; `hmn ps --device 0` exits `2` with the `NoGpuSource` text naming Metal | `v0214_part1/app_sandbox/README.md` |
| the App Sandbox build, PR C's form: exit `2` with the denial line carrying `process list unreadable (` | C | `hmn ps`, `hmn ps --device 0` and `hmn watch 1 --duration 1s --interval 1s` each exit `2`; plain `ps` ends the line with `(skipped)` | `hmn: ps failed to query device 0: process list unreadable (969 refused, none other than the caller's could be read) — re-run outside the sandbox` |
| the Claude Code sandbox | B | lists normally with both builds (18 processes, exit `0`) | `v0214_part1/claude_code_sandbox/pr_b.md` |
| a profile denying only `process-info-ledger` | B | the residual: `0 GPU processes found.`, exit `0`; PR C closes it | `v0214_part1/00-findings_v0.md` (17) |
| `hmn ps --device 1` | B | `device index 1 out of range (have 1 devices)`, unsandboxed and under the report's profile | `v0214_part1/fixtures/cli.txt` |
| `hmn watch 0` | B | no warning | `v0214_part1/fixtures/cli.txt` |
| `cargo test --test macos_smoke -- --ignored` | B | 3/3 (2/2 at c1810a5) | `v0214_part1/fixtures/tests.txt` |
| the `kinfo_proc` test under Rosetta 2 | B | passes | `v0214_part1/fixtures/tests.txt` |
| `tests/cli_ps.rs` on PR B's `macos-latest` CI | B | run 37210951264: `branch=expected` four times, with and without `--exit-status` on both jobs | `v0214_part1/ci_macos_cli_ps.md` |
| the same policy run directly (profile S0) | C | `hmn ps` and `hmn ps --device 0` exit `2` with the denial line; `hmn watch 1` exits `2` too | `hmn: ps failed to query device 0: process list unreadable (953 refused, none other than the caller's could be read) — re-run outside the sandbox` |
| a profile denying only `process-info-ledger` (profile L), PR C's form | C | exit `2` with the denial line, where PR B exited `0` with `0 GPU processes found.`: the caller's own ledger read is denied too | `hmn: ps failed to query device 0: process list unreadable (940 refused, none other than the caller's could be read) — re-run outside the sandbox` |
| the lib tests under Rosetta 2 | C | 118 passed, 0 failed, natively as well (95 on PR B) | `test result: ok. 118 passed; 0 failed` |
| the architectures the 648-byte `kinfo_proc` layout was verified on | C | arm64 natively and x86_64 under Rosetta 2; real Intel hardware is untested, as `ROADMAP.md`'s untested-hardware row says | the lib tests pass on `aarch64-apple-darwin` and `x86_64-apple-darwin` |

Gate set on every pushed commit (a test-first red commit is squashed into its green successor
before a branch is pushed):

- `rustup check` reports `stable` up to date, so the local toolchain is the current release
  before a green run is trusted: Rust 1.99.0's `assert_is_empty` lint reached CI before the
  local `stable` did (`587a6d5`). The command exits `0` whether or not an update exists, so the
  check is the `up to date` text on its `stable` line, not its exit status;
- `cargo fmt --check`;
- clippy with and without `--all-features`, and for `x86_64-unknown-linux-gnu`;
- `cargo test --locked --all-features`;
- `cargo doc` with `-D warnings`;
- `cargo +1.88 check --locked --all-features` (MSRV);
- `cargo +1.88 clippy --locked --all-targets --all-features -- -D warnings` (CI's 1.88 leg);
- `cargo +1.88 test --locked --all-features` (CI's 1.88 leg);
- `cargo check --locked --no-default-features` (`ci.yml`);
- `cargo check --locked --no-default-features --features nvml,dxgi,pdh` (`ci.yml`).

`tests/cli_ps.rs` accepts exit `2` with the denial skip line (label `skipped-device`) or with a skip line that carries the `NoGpuSource` text (label `skipped-device-nogpu`, which a macos-latest VM may legitimately give), since agents run `cargo test` inside sandboxes too; the PR's CI log says which fired, and on PR C's macos-latest jobs (run 37650659228) `branch=expected` fired four times, with and without `--exit-status` on both runners.
Future field checklists use a realistic dead PID: macOS PIDs
stop at 99999, and on Linux `pid_max` can exceed 999999.

---

## Consistency pass

Run by a reviewer with no part in the work, who read PR C's diff against the code the maintainer
wrote (`nvml.rs`, `pdh.rs`, `dxgi.rs`, `ps.rs`, `watch.rs`, `format.rs`), PR B's accepted
additions and `CONVENTIONS.md`. The question was consistency with that code, not correctness,
which the item reviews covered. `cargo fmt`, clippy and `cargo doc -D warnings` pass. Every
`unsafe`, `as` and `// EXPLICIT:` carries its annotation, and `# Errors`, `#[non_exhaustive]`,
the test-module allowances and the `cfg(any(.., test))` gating follow the conventions.

Adopted:

- One remedy join, `format::with_remedy`, behind the summary clause, the denial line and the
  unreadable-PID notice; one `--pid` rule, `PsFilters::pid_selected`, behind both the row filter
  and the unreadable count.
- `errno` read one way in the new `metal.rs` code, through `last_errno()`, and the
  `rc == 0 ? 0 : errno` read written once.
- The `KERN_PROC_ALL` attempt bound and buffer slack as named, documented constants, as NVML
  names its retry bounds; the `KERN_PROC_ALL` functions named after PR B's `kern_proc_pid_*`
  family.
- `ProcessListDenied`'s `Display` in the `<noun> <problem> (<context>)` shape of
  `CONVENTIONS.md`: `process list unreadable (N refused, none other than the caller's could be
  read)`.
- Each rule stated once, with the other sites pointing to it: the `ProcessListDenied` trigger on
  the variant, the never-parse-a-failed-fill rule on `classify_kern_proc_all`; and four stale or
  missing notes fixed (`RemedyPurpose::Names`, the raw `sample failed` line, two unwrapped
  paragraphs, the macOS listing cell of the `lib.rs` table).

Left as they are: the `--help` text that restates the 16-byte cut for `ps` and for `watch` (the
crate's help restates rules elsewhere), the exit-code rule's prose copies, which predate this
release, a shared helper for the two Seatbelt-profile tests in `tests/macos_smoke.rs`, the new
`hmn watch` lines, which stay inline in `run_watch`, a table test for the `KERN_PROC_ALL` buffer
length, the remedy clause's shape restated in `format_ps_summary`'s rustdoc, the second parse of
one `kinfo_proc` buffer in `kern_proc_pid_comm`, the `proc_pidpath` call shared by two functions,
and the qualified `kinfo::EPERM`.

The full gate set passes on the final tree: `cargo fmt --check`; clippy with `-D warnings` by
default, with `--all-features` and for `x86_64-unknown-linux-gnu`; `cargo test --locked
--all-features` (118 library tests); `cargo doc -D warnings`; `cargo +1.88` check, clippy and
test; both `--no-default-features` checks; and the library tests under Rosetta 2 (118).

---

## Decisions taken

Raised with the issue #3 reply, answered in the maintainer's comment of 2026-10-02:

- **Wanted now: yes, "in two parts",** as PRs against `main`: part 1 is the cross-platform fixes
  and docs (items 1, 2, 5, 7, 8), part 2 is measuring inside a sandbox (items 3, 4, 6). They can
  ship together or separately; part 1 goes first, since it takes the false statements out of
  `--help` soonest. See *PR split*.
- **API names fixed:** `gpu_process_listing`, `GpuProcessListing { entries, denied_pids }` and
  `HypomnesisError::ProcessListDenied { denied }`. The maintainer agreed that a list beats a
  count, because `hmn ps --pid` and `hmn watch` can ask about one PID.
- **Remedy wording fixed:** `N unreadable — re-run outside the sandbox`, mirroring Windows'
  `N protected — re-run elevated for names`. The library docs lead with the count, since
  inside an App Sandbox there is no 'outside' (the `gpu_process_listing` rustdoc is written with
  item 4). PR B ships the macOS remedy on the existing `N protected` count
  (`N protected — re-run outside the sandbox`); PR C adds the `N unreadable` count, which
  carries the same remedy.
- **`spilled: null` deferred to v0.3.0** under Principle 2. It is item 9, and the notice exists.
- **Checked by hand, 2026-10-02:**
  - `sudo` under the report's profile is refused like uid 501 (exit `2`), so the macOS docs drop
    the `sudo` advice on measurement, not only on reasoning;
  - run from Terminal.app as responsible process, unsandboxed, `hmn ps` lists 26 processes,
    WindowServer included.
- **The maintainer's three additions** landed in *Design decisions* (the `kinfo_proc` layout and
  the architectures it was checked on), *PR split* (CI) and *When it bites* (Claude Code).

Decided on 2026-10-03 while splitting the work into PRs; these are not from the reply:

- **The macOS remedy ships with PR B**, not with the rest of item 6 in PR C. The remedy wording
  and its `cfg!`-selected constant are a text change on lines part 1 already edits; without it,
  macOS `hmn ps` would keep saying "re-run elevated" until PR C.
- **`--exit-status` exits `2` on a partial device failure.** With `--exit-status`, nothing listed
  and a tried device failed is "can't tell" (`2`), not "nothing matched" (`1`), because a skipped
  device is not a negative answer. Without `--exit-status` nothing changes: exit `0` while some
  device answered. It is the fourth deliberate behaviour change in *Why v0.2.14*.
- **Every pushed commit passes the gate set.** A test-first red commit stays on the task branch
  while the work is done and is folded into its green successor before a branch is pushed, so no
  pushed commit is red. The set is *Verification*'s, extended with the MSRV check, CI's 1.88
  clippy and test leg, the two `ci.yml` feature-matrix checks, and the toolchain-freshness check.

---

## At release

- `Cargo.toml` bumped to `0.2.14`; this roadmap's status and the dogfooding report's `Status`
  flipped, per the dogfooding style guide.
- The README's "what's new" banner rotated: 🆕 `0.2.14`, `0.2.13` to 🚀, `0.2.11` dropped.
- The development reports directory dropped from the tree before the merge, as in `f3c6010`; it stays in history.
  Each link into it from `docs/` and `ROADMAP.md` becomes a plain mention of the file name and
  the SHA of the commit that last held it, so no link is left pointing at a deleted file.
  Since PR B, `Cargo.toml` excludes that directory from the package, so a release cut before
  the drop does not ship it.

---

## References

- [`docs/dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md`](dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md)
  — the report this release implements.
- `field_check_v0213/` at `f03298a7bb` — the findings, evidence,
  probes and the `spilled` notice.
- [`docs/roadmap-v0.2.13.md`](roadmap-v0.2.13.md) — `process_exists`, `--exit-status`, and the
  decision to skip a failing device when `--device` is not given.
- [`docs/roadmap-v0.2.10.md`](roadmap-v0.2.10.md) — the silent-`[]` fix whose shape (a stderr
  statement, an unchanged JSON shape) the denial counts follow.
- XNU `bsd/kern/sys_generic.c`, `ledger()`; Apple DTS on libproc in the App Sandbox
  ([691857](https://developer.apple.com/forums/thread/691857),
  [52941](https://developer.apple.com/forums/thread/52941)); OpenAI Codex
  `codex-rs/sandboxing/src/seatbelt_base_policy.sbpl`.
