// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end tests of `hmn` against a fake NVML, on Linux and Windows.
//!
//! `tests/fixtures/nvml_fake.c` is a stand-in NVML library that answers the
//! nine NVML calls the crate makes from environment variables: how many
//! devices, which ones are lost, which processes hold how much, and the
//! return codes of `nvmlInit_v2` and `nvmlDeviceGetCount_v2` (the file's
//! header lists them). It lets one machine show what only a multi-GPU node,
//! a broken driver or a datacenter GPU would otherwise show. Each test
//! builds it with a C compiler and runs `hmn` against it, so that nothing
//! touches a GPU:
//!
//! - **Linux:** the fake is `libnvidia-ml.so.1`, built with `$CC`, else
//!   `cc`, and `hmn` runs with `LD_LIBRARY_PATH` pointing at it, which the
//!   dynamic loader searches before its cache.
//! - **Windows:** the fake is `nvml.dll`, built with `$CC`, else `clang`
//!   (on `PATH`, or LLVM's default install folder), and placed beside a copy
//!   of `hmn.exe`: Windows looks for a DLL in the executable's own folder
//!   first. A host with a real NVIDIA driver skips these tests, since `DXGI`
//!   sees its adapters and `nvidia-smi.exe` in `System32` is reachable
//!   whatever `PATH` says. A host without one, the windows CI runner
//!   included, has `DXGI` counting zero NVIDIA adapters: exactly what a
//!   datacenter GPU in `TCC` mode looks like to `DXGI`, so a fake device
//!   there is a `TCC` GPU as `hmn` sees it.
//!
//! `PATH` holds no `nvidia-smi`, so the fallback cannot reach a real driver
//! either. Unlike the `live_*` tests these are not `#[ignore]`d: they run
//! on every Linux host and every Windows host without an NVIDIA driver, CI
//! included. Without a C compiler a test skips with a note on stderr,
//! except on CI (`CI` set), where it fails, so a runner image that loses
//! its compiler cannot silently drop the coverage. The tests that need
//! `NVML`'s process listing, which the crate reads on Linux only (Windows
//! lists through `PDH`), or a Linux `NoGpuSource` text, are Linux-only.
//!
//! `#![cfg(...)]`: Cargo defines `CARGO_BIN_EXE_hmn` only when the `hmn`
//! target (`required-features = ["cli"]`) is built; macOS loads no NVML.

#![cfg(all(feature = "cli", any(target_os = "linux", windows)))]

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

/// The file name the crate loads NVML by, which the fake takes.
#[cfg(target_os = "linux")]
const NVML_FILE: &str = "libnvidia-ml.so.1";
/// The file name the crate loads NVML by, which the fake takes.
#[cfg(windows)]
const NVML_FILE: &str = "nvml.dll";

/// The compilers to try for the fake, in order, when `CC` is not set.
#[cfg(target_os = "linux")]
const COMPILERS: &[&str] = &["cc"];
/// The compilers to try for the fake, in order, when `CC` is not set:
/// `clang` finds the MSVC headers and libraries on its own, without a
/// developer shell, so it is tried on `PATH`, then where LLVM installs it.
#[cfg(windows)]
const COMPILERS: &[&str] = &["clang", r"C:\Program Files\LLVM\bin\clang.exe"];

/// The compiler flags, the output path following: warnings are errors, so
/// the fake stays as clean as it was written (gcc and clang, `-pedantic`
/// included).
#[cfg(target_os = "linux")]
const COMPILE_FLAGS: &[&str] = &[
    "-std=c11", "-Wall", "-Wextra", "-Werror", "-shared", "-fPIC", "-o",
];
/// The compiler flags, the output path following: warnings are errors, so
/// the fake stays as clean as it was written; Windows needs no `-fPIC`.
#[cfg(windows)]
const COMPILE_FLAGS: &[&str] = &["-std=c11", "-Wall", "-Wextra", "-Werror", "-shared", "-o"];

/// What one run of `hmn` gave.
struct Run {
    /// The exit code; `None` if a signal ended the process.
    code: Option<i32>,
    /// Everything written to stdout.
    stdout: String,
    /// Everything written to stderr.
    stderr: String,
}

/// A scratch directory holding the built fake (`lib/`, with a copy of
/// `hmn.exe` beside it on Windows) and an empty `bin/` for `PATH`; removed
/// on drop.
struct FakeNvml {
    /// The scratch directory, unique to the test and the test process.
    root: PathBuf,
}

/// Write a skip note straight to stderr, so it is visible even for a
/// passing test (`eprintln!` is captured by `libtest`).
fn skip(test: &str, why: &str) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "cli_fake_nvml: {test} skipped: {why}"
    );
}

/// Whether a real NVIDIA driver is installed on this Windows host: its
/// `nvml.dll` sits in `System32`, beside an `nvidia-smi.exe` that the
/// fallback would find whatever `PATH` says.
#[cfg(windows)]
fn real_nvidia_driver() -> bool {
    std::env::var_os("SystemRoot")
        .is_some_and(|root| Path::new(&root).join("System32").join("nvml.dll").exists())
}

impl FakeNvml {
    /// Build the fake for the test named `test`, or `None`, with a note on
    /// stderr, when no C compiler can be run here, or (Windows) a real
    /// NVIDIA driver is installed. On CI a missing compiler fails the test
    /// instead, and so does a compiler that runs but cannot build the fake.
    #[allow(clippy::expect_used)] // test-only
    fn build(test: &str) -> Option<Self> {
        #[cfg(windows)]
        if real_nvidia_driver() {
            skip(test, "a real NVIDIA driver is installed on this host");
            return None;
        }
        let root = std::env::temp_dir().join(format!(
            "hypomnesis-fake-nvml-{}-{test}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("lib")).expect("failed to create the fake's lib dir");
        std::fs::create_dir_all(root.join("bin")).expect("failed to create the empty bin dir");
        // Owned from here, so the directory is removed on every return path.
        let fake = Self { root };

        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nvml_fake.c");
        let compilers: Vec<std::ffi::OsString> = std::env::var_os("CC")
            .map_or_else(|| COMPILERS.iter().map(Into::into).collect(), |cc| vec![cc]);
        let mut last_error = String::new();
        let mut built = None;
        for cc in &compilers {
            match Command::new(cc)
                .args(COMPILE_FLAGS)
                .arg(fake.lib().join(NVML_FILE))
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
                "no C compiler to build the fake NVML on CI: {last_error}"
            );
            skip(test, &last_error);
            return None;
        };
        assert!(
            output.status.success(),
            "{cc} failed to build the fake NVML: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        #[cfg(windows)]
        std::fs::copy(env!("CARGO_BIN_EXE_hmn"), fake.lib().join("hmn.exe"))
            .expect("failed to copy hmn.exe beside the fake");
        Some(fake)
    }

    /// The directory holding the fake, and nothing else (but, on Windows,
    /// the copy of `hmn.exe` it is loaded by).
    fn lib(&self) -> PathBuf {
        self.root.join("lib")
    }

    /// The `hmn` to run: the built one, which `LD_LIBRARY_PATH` points at
    /// the fake (Linux); the copy beside the fake (Windows).
    fn hmn_command(&self) -> Command {
        #[cfg(target_os = "linux")]
        {
            let mut command = Command::new(env!("CARGO_BIN_EXE_hmn"));
            command.env("LD_LIBRARY_PATH", self.lib());
            command
        }
        #[cfg(windows)]
        {
            Command::new(self.lib().join("hmn.exe"))
        }
    }

    /// Run `hmn` with `args` against the fake, configured by `env` (pairs
    /// of `FAKE_NVML_*` names and values).
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

impl Drop for FakeNvml {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A real process for the fake to list: `sleep 30`, whose name `hmn` reads
/// from `/proc` as for any GPU process; killed and reaped on drop.
#[cfg(target_os = "linux")]
struct Sleeper(std::process::Child);

#[cfg(target_os = "linux")]
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

    /// Its PID, as `FAKE_NVML_PROCS` and `hmn`'s rows write it.
    fn pid(&self) -> String {
        self.0.id().to_string()
    }
}

#[cfg(target_os = "linux")]
impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The PID of a process that has just exited and been reaped: a PID that
/// names no process, as a typo or a finished job would.
#[cfg(target_os = "linux")]
#[allow(clippy::expect_used)] // test-only
fn dead_pid() -> String {
    let mut child = Command::new("true").spawn().expect("failed to start true");
    let pid = child.id().to_string();
    let _ = child.wait();
    pid
}

/// The number of lines in `text` whose words include `pid` followed by
/// `name`: `hmn watch` rows (time, PID, NAME, …) and per-PID summary rows.
#[cfg(target_os = "linux")]
fn count_named_rows(text: &str, pid: &str, name: &str) -> usize {
    text.lines()
        .filter(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.windows(2).any(|pair| pair == [pid, name])
        })
        .count()
}

/// Whether `stderr` holds `line` as a whole line: a line, not the whole of
/// stderr, since `debug-output` builds trace there too.
fn has_line(stderr: &str, line: &str) -> bool {
    stderr.lines().any(|l| l == line)
}

/// The first word of `row` under `column` of `header`: the table aligns
/// every cell with its header word.
#[cfg(target_os = "linux")]
fn cell<'a>(header: &str, row: &'a str, column: &str) -> Option<&'a str> {
    let start = header.find(column)?;
    row.get(start..)?.split_whitespace().next()
}

/// The table rows of `stdout` whose first column is `pid`.
#[cfg(target_os = "linux")]
fn rows_of<'a>(stdout: &'a str, pid: &str) -> Vec<&'a str> {
    stdout
        .lines()
        .filter(|line| line.split_whitespace().next() == Some(pid))
        .collect()
}

/// `hmn ps` exits `2` with the reason and nothing on stdout, `--json`
/// included, where v0.2.14 printed an empty table and `0 GPU processes
/// found.`; with `--exit-status` it exits `2`, where v0.2.14 exited `1`,
/// "not on the GPU", while the GPUs may have been running the job. On
/// Windows, where `DXGI` counts zero NVIDIA adapters here as over a `TCC`
/// GPU, it also pins that a `DXGI` zero does not hide the broken `NVML`.
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

/// A driver that is not loaded (`nvmlInit_v2` returns
/// `NVML_ERROR_DRIVER_NOT_LOADED`, 9) or that found no GPU
/// (`NVML_ERROR_GPU_NOT_FOUND`, 28) leaves no GPU process that could
/// exist: both read as no GPU, not as "can't tell" the way a version
/// mismatch does. `hmn` says `no visible GPUs.` (exit `0`) and `hmn ps`
/// says `(no GPU to list)` (exit `0`, `1` under `--exit-status`), as on a
/// machine without an NVIDIA driver; a host with NVIDIA userspace but no
/// usable GPU (a CPU cloud instance on a GPU image) must not exit `2`.
#[test]
fn nvml_codes_that_mean_no_gpu_read_as_no_gpu() {
    let Some(fake) = FakeNvml::build("init-no-gpu") else {
        return;
    };
    for code in ["9", "28"] {
        let env = [("FAKE_NVML_INIT", code)];
        let run = fake.hmn(&[], &env);
        assert_eq!(run.code, Some(0), "code {code}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "hmn: no visible GPUs.\n", "code {code}");

        let run = fake.hmn(&["ps"], &env);
        assert_eq!(run.code, Some(0), "code {code}: stderr: {}", run.stderr);
        assert!(
            has_line(&run.stderr, "hmn: 0 GPU processes found (no GPU to list)."),
            "code {code}: stderr: {}",
            run.stderr
        );

        let run = fake.hmn(&["ps", "--exit-status"], &env);
        assert_eq!(run.code, Some(1), "code {code}: stderr: {}", run.stderr);
    }
    // `--device 0`: a driver that found no GPU counts zero, so the index
    // is out of range; one that is not loaded leaves no count source.
    let run = fake.hmn(&["ps", "--device", "0"], &[("FAKE_NVML_INIT", "28")]);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to query device 0: device index 0 out of range (have 0 devices)"
        ),
        "stderr: {}",
        run.stderr
    );
    // With the driver not loaded, Linux has no count source left; Windows
    // falls through to `DXGI`, which counts zero NVIDIA adapters.
    #[cfg(target_os = "linux")]
    let line = "hmn: ps failed to query device 0: no GPU measurement source available (NVML and \
                nvidia-smi both failed or are disabled)";
    #[cfg(windows)]
    let line = "hmn: ps failed to query device 0: device index 0 out of range (have 0 devices)";
    let run = fake.hmn(&["ps", "--device", "0"], &[("FAKE_NVML_INIT", "9")]);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert!(has_line(&run.stderr, line), "stderr: {}", run.stderr);
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

/// `hmn` with no subcommand exits `2` with the reason and nothing on
/// stdout, `--json` included, where v0.2.14 printed `hmn: no visible
/// GPUs.` and exited `0`. On Windows this is the `TCC` case: `DXGI`
/// counts zero NVIDIA adapters, as it does over a `TCC` GPU, and before
/// v0.2.15's audit fix that zero answered for the broken `NVML`.
#[test]
fn summary_exits_2_with_the_reason_when_nvml_cannot_start() {
    let Some(fake) = FakeNvml::build("summary-init") else {
        return;
    };
    let error_line = format!("hmn: failed to count devices: {NVML_INIT_ERROR}");
    let cases: [&[&str]; 2] = [&[], &["--json"]];
    for args in cases {
        let run = fake.hmn(args, &[("FAKE_NVML_INIT", "18")]);
        assert_eq!(run.code, Some(2), "hmn {args:?}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "", "hmn {args:?}: stdout");
        assert!(
            has_line(&run.stderr, &error_line),
            "hmn {args:?}: stderr: {}",
            run.stderr
        );
    }
}

/// The skip line `hmn` prints for a lost device, with Linux's backends.
#[cfg(target_os = "linux")]
const LOST_DEVICE_1_SKIP: &str = "hmn: failed to query device 1: no GPU measurement source \
     available (NVML and nvidia-smi both failed or are disabled) (skipped)";

/// The line the fake's one working H100 gives in `hmn`'s summary.
const H100_LINE: &str =
    "GPU 0 [NVIDIA H100 NVL]: free 15872 MiB / 16384 MiB (512 MiB reserved), driver 999.99";

/// A device that fails to answer is named on stderr and the others are
/// listed, exit `0`, where v0.2.14 dropped it without a word (issue #4,
/// note 5).
#[cfg(target_os = "linux")]
#[test]
fn summary_skips_a_device_it_cannot_read() {
    let Some(fake) = FakeNvml::build("summary-lost") else {
        return;
    };
    let env = [("FAKE_NVML_DEVICES", "NVIDIA H100 NVL,lost")];
    let run = fake.hmn(&[], &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, format!("{H100_LINE}\n"));
    assert!(
        has_line(&run.stderr, LOST_DEVICE_1_SKIP),
        "stderr: {}",
        run.stderr
    );

    let run = fake.hmn(&["--json"], &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(
        run.stdout.starts_with("[{\"index\":0,") && !run.stdout.contains("\"index\":1"),
        "stdout: {}",
        run.stdout
    );
    assert!(
        has_line(&run.stderr, LOST_DEVICE_1_SKIP),
        "stderr: {}",
        run.stderr
    );
}

/// With no device readable, `hmn` names each, says none was readable and
/// exits `2` with nothing on stdout, `--json` included, where v0.2.14
/// printed the line on stdout (or `[]`) and exited `0`.
#[cfg(target_os = "linux")]
#[test]
fn summary_exits_2_when_no_device_is_readable() {
    let Some(fake) = FakeNvml::build("summary-none-readable") else {
        return;
    };
    let cases: [&[&str]; 2] = [&[], &["--json"]];
    for args in cases {
        let run = fake.hmn(args, &[("FAKE_NVML_DEVICES", "lost,lost")]);
        assert_eq!(run.code, Some(2), "hmn {args:?}: stderr: {}", run.stderr);
        assert_eq!(run.stdout, "", "hmn {args:?}: stdout");
        for line in [
            LOST_DEVICE_1_SKIP.replace("device 1", "device 0").as_str(),
            LOST_DEVICE_1_SKIP,
            "hmn: 2 GPU(s) enumerated but none readable.",
        ] {
            assert!(
                has_line(&run.stderr, line),
                "hmn {args:?}: no {line:?} in stderr: {}",
                run.stderr
            );
        }
    }
}

/// A device whose memory answers but whose process list fails: `hmn`
/// lists it (it reads only memory), while `hmn ps` skips it and, with no
/// other device, exits `2` — the asymmetry the issue #4 review described.
#[cfg(target_os = "linux")]
#[test]
fn a_device_whose_process_list_fails_is_listed_by_hmn_and_skipped_by_ps() {
    let Some(fake) = FakeNvml::build("noprocs") else {
        return;
    };
    let env = [("FAKE_NVML_DEVICES", "noprocs:NVIDIA H100 NVL")];
    let run = fake.hmn(&[], &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, format!("{H100_LINE}\n"));

    let run = fake.hmn(&["ps"], &env);
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to query device 0: no GPU measurement source available (NVML and \
             nvidia-smi both failed or are disabled) (skipped)"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        has_line(&run.stderr, ALL_DEVICES_FAILED_LINE),
        "stderr: {}",
        run.stderr
    );
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

/// A lost device is skipped with a line naming only the backends Linux
/// has, where v0.2.14 named DXGI and PDH, and the device that answers is
/// listed: the job on device 0 shows, and `hmn ps` exits `0`.
#[cfg(target_os = "linux")]
#[test]
fn ps_skips_a_lost_device_naming_the_linux_backends() {
    let Some(fake) = FakeNvml::build("ps-lost") else {
        return;
    };
    let job = Sleeper::start();
    let procs = format!("0:{}:1073741824", job.pid());
    let run = fake.hmn(
        &["ps"],
        &[
            ("FAKE_NVML_DEVICES", "NVIDIA H100 NVL,lost"),
            ("FAKE_NVML_PROCS", &procs),
        ],
    );
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            "hmn: ps failed to query device 1: no GPU measurement source available (NVML and \
             nvidia-smi both failed or are disabled) (skipped)"
        ),
        "stderr: {}",
        run.stderr
    );
    let rows = rows_of(&run.stdout, &job.pid());
    assert_eq!(rows.len(), 1, "stdout: {}", run.stdout);
    assert!(
        rows.iter().all(|row| row.contains("sleep")),
        "stdout: {}",
        run.stdout
    );
}

/// One process on both GPUs of a node whose GPUs are one model: the GPU
/// column tells the two rows apart, where v0.2.14 printed two identical
/// rows (issue #4, note 3); `--json` carries the same indices.
#[cfg(target_os = "linux")]
#[test]
fn ps_gpu_column_tells_two_gpus_of_one_model_apart() {
    let Some(fake) = FakeNvml::build("ps-gpu-column") else {
        return;
    };
    let job = Sleeper::start();
    let pid = job.pid();
    let procs = format!("0:{pid}:1073741824,1:{pid}:1073741824");
    let env = [
        ("FAKE_NVML_DEVICES", "NVIDIA H100 NVL,NVIDIA H100 NVL"),
        ("FAKE_NVML_PROCS", procs.as_str()),
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
        rows.iter().all(|row| row.contains("NVIDIA H100 NVL")),
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

/// With no device to list (a job given no GPU: NVML starts and counts
/// zero), the summary says so, where v0.2.14 printed only `0 GPU processes
/// found.` (issue #4, note 6). The exit codes stay: `0`, `1` under
/// `--exit-status`, and `2` for a `--device` past the count.
#[test]
fn ps_says_there_is_no_gpu_to_list() {
    let Some(fake) = FakeNvml::build("ps-no-gpu") else {
        return;
    };
    let run = fake.hmn(&["ps"], &[]);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(
        run.stdout.split_whitespace().collect::<Vec<_>>(),
        ["PID", "NAME", "VRAM", "SHARED", "GPU", "DEVICE", "SPILL"]
    );
    assert!(
        has_line(&run.stderr, "hmn: 0 GPU processes found (no GPU to list)."),
        "stderr: {}",
        run.stderr
    );

    let run = fake.hmn(&["ps", "--filter", "fieldcheck", "--exit-status"], &[]);
    assert_eq!(run.code, Some(1), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            "hmn: 0 GPU processes found matching filter=\"fieldcheck\" (no GPU to list)."
        ),
        "stderr: {}",
        run.stderr
    );

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

/// The file name of this test binary, as `hmn` names its process: Cargo
/// names it `cli_fake_nvml-<16 hex digits>`, past the kernel's 15-byte
/// `comm` cut, which `hmn` extends from the `exe` link (same user).
#[cfg(target_os = "linux")]
#[allow(clippy::expect_used)] // test-only
fn this_test_name() -> String {
    std::env::current_exe()
        .expect("no current exe")
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a UTF-8 file name")
        .to_owned()
}

/// A watched PID absent from the device's listing is named by a direct
/// lookup, in its rows, the per-PID summary and `--json`, where v0.2.14
/// printed `?` (issue #4, note 2): this test's own process is alive,
/// readable and holds no GPU memory, like the PID 1 of the field check. A
/// PID that names no process keeps `?` and its warning.
#[cfg(target_os = "linux")]
#[test]
fn watch_names_a_pid_absent_from_the_listing() {
    let Some(fake) = FakeNvml::build("watch-name") else {
        return;
    };
    let me = std::process::id().to_string();
    let my_name = this_test_name();
    let dead = dead_pid();
    let env = [("FAKE_NVML_DEVICES", "NVIDIA H100 NVL")];
    let args = ["watch", &me, &dead, "--interval", "1s", "--duration", "1s"];
    let run = fake.hmn(&args, &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    // At least the attach row and the per-PID summary row.
    assert!(
        count_named_rows(&run.stdout, &me, &my_name) >= 2,
        "stdout: {}",
        run.stdout
    );
    assert!(
        count_named_rows(&run.stdout, &dead, "?") >= 2,
        "stdout: {}",
        run.stdout
    );
    assert!(
        has_line(
            &run.stderr,
            &format!("hmn watch: pid={dead} names no running process; its rows will read 0 MiB")
        ),
        "stderr: {}",
        run.stderr
    );

    let mut json_args = args.to_vec();
    json_args.push("--json");
    let json = fake.hmn(&json_args, &env).stdout;
    assert!(
        json.contains(&format!("\"pid\":{me},\"name\":\"{my_name}\"")),
        "json: {json}"
    );
    assert!(
        json.contains(&format!("\"pid\":{dead},\"name\":null")),
        "json: {json}"
    );
}

/// `hmn watch --filter` watches GPU 0 unless told otherwise. With the job
/// on GPU 1 and another process on GPU 0, it says GPU 0 holds a process
/// but none to select, and where the filter does match, where v0.2.14
/// said "found no GPU processes on device 0" and pointed nowhere (issue
/// #4, note 1). Without `--follow-new` the exit stays `2`; with it, the
/// hint gets a line of its own and the watch goes on.
#[cfg(target_os = "linux")]
#[test]
fn watch_auto_select_names_the_device_where_the_filter_matches() {
    let Some(fake) = FakeNvml::build("watch-hint") else {
        return;
    };
    let job = Sleeper::start();
    let pid = job.pid();
    // This test's own process (not a `sleep`, whatever PID 1 is in a
    // container) holds memory on GPU 0, the job on GPU 1.
    let me = std::process::id();
    let procs = format!("0:{me}:1073741824,1:{pid}:1073741824");
    let env = [
        ("FAKE_NVML_DEVICES", "NVIDIA H100 NVL,NVIDIA H100 NVL"),
        ("FAKE_NVML_PROCS", procs.as_str()),
    ];
    let hint = format!("device 1 has a match (pid {pid}): re-run with --device 1");

    let run = fake.hmn(
        &[
            "watch",
            "--filter",
            "sleep",
            "--interval",
            "1s",
            "--duration",
            "1s",
        ],
        &env,
    );
    assert_eq!(run.code, Some(2), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            &format!(
                "hmn: watch found 1 GPU process on device 0, none to auto-select (top 5 among \
                 names containing \"sleep\" (case-insensitive)); {hint}"
            )
        ),
        "stderr: {}",
        run.stderr
    );

    let run = fake.hmn(
        &[
            "watch",
            "--follow-new",
            "--filter",
            "sleep",
            "--interval",
            "1s",
            "--duration",
            "1s",
        ],
        &env,
    );
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            "hmn: watch found 1 GPU process on device 0, none to follow yet (top 5 by committed \
             among names containing \"sleep\" (case-insensitive)); waiting for work to appear"
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        has_line(&run.stderr, &format!("hmn watch: {hint}")),
        "stderr: {}",
        run.stderr
    );
}

/// An explicit PID that holds memory on another GPU gets a notice naming
/// that GPU and its size, not the "names no running process" warning, and
/// its rows carry its name though they read 0 MiB (issue #4, note 1).
#[cfg(target_os = "linux")]
#[test]
fn watch_flags_an_explicit_pid_on_another_device() {
    let Some(fake) = FakeNvml::build("watch-elsewhere") else {
        return;
    };
    let job = Sleeper::start();
    let pid = job.pid();
    let procs = format!("1:{pid}:10737418240");
    let env = [
        ("FAKE_NVML_DEVICES", "NVIDIA H100 NVL,NVIDIA H100 NVL"),
        ("FAKE_NVML_PROCS", procs.as_str()),
    ];
    let run = fake.hmn(
        &["watch", &pid, "--interval", "1s", "--duration", "1s"],
        &env,
    );
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert!(
        has_line(
            &run.stderr,
            &format!(
                "hmn watch: pid={pid} holds 10.0 GiB on device 1, not device 0; its rows will \
                 read 0 MiB — re-run with --device 1"
            )
        ),
        "stderr: {}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("names no running process"),
        "stderr: {}",
        run.stderr
    );
    assert!(
        count_named_rows(&run.stdout, &pid, "sleep") >= 2,
        "stdout: {}",
        run.stdout
    );
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
    assert_eq!(run.stdout, format!("{H100_LINE}\n"));
}
