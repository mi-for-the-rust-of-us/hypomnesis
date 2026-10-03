# Field check v0.2.13 — Notice (v0): `spilled` should be `null` when not measurable

Date: 2026-10-01

---
type: notice
title: Make `spilled` null when spill is not measurable
tag: improvement
effort: S
reversibility: wire contract; one-way once shipped
evidence: verified
spotted-during: planning v0.2.14 from the issue #3 field check (F6, the JSON half)
date: 2026-10-01
status: deferred
---

## TL;DR
- `"spilled"` becomes `null`, not `false`, whenever `"measurable"` is `false`.
- It is the last spill field that collapses "not measured" into "no".
- It changes a bool to bool-or-null: a minor bump by ROADMAP Principle 2.

## Context
The v0.2.11 honesty contract made `spilling`, `paged` and `spilling_at_attach` `null` when spill
is unmeasurable, and v0.2.13 removed the same collapse from `hmn watch`'s text summary.
`write_spill_report_fields` (`src/bin/hmn/spill.rs`) still writes `"spilled":false` on every Linux
and macOS run. It is the one wire spelling behind both `hmn spill --json` and the
`hmn watch --json` summary.

## Scope Delta
- `write_spill_report_fields`: write `null` for `spilled` when `measurable` is `false`, routed
  through `json_value_or_null` like its siblings.
- Both commands change together, since they share the writer. The key order in
  `SPILL_REPORT_JSON_KEYS` is unchanged.
- The unit tests that pin `"measurable":false,"spilled":false` (`spill.rs`, `watch.rs`) are
  updated.
- `CHANGELOG.md` gets a **Changed** entry with a compatibility note: consumers that deserialize a
  strict bool, such as serde `bool` or `j["spilled"] == False`, must accept `null`.
- The README/FAQ CI gate `jq -e '.measurable and (.spilled | not)'` keeps working unchanged,
  because `null | not` is `true`. The FAQ sentence "check `measurable` before trusting
  `spilled: false`" is reworded.
- The library is untouched: `SpillReport::spilled()` stays `bool`. This is a wire-format change
  only.

## Accept / Decline

| | Accept | Decline |
|---|---|---|
| **Benefit** | Every spill field obeys one rule: `null`/`?` means not measured, never "no". A consumer that reads only `spilled` can no longer be misled. | 0.2.x consumers keep a stable bool. The `measurable` gate the README and FAQ already teach keeps working and is documented. |
| **Cost** | It breaks strict-bool consumers, so it needs the v0.3.0 minor bump and a compatibility note. | `spilled` stays the one field that reads `false` for "not measured", and the FAQ caveat has to stay. |

## If Accepted — Next Step
Land it in v0.3.0's roadmap as a `Changed` item, next to the `ROADMAP.md` "Speculative: v0.3.0"
entry added in v0.2.14.

## If Declined — Next Step
Keep the FAQ's "check `measurable` first" caveat and archive this notice. Revisit only if a
consumer reports being misled by `spilled: false`.
