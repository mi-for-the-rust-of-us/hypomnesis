/* SPDX-License-Identifier: MIT OR Apache-2.0 */

/* A stand-in libnvidia-ml.so.1 for an NVIDIA stack that is installed but
 * fails to start: nvmlInit_v2 returns NVML_ERROR_LIB_RM_VERSION_MISMATCH
 * (18), as after a driver upgrade without a reboot. It exports only the two
 * symbols hypomnesis resolves before init, and calls no real NVML, so
 * nothing reaches a GPU. Built and loaded by tests/cli_broken_nvml.rs. */

int nvmlInit_v2(void) { return 18; }

int nvmlShutdown(void) { return 0; }
