# Field check v0.2.13 (macOS, Apple M3 Pro): `hmn` inside Claude Code's Bash sandbox

The maintainer's reply on issue #3 asked for a *When it bites* row for Claude Code's own macOS
sandbox. This file records one run of `hmn` 0.2.13 and the field-check probes inside it, proven
sandboxed in the same command.

## Environment

```
git: src/, Cargo.toml and Cargo.lock as at upstream a602e05 (this branch adds docs only)
ProductName:		macOS
ProductVersion:		26.6.2
BuildVersion:		25G83
Apple M3 Pro
binary: target/release/hmn (cargo build --release --locked), built before the sandbox was on
host: Claude desktop app, Code tab; Claude Code version as printed by the run below
date: 2026-10-03
```

enabled-by: /sandbox, applied as a setting that takes effect for new sessions, so the run was made in a fresh session; user-confirmed: yes

The user enabled the sandbox; neither the session that made the run nor the one that prepared it
edited settings. Their words, in chat: "Enabled via
/sandbox", then "I applied the setting but it says it will apply only for new sessions".

## The run

One Bash tool call, inside the sandbox, with no override and no hand-written profile. It runs the
proof lines first, then the probes in `probes/`, then `hmn`, so the proof and the measurements
share one process tree. The command:

```
cd <worktree> && bash <<'EOF' 2>&1 | tee "$TMPDIR/cc_sandbox_run.txt"
P=__reports__/field_check_v0213/evidence/probes
PLAIN=<scratchpad>/plain_probe   # clang -O1 probes/appsandbox/appsandbox_probe.c, no plist
echo "claude-code: $(claude --version)"
sandbox-exec -p '(version 1)(allow default)' /usr/bin/true; echo "nested sandbox-exec rc=$?"
env | grep -i -E '^[A-Za-z_]*sandbox[A-Za-z_]*=' | sed 's/^/envsandbox: /'
python3 $P/sandbox_probe.py
sleep 300 & SL=$!
PS_OUT=$(ps -axo pid=,user=,comm= 2>&1); echo "ps(1) rc=$? lines=$(printf '%s\n' "$PS_OUT" | wc -l | tr -d ' ')"
WS=$(printf '%s\n' "$PS_OUT" | awk '$3 ~ /\/WindowServer$/ {print $1; exit}')
APP=$(printf '%s\n' "$PS_OUT" | awk '$3 ~ /\/Finder\.app\/Contents\/MacOS\/Finder$/ {print $1; exit}')
echo "pids: WindowServer=${WS:-none} Finder=${APP:-none} shell=$$ sleep=$SL"
python3 $P/sandbox_sysctl_probe.py 1 ${WS:-1} ${APP:-1} $$ $SL
python3 $P/kern_proc_pid_probe.py
$PLAIN
$PWD/target/release/hmn --version
$PWD/target/release/hmn ps; echo "ps rc=$?"
$PWD/target/release/hmn ps --device 0; echo "rc=$?"
$PWD/target/release/hmn watch 0 --duration 2s --interval 1s; echo "rc=$?"
kill $SL
EOF
```

Output, verbatim (stdout and stderr merged):

```
claude-code: 2.1.273 (Claude Code)
sandbox-exec: sandbox_apply: Operation not permitted
nested sandbox-exec rc=71
envsandbox: SANDBOX_RUNTIME=1
responsible pid 31866
pid 393: ledger rc=0 errno=0  proc_pidpath ret=21 errno=0
pid 1: ledger rc=0 errno=0  proc_pidpath ret=13 errno=0
pid 2561: ledger rc=-1 errno=ESRCH  proc_pidpath ret=0 errno=ESRCH
pid 33066: ledger rc=0 errno=0  proc_pidpath ret=39 errno=0
sandboxed: 1
ps(1) rc=126 lines=1
pids: WindowServer=none Finder=none shell=33059 sleep=33068
ledger 1 0 0
ledger 1 0 0
ledger 1 0 0
ledger 33059 0 0
ledger 33068 0 0
sysctl kern.proc.all size probe 0 None 722520
fill 0 None kinfo_proc count 1110
kern.proc.pid 0 rc 0 None len 648 b'kernel_task'
kern.proc.pid 99998 rc 0 None len 0 b''
kern.proc.pid 393 rc 0 None len 648 b'sandboxd'
proc_listpids size probe: 4508 errno=0
sysctl KERN_PROC_ALL size probe: rc=0 errno=0 bytes=721224
sysctl KERN_PROC_ALL fill: rc=0 records=1108
KERN_PROC_PID 0: rc=0 len=648 comm=kernel_task
  proc_pidpath: 0 No such process
  ledger: rc=0 ok
KERN_PROC_PID 1: rc=0 len=648 comm=launchd
  proc_pidpath: 13 ok
  ledger: rc=0 ok
KERN_PROC_PID 393: rc=0 len=648 comm=sandboxd
  proc_pidpath: 21 ok
  ledger: rc=0 ok
ledger self: rc=0 ok
hmn 0.2.13
PID   NAME                  VRAM     SHARED  DEVICE        SPILL
2595  Claude Helper         244 MiB  0 MiB   Apple M3 Pro  ?    
399   WindowServer          164 MiB  0 MiB   Apple M3 Pro  ?    
781   Spotlight             5 MiB    0 MiB   Apple M3 Pro  ?    
402   loginwindow           3 MiB    0 MiB   Apple M3 Pro  ?    
762   NotificationCenter    2 MiB    0 MiB   Apple M3 Pro  ?    
662   ControlCenter         1 MiB    0 MiB   Apple M3 Pro  ?    
773   mediaanalysisd        0 MiB    0 MiB   Apple M3 Pro  ?    
770   com.apple.dock.extra  0 MiB    0 MiB   Apple M3 Pro  ?    
717   iconservicesagent     0 MiB    0 MiB   Apple M3 Pro  ?    
680   Finder                0 MiB    0 MiB   Apple M3 Pro  ?    
897   com.apple.WebKit.GPU  0 MiB    0 MiB   Apple M3 Pro  ?    
hmn: 11 GPU processes found (423 MiB committed total).
ps rc=0
PID   NAME                  VRAM     SHARED  DEVICE        SPILL
2595  Claude Helper         244 MiB  0 MiB   Apple M3 Pro  ?    
399   WindowServer          164 MiB  0 MiB   Apple M3 Pro  ?    
781   Spotlight             5 MiB    0 MiB   Apple M3 Pro  ?    
402   loginwindow           3 MiB    0 MiB   Apple M3 Pro  ?    
762   NotificationCenter    2 MiB    0 MiB   Apple M3 Pro  ?    
662   ControlCenter         1 MiB    0 MiB   Apple M3 Pro  ?    
773   mediaanalysisd        0 MiB    0 MiB   Apple M3 Pro  ?    
770   com.apple.dock.extra  0 MiB    0 MiB   Apple M3 Pro  ?    
717   iconservicesagent     0 MiB    0 MiB   Apple M3 Pro  ?    
680   Finder                0 MiB    0 MiB   Apple M3 Pro  ?    
897   com.apple.WebKit.GPU  0 MiB    0 MiB   Apple M3 Pro  ?    
hmn: 11 GPU processes found matching device=0 (423 MiB committed total).
rc=0
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
hmn watch: pid=0 names no running process; its rows will read 0 MiB
+0.0s     0        ?             0 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     0        ?             0 MiB      +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID  NAME  BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    0    ?     0 MiB            0 MiB        0 MiB            0 MiB        ?    
rc=0
```

## Proof that the run was sandboxed

| Proof | Unsandboxed (same day, same machine) | This run |
|---|---|---|
| nested `sandbox-exec` | `rc=0` | `rc=71`, `sandbox_apply: Operation not permitted` |
| `sandbox_check` on the probe itself | `0` | `1` |
| a variable named for the sandbox | none | `SANDBOX_RUNTIME=1` |

## Mapping to the *When it bites* columns

Only PIDs live this boot are read: 0 (`kernel_task`), 1 (`launchd`), 393 (`sandboxd`), and the
probes' own. The probes' hardcoded PID 2561 is gone this boot, so its `ESRCH` is not a denial.

| Column | Lines | Reading |
|---|---|---|
| `proc_listpids` | `proc_listpids size probe: 4508 errno=0` | ok |
| others' `ledger` | PIDs 0, 1 and 393: `ledger rc=0`, `ledger: rc=0 ok`; `hmn ps` read WindowServer (399) and Finder (680) | ok |
| `proc_pidpath` | PID 1 `ret=13`, PID 393 `ret=21`, `errno=0` | ok |
| `sysctl kern.proc` | `KERN_PROC_ALL fill: rc=0 records=1108`; `KERN_PROC_PID` 0, 1, 393 `rc=0`, `comm=kernel_task` | ok (1108 processes, `kernel_task` named) |
| `hmn` 0.2.13 | `ps`: 11 listed with VRAM, rc 0; `ps --device 0`: the same 11, rc 0; `watch 0`: `pid=0 names no running process`, rc 0 | correct |

**Verdict:** Claude Code's Bash sandbox refuses none of the calls `hmn` makes, and `hmn` 0.2.13 is
correct there, as under Codex's policy; unlike Codex's policy, `kern.proc.all` is allowed too.

## Findings

- **A case no scope item covers: `hmn` is correct.** This agent sandbox does not deny
  `process-info`, so the silent zero of the report's profile does not occur here. For the five
  columns it behaves like an unsandboxed shell.
- **`ps(1)` could not run** (`ps(1) rc=126`; a separate sandboxed call printed
  `operation not permitted: ps`). `/bin/ps` is setuid root. So the probe's WindowServer and Finder
  arguments fell back to PID 1, and the three `ledger 1 0 0` lines are all `launchd`. `hmn`, which
  reads through libproc, listed WindowServer and Finder with their bytes.
- **11 rows, where the unsandboxed run of the same day listed 12.** The missing row is `hmn`'s own,
  which is listed at 0 MiB when present. `ledger self: rc=0 ok`, so it is not a denial; why it was
  absent this time was not established.
