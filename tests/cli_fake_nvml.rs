// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end tests of `hmn` against a fake NVML, on Linux.
//!
//! `tests/fixtures/nvml_fake.c` is a stand-in `libnvidia-ml.so.1` that
//! answers the nine NVML calls the crate makes from environment variables:
//! how many devices, which ones are lost, which processes hold how much,
//! and the return codes of `nvmlInit_v2` and `nvmlDeviceGetCount_v2` (the
//! file's header lists them). It lets one machine show what only a
//! multi-GPU node, or a broken driver, would otherwise show. Each test
//! builds it with the C compiler (`$CC`, else `cc`) and runs the compiled
//! `hmn` with `LD_LIBRARY_PATH` pointing at it, which the dynamic loader
//! searches before its cache, and with a `PATH` holding no `nvidia-smi`, so
//! the fallback cannot reach a real driver either. Nothing touches a GPU,
//! so, unlike the `live_*` tests, they are not `#[ignore]`d: they run on
//! every Linux host, the ubuntu CI runner included. Without a C compiler a
//! test skips with a note on stderr, except on CI (`CI` set), where it
//! fails, so a runner image that loses its compiler cannot silently drop
//! the coverage.
//!
//! `#![cfg(...)]`: Cargo defines `CARGO_BIN_EXE_hmn` only when the `hmn`
//! target (`required-features = ["cli"]`) is built, and `LD_LIBRARY_PATH`
//! is the Linux loader's.

#![cfg(all(feature = "cli", target_os = "linux"))]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The error a fake whose `nvmlInit_v2` returns 18 produces, as
/// `HypomnesisError::Nvml` displays it.
const NVML_INIT_ERROR: &str =
    "NVML error: nvmlInit_v2 returned NVML_ERROR_LIB_RM_VERSION_MISMATCH (18)";

/// `hmn ps`'s closing line when nothing could be listed.
const ALL_DEVICES_FAILED_LINE: &str =
    "hmn: ps: no device could be queried, so nothing could be listed";

/// The environment variables the fake reads, cleared before each run so a
/// test sees only what it sets.
const FAKE_VARS: [&str; 4] = [
    "FAKE_NVML_INIT",
    "FAKE_NVML_COUNT",
    "FAKE_NVML_DEVICES",
    "FAKE_NVML_PROCS",
];

/// What one run of `hmn` gave.
struct Run {
    /// The exit code; `None` if a signal ended the process.
    code: Option<i32>,
    /// Everything written to stdout.
    stdout: String,
    /// Everything written to stderr.
    stderr: String,
}

/// A scratch directory holding the built fake (`lib/`) and an empty `bin/`
/// for `PATH`; removed on drop.
struct FakeNvml {
    /// The scratch directory, unique to the test and the test process.
    root: PathBuf,
}

impl FakeNvml {
    /// Build the fake for the test named `test`, or `None`, with a note on
    /// stderr, when no C compiler can be run here. On CI a missing compiler
    /// fails the test instead, and so does a compiler that runs but cannot
    /// build the fake.
    #[allow(clippy::expect_used)] // test-only
    fn build(test: &str) -> Option<Self> {
        let root = std::env::temp_dir().join(format!(
            "hypomnesis-fake-nvml-{}-{test}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("lib")).expect("failed to create the fake's lib dir");
        std::fs::create_dir_all(root.join("bin")).expect("failed to create the empty bin dir");
        // Owned from here, so the directory is removed on every return path.
        let fake = Self { root };

        let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nvml_fake.c");
        let built = Command::new(&cc)
            .args(["-std=c11", "-shared", "-fPIC", "-o"])
            .arg(fake.lib().join("libnvidia-ml.so.1"))
            .arg(&source)
            .output();
        let output = match built {
            Ok(output) => output,
            Err(e) => {
                assert!(
                    std::env::var_os("CI").is_none(),
                    "cannot run {} to build the fake NVML on CI: {e}",
                    cc.to_string_lossy()
                );
                // Straight to stderr, so the skip is visible even for a
                // passing test (`eprintln!` is captured by `libtest`).
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "cli_fake_nvml: {test} skipped: cannot run {}: {e}",
                    cc.to_string_lossy()
                );
                return None;
            }
        };
        assert!(
            output.status.success(),
            "{} failed to build the fake NVML: {}",
            cc.to_string_lossy(),
            String::from_utf8_lossy(&output.stderr)
        );
        Some(fake)
    }

    /// The directory `LD_LIBRARY_PATH` names: the fake, and nothing else.
    fn lib(&self) -> PathBuf {
        self.root.join("lib")
    }

    /// Run `hmn` with `args` against the fake, configured by `env` (pairs
    /// of `FAKE_NVML_*` names and values).
    #[allow(clippy::expect_used)] // test-only
    fn hmn(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hmn"));
        command
            .args(args)
            .env("LD_LIBRARY_PATH", self.lib())
            .env("PATH", self.root.join("bin"));
        for var in FAKE_VARS {
            command.env_remove(var);
        }
        command.envs(env.iter().copied());
        let out = command.output().expect("failed to run hmn");
        Run {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

impl Drop for FakeNvml {
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
    let Some(fake) = FakeNvml::build("ps-init") else {
        return;
    };
    let count_line = format!("hmn: ps failed to count devices: {NVML_INIT_ERROR}");
    let cases: [&[&str]; 3] = [
        &["ps"],
        &["ps", "--json"],
        &["ps", "--filter", "fieldcheck", "--exit-status"],
    ];
    for args in cases {
        let run = fake.hmn(args, &[("FAKE_NVML_INIT", "18")]);
        assert_eq!(run.code, Some(2), "hmn {args:?}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&run.stderr, &count_line),
            "hmn {args:?}: stderr: {}",
            run.stderr
        );
        assert!(
            has_line(&run.stderr, ALL_DEVICES_FAILED_LINE),
            "hmn {args:?}: stderr: {}",
            run.stderr
        );
    }
}

/// A count call that fails after a good init is reported the same way,
/// naming that call: the `nvmlDeviceGetCount_v2` path of `device_count`.
#[test]
fn ps_exits_2_naming_the_count_call_when_it_fails() {
    let Some(fake) = FakeNvml::build("ps-count") else {
        return;
    };
    let run = fake.hmn(&["ps"], &[("FAKE_NVML_COUNT", "999")]);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to count devices: NVML error: nvmlDeviceGetCount_v2 returned \
             NVML_ERROR_UNKNOWN (999)"
        ),
        "stderr: {}",
        run.stderr
    );
}

/// `hmn` with no subcommand exits `1` with the reason and nothing on
/// stdout, `--json` included, where v0.2.14 printed `hmn: no visible
/// GPUs.` and exited `0`.
#[test]
fn summary_exits_1_with_the_reason_when_nvml_cannot_start() {
    let Some(fake) = FakeNvml::build("summary-init") else {
        return;
    };
    let error_line = format!("hmn: {NVML_INIT_ERROR}");
    let cases: [&[&str]; 2] = [&[], &["--json"]];
    for args in cases {
        let run = fake.hmn(args, &[("FAKE_NVML_INIT", "18")]);
        assert_eq!(run.code, Some(1), "hmn {args:?}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&run.stderr, &error_line),
            "hmn {args:?}: stderr: {}",
            run.stderr
        );
    }
}

/// The commands that query one device exit `2` naming the reason, where
/// v0.2.14 named no measurement source: every backend failed, and the
/// bounds check now says why.
#[test]
fn per_device_commands_name_the_reason_when_nvml_cannot_start() {
    let Some(fake) = FakeNvml::build("per-device-init") else {
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
        let run = fake.hmn(args, &[("FAKE_NVML_INIT", "18")]);
        assert_eq!(run.code, Some(2), "hmn {args:?}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&run.stderr, &format!("{prefix}: {NVML_INIT_ERROR}")),
            "hmn {args:?}: stderr: {}",
            run.stderr
        );
    }
}

/// The control: a fake that works reads as a GPU, every figure from the
/// fake's constants (16 GiB, 512 MiB reserved, driver `999.99`), so the
/// tests above fail for the reason they name and not because the fake
/// does not load.
#[test]
fn summary_lists_a_working_fake_gpu() {
    let Some(fake) = FakeNvml::build("summary-working") else {
        return;
    };
    let run = fake.hmn(&[], &[("FAKE_NVML_DEVICES", "NVIDIA H100 NVL")]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(
        run.stdout,
        "GPU 0 [NVIDIA H100 NVL]: free 15872 MiB / 16384 MiB (512 MiB reserved), driver 999.99\n"
    );
}
