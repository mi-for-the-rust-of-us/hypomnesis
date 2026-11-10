// SPDX-License-Identifier: MIT OR Apache-2.0

//! Live-GPU integration tests for the `ROCm` backend. Every test is
//! ignore-gated — they require an AMD GPU + `ROCm` to be installed on
//! the host. CI does not run these (hosted runners have no AMD hardware);
//! run them on an AMD host with:
//!
//! ```sh
//! cargo test --test live_rocm -- --ignored
//! ```
//!
//! The assertions hold for any device count of at least one.

#![cfg(all(target_os = "linux", feature = "rocm"))]

use hypomnesis::{GpuQuerySource, HypomnesisError, device_count, device_info, process_gpu_info};

#[test]
#[ignore = "requires AMD GPU + ROCm"]
#[allow(clippy::expect_used)]
fn device_count_is_positive() {
    let count = device_count().expect("AMD GPU + ROCm required");
    assert!(count >= 1, "expected at least one AMD GPU, got {count}");
}

#[test]
#[ignore = "requires AMD GPU + ROCm"]
#[allow(clippy::expect_used)]
fn device_totals_are_plausible() {
    let info = device_info(0).expect("device_info(0) requires AMD GPU + ROCm");
    assert!(
        info.total_bytes > info.used_bytes,
        "total_bytes={} <= used_bytes={}",
        info.total_bytes,
        info.used_bytes
    );
    assert_eq!(
        info.free_bytes,
        info.total_bytes - info.used_bytes,
        "free_bytes={} (expected total_bytes - used_bytes)",
        info.free_bytes
    );
}

#[test]
#[ignore = "requires AMD GPU + ROCm"]
#[allow(clippy::expect_used)]
fn reserved_bytes_is_none() {
    let info = device_info(0).expect("device_info(0) requires AMD GPU + ROCm");
    assert_eq!(
        info.reserved_bytes, None,
        "the ROCm path reports no driver carve-out"
    );
}

#[test]
#[ignore = "requires AMD GPU + ROCm"]
#[allow(clippy::expect_used)]
fn source_is_rocm() {
    let info = process_gpu_info(0).expect("process_gpu_info(0) requires AMD GPU + ROCm");
    assert_eq!(info.source, GpuQuerySource::Rocm);
}

#[test]
#[ignore = "requires AMD GPU + ROCm"]
#[allow(clippy::expect_used)]
fn index_past_the_count_is_out_of_range() {
    let count = device_count().expect("AMD GPU + ROCm required");
    assert!(count >= 1, "expected at least one AMD GPU, got {count}");

    let past = device_info(count);
    assert!(
        matches!(past, Err(HypomnesisError::DeviceIndexOutOfRange { .. })),
        "expected DeviceIndexOutOfRange for index {count}, got {past:?}"
    );

    let last = device_info(count - 1);
    assert!(last.is_ok(), "device_info({}) failed: {last:?}", count - 1);
}
