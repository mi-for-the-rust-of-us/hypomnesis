# Dogfooding report (from the issue #3 field check on Apple Silicon): v0.2.13 passes all eight checks — but macOS's `ledger` `EPERM` comes from the sandbox, not from process ownership, and a sandboxed `hmn ps` reports `0 GPU processes found` with exit 0

**Date:** 2026-10-01
**Reporter:** field check for issue #3, Apple M3 Pro (arm64), macOS 26.6.2 (25G83), unprivileged uid 501, `hmn 0.2.13` built from `cf5ada0` with `cargo build --release`
**Severity:** validation of `process_exists` and the v0.2.13 `hmn ps`/`hmn watch` output on macOS + request for sandbox-aware reporting, doc, error-path and JSON/text-cell corrections
**Affected area:** `metal::list_compute_processes` on `EPERM`; the macOS cross-user doc claim (`hmn --help` Limitations, `metal.rs` and `gpu/mod.rs` docs); `gpu::bounds_check`; `gpu::process_exists` at PID 0; the SPILL/PAGED cells and the `spilled` field where spill does not exist
**Status:** Accepted — v0.2.14, in three PRs (docs; cross-platform fixes; sandbox measurement), per [the maintainer's reply on issue #3](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/3#issuecomment-5947395182). Fix plan and PR split: [`docs/roadmap-v0.2.14.md`](../roadmap-v0.2.14.md). Checking this report against the code corrected its account of the silent zero; see *Misreadings recorded*.

---

## TL;DR

All eight checks from issue #3 pass as worded. `cargo test --all-features` exits 0: 338 passed, 0
failed, and both `process_exists` unit tests `ok`. `hmn watch` warns about a dead PID and only that
PID. The spill summary says `not measurable`, the JSON says `"measurable":false`, `--top 5` lines
up, and `ps --filter`, `--exit-status`, `--device 1` and `--help` behave as specified.

The finding is the premise of check 3. The crate documents that macOS **cross-user** PIDs are
skipped because `ledger` returns `EPERM`. Measured, **ownership does not matter; the sandbox
does:**

- Unsandboxed, `ledger` returned rc 0 for 249 of 250 other-user PIDs, root included. The one
  failure was `ESRCH`.
- Under a sandbox that denies `process-info*` on other processes, a **same-user** Safari gets
  `EPERM`.
- In that sandbox, `hmn ps` prints `0 GPU processes found.` and exits 0. `hmn watch` exits 2,
  blaming a missing GPU backend. Both have one cause: the sandbox refuses `proc_listpids`, the
  call that lists every PID, so the Metal backend gives up and the library reports `NoGpuSource`.
  `hmn ps` then drops the failed device without a word.
- A real App Sandbox build behaves the same way. The sandbox still answers `sysctl(KERN_PROC…)`,
  which lists and names every process, but `hmn` does not use it.

The `None` ("can't tell") branch of `process_exists`, which check 3 was meant to exercise, was
reached by the probe but never by `hmn`.

Smaller defects:

- `--device 1` prints the generic `NoGpuSource` text, because `bounds_check` has no Metal branch.
- `hmn watch 0` claims kernel_task does not exist.
- On UMA, SPILL reads `?` and the JSON says `spilled:false`. Both describe something that does not
  apply as if it were unknown or false.

## What worked — eight checks, no surprises in the output

These parts behave exactly as issue #3 describes, so they are the ones not to break.

```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 2 PID(s)
hmn watch: pid=999999 names no running process; its rows will read 0 MiB
```

That is the whole stderr of `hmn watch 999999 2561 --interval 1s --duration 2s`, where 2561 is
Safari. There is no line for 2561. A dead PID inside the valid range (99998) warns the same way.

```
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID   NAME    BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    2561  Safari  2 MiB            2 MiB        0 MiB            0 MiB        ?
```

`no spill observed` appears 0 times across every captured run. Parsed with `json`, the `--json`
summary has `"measurable":false` and `"spilling_at_attach":null`, and the samples carry
`"spilling":null,"paged":null`.

```
TIME      PID      NAME                         COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     393      WindowServer                 264 MiB    +0 B       0 MiB      +0 B       ?
+0.0s     396      loginwindow                  129 MiB    +0 B       0 MiB      +0 B       ?
+0.0s     2728     com.apple.WebKit.WebContent  56 MiB     +0 B       0 MiB      +0 B       ?
```

`--top 5` has 0 misaligned cells, measured by display width, not by eye. The header is 97 columns
but 99 bytes because of the two `Δ`, so the padding counts `char`s, as it should. Explicit PIDs
widen NAME to fit a 44-char `com.apple.appkit.xpc.openAndSavePanelService`, and the columns stay
aligned.

```
hmn: 1 GPU process found matching filter="sAfArI" (2 MiB committed total).     # exit 0
hmn: 0 GPU processes found matching filter="ZzNoSuchApp".                      # exit 1
```

`hmn --help` puts `Commands:` at character offset 1440 and `Limitations (per-platform):` at 3513.

## What is wrong — `EPERM` is a sandbox verdict, not an ownership one

Issue #3 says that WindowServer's `ledger` read gets `EPERM`, `so this is exactly the case where
process_exists is asked`. The help text says the same (`src/bin/hmn/main.rs:169-171`):

```
- macOS: cross-user PIDs are silently skipped — the per-PID `ledger` syscall returns `EPERM` for
  processes owned by another user. To list every PID on the system, run elevated (`sudo hmn ps`).
```

I called `ledger` and `proc_pidpath` directly from Python `ctypes`, sharing no code with `hmn`,
with and without a `sandbox-exec` profile that lets a process inspect only itself:

```
(version 1)(allow default)(deny process-info*)(allow process-info* (target self))
```

| PID | owner | unsandboxed `ledger` / `proc_pidpath` | sandboxed `ledger` / `proc_pidpath` |
|---|---|---|---|
| 393 WindowServer | `_windowserver` | rc 0 / 86 B | **`EPERM` / `EPERM`** |
| 1 launchd | root | rc 0 / 13 B | **`EPERM` / `EPERM`** |
| 2561 Safari | **hacker (same user)** | rc 0 / 90 B | **`EPERM` / `EPERM`** |
| self | hacker | rc 0 / 39 B | rc 0 / 39 B |

The "responsible process", which is the app macOS charges a privacy (TCC) request to, is the same
in both columns: PID 25217, the Claude Code app that spawned the shell. So nothing the parent app
was granted explains the difference. The only variable is the sandbox. A whole-system unsandboxed
scan agrees: `{(False, 0, 0): 249, (True, 0, 0): 670, (False, -1, 3): 1}`, keyed `(same user?,
rc, errno)`. That is 0 `EPERM` among 920 PIDs, and one cross-user process exited mid-scan with
`ESRCH`. A denial of only `process-info-ledger` gives `EPERM` from `ledger` even for the caller's
own PID.

This turns the doc claim around in both directions. Another user's process is readable
unsandboxed, and the caller's own processes are not readable sandboxed. `sudo` does not help a
sandboxed caller. Measured on 2026-10-02 with
`sudo sandbox-exec -p '<profile>' hmn ps --device 0`, the root run gets the same `NoGpuSource`
text and exit 2 as the unprivileged one. The advice to re-run elevated is wrong on macOS.

The kernel source agrees. XNU's `ledger()` (`bsd/kern/sys_generic.c`) returns `ESRCH` when
`proc_find` fails, then calls only `mac_proc_check_ledger`, which is the sandbox's hook. There is
no uid check. The crate's own first macOS probe, in May 2026, had already read WindowServer's
ledger unprivileged. The "cross-user needs root" claim came from `task_for_pid`.

The same claim, worded differently, appears in at least these places:

- `src/bin/hmn/main.rs:169-171` and `:194-196` ("run elevated (`sudo`) to include cross-user PIDs");
- `src/bin/hmn/ps.rs:505-509`;
- `src/gpu/metal.rs:5-11`, `:610-613` and `:682-684`;
- `src/gpu/mod.rs:374-377`.

That was the first count. A second pass found about sixteen sites, among them three in
`README.md`:

- the capability table's "same-user; `sudo` for cross-user";
- "libSystem syscalls always succeed on Apple Silicon";
- Limitations bullet 9.

There are more in `docs/FAQ.md`, in `ROADMAP.md` Principle 4, and in the "re-run elevated" hints,
which print on macOS too. The full list is in
[`docs/roadmap-v0.2.14.md`](../roadmap-v0.2.14.md).

## What `hmn` does under a sandbox — a silent zero

Here is the same binary under the profile above:

```
$ hmn ps
PID  NAME  VRAM  SHARED  DEVICE  SPILL
hmn: 0 GPU processes found.                                          # exit 0
$ hmn ps --filter safari --exit-status
hmn: 0 GPU processes found matching filter="safari".                 # exit 1
$ hmn watch 393 1 999999 99998 --interval 1s --duration 1s
hmn: watch failed to query device 0: no GPU measurement source available (NVML, DXGI, PDH, and nvidia-smi all failed or are disabled)   # exit 2
```

Unsandboxed, the same moment lists WindowServer at 264 MiB, Safari, WebKit and others.

- **`hmn ps` is wrong and says it is right.** Under this profile `proc_listpids` itself returns
  `EPERM`. `metal::list_compute_processes` (`src/gpu/metal.rs:619-708`) gives up and returns
  `None`, and `gpu_processes(0)` falls through to `NoGpuSource`.
  - `hmn ps --device 0` shows it: exit 2, with the `NoGpuSource` text.
  - Without `--device`, `run_ps` skips a device that fails (`src/bin/hmn/ps.rs:403`,
    `Err(_) => continue`) and prints nothing about it. The table comes out empty, with exit 0.
    That silence happens on every platform; on macOS this profile is what triggers it.
  - The `N protected — re-run elevated for names` part of the summary counts only rows that were
    *listed* without a name, so it stays silent too.
- **`--exit-status` exits 1, which also means "no match".** A CI gate cannot tell "nothing
  matched" from "nothing was readable".
- **`hmn watch` blames the wrong cause.** It exits 2 with the `NoGpuSource` text. Traced: its
  first `gpu_processes(device)` call (`src/bin/hmn/watch.rs:1094`) hits the same refused
  `proc_listpids`, and `NoGpuSource` names four backends macOS doesn't have.
- **The per-PID silent skip is real, under a narrower profile.** `list_compute_processes`
  treats a failed `read_graphics_footprint` (`EPERM`, `ESRCH` or absent index) as "skip". With
  only `process-info-ledger` denied, `proc_listpids` works, every row disappears, and the result is
  `Some(vec![])`. `hmn ps --device 0` then exits 0 with an empty table. `hmn watch` shows
  WindowServer at `0 MiB` with no notice, because `process_exists` correctly says the PID exists
  and the row reads 0.

Who runs `hmn` sandboxed? Measured after the first draft of this report:

| Caller | `proc_listpids` | others' `ledger` | `sysctl kern.proc` | `hmn` 0.2.13 |
|---|---|---|---|---|
| unsandboxed | ok | ok | ok | correct |
| OpenAI Codex's Seatbelt policy (`deny default`, `process-info*` allowed for `same-sandbox`) | ok | ok | `kern.proc.all` denied, `kern.proc.pid` ok | correct |
| a real App Sandbox (ad-hoc signed `com.apple.security.app-sandbox`) | `EPERM` | `EPERM`, self ok | ok (823 processes, `kernel_task` named) | `0 found`, exit 0 |
| the profile above (explicit `deny process-info*`) | `EPERM` | `EPERM` | ok (969 processes) | `0 found`, exit 0 |
| the same, with `same-sandbox` allowed | `EPERM` | ok for the sandbox's own jobs | ok | `0 found`, exit 0, though the job is readable |

`(deny default)` alone does not deny `process-info`. It takes an explicit `deny`, as some agent
sandboxes now add to stop argv leaks, or the App Sandbox.

A harsher profile that also denies `process-info*` on **self** crashes `hmn ps` with exit 133
(`SIGTRAP`). The fault is Apple's, not `hmn`'s: `libdispatch` aborts with `BUG IN LIBDISPATCH:
Unable to get the unique pid` inside `+[NSBundle mainBundle]`. Any Foundation or Metal program
dies there, and `hmn --version` alone runs fine.

## What check 3 actually exercised

`process_exists` is asked only about explicit PIDs absent from the first sample
(`src/bin/hmn/watch.rs:964-977`, called at `:1175`). Here is which branch each case took:

| PID | context | listed by `gpu_processes(0)` | `process_exists` asked? | answer |
|---|---|---|---|---|
| 393 WindowServer | unsandboxed | yes, 264 MiB | no | — |
| 1 launchd, 332 configd | unsandboxed | no (zero balance) | yes | `Some(true)`, no warning |
| 99998, 999999 | unsandboxed | no | yes | `Some(false)`, warns ✅ |
| any | sandboxed | `watch` exits 2 before asking | — | — |

The `None` arm (`src/gpu/metal.rs:710-731`) is correct and reachable: under the sandbox,
`proc_pidpath` gives `EPERM`. `hmn watch` never reaches it, because it fails earlier. Keep the
arm, and correct its doc example from "cross-user" to "sandboxed".

## What else is wrong — four smaller defects

**`--device 1` reports a missing backend instead of an out-of-range index.**

```
hmn: ps failed to query device 1: no GPU measurement source available (NVML, DXGI, PDH, and nvidia-smi all failed or are disabled)
```

Exit 2 and the prefix are as specified. The body names four backends that do not exist on macOS,
on a machine where device 0 works:

- `metal::list_compute_processes` returns `None` for any index other than 0 (`metal.rs:620`).
- That falls through to `bounds_check(device_index)?; Err(NoGpuSource)` (`src/gpu/mod.rs:456-457`).
- `bounds_check` (`src/gpu/mod.rs:604-624`) consults only NVML and DXGI.
- The public `device_count()` (`src/gpu/mod.rs:56-60`) also consults `metal::device_count` and
  gets 1.

So `DeviceIndexOutOfRange { index: 1, count: 1 }` is never produced on macOS. `watch --device 1`
prints the same text. `device_info`'s docs promise `OutOfRange` only for NVML/DXGI counts, so this
is a gap the docs allow, not a regression.

**`hmn watch 0` says kernel_task does not exist.**

```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
hmn watch: pid=0 names no running process; its rows will read 0 MiB
```

`proc_pidpath(0)` returns `ESRCH`. This is the one false "no" observed, and `Some(false)` is the
answer the API promises never to give for a live process.

**SPILL and PAGED read `?` where spill does not exist.** The README says macOS `UMA` "has nothing
to spill *into*". That describes something that doesn't apply, not something unknown. The
crate already writes `n/a` for that kind of absence: the README capability table uses it for
reserved memory and driver version on Apple Silicon. `?`, meanwhile, carries two meanings in the
SPILL column, "doesn't apply" and "applies but unreadable now" (PDH hiccup, pre-`WDDM 2.0`), plus
a third in the NAME column ("unresolved name").

**The JSON summary says `"spilled":false` when it means "does not apply".** The summary line
begins `{"kind":"summary","measurable":false,"spilled":false,…`. The README and FAQ tell consumers
to check `measurable` first, but the field itself still says `false`. That is the collapse v0.2.13
removed from the text summary.

## Requests, in order of how much they would help

1. **Make an unreadable process list loud, not empty.** When `ledger` returns `EPERM` for every
   PID but the caller's own, or for most of them, `hmn ps` should say so. A possible shape:
   `hmn: 0 GPU processes found; 917 PIDs unreadable (EPERM — sandboxed caller?)`. It should not
   exit 0 as if the list were complete, and `--exit-status` should not return the "no match" code
   for it. `hmn watch` should report the same cause instead of `NoGpuSource`. **This is the change
   that matters most**: it is the one case where the instrument reports a wrong measurement as a
   correct one. The library side may need a count of `EPERM` skips next to the entries, in
   whatever shape fits the crate's API. Before counting, **measure what the sandbox still
   permits**. When `proc_listpids` is refused, `sysctl(KERN_PROC_ALL)` lists every process, and
   `ledger` reads of the caller's own sandbox succeed, so an agent's own job could still be
   measured. Only what stays unreadable after that needs counting.
2. **Restate the macOS limitation from this evidence, at every site listed above.** The
   readable set is decided by the caller's sandbox, not by process ownership. Unsandboxed, every
   user's processes are readable. Drop or qualify the `sudo` advice until someone tests whether
   it helps a sandboxed caller.
3. **Give `bounds_check` a Metal branch**, the same shape as the NVML and DXGI branches, fed by
   `metal::device_count()`. A unit test can pin it: on macOS, `gpu_processes(1)` returns
   `DeviceIndexOutOfRange`.
4. **Write `n/a` instead of `?` in SPILL and PAGED where the memory model has no separate pool**
   (macOS UMA), and keep `?` for "applies but unreadable now". Decide Linux on the same rule: it
   OOMs rather than pages, though managed-memory oversubscription exists. JSON can stay `null`,
   because `measurable` already carries the reason. **Make `spilled` null** (or absent) when
   `measurable` is false. That changes a persisted JSON contract, so it needs whatever
   compatibility note the crate gives such changes.
5. **Treat PID 0 as existing on macOS**, or skip the notice for it. kernel_task is always there.
   `sysctl(KERN_PROC_PID, 0)` returns a record named `kernel_task`, with or without a sandbox, so
   existence can be answered without a special case.

## Smaller observations

1. Check 2's PID, 999999, is above the macOS PID max of 99999 (`ps -p 999999` →
   `process id too large`). It can never name a process, so the check is safe from PID reuse but
   does not test a realistic dead PID. 99998 does, and it passes too.
2. `tests/macos_smoke.rs` has two Metal tests that stay `#[ignore]` under `cargo test
   --all-features`: `device_info_reports_apple_brand` and `process_gpu_info_returns_metal_source`.
   They were not run in the first pass. `cargo test --test macos_smoke -- --ignored`, run later
   on the same machine: 2 passed.
3. Column padding counts `char`s. That is correct for every name seen here; wide CJK or emoji
   process names would probably misalign. Not tested. More precisely, widths are measured in
   bytes (`format::column_width`) and padded in chars, which stays aligned wherever one char is
   one column.
4. **Misreadings recorded.**
   - The first pass explained the `?`/0 MiB `watch` rows for launchd and configd as "the
     ledger/name lookups fail for them". The tool was right: the read succeeds with a zero
     balance, so the PID is unlisted and its row is built with name `?`.
   - The second pass concluded "no `EPERM` on this OS", which held only for an unsandboxed
     caller.

   The question that corrected it was whether the parent app's permissions were hiding a prompt.
   Answering it took the controlled sandbox comparison above.
   - The first draft of this report explained the sandboxed silent zero as per-PID `ledger`
     skips adding up to `Some(vec![])`. Under its own profile, `proc_listpids` fails first.
     Running `hmn ps --device 0` in the sandbox (exit 2) would have shown it. The per-PID skip is
     real, but only under a profile that denies `process-info-ledger` alone. The first draft
     also listed seven doc sites; a second pass found about sixteen. Both corrections came from
     checking the report against the code while planning v0.2.14.

## Acceptance fixtures (already run, free to regress against)

| Case | Command (uid 501, macOS 26.6.2, M3 Pro) | Expected | Observed |
|---|---|---|---|
| dead PID above PID max | `hmn watch 999999 <own PID> --interval 1s --duration 2s` | one warning, for 999999 | ✅ |
| dead PID in range | `hmn watch 99998 …` | warning | ✅ |
| cross-user, listed | `hmn watch 393` (WindowServer) | no warning, row from first sample | ✅ 264 MiB |
| root, zero balance | `hmn watch 1`, `hmn watch 332` | no warning | ✅ `?`/0 MiB |
| PID 0 | `hmn watch 0` | no warning | ❌ warns (request 5) |
| spill verdict | any `hmn watch` | `spill not measurable…` | ✅; cells `?` (request 4) |
| JSON summary | `hmn watch --json …` | `measurable:false`, `spilling_at_attach:null` | ✅; `spilled:false` (request 4) |
| out-of-range device | `hmn ps --device 1` | exit 2, `device index 1 out of range` | exit ✅, message ❌ (request 3) |
| `ledger`, unsandboxed | ctypes over all PIDs | per docs: `EPERM` for other users | rc 0 for 919/920, 1 `ESRCH`, 0 `EPERM` |
| `ledger`, sandboxed | ctypes, profile above | — | `EPERM` for 393, 1 **and same-user 2561**; rc 0 for self |
| sandboxed `ps` | `sandbox-exec -p '<profile>' hmn ps` | an unreadable-list notice, non-zero exit | ❌ `0 GPU processes found.`, exit 0 (request 1) |
| sandboxed `watch` | `sandbox-exec -p '<profile>' hmn watch 393 …` | the same notice | ❌ `NoGpuSource` text, exit 2 (request 1) |
| sandboxed `ps --device 0` | `sandbox-exec -p '<profile>' hmn ps --device 0` | the same notice | ❌ `NoGpuSource` text, exit 2: `proc_listpids` refused (request 1) |
| ledger-only denial | `sandbox-exec -p '(version 1)(allow default)(deny process-info-ledger)' hmn ps --device 0` | the same notice | ❌ `0 GPU processes found.`, exit 0 (request 1) |
| real App Sandbox | `hmn` built with an embedded `Info.plist`, ad-hoc signed with `com.apple.security.app-sandbox` | the same notice | ❌ `0 found`, exit 0; `--device 0` exit 2 (request 1) |
| `sysctl` under sandbox | a C probe in the App Sandbox; `ctypes` under the profile above | — | `KERN_PROC_ALL` lists 823 / 969 processes; `KERN_PROC_PID 0` → `kernel_task` |
| `sudo` under the profile | `sudo sandbox-exec -p '<profile>' hmn ps --device 0` | — | `NoGpuSource` text, exit 2: root is refused too |
| from Terminal.app, unsandboxed | `hmn ps`; `sandbox_probe.py` | unchanged output | ✅ 26 processes; `ledger` rc 0 for 393, 1, 2561; responsible PID = Terminal |
| Codex Seatbelt policy | `sandbox-exec -f codex seatbelt_base_policy.sbpl + (allow file-read*)` `hmn ps` | unchanged output | ✅ 20 processes, WindowServer included |

## Confidence

**High** for the eight check verdicts:

- every command's stdout, stderr and exit code were captured separately and re-read by a second
  pass;
- the JSON was parsed, not eyeballed;
- the column alignment was computed, not judged.

**High** that the sandbox, not ownership, decides `EPERM`. The comparison is controlled: same
binary, same PIDs, same responsible app, sandbox on or off. A same-user PID flips to `EPERM` and
other-user PIDs read fine. The independent signal is direct `ctypes` syscalls that share no code
with `hmn`. `hmn ps`'s own unsandboxed listing of `_windowserver` corroborates them.

**Residuals closed after the first draft (2026-10-02), by measurement:**

- **Responsible process.** The first runs all had the Claude Code app as responsible process. A
  run from Terminal.app (responsible PID 68779, Terminal itself) gives the same result unsandboxed:
  - `ledger` and `proc_pidpath` are rc 0 for WindowServer (393), launchd (1) and Safari (2561);
  - `hmn ps` lists 26 processes, among them WindowServer at 328 MiB and loginwindow.

  XNU's `ledger()` agrees: it has no TCC path.
- **`sudo` under a sandbox.** Root under the same profile is refused like uid 501:
  `hmn ps --device 0` prints the `NoGpuSource` text and exits 2.
- **A real App Sandbox** behaves like the `sandbox-exec` profile (see *Who runs `hmn`
  sandboxed?*).

**Residual, reasoning rather than measurement:**

- The probe's balance offsets were guessed. Only the zero-balance claims rest on them, and those
  are corroborated by `hmn ps` not listing the PIDs.
- Untested: other macOS versions.

## References

- Issue: [mi-for-the-rust-of-us/hypomnesis#3](https://github.com/mi-for-the-rust-of-us/hypomnesis/issues/3)
  (the checklist this report answers)
- [dogfooding-spill-verdict-wording-and-ps-filters.md](dogfooding-spill-verdict-wording-and-ps-filters.md):
  the report v0.2.13 implements. Its "`?`, never `no`" rule is what request 4 refines into `n/a`
  vs `?`.
- `docs/roadmap-v0.2.13.md`; FAQ "What does a `?` in the NAME column mean"; README
  "macOS UMA semantics"
- [`docs/roadmap-v0.2.14.md`](../roadmap-v0.2.14.md): the fix plan for these requests, with the
  full doc-site list.
- XNU `bsd/kern/sys_generic.c`, `ledger()`: the `mac_proc_check_ledger` hook, with no uid check.
- Findings briefing and evidence: `field_check_v0213/01-findings_v1.md` at `f03298a7bb` and
  `field_check_v0213/evidence/` at `f03298a7bb`. The probes are `probes/p.py` (`proc_pidpath`),
  `probes/l.py` (`ledger` scan) and `probes/sandbox_probe.py` (sandbox on/off).
