# Check 1: cargo test --all-features (v0.2.13, Apple M3 Pro)

## Environment
```
ProductName:		macOS
ProductVersion:		26.6.2
BuildVersion:		25G83
rustc 1.92.0 (ded5c06cf 2025-12-08)
cargo 1.92.0 (344c4567c 2025-10-21)
arch: arm64
Apple M3 Pro
HEAD: cf5ada0082013d3c07ec1ecdaad3e8bc3abc3762 (cf5ada0)
git status --short: []
```

## Exit code

cargo test --all-features exit=0 (run 1 raw log: check1_raw.log; its 'exit=' echo printed empty because the shell is zsh, where PIPESTATUS is lowercase 'pipestatus'. Re-run under bash, check1_raw_rerun.log: exit=0. Identical pass/fail set in both runs; only parallel output ordering differs.)

## Test results (verbatim, run 1)
```
test result: ok. 83 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 234 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
test result: ok. 0 passed; 0 failed; 9 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

## Named tests
```
test gpu::tests::process_exists_does_not_find_an_impossible_pid ... ok
test gpu::tests::process_exists_finds_this_process ... ok
```

## Other findings

- tests/macos_smoke.rs: 2 tests are #[ignore]d by design ('requires Apple Silicon with a usable Metal device'): device_info_reports_apple_brand, process_gpu_info_returns_metal_source. They were NOT run (not part of check 1 as specified); run with 'cargo test --test macos_smoke -- --ignored' if wanted.
- tests/live_gpu.rs: 9 tests ignored (NVIDIA only), expected. live_pdh, live_watch, live_watch_filter, live_watch_follow_new: 0 tests on macOS (cfg-gated), expected.

## Failures

None.
