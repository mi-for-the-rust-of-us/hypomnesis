# field_check_v0213

Field check of hypomnesis v0.2.13 on an Apple M3 Pro, for issue #3.

## Round 00
- [00-findings_v0.md](00-findings_v0.md) — superseded by v1. All 8 checks passed. It found four
  problems (F1–F4), but its F1 ("no `EPERM` on this OS") holds only for unsandboxed callers.

## Round 01
- [01-findings_v1.md](01-findings_v1.md) — **latest**. It revises F1: whether `EPERM` happens
  depends on the caller's sandbox, not on who owns the process. It adds F5 (a sandboxed `hmn ps`
  reports "0 GPU processes found" and exits 0) and F6 (`n/a` instead of `?` where spill cannot
  happen; `spilled:false`).

## Round 02
- [02-notice_spilled_null_v0.md](02-notice_spilled_null_v0.md) — **deferred to v0.3.0**. The JSON
  half of F6: `spilled` should be `null`, not `false`, when spill is not measurable. That is a
  wire type change, so under ROADMAP Principle 2 it waits for a minor release.

## Evidence
- [evidence/](evidence/) — evidence summaries for checks 1–8, the verifier's report, and the
  `ctypes` and Rust probes. Raw process listings from this machine are not committed.

## Status
Evidence collected and verified. The write-up is in
`docs/dogfooding-feedbacks/dogfooding-macos-sandbox-eperm-and-device-bounds.md`. The fix plan is
`docs/roadmap-v0.2.14.md`. Checking the findings against the code overturned one claim: under the
report's sandbox profile, `hmn ps` fails because `proc_listpids` is refused, not because the
per-PID reads are skipped. The roadmap corrects the dogfooding report before the issue comment is
posted.
