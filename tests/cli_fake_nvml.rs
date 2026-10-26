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

    /// Its PID, as `FAKE_NVML_PROCS` and `hmn`'s rows write it.
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

/// The PID of a process that has just exited and been reaped: a PID that
/// names no process, as a typo or a finished job would.
#[allow(clippy::expect_used)] // test-only
fn dead_pid() -> String {
    let mut child = Command::new("true").spawn().expect("failed to start true");
    let pid = child.id().to_string();
    let _ = child.wait();
    pid
}

/// Whether `text` holds a line whose words include `pid` followed by
/// `name`: a `hmn watch` row (time, PID, NAME, …) or a per-PID summary row.
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
const LOST_DEVICE_1_SKIP: &str = "hmn: failed to query device 1: no GPU measurement source \
     available (NVML and nvidia-smi both failed or are disabled) (skipped)";

/// The line the fake's one working H100 gives in `hmn`'s summary.
const H100_LINE: &str =
    "GPU 0 [NVIDIA H100 NVL]: free 15872 MiB / 16384 MiB (512 MiB reserved), driver 999.99";

/// A device that fails to answer is named on stderr and the others are
/// listed, exit `0`, where v0.2.14 dropped it without a word (issue #4,
/// note 5).
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

    let json = fake.hmn(&["ps", "--json"], &env).stdout;
    for index in [0, 1] {
        assert!(
            json.contains(&format!("\"pid\":{pid},"))
                && json.contains(&format!("\"device_index\":{index},")),
            "json: {json}"
        );
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

/// A watched PID absent from the device's listing is named by a direct
/// lookup, in its rows, the per-PID summary and `--json`, where v0.2.14
/// printed `?` (issue #4, note 2): PID 1 is alive, readable and holds no
/// GPU memory. A PID that names no process keeps `?` and its warning.
#[test]
fn watch_names_a_pid_absent_from_the_listing() {
    let Some(fake) = FakeNvml::build("watch-name") else {
        return;
    };
    let Ok(comm) = std::fs::read_to_string("/proc/1/comm") else {
        // A `/proc` mounted with `hidepid`: PID 1 cannot be named here.
        return;
    };
    let init = comm.trim();
    let dead = dead_pid();
    let env = [("FAKE_NVML_DEVICES", "NVIDIA H100 NVL")];
    let args = ["watch", "1", &dead, "--interval", "1s", "--duration", "1s"];
    let run = fake.hmn(&args, &env);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    // At least the attach row and the per-PID summary row.
    assert!(
        count_named_rows(&run.stdout, "1", init) >= 2,
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
        json.contains(&format!("\"pid\":1,\"name\":\"{init}\"")),
        "json: {json}"
    );
    assert!(
        json.contains(&format!("\"pid\":{dead},\"name\":null")),
        "json: {json}"
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
