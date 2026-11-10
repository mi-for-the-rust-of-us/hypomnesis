// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end tests of `hmn` against a fake `ROCm` SMI, on Linux.
//!
//! `tests/fixtures/rocm_smi_fake.c` is a stand-in `ROCm` SMI library that
//! answers the ten `ROCm` SMI calls the crate makes from environment
//! variables: how many devices, which processes hold how much, and the
//! return codes of `rsmi_init` and `rsmi_num_monitor_devices` (the file's
//! header lists them). It lets one machine show what only an AMD GPU node
//! or a broken `ROCm` install would otherwise show. Each test builds it with
//! a C compiler and runs `hmn` against it, so that nothing touches a GPU:
//! the fake is `librocm_smi64.so.1`, built with `$CC`, else `cc`, and `hmn`
//! runs with `LD_LIBRARY_PATH` pointing at it, which the dynamic loader
//! searches before its cache.
//!
//! `PATH` holds no `nvidia-smi`, so the fallback cannot reach a real driver
//! either. A host where a real NVIDIA stack still counts a GPU skips these
//! tests, since that GPU would answer for the fake's absent ones. Unlike the
//! `live_*` tests these are not `#[ignore]`d: they run on every other Linux
//! host, CI included. Without a C compiler a test skips with a note on
//! stderr, except on CI (`CI` set), where it fails, so a runner image that
//! loses its compiler cannot silently drop the coverage.
//!
//! `#![cfg(...)]`: Cargo defines `CARGO_BIN_EXE_hmn` only when the `hmn`
//! target (`required-features = ["cli"]`) is built; only Linux loads
//! `ROCm` SMI.

#![cfg(all(feature = "cli", target_os = "linux"))]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The error a fake whose `rsmi_init` returns 6 produces, as
/// `HypomnesisError::Rocm` displays it.
const ROCM_INIT_ERROR: &str = "ROCm error: rsmi_init returned RSMI_STATUS_INTERNAL_EXCEPTION (6)";

/// `hmn ps`'s closing line when nothing could be listed.
const ALL_DEVICES_FAILED_LINE: &str =
    "hmn: ps: no device could be queried, so nothing could be listed";

/// The environment variables the fake reads, cleared before each run so a
/// test sees only what it sets.
const FAKE_VARS: [&str; 4] = [
    "FAKE_RSMI_INIT",
    "FAKE_RSMI_COUNT",
    "FAKE_RSMI_DEVICES",
    "FAKE_RSMI_PROCS",
];

/// The file name the crate loads `ROCm` SMI by, which the fake takes.
const ROCM_FILE: &str = "librocm_smi64.so.1";

/// The compilers to try for the fake, in order, when `CC` is not set.
const COMPILERS: &[&str] = &["cc"];

/// The compiler flags, the output path following: warnings are errors, so
/// the fake stays as clean as it was written (gcc and clang, `-pedantic`
/// included).
const COMPILE_FLAGS: &[&str] = &[
    "-std=c11", "-Wall", "-Wextra", "-Werror", "-shared", "-fPIC", "-o",
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
struct FakeRocm {
    /// The scratch directory, unique to the test and the test process.
    root: PathBuf,
}

/// Write a skip note straight to stderr, so it is visible even for a
/// passing test (`eprintln!` is captured by `libtest`).
fn skip(test: &str, why: &str) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "cli_fake_rocm: {test} skipped: {why}"
    );
}

impl FakeRocm {
    /// Build the fake for the test named `test`, or `None`, with a note on
    /// stderr, when no C compiler can be run here, or a real NVIDIA stack
    /// answers on this host. On CI a missing compiler fails the test
    /// instead, and so does a compiler that runs but cannot build the fake.
    #[allow(clippy::expect_used)] // test-only
    fn build(test: &str) -> Option<Self> {
        let root = std::env::temp_dir().join(format!(
            "hypomnesis-fake-rocm-{}-{test}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("lib")).expect("failed to create the fake's lib dir");
        std::fs::create_dir_all(root.join("bin")).expect("failed to create the empty bin dir");
        // Owned from here, so the directory is removed on every return path.
        let fake = Self { root };

        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rocm_smi_fake.c");
        let compilers: Vec<std::ffi::OsString> = std::env::var_os("CC")
            .map_or_else(|| COMPILERS.iter().map(Into::into).collect(), |cc| vec![cc]);
        let mut last_error = String::new();
        let mut built = None;
        for cc in &compilers {
            match Command::new(cc)
                .args(COMPILE_FLAGS)
                .arg(fake.lib().join(ROCM_FILE))
                .arg(&source)
                .output()
            {
                Ok(output) => {
                    built = Some((cc.to_string_lossy().into_owned(), output));
                    break;
                }
                Err(e) => last_error = format!("cannot run {}: {e}", cc.to_string_lossy()),
            }
        }
        let Some((cc, output)) = built else {
            assert!(
                std::env::var_os("CI").is_none(),
                "no C compiler to build the fake ROCm SMI on CI: {last_error}"
            );
            skip(test, &last_error);
            return None;
        };
        assert!(
            output.status.success(),
            "{cc} failed to build the fake ROCm SMI: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // With the fake reporting no driver, `hmn` must see no GPU: one it
        // still counts belongs to a real NVIDIA stack.
        let probe = fake.hmn(&[], &[("FAKE_RSMI_INIT", "8")]);
        if probe.stdout.lines().any(|line| line.starts_with("GPU ")) {
            skip(test, "a real NVIDIA stack answers on this host");
            return None;
        }
        Some(fake)
    }

    /// The directory holding the fake, and nothing else.
    fn lib(&self) -> PathBuf {
        self.root.join("lib")
    }

    /// The `hmn` to run: the built one, which `LD_LIBRARY_PATH` points at
    /// the fake.
    fn hmn_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hmn"));
        command.env("LD_LIBRARY_PATH", self.lib());
        command
    }

    /// Run `hmn` with `args` against the fake, configured by `env` (pairs
    /// of `FAKE_RSMI_*` names and values).
    #[allow(clippy::expect_used)] // test-only
    fn hmn(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut command = self.hmn_command();
        command.args(args).env("PATH", self.root.join("bin"));
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

impl Drop for FakeRocm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A real process for the fake to list: `sleep 30`, whose name `hmn` reads
/// from `/proc` as for any GPU process; killed and reaped on drop.
struct Sleeper(std::process::Child);

impl Sleeper {
    /// Start the process.
    #[allow(clippy::expect_used)] // test-only
    fn start() -> Self {
        Self(
            Command::new("sleep")
                .arg("30")
                .spawn()
                .expect("failed to start sleep"),
        )
    }

    /// Its PID, as `FAKE_RSMI_PROCS` and `hmn`'s rows write it.
    fn pid(&self) -> String {
        self.0.id().to_string()
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Whether `stderr` holds `line` as a whole line: a line, not the whole of
/// stderr, since `debug-output` builds trace there too.
fn has_line(stderr: &str, line: &str) -> bool {
    stderr.lines().any(|l| l == line)
}

/// The first word of `row` under `column` of `header`: the table aligns
/// every cell with its header word.
fn cell<'a>(header: &str, row: &'a str, column: &str) -> Option<&'a str> {
    let start = header.find(column)?;
    row.get(start..)?.split_whitespace().next()
}

/// The table rows of `stdout` whose first column is `pid`.
fn rows_of<'a>(stdout: &'a str, pid: &str) -> Vec<&'a str> {
    stdout
        .lines()
        .filter(|line| line.split_whitespace().next() == Some(pid))
        .collect()
}

/// `hmn ps` exits `2` with the reason and nothing on stdout, `--json`
/// included; with `--exit-status` it exits `2`, not `1`, "not on the GPU",
/// while the GPUs may have been running the job.
#[test]
fn ps_exits_2_with_the_reason_when_rocm_cannot_start() {
    let Some(fake) = FakeRocm::build("ps-init") else {
        return;
    };
    let count_line = format!("hmn: ps failed to count devices: {ROCM_INIT_ERROR}");
    let cases: [&[&str]; 3] = [
        &["ps"],
        &["ps", "--json"],
        &["ps", "--filter", "fieldcheck", "--exit-status"],
    ];
    for args in cases {
        let run = fake.hmn(args, &[("FAKE_RSMI_INIT", "6")]);
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

/// A driver that is not there (`rsmi_init` returns `RSMI_STATUS_INIT_ERROR`,
/// 8: the `ROCm` userland on a host without the `amdgpu` module) leaves no
/// GPU process that could exist, so it reads as no GPU, not as "can't tell"
/// the way any other code does. `hmn` says `no visible GPUs.` (exit `0`)
/// and `hmn ps` says `(no GPU to list)` (exit `0`, `1` under
/// `--exit-status`), as on a machine without an AMD driver. A library that
/// starts and counts zero is a count: `--device 0` is out of range.
#[test]
fn rocm_codes_that_mean_no_gpu_read_as_no_gpu() {
    let Some(fake) = FakeRocm::build("init-no-gpu") else {
        return;
    };
    let env = [("FAKE_RSMI_INIT", "8")];
    let run = fake.hmn(&[], &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "hmn: no visible GPUs.\n");

    let run = fake.hmn(&["ps"], &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(
        has_line(&run.stderr, "hmn: 0 GPU processes found (no GPU to list)."),
        "stderr: {}",
        run.stderr
    );

    let run = fake.hmn(&["ps", "--exit-status"], &env);
    assert_eq!(run.code, Some(1), "stderr: {}", run.stderr);

    // With no driver, no count source is left.
    let run = fake.hmn(&["ps", "--device", "0"], &env);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to query device 0: no GPU measurement source available (NVML, \
             ROCm, and nvidia-smi all failed or are disabled)"
        ),
        "stderr: {}",
        run.stderr
    );

    // A library that starts and counts zero: the index is out of range.
    let run = fake.hmn(&["ps", "--device", "0"], &[]);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to query device 0: device index 0 out of range (have 0 devices)"
        ),
        "stderr: {}",
        run.stderr
    );
}

/// A count call that fails after a good init is reported the same way,
/// naming that call: the `rsmi_num_monitor_devices` path of `device_count`.
#[test]
fn ps_exits_2_naming_the_count_call_when_it_fails() {
    let Some(fake) = FakeRocm::build("ps-count") else {
        return;
    };
    let run = fake.hmn(&["ps"], &[("FAKE_RSMI_COUNT", "6")]);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to count devices: ROCm error: rsmi_num_monitor_devices returned \
             RSMI_STATUS_INTERNAL_EXCEPTION (6)"
        ),
        "stderr: {}",
        run.stderr
    );
}

/// `hmn` with no subcommand exits `2` with the reason and nothing on
/// stdout, `--json` included.
#[test]
fn summary_exits_2_with_the_reason_when_rocm_cannot_start() {
    let Some(fake) = FakeRocm::build("summary-init") else {
        return;
    };
    let error_line = format!("hmn: failed to count devices: {ROCM_INIT_ERROR}");
    let cases: [&[&str]; 2] = [&[], &["--json"]];
    for args in cases {
        let run = fake.hmn(args, &[("FAKE_RSMI_INIT", "6")]);
        assert_eq!(run.code, Some(2), "hmn {args:?}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&run.stderr, &error_line),
            "hmn {args:?}: stderr: {}",
            run.stderr
        );
    }
}

/// The line the fake's one working GPU gives in `hmn`'s summary.
const MI300X_LINE: &str = "GPU 0 [AMD Instinct MI300X]: free 16384 MiB / 16384 MiB, driver 6.99.0";

/// One process on both GPUs of a node whose GPUs are one model: the GPU
/// column tells the two rows apart; `--json` carries the same indices.
#[test]
fn ps_gpu_column_tells_two_gpus_of_one_model_apart() {
    let Some(fake) = FakeRocm::build("ps-gpu-column") else {
        return;
    };
    let job = Sleeper::start();
    let pid = job.pid();
    let procs = format!("0:{pid}:1073741824,1:{pid}:1073741824");
    let env = [
        (
            "FAKE_RSMI_DEVICES",
            "AMD Instinct MI300X,AMD Instinct MI300X",
        ),
        ("FAKE_RSMI_PROCS", procs.as_str()),
    ];
    let run = fake.hmn(&["ps"], &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let header = run.stdout.lines().next().unwrap_or_default();
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        ["PID", "NAME", "VRAM", "SHARED", "GPU", "DEVICE", "SPILL"]
    );
    let rows = rows_of(&run.stdout, &pid);
    let gpus: Vec<_> = rows.iter().map(|row| cell(header, row, "GPU")).collect();
    assert_eq!(gpus, [Some("0"), Some("1")], "stdout: {}", run.stdout);
    assert!(
        rows.iter().all(|row| row.contains("AMD Instinct MI300X")),
        "stdout: {}",
        run.stdout
    );

    // One object per GPU, each carrying its own index.
    let json = fake.hmn(&["ps", "--json"], &env).stdout;
    for index in [0, 1] {
        let object = format!(
            "{{\"pid\":{pid},\"name\":\"sleep\",\"used_bytes\":1073741824,\
             \"shared_used_bytes\":0,\"device_index\":{index},"
        );
        assert!(json.contains(&object), "no {object} in json: {json}");
    }
}

/// The control: a fake that works reads as a GPU, every figure from the
/// fake's constants (16 GiB, driver `6.99.0`), so the tests above fail for
/// the reason they name and not because the fake does not load.
#[test]
fn summary_lists_a_working_fake_gpu() {
    let Some(fake) = FakeRocm::build("summary-working") else {
        return;
    };
    let run = fake.hmn(&[], &[("FAKE_RSMI_DEVICES", "AMD Instinct MI300X")]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, format!("{MI300X_LINE}\n"));
}
