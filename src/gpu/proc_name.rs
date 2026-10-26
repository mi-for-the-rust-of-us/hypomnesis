// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linux process names for the `NVML` process listing and for
//! [`crate::process_name`].
//!
//! The kernel's `comm` (`/proc/<pid>/comm`) is world-readable but cut to
//! [`COMM_MAX_BYTES`] bytes, so an executable named
//! `figure13_newline_patch` is listed as `figure13_newlin` — and a name
//! filter such as `hmn watch --filter figure13_newline_patch` can never
//! match it. [`read_proc_name`] keeps `comm` as the source and only
//! *extends* a full-length one, from the first longer name that starts
//! with it: the file name of the `/proc/<pid>/exe` link, else of
//! `argv[0]` in `/proc/<pid>/cmdline`. A `comm` shorter than the limit
//! was never cut and is returned as is, so no name that fits changes.
//!
//! The `exe` link is readable only for processes the caller could
//! `ptrace` (same user, or root); `cmdline` is world-readable on standard
//! kernels, so `argv[0]` covers most other users' processes. When neither
//! extends it, the cut `comm` is still returned — as before.

use std::os::unix::ffi::OsStringExt;

/// The longest `comm` the kernel stores: `TASK_COMM_LEN` (16) minus the
/// terminating NUL. A `comm` of exactly this length may have been cut.
const COMM_MAX_BYTES: usize = 15;

/// The name of process `pid`: its `comm`, extended past the kernel's
/// [`COMM_MAX_BYTES`] cut when the `exe` link or `argv[0]` shows the full
/// name — see the [module docs][self].
///
/// `None` when `comm` cannot be read (process exited, permissions
/// stripped), is blank, or is not UTF-8 — name resolution is
/// best-effort.
pub(super) fn read_proc_name(pid: u32) -> Option<String> {
    let comm = read_comm(pid)?;
    if comm.len() < COMM_MAX_BYTES {
        return String::from_utf8(comm).ok();
    }
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        // BORROW: PathBuf → raw bytes; a Linux path need not be UTF-8,
        // and `untruncate` compares bytes.
        .map(|p| exe_file_name(&p.into_os_string().into_vec()));
    let argv0 = std::fs::read(format!("/proc/{pid}/cmdline"))
        .ok()
        .and_then(|c| argv0_file_name(&c));
    String::from_utf8(untruncate(comm, [exe, argv0])).ok()
}

/// `/proc/<pid>/comm` without its trailing newline and surrounding ASCII
/// whitespace; `None` if unreadable or blank.
fn read_comm(pid: u32) -> Option<Vec<u8>> {
    let content = std::fs::read(format!("/proc/{pid}/comm")).ok()?;
    let trimmed = content.trim_ascii();
    // BORROW: trimmed slice → owned bytes, returned past `content`.
    (!trimmed.is_empty()).then(|| trimmed.to_vec())
}

/// The first of `candidates` that starts with `comm` — `comm` itself
/// when none does. A candidate equal to `comm` proves it was not cut and
/// ends the search, so a later, longer candidate cannot override it.
#[must_use]
fn untruncate(comm: Vec<u8>, candidates: impl IntoIterator<Item = Option<Vec<u8>>>) -> Vec<u8> {
    candidates
        .into_iter()
        .flatten()
        .find(|c| c.starts_with(&comm))
        .unwrap_or(comm)
}

/// The file name of an `exe` link target, without the ` (deleted)` the
/// kernel appends once the executable has been removed from disk.
#[must_use]
fn exe_file_name(target: &[u8]) -> Vec<u8> {
    let target = target.strip_suffix(b" (deleted)").unwrap_or(target);
    // BORROW: borrowed file-name slice → owned bytes for the caller.
    file_name(target).to_vec()
}

/// The file name of `argv[0]`, the first NUL-terminated field of
/// `/proc/<pid>/cmdline`; `None` when it is empty (kernel threads, or a
/// process that has cleared its command line).
#[must_use]
fn argv0_file_name(cmdline: &[u8]) -> Option<Vec<u8>> {
    let argv0 = cmdline.split(|&b| b == 0).next().unwrap_or_default();
    let name = file_name(argv0);
    // BORROW: borrowed file-name slice → owned bytes for the caller.
    (!name.is_empty()).then(|| name.to_vec())
}

/// The last `/`-separated component of `path`.
#[must_use]
fn file_name(path: &[u8]) -> &[u8] {
    path.rsplit(|&b| b == b'/').next().unwrap_or(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn bytes(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn untruncate_extends_from_the_first_candidate_that_starts_with_comm() {
        let comm = bytes("figure13_newlin");
        let exe = Some(bytes("figure13_newline_patch"));
        assert_eq!(
            untruncate(comm, [exe, Some(bytes("ignored"))]),
            b"figure13_newline_patch"
        );
    }

    #[test]
    fn untruncate_skips_a_non_matching_exe_for_argv0() {
        // Run through a symlink: `exe` resolves to the target, `argv[0]`
        // keeps the name the process was started by.
        let comm = bytes("figure13_newlin");
        let exe = Some(bytes("sleep"));
        let argv0 = Some(bytes("figure13_newline_patch"));
        assert_eq!(untruncate(comm, [exe, argv0]), b"figure13_newline_patch");
    }

    #[test]
    fn untruncate_keeps_comm_when_nothing_matches() {
        let comm = bytes("figure13_newlin");
        assert_eq!(untruncate(comm.clone(), [None, None]), comm);
        assert_eq!(
            untruncate(comm.clone(), [Some(bytes("python3")), Some(bytes("x"))]),
            comm
        );
    }

    #[test]
    fn untruncate_stops_at_a_candidate_equal_to_comm() {
        // An `exe` exactly as long as `comm` proves it was never cut; a
        // rewritten `argv[0]` that happens to extend it must not win.
        let comm = bytes("exactly15bytes_");
        let exe = Some(comm.clone());
        let argv0 = Some(bytes("exactly15bytes_and_more"));
        assert_eq!(untruncate(comm.clone(), [exe, argv0]), comm);
    }

    #[test]
    fn exe_file_name_strips_directories_and_deleted_marker() {
        assert_eq!(
            exe_file_name(b"/opt/run/figure13_newline_patch"),
            b"figure13_newline_patch"
        );
        assert_eq!(exe_file_name(b"/opt/run/train (deleted)"), b"train");
        assert_eq!(exe_file_name(b"train"), b"train");
    }

    #[test]
    fn argv0_file_name_takes_the_first_field() {
        assert_eq!(
            argv0_file_name(b"./target/release/figure13_newline_patch\0--layer\x0012\0").unwrap(),
            b"figure13_newline_patch"
        );
        assert!(argv0_file_name(b"").is_none());
        assert!(argv0_file_name(b"\0").is_none());
    }

    #[test]
    fn read_proc_name_recovers_this_test_binarys_full_name() {
        // Cargo names unit-test binaries `<crate>-<16 hex digits>`, well
        // past the 15-byte cut, so this exercises the real `/proc` path.
        let exe = std::env::current_exe().unwrap();
        let expected = exe.file_name().unwrap().to_str().unwrap();
        assert!(expected.len() > COMM_MAX_BYTES, "{expected}");
        assert_eq!(read_proc_name(std::process::id()).unwrap(), expected);
    }

    #[test]
    fn read_proc_name_recovers_a_symlinked_process_from_argv0() {
        // `exe` resolves the symlink to `sleep`; only `argv[0]` still
        // carries the long name the process was started by.
        let dir = std::env::temp_dir().join(format!("hypomnesis-proc-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("hypomnesis_long_probe_name");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/bin/sleep", &link).unwrap();
        let mut child = std::process::Command::new(&link).arg("30").spawn().unwrap();
        // The child's `comm` is set by `execve`; poll until it is, rather
        // than racing the fork.
        let mut name = None;
        for _ in 0..100 {
            name = read_proc_name(child.id());
            if name.as_deref() == Some("hypomnesis_long_probe_name") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(name.as_deref(), Some("hypomnesis_long_probe_name"));
    }
}
