// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end exit-code tests for `hmn ps` that hold on any machine, GPU
//! or not — so, unlike the `live_*` tests, they are not `#[ignore]`d. The
//! one exception is macOS-only and `#[ignore]`d: it needs a usable Metal
//! device and applies a Seatbelt profile with `/usr/bin/sandbox-exec`.
//! `#![cfg(feature = "cli")]`: they run the compiled binary through
//! `env!("CARGO_BIN_EXE_hmn")`, which Cargo only defines when the `hmn`
//! target (`required-features = ["cli"]`) is built.

#![cfg(feature = "cli")]

use std::io::Write as _;
use std::process::Command;

/// Run `hmn` with `args`, returning its exit code and stderr.
#[allow(clippy::expect_used)] // test-only
fn hmn(args: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_hmn"))
        .args(args)
        .output()
        .expect("failed to run hmn");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `--device` naming a GPU that cannot be listed exits `2` with the
/// reason, where it used to exit `0` with an empty table — a mistyped
/// index read as an idle card. Index 99 cannot be listed anywhere: past
/// the device count where one is known, and without any GPU source
/// otherwise.
#[test]
fn ps_device_out_of_range_exits_2_with_the_reason() {
    let (code, stderr) = hmn(&["ps", "--device", "99"]);
    assert_eq!(code, Some(2), "stderr: {stderr}");
    // A line, not the whole of stderr: `debug-output` builds trace there too.
    assert!(
        stderr
            .lines()
            .any(|l| l.starts_with("hmn: ps failed to query device 99: ")),
        "stderr: {stderr}"
    );
}

/// Whether `stderr` holds a skipped-device line that carries `text`: `hmn ps`
/// without `--device` names each device whose query failed, ending
/// ` (skipped)`.
fn skipped_device_line(stderr: &str, text: &str) -> bool {
    stderr.lines().any(|l| {
        l.starts_with("hmn: ps failed to query device ")
            && l.ends_with(" (skipped)")
            && l.contains(text)
    })
}

/// The start of `HypomnesisError::ProcessListDenied`'s `Display`, and so of
/// the detail a denied device's skip line carries.
const DENIAL_TEXT: &str = "process list unreadable (";

/// The start of `HypomnesisError::NoGpuSource`'s `Display`, the same on
/// every platform before its list of backends.
const NO_GPU_SOURCE_TEXT: &str = "no GPU measurement source available";

/// The remedy a denied device's line ends with, apart from ` (skipped)`.
#[cfg(target_os = "macos")]
const REMEDY_TEXT: &str = "re-run outside the sandbox";

/// Judge an exit code that should be `expected` on a host where every
/// device answers (or none is there to try), and may instead be `2` where
/// a tried device failed, but only together with its skip line: a bare
/// exit `2` never passes. The skip line must say why: the process list was
/// unreadable (a sandbox that denies `process-info*`), or no GPU source
/// is available (a host whose source fails for another reason, such as a
/// macOS VM) under its own label. Writes `cli_ps: <label> branch=<branch>`
/// straight to stderr, so the branch taken is in CI's log even for a passing
/// test (`eprintln!` is captured by `libtest` and hidden), and asserts the
/// branch is not `rejected`.
fn accept(label: &str, code: Option<i32>, stderr: &str, expected: i32) {
    let branch = if code == Some(expected) {
        "expected"
    } else if code == Some(2) && skipped_device_line(stderr, DENIAL_TEXT) {
        "skipped-device"
    } else if code == Some(2) && skipped_device_line(stderr, NO_GPU_SOURCE_TEXT) {
        "skipped-device-nogpu"
    } else {
        "rejected"
    };
    let _ = writeln!(std::io::stderr().lock(), "cli_ps: {label} branch={branch}");
    assert_ne!(
        branch, "rejected",
        "{label}: code {code:?}, expected {expected} or 2 with a skip line; stderr: {stderr}"
    );
}

/// `--exit-status` makes "nothing listed" exit `1`, as `pgrep` does;
/// without it the same listing exits `0`. No process has PID `u32::MAX`.
/// On a host with no device to try (`device_count()` fails: the ubuntu and
/// windows runners) that still holds. On a host whose GPU source fails both
/// listings exit `2` with a skipped-device line instead, since nothing
/// could be listed; `accept` takes that branch only with the line.
#[test]
fn ps_exit_status_is_1_when_nothing_is_listed_and_opt_in() {
    let (code, stderr) = hmn(&["ps", "--pid", "4294967295", "--exit-status"]);
    accept("with --exit-status", code, &stderr, 1);
    let (code, stderr) = hmn(&["ps", "--pid", "4294967295"]);
    accept("without --exit-status", code, &stderr, 0);
}

/// Run `hmn` with `args` under the field report's profile P, which denies
/// `process-info*` except on itself, returning its exit code, stdout and
/// stderr. Fails when `sandbox-exec` cannot apply P because this test
/// already runs inside a sandbox: a run that never applied P must not
/// report `ok`.
#[cfg(target_os = "macos")]
#[allow(clippy::expect_used)] // test-only
fn hmn_under_denied_process_info(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg("(version 1)(allow default)(deny process-info*)(allow process-info* (target self))")
        .arg(env!("CARGO_BIN_EXE_hmn"))
        .args(args)
        .output()
        .expect("failed to run sandbox-exec");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        !stderr.contains("sandbox_apply"),
        "sandbox-exec cannot apply profile P here (already inside a sandbox?), \
         so the denied-process-info case cannot be exercised: {stderr}"
    );
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr,
    )
}

/// Inside a sandbox that denies `process-info*`, `hmn ps` cannot list its
/// one device: it names the device with a skip line that says the process
/// list was unreadable and what to do about it, prints nothing on stdout,
/// closes with the all-failed line and exits `2`, never an empty table and
/// exit `0`. With `--exit-status`, the empty listing exits `2` (can't tell),
/// not `1` (nothing matched). With `--device` the line is the same without
/// its ` (skipped)`.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires a usable Metal device and an unsandboxed parent (it applies a Seatbelt profile with /usr/bin/sandbox-exec)"]
fn ps_exits_2_with_the_skip_line_when_process_info_is_denied() {
    let (code, stdout, stderr) = hmn_under_denied_process_info(&["ps"]);
    assert_eq!(code, Some(2), "stderr: {stderr}");
    assert!(
        skipped_device_line(&stderr, DENIAL_TEXT),
        "stderr: {stderr}"
    );
    assert!(
        skipped_device_line(&stderr, REMEDY_TEXT),
        "stderr: {stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l == "hmn: ps: no device could be queried, so nothing could be listed"),
        "stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(
        !stderr.contains("0 GPU processes found"),
        "stderr: {stderr}"
    );

    let (code, _stdout, stderr) =
        hmn_under_denied_process_info(&["ps", "--pid", "4294967295", "--exit-status"]);
    assert_eq!(code, Some(2), "stderr: {stderr}");

    // `--device` names the same denial, with the same remedy, and no
    // ` (skipped)`: the device was asked for, not skipped.
    let (code, _stdout, stderr) = hmn_under_denied_process_info(&["ps", "--device", "0"]);
    assert_eq!(code, Some(2), "stderr: {stderr}");
    assert!(
        stderr.lines().any(|l| {
            l.starts_with("hmn: ps failed to query device 0: ")
                && l.contains(DENIAL_TEXT)
                && l.ends_with(REMEDY_TEXT)
        }),
        "stderr: {stderr}"
    );
    assert!(
        !skipped_device_line(&stderr, DENIAL_TEXT),
        "stderr: {stderr}"
    );
}
