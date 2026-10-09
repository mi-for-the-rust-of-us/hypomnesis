// SPDX-License-Identifier: MIT OR Apache-2.0

//! macOS smoke test: exercises the Metal-backed surfaces of the public
//! API (`process_rss`, `device_count`, `device_info`, `process_gpu_info`,
//! `gpu_processes`, `gpu_process_listing`, `Snapshot::now`) and asserts the
//! contract values are sane on an Apple Silicon host.
//!
//! The whole file is gated on `target_os = "macos"`; on Windows and
//! Linux the file compiles to zero tests. The module-level `//!` doc
//! is placed **above** the `#![cfg]` attribute so the integration-test
//! crate retains documentation even when the body is cfg'd-out (the
//! `missing_docs` lint would otherwise fire on non-macOS builds where
//! the crate appears empty).
//!
//! Tests 1, 2, 5, 6 and 7 run unconditionally on any macOS host (test 7
//! prints a skip line where `device_count` is not 1). Tests 3 and 4
//! are `#[ignore]`-gated because they require Apple Silicon hardware
//! with a real Metal device — they would fail on Intel Macs (where the
//! Metal backend returns `None` and the dispatcher falls through to
//! `NoGpuSource`) or on hosted runners without a usable GPU. Tests 8 and 9
//! are `#[ignore]`d too: they apply Seatbelt profiles with
//! `/usr/bin/sandbox-exec`, which needs an unsandboxed parent, and fail
//! rather than skip where they cannot. Run them locally on Apple Silicon
//! with `cargo test -- --ignored`.

#![cfg(target_os = "macos")]

use hypomnesis::{GpuQuerySource, HypomnesisError, Snapshot};

#[test]
#[allow(clippy::expect_used)] // process_rss should never fail on a running test process
fn process_rss_returns_positive_on_macos() {
    let rss = hypomnesis::process_rss().expect("process_rss failed on a running macOS process");
    assert!(
        rss > 1_000_000,
        "expected process_rss > 1 MB on macOS, got {rss}"
    );
}

#[test]
fn device_count_is_one_on_apple_silicon() {
    // Apple Silicon exposes a single Metal device; Intel Macs (and any
    // host where the Metal backend cannot enumerate) fall through to
    // NoGpuSource. Anything else is unexpected.
    match hypomnesis::device_count() {
        Ok(count) => assert_eq!(
            count, 1,
            "expected device_count == 1 on Apple Silicon, got {count}"
        ),
        Err(e) => assert!(
            matches!(e, HypomnesisError::NoGpuSource),
            "unexpected error from device_count(): {e:?}"
        ),
    }
}

#[test]
#[ignore = "requires Apple Silicon with a usable Metal device"]
fn device_info_reports_apple_brand() {
    match hypomnesis::device_info(0) {
        Ok(info) => {
            let name = info.name.as_deref().unwrap_or("");
            assert!(
                name.contains("Apple"),
                "expected device name to contain \"Apple\", got {name:?}"
            );
            assert!(
                info.total_bytes >= 8 << 30,
                "expected total_bytes >= 8 GiB on Apple Silicon, got {}",
                info.total_bytes
            );
        }
        Err(e) => {
            eprintln!("device_info(0) unavailable on this host: {e:?}");
        }
    }
}

#[test]
#[ignore = "requires Apple Silicon with a usable Metal device"]
#[allow(clippy::panic, clippy::indexing_slicing)] // tests are allowed to panic; v[i] is bounded by step_by(4096)
fn process_gpu_info_returns_metal_source() {
    // Residency-touch dance: allocate 256 MiB and touch one byte per
    // 4 KiB page so the kernel ledger reports a non-trivial
    // graphics_footprint. The cast on the page index is bounded by
    // v.len() (256 MiB / 4 KiB = 65_536 pages), well within u8 range
    // after the & 0xff mask.
    let mut v = vec![0u8; 256 << 20];
    for i in (0..v.len()).step_by(4096) {
        // CAST: usize → u8, masked with 0xff so the truncation is intentional.
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
        let byte = (i & 0xff) as u8;
        // INDEX: i ranges over step_by(4096) of (0..v.len()), so v[i] is always in-bounds.
        v[i] = byte;
    }

    match hypomnesis::process_gpu_info(0) {
        Ok(info) => {
            assert_eq!(
                info.source,
                GpuQuerySource::Metal,
                "expected GpuQuerySource::Metal on macOS, got {:?}",
                info.source
            );
            assert!(
                info.is_per_process,
                "expected is_per_process == true for the Metal backend"
            );
        }
        Err(e) => panic!("process_gpu_info(0) failed on macOS: {e:?}"),
    }

    // Keep the buffer alive past the query so the touched pages stay
    // resident at sample time.
    drop(v);
}

#[test]
fn gpu_processes_returns_metal_rows_for_self() {
    // The leaf's success-gate asks us to look for `pid == std::process::id()`
    // in the returned rows. Empirically a vanilla `cargo test` binary holds
    // no Metal device context and so produces no `graphics_footprint` entry
    // in the kernel ledger — `gpu_processes(0)` returns plenty of other
    // PIDs (WindowServer, Safari, etc.) but never ours. We therefore
    // accept these outcomes: (a) self is present (binary with Metal
    // residency), (b) Ok with self absent but every row uses the Metal
    // source (parity with `tests/smoke.rs::gpu_processes_returns_result_or_no_gpu_source`),
    // (c) Err(NoGpuSource) on a non-GPU host, (d) Err(ProcessListDenied)
    // where a sandbox refuses every process but the caller's.
    match hypomnesis::gpu_processes(0) {
        Ok(rows) => {
            let self_pid = std::process::id();
            let saw_self = rows.iter().any(|r| r.pid == self_pid);
            if !saw_self {
                eprintln!(
                    "gpu_processes(0) returned {} rows but the test PID {self_pid} is absent; \
                     this is expected for a vanilla test binary that holds no Metal device",
                    rows.len()
                );
            }
            for row in &rows {
                assert!(row.pid > 0, "expected positive PID, got {}", row.pid);
                assert_eq!(
                    row.source,
                    GpuQuerySource::Metal,
                    "expected GpuQuerySource::Metal for a macOS row, got {:?}",
                    row.source
                );
            }
        }
        Err(e) => {
            assert!(
                matches!(
                    e,
                    HypomnesisError::NoGpuSource | HypomnesisError::ProcessListDenied { .. }
                ),
                "unexpected error from gpu_processes(0): {e:?}"
            );
        }
    }
}

#[test]
#[allow(clippy::expect_used)] // Snapshot::now's RAM query should never fail; GPU should be present on macOS
fn snapshot_now_includes_gpu_on_macos() {
    let snap = Snapshot::now(0).expect("Snapshot::now's RAM query should succeed on macOS");
    assert!(
        snap.gpu.is_some(),
        "expected snap.gpu to be Some on macOS (Metal backend should populate it)"
    );
}

#[test]
fn device_index_past_count_is_out_of_range_on_apple_silicon() {
    // Apple Silicon reports one Metal device, so index 1 is past the end.
    // An Intel Mac (or any host without a count source) skips. The count
    // comes from `sysctl machdep.cpu.brand_string`, which a sandbox that
    // denies `process-info*` still allows, so this runs sandboxed too.
    let count = hypomnesis::device_count();
    let Ok(1) = count else {
        eprintln!("device_count() is {count:?} on this host, not Ok(1): skipping");
        return;
    };
    for (name, error) in [
        ("device_info", hypomnesis::device_info(1).err()),
        ("process_gpu_info", hypomnesis::process_gpu_info(1).err()),
        ("gpu_processes", hypomnesis::gpu_processes(1).err()),
    ] {
        assert!(
            matches!(
                error,
                Some(HypomnesisError::DeviceIndexOutOfRange { index: 1, count: 1 })
            ),
            "expected DeviceIndexOutOfRange {{ index: 1, count: 1 }} from {name}(1), got {error:?}"
        );
    }
}

/// The `process_exists` probes of `process_exists_under_sandbox_profiles`:
/// PID 0 (`kernel_task`), PID 1 (`launchd`), a dead PID (`i32::MAX`, a
/// valid `pid_t` no process holds) and the calling process.
fn process_exists_probes() -> [(&'static str, Option<bool>); 4] {
    [
        ("zero", hypomnesis::process_exists(0)),
        ("launchd", hypomnesis::process_exists(1)),
        ("dead", hypomnesis::process_exists(2_147_483_647)),
        ("self", hypomnesis::process_exists(std::process::id())),
    ]
}

/// The probe table as the `PE <label>=<result:?>` lines a child prints.
fn process_exists_lines(table: &[(&str, Option<bool>)]) -> Vec<String> {
    table
        .iter()
        .map(|(label, result)| format!("PE {label}={result:?}"))
        .collect()
}

/// A Seatbelt profile no caller runs under. macOS lets a sandboxed process
/// re-apply the very profile it runs under (measured: `(allow default)`
/// inside `(allow default)` exits `0`), but no other one, so only a profile
/// unique to this file tells "inside a sandbox" from "outside" everywhere.
const PROBE_PROFILE: &str =
    "(version 1)(allow default)(deny file-write* (literal \"/hmn-macos-smoke-probe\"))";

/// Whether `/usr/bin/sandbox-exec` can apply `PROBE_PROFILE` here: true
/// outside any sandbox; false inside one, where it exits `71`
/// (`sandbox_apply: Operation not permitted`).
#[allow(clippy::expect_used)] // test-only
fn sandbox_can_apply_a_profile() -> bool {
    std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", PROBE_PROFILE, "/usr/bin/true"])
        .output()
        .expect("spawn /usr/bin/sandbox-exec")
        .status
        .success()
}

#[test]
#[ignore = "requires an unsandboxed parent and /usr/bin/sandbox-exec (applies Seatbelt profiles P and Q)"]
#[allow(clippy::expect_used)] // test-only
fn process_exists_under_sandbox_profiles() {
    // No skip in either role: this test is `#[ignore]`d and run by hand on
    // hardware, so a run that cannot exercise profiles P and Q must fail
    // rather than report `ok` for a table it never checked.

    // Child role: prove it runs inside a sandbox, then print the table and
    // let the parent judge it. A top-level run that inherited
    // `HMN_PE_CHILD` fails here instead of asserting nothing.
    if std::env::var_os("HMN_PE_CHILD").is_some() {
        assert!(
            !sandbox_can_apply_a_profile(),
            "HMN_PE_CHILD set outside a sandbox"
        );
        for line in process_exists_lines(&process_exists_probes()) {
            println!("{line}");
        }
        return;
    }

    // Parent role: a sandbox cannot nest another one, so check before
    // asserting anything that profiles can be applied from here.
    assert!(
        sandbox_can_apply_a_profile(),
        "sandbox-exec cannot apply a profile here (already inside a sandbox?), \
         so profiles P and Q cannot be exercised"
    );

    let open = [
        ("zero", Some(true)),
        ("launchd", Some(true)),
        ("dead", Some(false)),
        ("self", Some(true)),
    ];
    assert_eq!(process_exists_probes(), open, "unsandboxed");

    // P is the issue #3 report profile: libproc refused for every PID
    // but our own. Q adds a `kern.proc` denial. Measured while drafting:
    // the `kern.proc` line alone refuses only dead PIDs (live ones still
    // answer); only together with P's `process-info` denial does it
    // refuse live ones too. So Q must keep P's lines, and Q is not P.
    let p = "(version 1)(allow default)(deny process-info*)(allow process-info* (target self))";
    let q = format!("{p}(deny sysctl-read (sysctl-name-prefix \"kern.proc\"))");
    let refused = [
        ("zero", None),
        ("launchd", None),
        ("dead", Some(false)),
        ("self", Some(true)),
    ];

    let exe = std::env::current_exe().expect("current_exe of the test binary");
    for (name, profile, expected) in [("P", p, open), ("Q", q.as_str(), refused)] {
        let out = std::process::Command::new("/usr/bin/sandbox-exec")
            .args(["-p", profile])
            .arg(&exe)
            .args([
                "--ignored",
                "--exact",
                "process_exists_under_sandbox_profiles",
                "--nocapture",
            ])
            .env("HMN_PE_CHILD", "1")
            .output()
            .expect("spawn /usr/bin/sandbox-exec");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let got: Vec<&str> = stdout.lines().filter(|l| l.starts_with("PE ")).collect();
        assert_eq!(
            got,
            process_exists_lines(&expected),
            "profile {name}: child stdout {stdout:?}, stderr {stderr:?}"
        );
    }
}

/// How `gpu_process_listing(0)` answers this process, as the one label the
/// child of `gpu_process_listing_under_sandbox_profiles` prints.
fn listing_label() -> String {
    match hypomnesis::gpu_process_listing(0) {
        Ok(listing) if listing.denied_pids.contains(&std::process::id()) => {
            // BORROW: `to_owned` builds the label's `String` from a literal.
            "caller_denied".to_owned()
        }
        // BORROW: `to_owned` builds the label's `String` from a literal.
        Ok(listing) if listing.denied_pids.is_empty() => "open".to_owned(),
        Ok(_) => "partial".to_owned(),
        // BORROW: `to_owned` builds the label's `String` from a literal.
        Err(HypomnesisError::ProcessListDenied { .. }) => "denied".to_owned(),
        Err(other) => format!("{other:?}"),
    }
}

#[test]
#[ignore = "requires an unsandboxed parent and /usr/bin/sandbox-exec (applies Seatbelt profiles S, S0 and L)"]
#[allow(clippy::expect_used)] // test-only
fn gpu_process_listing_under_sandbox_profiles() {
    // No skip in either role, as in `process_exists_under_sandbox_profiles`.
    // Child role: prove it runs inside a sandbox, then print the label.
    if std::env::var_os("HMN_GPL_CHILD").is_some() {
        assert!(
            !sandbox_can_apply_a_profile(),
            "HMN_GPL_CHILD set outside a sandbox"
        );
        println!("LISTING {}", listing_label());
        return;
    }
    assert!(
        sandbox_can_apply_a_profile(),
        "sandbox-exec cannot apply a profile here (already inside a sandbox?), \
         so profiles S, S0 and L cannot be exercised"
    );
    assert_eq!(listing_label(), "open", "unsandboxed");

    // S0 denies `process-info*` except for the caller and its sandbox; S is
    // S0 with a resident `bash`, a sibling the caller can read, so the list
    // is partial. L denies the ledger read of every process, the caller's too.
    let s0 = "(version 1)(allow default)(deny process-info*)(allow process-info* (target self))(allow process-info* (target same-sandbox))";
    let l = "(version 1)(allow default)(deny process-info-ledger)";
    let exe = std::env::current_exe().expect("current_exe of the test binary");
    for (name, profile, under_bash, label) in [
        ("S", s0, true, "partial"),
        ("S0", s0, false, "denied"),
        ("L", l, false, "denied"),
    ] {
        let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
        command.args(["-p", profile]);
        if under_bash {
            command.args(["/bin/bash", "-c", "\"$@\"; rc=$?; exit $rc", "_"]);
        }
        let out = command
            .arg(&exe)
            .args([
                "--ignored",
                "--exact",
                "gpu_process_listing_under_sandbox_profiles",
                "--nocapture",
            ])
            .env("HMN_GPL_CHILD", "1")
            .output()
            .expect("spawn /usr/bin/sandbox-exec");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let got: Vec<&str> = stdout
            .lines()
            .filter(|l| l.starts_with("LISTING "))
            .collect();
        assert_eq!(
            got,
            [format!("LISTING {label}")],
            "profile {name}: child stdout {stdout:?}, stderr {stderr:?}"
        );
    }
}
