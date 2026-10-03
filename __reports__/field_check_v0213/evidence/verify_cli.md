# Adversarial verification of checks 2-8 (hmn 0.2.13, macOS 26.6.2, uid 501, non-root)

Probes (throwaway, outside repo): scratchpad/verify_pe/{p.py,l.py,rs/}

## 1. Check 3 premise -- QUALIFIED (verdict "PASS" stands, but the EPERM path was NOT exercised)

When process_exists is consulted: src/bin/hmn/watch.rs:964-977 (missing_pid_notices), called at :1175.
Only for explicit PIDs NOT in the first sample (`first_rows`, i.e. gpu_processes(0) = nonzero graphics_footprint
rows) AND exists(pid)==Some(false). A listed PID is never asked.

Probe results (python ctypes proc_pidpath + crate API process_exists + ledger syscall 373):
| pid | proc_pidpath ret/errno | ledger(ENTRY_INFO_V2) rc/errno | in gpu_processes | process_exists | consulted by watch? |
| 393 WindowServer (_windowserver) | 86 / 0 | 0 / 0 (balance nonzero, 389-515 MiB, matches hmn ps) | yes | Some(true) | NO (listed) |
| 1 launchd (root) | 13 / 0 | 0 / 0 (balance 0) | no | Some(true) | YES -> Some(true) -> no warning |
| 332 configd (root) | 20 / 0 | 0 / 0 (balance 0) | no | Some(true) | YES -> Some(true) -> no warning |
| 99998 | 0 / ESRCH(3) | -1 / ESRCH | no | Some(false) | YES -> warning |
| 999999 | 0 / ESRCH(3) | -1 / ESRCH | no | Some(false) | YES -> warning |
| self | >0 / 0 | 0/0 | no | Some(true) | - |
| 0 (kernel_task) | 0 / ESRCH | - | no | Some(false) | -> spurious warning (see 1c) |

Whole-system scan: ledger read for all 253 cross-user PIDs (incl. root) returned rc=0 errno=0 (zero EPERM);
proc_pidpath succeeded for 935/936 PIDs (the single failure was the short-lived `ps` itself, ESRCH).
So on this OS/uid neither ledger nor proc_pidpath ever yields EPERM. The `None`/EPERM branch of
metal.rs:710-731 is unreachable here. Check 3 therefore verifies only "no false ESRCH for live cross-user
processes" (true: launchd and configd got Some(true)); the issue's premise (ledger EPERM => process_exists asked)
is empirically false on macOS 26.6.2: cross-user PIDs with a nonzero balance are LISTED (WindowServer), those with
balance 0 are simply absent from the listing (not EPERM-skipped), and are then asked and found alive.
1a. Implementer's side note "root PIDs read ?/0 MiB (the ledger/name lookups fail for them)" is wrong: the
    ledger read succeeds (balance 0); name is "?" because the row is synthesized for an unlisted PID.
1b. Help text main.rs:169-171 ("cross-user PIDs are silently skipped -- ledger returns EPERM"), also ps.rs:508-509,
    main.rs:196, metal.rs:5-11/610-613/682-684, gpu/mod.rs:374-377: INACCURATE on macOS 26.6.2 non-root
    (WindowServer, user _windowserver, is listed unprivileged; 0 EPERM across 253 cross-user PIDs). May hold on older
    macOS or sandboxed callers; unverified there.
1c. Edge: proc_pidpath(0) -> ESRCH even though kernel_task (pid 0) exists, so `hmn watch 0` prints
    "pid=0 names no running process" (reproduced). Minor false positive.

## 2. Check 2 -- CONFIRMED
999999: proc_pidpath ret 0, errno 3 (ESRCH) -> Some(false) -> warning. (u32->i32 conversion fits; PID_MAX 99999 is irrelevant to the
syscall, which just reports no such pid.) 99998: ESRCH from proc_pidpath and ledger at my probe time too (dead, as os.kill said).
Residual race only (PID could be allocated between probes); not an issue.

## 3. stderr AND stdout inspection -- CONFIRMED
Read raw .err/.out/.rc for c2_watch, c2_watch_realonly, c3_dead, c3_ws, c3_launchd, c3_root2: warning
"names no running process" appears only in .err of c2_watch (999999 only) and c3_dead (99998); count in every .out = 0;
no warning for 2561/393/1/332; rc=0 all. run.sh captures streams separately.

## 4. Check 4 -- CONFIRMED
python json per line of c4_json.out: kinds [start, sample, sample, summary]; summary has key `measurable` == False (`is False`),
key `spilling_at_attach` present == None; raw line contains `"measurable":false` and `"spilling_at_attach":null`.
samples: spilling=null, paged=null; per_pid paged=null. "no spill observed" occurs 0 times across all .out files.
Side note: summary also has `"spilled":false` alongside measurable:false (consumers must gate on measurable).

## 5. Check 5 -- CONFIRMED
Recomputed with display width (east_asian_width; Delta = 1 col) on c5_top, c5_longname, c5_long (live + per-PID summary tables):
0 misaligned cells. Header is 97 chars/97 cols but 99 bytes (2 Delta x 2 bytes) -- padding is by chars, not bytes, as required.
Caveat (untested, out of scope): char-count padding would misalign wide (CJK/emoji) process names.

## 6. Check 7 -- QUALIFIED: implementer's finding is real and has a concrete root cause
Output reproduced for `ps --device 1`; also `watch --device 1` (same text, rc 2).
Crate API on macOS: device_count()=Ok(1) but device_info(1)/gpu_processes(1)/process_gpu_info(1) = Err(NoGpuSource).
Message: src/error.rs:77-80 (HypomnesisError::NoGpuSource). Path: metal::list_compute_processes returns None for index!=0
(metal.rs:620), falls through all backends to `bounds_check(device_index)?; Err(NoGpuSource)` (gpu/mod.rs:456-457).
bounds_check (gpu/mod.rs:604-628) consults only nvml::device_count and dxgi::device_count; it has NO metal arm although
public device_count() (gpu/mod.rs:46-50) uses metal::device_count (metal.rs:557, Some(1)). So on macOS the intended
DeviceIndexOutOfRange{index:1,count:1} ("device index 1 out of range (have 1 devices)") is never produced.
Worth reporting: wording bug whose cause is a missing metal arm in bounds_check; exit code/prefix are fine. (device_info docs
at mod.rs:96-101 only promise OutOfRange for NVML/DXGI counts, so it is a doc-consistent gap, not a regression.)
