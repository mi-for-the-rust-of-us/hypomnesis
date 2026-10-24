// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end tests for an NVIDIA stack that is installed but fails to
//! start, on Linux. `tests/fixtures/nvml_broken.c` is a stand-in
//! `libnvidia-ml.so.1` whose `nvmlInit_v2` returns
//! `NVML_ERROR_LIB_RM_VERSION_MISMATCH` (18), as after a driver upgrade
//! without a reboot. Each test builds it with the C compiler (`$CC`, else
//! `cc`) and runs the compiled `hmn` with `LD_LIBRARY_PATH` pointing at it,
//! which the dynamic loader searches before its cache, and with a `PATH`
//! holding no `nvidia-smi`, so the fallback cannot reach a real driver
//! either. Nothing touches a GPU, so, unlike the `live_*` tests, they are
//! not `#[ignore]`d: they run on every Linux host, the ubuntu CI runner
//! included, and skip with a note on stderr where no C compiler runs.
//! `#![cfg(...)]`: Cargo defines `CARGO_BIN_EXE_hmn` only when the `hmn`
//! target (`required-features = ["cli"]`) is built, and `LD_LIBRARY_PATH`
//! is the Linux loader's.

#![cfg(all(feature = "cli", target_os = "linux"))]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The error the stand-in produces, as `HypomnesisError::Nvml` displays it.
const NVML_ERROR: &str = "NVML error: nvmlInit_v2 returned NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)";

/// `hmn ps`'s closing line when nothing could be listed.
const ALL_DEVICES_FAILED_LINE: &str =
    "hmn: ps: no device could be queried, so nothing could be listed";

/// A scratch directory holding the built stand-in (`lib/`) and an empty
/// `bin/` for `PATH`; removed on drop.
struct StandIn {
    /// The scratch directory, unique to the test and the test process.
    root: PathBuf,
}

impl StandIn {
    /// Build the stand-in for the test named `test`, or `None`, with a note
    /// on stderr, when no C compiler can be run here. A compiler that runs
    /// but cannot build two one-line functions fails the test.
    #[allow(clippy::expect_used)] // test-only
    fn build(test: &str) -> Option<Self> {
        let root = std::env::temp_dir().join(format!(
            "hypomnesis-broken-nvml-{}-{test}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("lib")).expect("failed to create the stand-in lib dir");
        std::fs::create_dir_all(root.join("bin")).expect("failed to create the empty bin dir");
        // Owned from here, so the directory is removed on every return path.
        let stand_in = Self { root };

        let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nvml_broken.c");
        let built = Command::new(&cc)
            .args(["-shared", "-fPIC", "-o"])
            .arg(stand_in.lib().join("libnvidia-ml.so.1"))
            .arg(&source)
            .output();
        let output = match built {
            Ok(output) => output,
            Err(e) => {
                // Straight to stderr, so the skip is in CI's log even for a
                // passing test (`eprintln!` is captured by `libtest`).
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "cli_broken_nvml: {test} skipped: cannot run {}: {e}",
                    cc.to_string_lossy()
                );
                return None;
            }
        };
        assert!(
            output.status.success(),
            "{} failed to build the stand-in: {}",
            cc.to_string_lossy(),
            String::from_utf8_lossy(&output.stderr)
        );
        Some(stand_in)
    }

    /// The directory `LD_LIBRARY_PATH` names: the stand-in, and nothing else.
    fn lib(&self) -> PathBuf {
        self.root.join("lib")
    }

    /// Run `hmn` with `args` against the stand-in, returning its exit code,
    /// stdout and stderr.
    #[allow(clippy::expect_used)] // test-only
    fn hmn(&self, args: &[&str]) -> (Option<i32>, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_hmn"))
            .args(args)
            .env("LD_LIBRARY_PATH", self.lib())
            .env("PATH", self.root.join("bin"))
            .output()
            .expect("failed to run hmn");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Whether `stderr` holds `line` as a whole line: a line, not the whole of
/// stderr, since `debug-output` builds trace there too.
fn has_line(stderr: &str, line: &str) -> bool {
    stderr.lines().any(|l| l == line)
}

/// `hmn ps` exits `2` with the reason and nothing on stdout, `--json`
/// included, where v0.2.14 printed an empty table and `0 GPU processes
/// found.`; with `--exit-status` it exits `2`, where v0.2.14 exited `1`,
/// "not on the GPU", while the GPUs may have been running the job.
#[test]
fn ps_exits_2_with_the_reason_when_nvml_cannot_start() {
    let Some(stand_in) = StandIn::build("ps") else {
        return;
    };
    let count_line = format!("hmn: ps failed to count devices: {NVML_ERROR}");
    let cases: [&[&str]; 3] = [
        &["ps"],
        &["ps", "--json"],
        &["ps", "--filter", "fieldcheck", "--exit-status"],
    ];
    for args in cases {
        let (code, stdout, stderr) = stand_in.hmn(args);
        assert_eq!(code, Some(2), "hmn {args:?}: stderr: {stderr}");
        assert_eq!(stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&stderr, &count_line),
            "hmn {args:?}: stderr: {stderr}"
        );
        assert!(
            has_line(&stderr, ALL_DEVICES_FAILED_LINE),
            "hmn {args:?}: stderr: {stderr}"
        );
    }
}

/// `hmn` with no subcommand exits `1` with the reason and nothing on
/// stdout, `--json` included, where v0.2.14 printed `hmn: no visible
/// GPUs.` and exited `0`.
#[test]
fn summary_exits_1_with_the_reason_when_nvml_cannot_start() {
    let Some(stand_in) = StandIn::build("summary") else {
        return;
    };
    let error_line = format!("hmn: {NVML_ERROR}");
    let cases: [&[&str]; 2] = [&[], &["--json"]];
    for args in cases {
        let (code, stdout, stderr) = stand_in.hmn(args);
        assert_eq!(code, Some(1), "hmn {args:?}: stderr: {stderr}");
        assert_eq!(stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&stderr, &error_line),
            "hmn {args:?}: stderr: {stderr}"
        );
    }
}

/// The commands that query one device exit `2` naming the reason, where
/// v0.2.14 named no measurement source: every backend failed, and the
/// bounds check now says why.
#[test]
fn per_device_commands_name_the_reason_when_nvml_cannot_start() {
    let Some(stand_in) = StandIn::build("per-device") else {
        return;
    };
    let cases: [(&[&str], &str); 3] = [
        (&["ps", "--device", "0"], "hmn: ps failed to query device 0"),
        (
            &["watch", "--interval", "1s", "--duration", "1s"],
            "hmn: watch failed to query device 0",
        ),
        (&["fits", "1GiB"], "hmn: fits failed to query device 0"),
    ];
    for (args, prefix) in cases {
        let (code, stdout, stderr) = stand_in.hmn(args);
        assert_eq!(code, Some(2), "hmn {args:?}: stderr: {stderr}");
        assert_eq!(stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&stderr, &format!("{prefix}: {NVML_ERROR}")),
            "hmn {args:?}: stderr: {stderr}"
        );
    }
}
