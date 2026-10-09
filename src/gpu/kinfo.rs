// SPDX-License-Identifier: MIT OR Apache-2.0

//! macOS `kinfo_proc` records, read as plain bytes.
//!
//! `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID, pid)` answers with the one
//! `struct kinfo_proc` of a process, and `KERN_PROC_ALL` with one per
//! process in the table. This module parses those records from a byte
//! buffer at fixed offsets, and holds the rule that turns a
//! `proc_pidpath` result and a `KERN_PROC_PID` result into the answer
//! of `process_exists`. It has no `unsafe`: the `sysctl` call itself is
//! in `src/gpu/metal.rs`.
//!
//! # Layout, and the architectures it was checked on
//!
//! A record is `KINFO_PROC_SIZE` = 648 bytes, with `kp_proc.p_pid` at
//! byte 40 and `kp_proc.p_comm` (17 bytes, `MAXCOMLEN + 1`) at byte 243.
//! The values come from the SDK header (Xcode 26.2, the macOS SDK, read
//! with `clang -isysroot $(xcrun --show-sdk-path)`), checked here:
//!
//! - arm64 (native): the header probe with `clang -arch arm64`, and a
//!   live `KERN_PROC_PID` read of `launchd` and of the test process on
//!   an Apple M3 Pro, macOS 26.6.2.
//! - `x86_64`: the same header probe with `clang -arch x86_64`, and
//!   `cargo test --target x86_64-apple-darwin --lib` under Rosetta 2,
//!   which runs the live anchor test
//!   `kern_proc_pid_record_matches_the_kernel_for_this_process`. Rosetta
//!   shows what `x86_64` userland sees on the arm64 kernel, not what an
//!   Intel kernel returns.
//! - Native Intel hardware: untested (`ROADMAP.md` lists Intel Macs as
//!   untested hardware).
//!
//! The measurements are in
//! `field_check_v0213/evidence/kinfo_proc_layout.md` at `f03298a7bb`.

/// `sizeof(struct kinfo_proc)`, in bytes.
pub(super) const KINFO_PROC_SIZE: usize = 648;

/// `offsetof(struct kinfo_proc, kp_proc.p_pid)`: an `i32` in native
/// byte order.
pub(super) const P_PID_OFFSET: usize = 40;

/// `offsetof(struct kinfo_proc, kp_proc.p_comm)`.
pub(super) const P_COMM_OFFSET: usize = 243;

/// `sizeof(kp_proc.p_comm)`: `MAXCOMLEN + 1`, the last byte always NUL.
pub(super) const P_COMM_SIZE: usize = 17;

/// `ESRCH` from `<errno.h>`: no process with this PID. `proc_pidpath`
/// answers it for a PID that names nothing, or that has no executable
/// path (`kernel_task`); `sysctl` never answers it.
pub(super) const ESRCH: i32 = 3;

/// `ENOMEM` from `<errno.h>`: the buffer is too small. `sysctl` answers
/// it when the records do not all fit the buffer, and writes back a `len`
/// of 0. [`classify_kern_proc_pid`] and [`classify_kern_proc_all`] answer
/// it.
pub(super) const ENOMEM: i32 = 12;

/// `EPERM` from `<errno.h>`: the caller's sandbox refuses the call.
/// `ledger`, `proc_listpids`, `proc_pidpath` and `sysctl` answer it that
/// way.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(super) const EPERM: i32 = 1;

/// `CTL_KERN` from `<sys/sysctl.h>`: the top-level kernel MIB.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(super) const CTL_KERN: i32 = 1;

/// `KERN_PROC` from `<sys/sysctl.h>`: the process-table MIB.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(super) const KERN_PROC: i32 = 14;

/// `KERN_PROC_PID` from `<sys/sysctl.h>`: select one process by PID.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(super) const KERN_PROC_PID: i32 = 1;

/// `KERN_PROC_ALL` from `<sys/sysctl.h>`: select every process.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub(super) const KERN_PROC_ALL: i32 = 0;

/// The fields of one `kinfo_proc` record this crate reads.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct KinfoRecord {
    /// `kp_proc.p_pid`.
    pub(super) pid: i32,
    /// `kp_proc.p_comm`, the bytes before its first NUL (at most 16).
    pub(super) comm: Vec<u8>,
}

/// What one `KERN_PROC_ALL` fill said.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum KernProcAllAttempt {
    /// The call succeeded with these records.
    Records(Vec<KinfoRecord>),
    /// The call failed with `ENOMEM`: the table outgrew the buffer.
    Retry,
    /// The call failed with another `errno`, or its answer is unusable.
    Failed,
}

/// What `proc_pidpath` said about a PID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PathLookup {
    /// It returned a path: the process exists.
    Found,
    /// It returned no path, with this `errno`.
    Failed {
        /// The `errno` `proc_pidpath` left (`ESRCH`, `EPERM`, ...).
        errno: i32,
    },
}

/// What `sysctl` `KERN_PROC_PID` said about a PID.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PidLookup {
    /// The call succeeded with one record for the requested PID.
    Record,
    /// The call succeeded with no record: no such process.
    NoRecord,
    /// The call failed with this `errno`, any but `ENOMEM` (a sandbox
    /// refusal, ...).
    Refused {
        /// The `errno` `sysctl` left.
        errno: i32,
    },
    /// The call succeeded but its buffer is not one record for the
    /// requested PID, or it failed with `ENOMEM` because the record does
    /// not fit the buffer: can't tell.
    Unusable,
}

/// Parse a buffer of whole `kinfo_proc` records.
///
/// `None` unless `buf.len()` is a multiple of `KINFO_PROC_SIZE`: a
/// partial record is never parsed. An empty buffer gives no records.
/// `comm` is `p_comm` cut at its first NUL; its 17th byte is always NUL,
/// so a 16-byte name comes back whole.
///
/// The offsets are the SDK header's, verified on arm64 natively and on
/// `x86_64` under Rosetta 2 (by the header probe and the live anchor
/// test); native Intel hardware is untested. See the module docs.
pub(super) fn parse_kinfo_records(buf: &[u8]) -> Option<Vec<KinfoRecord>> {
    let (chunks, rest) = buf.as_chunks::<KINFO_PROC_SIZE>();
    if !rest.is_empty() {
        return None;
    }
    chunks
        .iter()
        .map(|chunk| {
            let pid_bytes = chunk.get(P_PID_OFFSET..P_PID_OFFSET + size_of::<i32>())?;
            let pid = i32::from_ne_bytes(pid_bytes.try_into().ok()?);
            let comm_field = chunk.get(P_COMM_OFFSET..P_COMM_OFFSET + P_COMM_SIZE)?;
            let comm = comm_field
                .split(|&byte| byte == 0)
                .next()
                .unwrap_or(comm_field);
            // BORROW: `to_vec` copies the name out of the caller's buffer.
            Some(KinfoRecord {
                pid,
                comm: comm.to_vec(),
            })
        })
        .collect()
}

/// Classify the result of one `KERN_PROC_PID` call: its return code
/// `rc`, the `errno` it left, its buffer and the `len` it wrote back,
/// for the requested `pid`.
///
/// `rc` is read first, and on a failed call `buf` and `len` are never
/// read, because a refused call still leaves `len` at 648 and the buffer
/// zeroed (measured under a sandbox that denies `kern.proc`), which would
/// parse as a record for PID 0. A call that failed with `ENOMEM` is
/// `Unusable`: the kernel's record is larger than `KINFO_PROC_SIZE`, so
/// it did not fit and nothing was copied. Any other failure is `Refused`.
///
/// A successful call that wrote nothing (`len == 0`) is `NoRecord`: no
/// such process. Otherwise the first `len` bytes must be exactly one
/// record for `pid`; anything else (a partial record, two records,
/// another PID, a `len` past the buffer) is `Unusable`.
pub(super) fn classify_kern_proc_pid(
    rc: i32,
    errno: i32,
    buf: &[u8],
    len: usize,
    pid: i32,
) -> PidLookup {
    if rc != 0 && errno == ENOMEM {
        return PidLookup::Unusable;
    }
    if rc != 0 {
        return PidLookup::Refused { errno };
    }
    if len == 0 {
        return PidLookup::NoRecord;
    }
    let Some(bytes) = buf.get(..len) else {
        return PidLookup::Unusable;
    };
    match parse_kinfo_records(bytes).as_deref() {
        Some([record]) if record.pid == pid => PidLookup::Record,
        Some(_) | None => PidLookup::Unusable,
    }
}

/// Classify the result of one `KERN_PROC_ALL` fill: its return code `rc`,
/// the `errno` it left, its buffer and the `len` it wrote back.
///
/// `rc` is read first, and on a failed call `buf` and `len` are never read:
/// a fill that fails with `ENOMEM` has copied the whole records that fit
/// and written back a `len` of 0, so its buffer holds records that must not
/// be parsed. `ENOMEM` is `Retry`, any other `errno` is `Failed`. On success
/// the first `len` bytes must be whole records (`len` past the buffer, or a
/// partial record, is `Failed`).
pub(super) fn classify_kern_proc_all(
    rc: i32,
    errno: i32,
    buf: &[u8],
    len: usize,
) -> KernProcAllAttempt {
    if rc != 0 {
        return if errno == ENOMEM {
            KernProcAllAttempt::Retry
        } else {
            KernProcAllAttempt::Failed
        };
    }
    buf.get(..len)
        .and_then(parse_kinfo_records)
        .map_or(KernProcAllAttempt::Failed, KernProcAllAttempt::Records)
}

/// Decide whether a PID exists: `proc_pidpath` first, then `sysctl`
/// `KERN_PROC_PID`, asked only when there is no path.
///
/// | `proc_pidpath` | `KERN_PROC_PID` | answer |
/// |---|---|---|
/// | a path | not asked | `Some(true)` |
/// | no path | `Record` | `Some(true)` |
/// | no path | `NoRecord` | `Some(false)` |
/// | no path, `ESRCH` | `Refused` | `Some(false)` |
/// | no path, other `errno` | `Refused` | `None` |
/// | no path | `Unusable` (including `ENOMEM`) | `None` |
///
/// A path answers first, so every case `proc_pidpath` answered before
/// keeps its answer. `kernel_task` (PID 0) has no path, so `proc_pidpath`
/// says `ESRCH` and `sysctl` finds it; no PID is special-cased. When
/// both calls are refused, the `ESRCH` that decides is `proc_pidpath`'s:
/// `sysctl` never answers `ESRCH`, so R01's "refused too: `ESRCH` →
/// `Some(false)`" can only mean that one. The accepted residual: PID 0
/// with both calls refused reads `None` where `proc_pidpath` says
/// `EPERM`, but would read `Some(false)` where it says `ESRCH`.
pub(super) fn decide_exists(
    path: PathLookup,
    kern_proc_pid: impl FnOnce() -> PidLookup,
) -> Option<bool> {
    match path {
        PathLookup::Found => Some(true),
        PathLookup::Failed { errno: path_errno } => match kern_proc_pid() {
            PidLookup::Record => Some(true),
            PidLookup::NoRecord => Some(false),
            PidLookup::Refused { .. } => (path_errno == ESRCH).then_some(false),
            PidLookup::Unusable => None,
        },
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    // INDEX: `record` writes at fixed offsets into its 648-byte buffer.
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A zeroed 648-byte record with `pid` at bytes 40..44 and `comm`
    /// from byte 243, written at literal offsets so a changed constant
    /// fails a test.
    fn record(pid: i32, comm: &[u8]) -> Vec<u8> {
        let mut buf = vec![0_u8; 648];
        buf[40..44].copy_from_slice(&pid.to_ne_bytes());
        buf[243..243 + comm.len()].copy_from_slice(comm);
        buf
    }

    #[test]
    fn parse_kinfo_records_reads_pid_and_comm_at_the_header_offsets() {
        let records = parse_kinfo_records(&record(1, b"launchd")).unwrap();
        assert_eq!(
            records,
            vec![KinfoRecord {
                pid: 1,
                comm: b"launchd".to_vec()
            }]
        );

        // A 16-byte name fills `p_comm` up to its always-NUL 17th byte,
        // and comes back whole.
        let full = b"abcdefghijklmnop";
        let records = parse_kinfo_records(&record(4242, full)).unwrap();
        assert_eq!(
            records,
            vec![KinfoRecord {
                pid: 4242,
                comm: full.to_vec()
            }]
        );
    }

    #[test]
    fn parse_kinfo_records_rejects_a_partial_record() {
        assert_eq!(parse_kinfo_records(&[0_u8; 647]), None);
        assert_eq!(parse_kinfo_records(&[0_u8; 649]), None);
        assert_eq!(parse_kinfo_records(&[0_u8; 1]), None);

        let v = parse_kinfo_records(&[]).unwrap();
        assert!(v.is_empty(), "{v:?}");

        let mut two = record(1, b"launchd");
        two.extend_from_slice(&record(77, b"WindowServer"));
        let records = parse_kinfo_records(&two).unwrap();
        assert_eq!(
            records,
            vec![
                KinfoRecord {
                    pid: 1,
                    comm: b"launchd".to_vec()
                },
                KinfoRecord {
                    pid: 77,
                    comm: b"WindowServer".to_vec()
                },
            ]
        );
    }

    #[test]
    fn classify_kern_proc_pid_refused_call_is_not_a_record() {
        // Measured under a profile that denies `kern.proc`: the refused
        // call leaves `len` at 648 and the buffer zeroed, which parses
        // as a record for PID 0 if `rc` is ignored.
        let zeroed = [0_u8; 648];
        assert_eq!(
            classify_kern_proc_pid(-1, 1, &zeroed, 648, 0),
            PidLookup::Refused { errno: 1 }
        );
        // Every errno but `ENOMEM` is `Refused` (here `EINVAL`), so a dead
        // PID whose `proc_pidpath` says `ESRCH` keeps reading `Some(false)`.
        assert_eq!(
            classify_kern_proc_pid(-1, 22, &zeroed, 648, 0),
            PidLookup::Refused { errno: 22 }
        );
    }

    #[test]
    fn classify_kern_proc_pid_a_record_that_does_not_fit_is_unusable() {
        // A kernel whose `kinfo_proc` is larger than 648 bytes copies
        // nothing, fails with `ENOMEM` and writes back a `len` of 0.
        let zeroed = [0_u8; 648];
        assert_eq!(
            classify_kern_proc_pid(-1, 12, &zeroed, 0, 0),
            PidLookup::Unusable
        );
        // `errno` counts only when `rc` says the call failed.
        assert_eq!(
            classify_kern_proc_pid(0, 12, &record(1, b"launchd"), 648, 1),
            PidLookup::Record
        );
    }

    #[test]
    fn classify_kern_proc_pid_no_record_is_len_zero() {
        let zeroed = [0_u8; 648];
        assert_eq!(
            classify_kern_proc_pid(0, 0, &zeroed, 0, 2_147_483_647),
            PidLookup::NoRecord
        );
    }

    #[test]
    fn classify_kern_proc_pid_record_must_name_the_requested_pid() {
        let one = record(1, b"launchd");
        assert_eq!(
            classify_kern_proc_pid(0, 0, &one, 648, 1),
            PidLookup::Record
        );
        // A record for another PID.
        assert_eq!(
            classify_kern_proc_pid(0, 0, &one, 648, 2),
            PidLookup::Unusable
        );
        // A partial record.
        assert_eq!(
            classify_kern_proc_pid(0, 0, &one, 647, 1),
            PidLookup::Unusable
        );
        // A length past the buffer.
        assert_eq!(
            classify_kern_proc_pid(0, 0, &one, 1296, 1),
            PidLookup::Unusable
        );
        // Two records.
        let mut two = record(1, b"launchd");
        two.extend_from_slice(&record(1, b"launchd"));
        assert_eq!(
            classify_kern_proc_pid(0, 0, &two, 1296, 1),
            PidLookup::Unusable
        );
    }

    #[test]
    fn classify_kern_proc_all_whole_records_with_rc_zero_are_records() {
        let mut two = record(1, b"launchd");
        two.extend_from_slice(&record(77, b"WindowServer"));
        assert_eq!(
            classify_kern_proc_all(0, 0, &two, 1296),
            KernProcAllAttempt::Records(vec![
                KinfoRecord {
                    pid: 1,
                    comm: b"launchd".to_vec()
                },
                KinfoRecord {
                    pid: 77,
                    comm: b"WindowServer".to_vec()
                },
            ])
        );
        // Only the first `len` bytes are records.
        assert_eq!(
            classify_kern_proc_all(0, 0, &two, 648),
            KernProcAllAttempt::Records(vec![KinfoRecord {
                pid: 1,
                comm: b"launchd".to_vec()
            }])
        );
    }

    #[test]
    fn classify_kern_proc_all_enomem_is_retry_even_when_the_buffer_holds_records() {
        // `ENOMEM` is `Retry` whatever the buffer holds.
        let mut two = record(1, b"launchd");
        two.extend_from_slice(&record(77, b"WindowServer"));
        assert_eq!(
            classify_kern_proc_all(-1, 12, &two, 0),
            KernProcAllAttempt::Retry
        );
        assert_eq!(
            classify_kern_proc_all(-1, 12, &two, 1296),
            KernProcAllAttempt::Retry
        );
    }

    #[test]
    fn classify_kern_proc_all_a_failed_call_other_than_enomem_is_failed() {
        let zeroed = [0_u8; 648];
        // `EPERM`, `ESRCH` and `EINVAL`.
        assert_eq!(
            classify_kern_proc_all(-1, 1, &zeroed, 648),
            KernProcAllAttempt::Failed
        );
        assert_eq!(
            classify_kern_proc_all(-1, 3, &zeroed, 0),
            KernProcAllAttempt::Failed
        );
        assert_eq!(
            classify_kern_proc_all(-1, 22, &zeroed, 0),
            KernProcAllAttempt::Failed
        );
    }

    #[test]
    fn classify_kern_proc_all_an_unusable_success_is_failed() {
        let one = record(1, b"launchd");
        // A `len` past the buffer.
        assert_eq!(
            classify_kern_proc_all(0, 0, &one, 1296),
            KernProcAllAttempt::Failed
        );
        // A partial record.
        assert_eq!(
            classify_kern_proc_all(0, 0, &one, 647),
            KernProcAllAttempt::Failed
        );
    }

    #[test]
    fn decide_exists_follows_the_lookup_rule() {
        // A path answers first: `sysctl` is not asked.
        let called = Cell::new(false);
        assert_eq!(
            decide_exists(PathLookup::Found, || {
                called.set(true);
                PidLookup::Unusable
            }),
            Some(true)
        );
        assert!(
            !called.get(),
            "KERN_PROC_PID asked although a path was found"
        );

        // `ESRCH` here is `proc_pidpath`'s errno: `sysctl` never answers it.
        let esrch = PathLookup::Failed { errno: 3 };
        assert_eq!(decide_exists(esrch, || PidLookup::Record), Some(true));
        assert_eq!(decide_exists(esrch, || PidLookup::NoRecord), Some(false));
        assert_eq!(
            decide_exists(esrch, || PidLookup::Refused { errno: 1 }),
            Some(false)
        );
        assert_eq!(decide_exists(esrch, || PidLookup::Unusable), None);
        // A record that does not fit (`ENOMEM`) is can't tell, not
        // no such process.
        assert_eq!(
            decide_exists(esrch, || classify_kern_proc_pid(-1, 12, &[0_u8; 648], 0, 0)),
            None
        );

        let eperm = PathLookup::Failed { errno: 1 };
        assert_eq!(decide_exists(eperm, || PidLookup::Record), Some(true));
        assert_eq!(decide_exists(eperm, || PidLookup::NoRecord), Some(false));
        assert_eq!(
            decide_exists(eperm, || PidLookup::Refused { errno: 1 }),
            None
        );
        assert_eq!(decide_exists(eperm, || PidLookup::Unusable), None);
    }
}
