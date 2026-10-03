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

Four behaviour changes are deliberate. Each turns a silent wrong answer into a stated one:

- `gpu_processes` returns an error, not an empty list, when the process list was enumerated but
  no process other than the caller's could be read;
- `hmn ps` exits `2` when every device it tried failed, where it now prints an empty table and
  exits `0`;
- `hmn ps --exit-status` exits `2` ("can't tell") rather than `1` ("nothing matched") when nothing
  is listed and some processes could not be read;
- `hmn ps --exit-status` also exits `2`, not `1`, when nothing is listed and a
  tried device failed.

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
  is no uid check. The crate's own first macOS probe (May 2026, `__reports__/macos_ledger/00-findings_v0.md`
  in commit `7045b5c`) had already read WindowServer's ledger unprivileged. "Cross-user needs
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
([evidence](../__reports__/field_check_v0213/evidence/claude_code_sandbox.md)). It takes an
explicit `(deny process-info…)`, or the App Sandbox:

| Caller | `proc_listpids` | others' `ledger` | `proc_pidpath` | `sysctl kern.proc` | `hmn` 0.2.13 |
|---|---|---|---|---|---|
| unsandboxed | ok | ok | ok | ok | correct |
| Codex Seatbelt policy | ok | ok | ok | `kern.proc.all` denied, `kern.proc.pid` ok | correct |
| App Sandbox | `EPERM` | `EPERM` (self ok) | ok | ok (823 processes, `kernel_task` named) | `0 found`, exit `0` |
| explicit `deny process-info*` (the report's profile; agent sandboxes that deny it to stop argv leaks) | `EPERM` | `EPERM` | `EPERM` | ok (969 processes) | `0 found`, exit `0` |
| the same, with `same-sandbox` allowed | `EPERM` | ok for the sandbox's own jobs | ok for them | ok | `0 found`, exit `0`, though the job is readable |
| `process-info-pidinfo` denied outside the sandbox (`agent-safehouse` v0.12) | ok | ok | `EPERM` | ok | right numbers, names `?`, "re-run elevated" |
| Claude Code's Bash sandbox (macOS Seatbelt, `/sandbox`) | ok | ok | ok | ok (1108 processes, `kernel_task` named) | correct |

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
  works today, unsandboxed and under Codex, keeps byte-identical output.

  One private helper reads `kinfo_proc` records as `[u8; 648]` with named offsets. It needs no
  `libc` dependency, checks that the length is a whole number of records, and takes `p_comm` from
  the record, so the enumeration fallback gets names in the same pass. Record parsing and errno
  classification are pure functions with unit tests, the way `proc_name.rs` tests its own; the
  sandbox paths cannot be unit-tested any other way.
- **The 648-byte `kinfo_proc` layout, and where it was checked.** Measured 2026-10-02 on the M3
  Pro (macOS 26.6.2, SDK 26.2) and re-run 2026-10-03; the programs and their verbatim output are
  in [`__reports__/field_check_v0213/evidence/kinfo_proc_layout.md`](../__reports__/field_check_v0213/evidence/kinfo_proc_layout.md).
  - arm64, natively: `sizeof(struct kinfo_proc)` is 648, `p_pid` sits at offset 40 and `p_comm`
    at offset 243 (17 bytes with the NUL). A live `KERN_PROC_PID` read of PID 1 returns one
    648-byte record named `launchd`, and `KERN_PROC_ALL` returns a whole number of records.
  - x86_64, against the SDK header: the same `sizeof`/`offsetof` program compiled with
    `clang -arch x86_64` prints 648, 40 and 243. The same live reads, run as an x86_64 process
    under Rosetta 2, give the same lengths.
  - x86_64, by test: `cargo test --target x86_64-apple-darwin --lib` under Rosetta 2 is a PR B
    check. It covers the parser's unit tests and one live read. Until it passes, the x86_64 claim
    covers the layout only.
  - Not verified: a native Intel Mac.
    Rosetta 2 runs x86_64 userland on the arm64 kernel, so it cannot show what an
    Intel kernel returns, and `ROADMAP.md` lists Apple Metal on Intel Macs as untested hardware
    (Principle 3, no Intel-Mac test hardware). This release changes neither, so the whole-records
    length check stays the parser's only guard against a layout that differs.
- **A per-PID read has four outcomes, not two.** `read_graphics_footprint` stops folding
  everything into `None`. It returns bytes; *denied* (`EPERM`); *gone* (`ESRCH`); or
  *unavailable*, when the `graphics_footprint` template index did not resolve. *Unavailable*, or
  both enumerations refused, makes the backend return `None`. The dispatcher then falls through
  to `NoGpuSource`, as for every other backend, instead of today's silent empty list.
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
  [`__reports__/field_check_v0213/02-notice_spilled_null_v0.md`](../__reports__/field_check_v0213/02-notice_spilled_null_v0.md)
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
| 1 | Metal arm in `bounds_check`; Metal named in `NoGpuSource` and the `# Errors` docs (request 3) | **fix** | ⬜ |
| 2 | `process_exists` through the lookup rule: PID 0, sandboxed callers (request 5) | **fix** | ⬜ |
| 3 | `sysctl kern.proc` enumeration and `p_comm` names when libproc is refused; four-outcome ledger read (request 1) | feature | ⬜ |
| 4 | `gpu_process_listing`, `denied_pids`, `ProcessListDenied` (request 1) | feature | ⬜ |
| 5 | `hmn ps` states a skipped device; exits `2` when every device failed, and under `--exit-status` when one failed and nothing is listed (request 1) | **fix** | ⬜ |
| 6 | `unreadable` counts, `--exit-status` `2`, `watch` notices (request 1); the platform remedy ships in PR B | feature | ⬜ |
| 7 | `n/a` vs `?` in SPILL and `PAGED` cells (request 4, cells) | fix | ⬜ |
| 8 | The macOS limitation restated from evidence, one canonical statement (request 2) | docs | ⬜ |
| 9 | `spilled: null` notice and `ROADMAP.md` v0.3.0 entries (request 4, JSON) | docs | ✅ |
| 10 | Correct the field report's F5 mechanism and site list, before the issue comment | docs | ✅ |
| 11 | README, FAQ, tutorials, `CHANGELOG.md`, `ROADMAP.md` | docs | ⬜ |

Items 3, 4 and 6 get an adversarial review before they merge. They change what the instrument
reports and the exit codes scripts gate on. Every item updates the `CHANGELOG.md`, `--help`,
README and FAQ text it makes stale, as in v0.2.13. Item 11 covers what remains.

---

## PR split

| PR | Contents | Scope items | Notes |
|---|---|---|---|
| A | docs only: the dogfooding report, this roadmap, the `ROADMAP.md` entries, `__reports__/field_check_v0213/` | 9, 10, the planning half of 11 | `__reports__/` is dropped at release, as `f3c6010` did |
| B | cross-platform fixes and docs, plus the macOS remedy `N protected — re-run outside the sandbox`, pulled forward from item 6 | 1, 2, 5, 7, 8 and their share of 11 | the `sudo` advice and the cross-user claim removed; item 5's silent wrong answer (a failing device dropped without a word, on every platform) stated; `--exit-status` `2` on a partial device failure |
| C | measuring inside a sandbox | 3, 4, 6 and the rest of 11 | new FFI and new public API, so it gets the adversarial review *Scope* asks for; the remedy text itself is already in B |

Each part can ship alone as its own release. Part 1 (PR B) goes first. Part 2 (PR C) reuses part
1's `kinfo_proc` parser (item 2), so it merges second, and whichever release ships alone must have
docs that describe only its own behaviour.

- **CI.** Item 5's "every device failed → exit 2" may change what `tests/cli_ps.rs` sees on
  GitHub's `macos-latest` runners, which are VMs. The denial line differs by PR:
  - PR B's test accepts exit `2` only together with a device line ending `(skipped)`, that is
    `hmn: ps failed to query device N: <err> (skipped)`;
  - PR C's accepts it only when that skip line carries `process list unreadable:`, or, on a VM
    whose ledger template does not resolve, the `NoGpuSource` text, under its own label in the
    test output.

  The change is not relied on until `gh pr checks` shows both `macos-latest` jobs passing on the
  PR's own CI, as the maintainer asked on issue #3.

---

## Verification

To be filled in as items land. These fixtures must be re-run:

- every row of the *When it bites* table, with `sandbox-exec` and the same profiles:
  - the report's profile:
    - PR B: exit `2` with the skip line `hmn: ps failed to query device 0: … (skipped)`;
    - PR C: exit `2` with the count and the remedy;
  - `same-sandbox` allowed: the job listed **with its bytes**, plus the unreadable count;
  - pidinfo denied: names, not `?`;
  - unsandboxed and the Codex policy: output unchanged;
- the App Sandbox build. Inside it, `KERN_PROC_ALL` and `KERN_PROC_PID` were already measured
  working (a C probe, 2026-10-02); after the change, `hmn ps` there must exit `2` with the
  denial, since only the caller's own `ledger` is readable;
- `hmn ps --device 1` → `device index 1 out of range (have 1 devices)`;
- `hmn watch 0` → no warning;
- `cargo test --test macos_smoke -- --ignored` (2/2 at `cf5ada0`).

Gate set on every pushed commit (a test-first red commit is squashed into its green successor
before a branch is pushed):

- `cargo fmt --check`;
- clippy with and without `--all-features`, and for `x86_64-unknown-linux-gnu`;
- `cargo test --locked --all-features`;
- `cargo doc` with `-D warnings`;
- `cargo +1.88 check --locked --all-features` (MSRV);
- `cargo +1.88 clippy --locked --all-targets --all-features -- -D warnings` (CI's 1.88 leg);
- `cargo +1.88 test --locked --all-features` (CI's 1.88 leg);
- `cargo check --locked --no-default-features` (`ci.yml`);
- `cargo check --locked --no-default-features --features nvml,dxgi,pdh` (`ci.yml`).

`tests/cli_ps.rs` accepts exit `2` only together with the denial line, since agents run
`cargo test` inside sandboxes too. Future field checklists use a realistic dead PID: macOS PIDs
stop at 99999, and on Linux `pid_max` can exceed 999999.

---

## Consistency pass

To be run with fresh eyes after the last commit.

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
  item 4).
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
  clippy and test leg, and the two `ci.yml` feature-matrix checks.

---

## At release

- `Cargo.toml` bumped to `0.2.14`; this roadmap's status and the dogfooding report's `Status`
  flipped, per the dogfooding style guide.
- The README's "what's new" banner rotated: 🆕 `0.2.14`, `0.2.13` to 🚀, `0.2.11` dropped.
- `__reports__/` dropped from the tree before the merge, as in `f3c6010`; it stays in history.
  Each link into it from `docs/` and `ROADMAP.md` becomes a plain mention of the file name and
  the SHA of the commit that last held it, so no link is left pointing at a deleted file.

---

## References

- [`docs/dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md`](dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md)
  — the report this release implements.
- [`__reports__/field_check_v0213/`](../__reports__/field_check_v0213/) — the findings, evidence,
  probes and the `spilled` notice.
- [`docs/roadmap-v0.2.13.md`](roadmap-v0.2.13.md) — `process_exists`, `--exit-status`, and the
  decision to skip a failing device when `--device` is not given.
- [`docs/roadmap-v0.2.10.md`](roadmap-v0.2.10.md) — the silent-`[]` fix whose shape (a stderr
  statement, an unchanged JSON shape) the denial counts follow.
- XNU `bsd/kern/sys_generic.c`, `ledger()`; Apple DTS on libproc in the App Sandbox
  ([691857](https://developer.apple.com/forums/thread/691857),
  [52941](https://developer.apple.com/forums/thread/52941)); OpenAI Codex
  `codex-rs/sandboxing/src/seatbelt_base_policy.sbpl`.
