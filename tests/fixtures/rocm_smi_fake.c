/* SPDX-License-Identifier: MIT OR Apache-2.0 */

/* A fake ROCm SMI for tests/cli_fake_rocm.rs, built as librocm_smi64.so.1
 * on Linux: it exports the ten ROCm SMI symbols hypomnesis resolves and
 * answers them from environment variables, read again on every call, so a
 * test describes the GPUs it wants without touching one. It loads no real
 * ROCm SMI.
 *
 *   FAKE_RSMI_INIT=<code>     rsmi_init's return code (default 0); 8 is
 *                             RSMI_STATUS_INIT_ERROR, no AMD driver, and 6
 *                             is RSMI_STATUS_INTERNAL_EXCEPTION.
 *   FAKE_RSMI_COUNT=<code>    rsmi_num_monitor_devices' return code
 *                             (default 0).
 *   FAKE_RSMI_DEVICES=<n>,... the devices in index order, each a model name;
 *                             unset or empty is a count of 0.
 *   FAKE_RSMI_PROCS=<dev>:<pid>:<bytes>,...
 *                             compute processes, `bytes` a decimal u64: the
 *                             bytes the PID holds on that device. A PID on
 *                             two devices has two entries.
 *
 * Like the real library, rsmi_compute_process_info_get lists every PID once
 * with vram_usage 0, rsmi_compute_process_gpus_get gives the devices of one
 * PID, and rsmi_compute_process_info_by_device_get gives its bytes on one
 * device (RSMI_STATUS_SUCCESS with vram_usage 0 for a listed PID that holds
 * none there, RSMI_STATUS_NOT_FOUND for a PID the list does not have).
 *
 * Every device has TOTAL_BYTES, of which its process rows use what they
 * hold, up to the total, so `free` never wraps; the driver reads
 * DRIVER_VERSION. Limits: at most MAX_DEVICES devices and MAX_PROCS rows
 * (the rest are ignored), and an empty entry in FAKE_RSMI_DEVICES (`"A,"`)
 * is a device with an empty name. The struct matches the repr(C) layout in
 * src/gpu/rocm.rs, pinned by the _Static_asserts below. */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define RSMI_STATUS_SUCCESS 0u
#define RSMI_STATUS_INVALID_ARGS 1u
#define RSMI_STATUS_NOT_FOUND 10u
#define RSMI_STATUS_INSUFFICIENT_SIZE 11u

#define MAX_DEVICES 16u
#define MAX_PROCS 64u
#define NAME_SIZE 96u
#define MIB (1024ull * 1024ull)
#define TOTAL_BYTES (16384ull * MIB)
#define DRIVER_VERSION "6.99.0"

typedef struct {
    uint32_t process_id;
    uint32_t pasid;
    uint64_t vram_usage;
    uint64_t sdma_usage;
    uint32_t cu_occupancy;
} rsmi_process_info_t;

_Static_assert(sizeof(rsmi_process_info_t) == 32, "RsmiProcessInfo is 32 bytes");
_Static_assert(_Alignof(rsmi_process_info_t) == 8, "RsmiProcessInfo is 8-byte aligned");
_Static_assert(offsetof(rsmi_process_info_t, process_id) == 0, "process_id at 0");
_Static_assert(offsetof(rsmi_process_info_t, pasid) == 4, "pasid at 4");
_Static_assert(offsetof(rsmi_process_info_t, vram_usage) == 8, "vram_usage at 8");
_Static_assert(offsetof(rsmi_process_info_t, sdma_usage) == 16, "sdma_usage at 16");
_Static_assert(offsetof(rsmi_process_info_t, cu_occupancy) == 24, "cu_occupancy at 24");

struct device {
    char name[NAME_SIZE];
};

struct proc {
    unsigned dev;
    uint32_t pid;
    uint64_t bytes;
};

/* The return code an environment variable names, 0 when unset. */
static unsigned code_from_env(const char *var) {
    const char *value = getenv(var);
    return value ? (unsigned)strtoul(value, NULL, 10) : RSMI_STATUS_SUCCESS;
}

/* FAKE_RSMI_DEVICES, parsed into `devs`; returns the device count. */
static unsigned parse_devices(struct device devs[MAX_DEVICES]) {
    const char *p = getenv("FAKE_RSMI_DEVICES");
    unsigned n = 0;
    if (p == NULL || *p == '\0') {
        return 0;
    }
    while (n < MAX_DEVICES) {
        const char *end = strchr(p, ',');
        size_t len = end ? (size_t)(end - p) : strlen(p);
        if (len >= NAME_SIZE) {
            len = NAME_SIZE - 1;
        }
        memcpy(devs[n].name, p, len);
        devs[n].name[len] = '\0';
        n++;
        if (end == NULL) {
            break;
        }
        p = end + 1;
    }
    return n;
}

/* FAKE_RSMI_PROCS, parsed into `procs`; returns the row count. */
static unsigned parse_procs(struct proc procs[MAX_PROCS]) {
    const char *p = getenv("FAKE_RSMI_PROCS");
    unsigned n = 0;
    if (p == NULL) {
        return 0;
    }
    while (n < MAX_PROCS && *p != '\0') {
        char *end;
        unsigned long dev = strtoul(p, &end, 10);
        if (*end != ':') {
            break;
        }
        unsigned long pid = strtoul(end + 1, &end, 10);
        if (*end != ':') {
            break;
        }
        unsigned long long bytes = strtoull(end + 1, &end, 10);
        procs[n].dev = (unsigned)dev;
        procs[n].pid = (uint32_t)pid;
        procs[n].bytes = (uint64_t)bytes;
        n++;
        if (*end != ',') {
            break;
        }
        p = end + 1;
    }
    return n;
}

/* The distinct PIDs of `procs`, in order of first appearance; returns how
 * many. */
static unsigned distinct_pids(const struct proc *procs, unsigned n, uint32_t pids[MAX_PROCS]) {
    unsigned count = 0;
    for (unsigned i = 0; i < n; i++) {
        unsigned seen = 0;
        for (unsigned j = 0; j < count; j++) {
            seen |= pids[j] == procs[i].pid;
        }
        if (!seen) {
            pids[count++] = procs[i].pid;
        }
    }
    return count;
}

/* The bytes the processes of device `index` hold, at most the total, so
 * the free figure below never wraps. */
static uint64_t process_bytes(unsigned index) {
    struct proc procs[MAX_PROCS];
    unsigned n = parse_procs(procs);
    uint64_t sum = 0;
    for (unsigned i = 0; i < n; i++) {
        if (procs[i].dev == index && procs[i].bytes <= TOTAL_BYTES) {
            sum += procs[i].bytes;
            if (sum > TOTAL_BYTES) {
                sum = TOTAL_BYTES;
            }
        }
    }
    return sum;
}

unsigned rsmi_init(uint64_t flags) {
    (void)flags;
    return code_from_env("FAKE_RSMI_INIT");
}

unsigned rsmi_shut_down(void) {
    return RSMI_STATUS_SUCCESS;
}

unsigned rsmi_num_monitor_devices(uint32_t *count) {
    unsigned code = code_from_env("FAKE_RSMI_COUNT");
    struct device devs[MAX_DEVICES];
    if (code != RSMI_STATUS_SUCCESS) {
        return code;
    }
    if (count == NULL) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    *count = parse_devices(devs);
    return RSMI_STATUS_SUCCESS;
}

unsigned rsmi_dev_name_get(uint32_t index, char *name, size_t length) {
    struct device devs[MAX_DEVICES];
    unsigned count = parse_devices(devs);
    if (name == NULL || index >= count) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    if (strlen(devs[index].name) + 1 > length) {
        return RSMI_STATUS_INSUFFICIENT_SIZE;
    }
    memcpy(name, devs[index].name, strlen(devs[index].name) + 1);
    return RSMI_STATUS_SUCCESS;
}

unsigned rsmi_dev_memory_total_get(uint32_t index, uint32_t type, uint64_t *total) {
    struct device devs[MAX_DEVICES];
    if (total == NULL || type != 0 || index >= parse_devices(devs)) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    *total = TOTAL_BYTES;
    return RSMI_STATUS_SUCCESS;
}

unsigned rsmi_dev_memory_usage_get(uint32_t index, uint32_t type, uint64_t *used) {
    struct device devs[MAX_DEVICES];
    if (used == NULL || type != 0 || index >= parse_devices(devs)) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    *used = process_bytes(index);
    return RSMI_STATUS_SUCCESS;
}

unsigned rsmi_version_str_get(uint32_t component, char *version, uint32_t length) {
    if (version == NULL || component != 0) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    if (sizeof DRIVER_VERSION > length) {
        return RSMI_STATUS_INSUFFICIENT_SIZE;
    }
    memcpy(version, DRIVER_VERSION, sizeof DRIVER_VERSION);
    return RSMI_STATUS_SUCCESS;
}

/* The library's contract: with a NULL buffer, set *count to the number of
 * processes; with *count smaller than that, return
 * RSMI_STATUS_INSUFFICIENT_SIZE; otherwise write one row per PID, vram_usage
 * zero, and set *count to how many were written. */
unsigned rsmi_compute_process_info_get(rsmi_process_info_t *infos, uint32_t *count) {
    struct proc procs[MAX_PROCS];
    uint32_t pids[MAX_PROCS];
    if (count == NULL) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    unsigned n = distinct_pids(procs, parse_procs(procs), pids);
    if (infos == NULL) {
        *count = n;
        return RSMI_STATUS_SUCCESS;
    }
    if (*count < n) {
        *count = n;
        return RSMI_STATUS_INSUFFICIENT_SIZE;
    }
    for (unsigned i = 0; i < n; i++) {
        memset(&infos[i], 0, sizeof infos[i]);
        infos[i].process_id = pids[i];
    }
    *count = n;
    return RSMI_STATUS_SUCCESS;
}

/* The devices one PID holds memory on, by the same two-call protocol. */
unsigned rsmi_compute_process_gpus_get(uint32_t pid, uint32_t *devices, uint32_t *count) {
    struct proc procs[MAX_PROCS];
    unsigned n = parse_procs(procs);
    unsigned rows = 0;
    if (count == NULL) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    for (unsigned i = 0; i < n; i++) {
        rows += procs[i].pid == pid;
    }
    if (devices == NULL) {
        *count = rows;
        return RSMI_STATUS_SUCCESS;
    }
    if (*count < rows) {
        *count = rows;
        return RSMI_STATUS_INSUFFICIENT_SIZE;
    }
    unsigned written = 0;
    for (unsigned i = 0; i < n; i++) {
        if (procs[i].pid == pid) {
            devices[written++] = procs[i].dev;
        }
    }
    *count = written;
    return RSMI_STATUS_SUCCESS;
}

unsigned rsmi_compute_process_info_by_device_get(uint32_t pid, uint32_t index,
                                                 rsmi_process_info_t *info) {
    struct proc procs[MAX_PROCS];
    unsigned n = parse_procs(procs);
    unsigned known = 0;
    if (info == NULL) {
        return RSMI_STATUS_INVALID_ARGS;
    }
    for (unsigned i = 0; i < n; i++) {
        if (procs[i].pid == pid) {
            known = 1;
            if (procs[i].dev == index) {
                memset(info, 0, sizeof *info);
                info->process_id = pid;
                info->vram_usage = procs[i].bytes;
                return RSMI_STATUS_SUCCESS;
            }
        }
    }
    if (!known) {
        return RSMI_STATUS_NOT_FOUND;
    }
    /* A PID the library lists, with no memory on this device: success, 0 B. */
    memset(info, 0, sizeof *info);
    info->process_id = pid;
    return RSMI_STATUS_SUCCESS;
}
