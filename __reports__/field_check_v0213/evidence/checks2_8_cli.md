# Field check v0.2.13 (macOS, Apple M3 Pro): checks 2-8

## Environment

```
git: cf5ada0
user: hacker
ProductName:		macOS
ProductVersion:		26.6.2
BuildVersion:		25G83
Apple M3 Pro
kern.maxproc: 9000
binary: target/release/hmn (cargo build --release)
hmn 0.2.13
```

All raw outputs: `cli/<label>.{cmd,out,err,rc}`; runner `cli/run.sh` captures stdout, stderr and exit code separately.

## Check 2 — warning for a dead PID only

**Verdict: PASS**

Liveness of 999999 (note: macOS `ps` rejects it outright, since macOS PIDs max out at 99999, so 999999 cannot exist at all):

```
$ ps -p 999999
ps: process id too large: 999999
exit=1
$ python3 os.kill(999999,0) -> ProcessLookupError [Errno 3] No such process
```

Chosen live GUI app (owned by current user `hacker`):

```
  PID USER   COMM
 2561 hacker /System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app/Contents/MacOS/Safari
```

**Command**: `target/release/hmn watch 999999 2561 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     999999   ?             0 MiB      +0 B       0 MiB      +0 B       ?    
+0.0s     2561     Safari        2 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     999999   ?             0 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     2561     Safari        2 MiB      +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID     NAME    BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    999999  ?       0 MiB            0 MiB        0 MiB            0 MiB        ?    
                    2561    Safari  2 MiB            2 MiB        0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 2 PID(s)
hmn watch: pid=999999 names no running process; its rows will read 0 MiB
```

Warning names 999999 only; none for 2561 (Safari). Control run with the real PID alone (no warning on stderr):

**Command**: `target/release/hmn watch 2561 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     2561     Safari        2 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     2561     Safari        2 MiB      +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID   NAME    BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    2561  Safari  2 MiB            2 MiB        0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
```

Supplementary: a PID in the valid macOS range that is provably dead (os.kill(99998,0) -> ESRCH):

**Command**: `target/release/hmn watch 99998 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     99998    ?             0 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     99998    ?             0 MiB      +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID    NAME  BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    99998  ?     0 MiB            0 MiB        0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
hmn watch: pid=99998 names no running process; its rows will read 0 MiB
```

## Check 3 — no warning for processes owned by other users / root

**Verdict: PASS** (no warning in any case; proc_pidpath did not return ESRCH for any live process)

Ownership proof (current user is `hacker`; WindowServer is owned by `_windowserver`, NOT the current user; launchd is root):

```
  PID USER          COMM
    1 root          /sbin/launchd
  393 _windowserver /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer

current user: hacker
```

### 3a WindowServer (_windowserver, cross-user)

**Command**: `target/release/hmn watch 393 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     393      WindowServer  264 MiB    +0 B       0 MiB      +0 B       ?    
+1.0s     393      WindowServer  264 MiB    +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID  NAME          BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    393  WindowServer  264 MiB          264 MiB      0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
```

### 3b launchd PID 1 (root)

**Command**: `target/release/hmn watch 1 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     1        ?             0 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     1        ?             0 MiB      +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID  NAME  BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    1    ?     0 MiB            0 MiB        0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
```

### 3c configd (root daemon)

```
  PID USER COMM
  332 root /usr/libexec/configd
```

**Command**: `target/release/hmn watch 332 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     332      ?             0 MiB      +0 B       0 MiB      +0 B       ?    
+1.0s     332      ?             0 MiB      +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID  NAME  BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    332  ?     0 MiB            0 MiB        0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
```

Side observation: for cross-user WindowServer the row resolves a name and VRAM (264 MiB), while the root PIDs read `?`/0 MiB (the ledger/name lookups fail for them), yet no 'names no running process' warning is issued, which is the intended behaviour. (`hmn --help` says cross-user PIDs are silently skipped on macOS, but `hmn ps` lists WindowServer and loginwindow unprivileged: possible doc inaccuracy, not part of the checks.)

## Check 4 — spill not measurable on macOS

**Verdict: PASS**

Text mode: closing line is `hmn watch: spill not measurable on this platform; per-PID VRAM below` (see Check 2/3/5 outputs); `no spill observed` never appears; SPILL cells and the summary PAGED column read `?`.

```
'no spill observed' in output: False
not-measurable line present: True
```

`--json`:

**Command**: `target/release/hmn watch 2561 --interval 1s --duration 2s --json`  
**Exit code**: 0

stdout:
```
{"kind":"start","t_ms":0,"wall_clock":"2026-10-01T08:22:49.053Z","hmn_version":"0.2.13","argv":["hmn","watch","2561","--interval","1s","--duration","2s","--json"],"device":0,"device_name":"Apple M3 Pro","interval_ms":1000,"duration_ms":2000,"selection":{"mode":"explicit","pids":[2561],"top":null,"filters":[],"min_bytes":null}}
{"kind":"sample","t_ms":0,"wall_clock":"2026-10-01T08:22:49.053Z","pid":2561,"name":"Safari","used_bytes":2260992,"used_delta_bytes":0,"shared_used_bytes":0,"shared_delta_bytes":0,"spilling":null,"paged":null}
{"kind":"sample","t_ms":1022,"wall_clock":"2026-10-01T08:22:50.076Z","pid":2561,"name":"Safari","used_bytes":2260992,"used_delta_bytes":0,"shared_used_bytes":0,"shared_delta_bytes":0,"spilling":null,"paged":null}
{"kind":"summary","measurable":false,"spilled":false,"observations":0,"baseline_shared_bytes":0,"peak_shared_bytes":0,"peak_dedicated_bytes":0,"dedicated_limit_bytes":0,"total_spill_duration_ms":0,"episodes":[],"spilling_at_attach":null,"per_pid":[{"pid":2561,"name":"Safari","baseline_used_bytes":2260992,"peak_used_bytes":2260992,"baseline_shared_bytes":0,"peak_shared_bytes":0,"paged":null}]}
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 1 PID(s)
```

Parsed programmatically (NDJSON, line by line):

```
kinds: ['start', 'sample', 'sample', 'summary']
summary measurable = False
summary spilling_at_attach = None
summary per_pid paged = [None]
sample spilling/paged = [(None, None), (None, None)]
```

## Check 5 — `watch --top 5` alignment

**Verdict: PASS**

**Command**: `target/release/hmn watch --top 5 --interval 1s --duration 2s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME                         COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     393      WindowServer                 264 MiB    +0 B       0 MiB      +0 B       ?    
+0.0s     396      loginwindow                  129 MiB    +0 B       0 MiB      +0 B       ?    
+0.0s     6092     Slack Helper                 99 MiB     +0 B       0 MiB      +0 B       ?    
+0.0s     2583     com.apple.WebKit.GPU         57 MiB     +0 B       0 MiB      +0 B       ?    
+0.0s     2728     com.apple.WebKit.WebContent  56 MiB     +0 B       0 MiB      +0 B       ?    
+1.0s     393      WindowServer                 264 MiB    +0 B       0 MiB      +0 B       ?    
+1.0s     396      loginwindow                  122 MiB    -6 MiB     0 MiB      +0 B       ?    
+1.0s     6092     Slack Helper                 99 MiB     +0 B       0 MiB      +0 B       ?    
+1.0s     2583     com.apple.WebKit.GPU         57 MiB     +0 B       0 MiB      +0 B       ?    
+1.0s     2728     com.apple.WebKit.WebContent  56 MiB     +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID   NAME                         BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    393   WindowServer                 264 MiB          264 MiB      0 MiB            0 MiB        ?    
                    396   loginwindow                  129 MiB          129 MiB      0 MiB            0 MiB        ?    
                    6092  Slack Helper                 99 MiB           99 MiB       0 MiB            0 MiB        ?    
                    2583  com.apple.WebKit.GPU         57 MiB           57 MiB       0 MiB            0 MiB        ?    
                    2728  com.apple.WebKit.WebContent  56 MiB           56 MiB       0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 5 PID(s) (top 5 by committed)
```

Alignment computed by character offset (a column is aligned if every row has a non-space char at the header's column offset, preceded by a space):

```
{'TIME': 0, 'PID': 10, 'NAME': 19, 'COMMITTED': 48, 'ΔCOMMIT': 59, 'SHARED': 70, 'ΔSHARED': 81, 'SPILL': 92}
header len 97 row lens [97]
misaligned rows: 0
longest name shown: com.apple.WebKit.WebContent 27
```

Longer name than the top-5 happened to show (44 chars; explicit PIDs, since 0-MiB processes do not rank in top 5):

**Command**: `target/release/hmn watch 24955 393 2728 --interval 1s --duration 1s`  
**Exit code**: 0

stdout:
```
TIME      PID      NAME                                          COMMITTED  ΔCOMMIT    SHARED     ΔSHARED    SPILL
+0.0s     24955    com.apple.appkit.xpc.openAndSavePanelService  0 MiB      +0 B       0 MiB      +0 B       ?    
+0.0s     393      WindowServer                                  264 MiB    +0 B       0 MiB      +0 B       ?    
+0.0s     2728     com.apple.WebKit.WebContent                   56 MiB     +0 B       0 MiB      +0 B       ?    
hmn watch: spill not measurable on this platform; per-PID VRAM below
hmn watch: per-PID  PID    NAME                                          BASELINE COMMIT  PEAK COMMIT  BASELINE SHARED  PEAK SHARED  PAGED
                    24955  com.apple.appkit.xpc.openAndSavePanelService  0 MiB            0 MiB        0 MiB            0 MiB        ?    
                    393    WindowServer                                  264 MiB          264 MiB      0 MiB            0 MiB        ?    
                    2728   com.apple.WebKit.WebContent                   56 MiB           56 MiB       0 MiB            0 MiB        ?    
```

stderr:
```
hmn watch: device 0 [Apple M3 Pro], interval 1.0s, watching 3 PID(s)
```

```
{'TIME': 0, 'PID': 10, 'NAME': 19, 'COMMITTED': 65, 'ΔCOMMIT': 76, 'SHARED': 87, 'ΔSHARED': 98, 'SPILL': 109}
misaligned rows: 0
longest: com.apple.appkit.xpc.openAndSavePanelService
```

The NAME column widens to the longest name (header and rows both), in both the live table and the closing per-PID table. Also ran with `--filter` (c5_long.*): aligned.

## Check 6 — `ps --filter` and `--exit-status`

**Verdict: PASS** (pattern `sAfArI` vs real name `Safari`)

### 6a filter, mixed case

**Command**: `target/release/hmn ps --filter sAfArI`  
**Exit code**: 0

stdout:
```
PID   NAME    VRAM   SHARED  DEVICE        SPILL
2561  Safari  2 MiB  0 MiB   Apple M3 Pro  ?    
```

stderr:
```
hmn: 1 GPU process found matching filter="sAfArI" (2 MiB committed total).
```

### 6b `--exit-status`, matching pattern (expect 0)

**Command**: `target/release/hmn ps --filter sAfArI --exit-status`  
**Exit code**: 0

stdout:
```
PID   NAME    VRAM   SHARED  DEVICE        SPILL
2561  Safari  2 MiB  0 MiB   Apple M3 Pro  ?    
```

stderr:
```
hmn: 1 GPU process found matching filter="sAfArI" (2 MiB committed total).
```

### 6c `--exit-status`, non-matching pattern (expect 1)

**Command**: `target/release/hmn ps --filter ZzNoSuchApp --exit-status`  
**Exit code**: 1

stdout:
```
PID  NAME  VRAM  SHARED  DEVICE  SPILL
```

stderr:
```
hmn: 0 GPU processes found matching filter="ZzNoSuchApp".
```

### 6d control: non-matching without `--exit-status` (exit 0, as documented)

**Command**: `target/release/hmn ps --filter ZzNoSuchApp`  
**Exit code**: 0

stdout:
```
PID  NAME  VRAM  SHARED  DEVICE  SPILL
```

stderr:
```
hmn: 0 GPU processes found matching filter="ZzNoSuchApp".
```

## Check 7 — `ps --device 1`

**Verdict: PASS on the stated expectation (exit 2, `hmn: ps failed to query device 1: …`); FINDING on the reason text**

**Command**: `target/release/hmn ps --device 1`  
**Exit code**: 2

stdout:
```
(empty)```

stderr:
```
hmn: ps failed to query device 1: no GPU measurement source available (NVML, DXGI, PDH, and nvidia-smi all failed or are disabled)
```

Control: `ps --device 0` works (exit 0, see c7_dev0.*).

FINDING (cosmetic): the reason after the colon reads `no GPU measurement source available (NVML, DXGI, PDH, and nvidia-smi all failed or are disabled)`. On macOS none of those backends exist and device 0 is served by the macOS backend, so the text is a generic Linux/Windows-flavoured message that misdescribes an out-of-range index. Behaviour (exit 2, prefix) is correct.

## Check 8 — `hmn --help` ordering

**Verdict: PASS**

**Command**: `target/release/hmn --help`  
**Exit code**: 0


```
'Usage: hmn'                        char offset 1408
'Commands:'                         char offset 1440
'Options:'                          char offset 2897
'Limitations (per-platform):'       char offset 3513
Commands: before Limitations: True
```

Full help text is in `cli/c8_help.out` (stderr empty). Commands list (offset 1440) precedes `Limitations (per-platform):` (offset 3513).
