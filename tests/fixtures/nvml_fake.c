/* SPDX-License-Identifier: MIT OR Apache-2.0 */

/* A fake libnvidia-ml.so.1 for tests/cli_fake_nvml.rs: it exports the nine
 * NVML symbols hypomnesis resolves and answers them from environment
 * variables, read again on every call, so a test describes the GPUs it
 * wants without touching one. It loads no real NVML.
 *
 *   FAKE_NVML_INIT=<code>     nvmlInit_v2's return code (default 0); 18 is
 *                             NVML_ERROR_LIB_RM_VERSION_MISMATCH, a driver
 *                             upgraded without a reboot.
 *   FAKE_NVML_COUNT=<code>    nvmlDeviceGetCount_v2's return code (default 0).
 *   FAKE_NVML_DEVICES=<e>,... the devices in index order; unset or empty is
 *                             a count of 0. Each entry is a model name, or
 *                             `lost` (no handle: NVML_ERROR_GPU_IS_LOST), or
 *                             `noprocs:<name>` (memory and name answer, the
 *                             process list returns NVML_ERROR_GPU_IS_LOST).
 *   FAKE_NVML_PROCS=<dev>:<pid>:<bytes>,...
 *                             compute processes, `bytes` a decimal u64.
 *
 * Every device has TOTAL_BYTES, of which RESERVED_BYTES is reserved, and
 * the driver reads DRIVER_VERSION; the memory figures count a device's
 * process rows up to what it can hold, so `free` never wraps. Limits: at
 * most MAX_DEVICES devices and MAX_PROCS rows (the rest are ignored, so
 * the crate's retry past 64 rows cannot be driven from here), and an empty
 * entry in FAKE_NVML_DEVICES (`"A,"`) is a device with an empty name. The
 * structs match the repr(C) layouts in src/gpu/nvml.rs, pinned by the
 * _Static_asserts below. */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define NVML_SUCCESS 0u
#define NVML_ERROR_INVALID_ARGUMENT 2u
#define NVML_ERROR_INSUFFICIENT_SIZE 7u
#define NVML_ERROR_GPU_IS_LOST 15u

#define MAX_DEVICES 16u
#define MAX_PROCS 64u
/* NVML_DEVICE_NAME_V2_BUFFER_SIZE, as NVML_DEVICE_NAME_BUFFER_SIZE in nvml.rs. */
#define NAME_SIZE 96u
#define MIB (1024ull * 1024ull)
#define TOTAL_BYTES (16384ull * MIB)
#define RESERVED_BYTES (512ull * MIB)
#define DRIVER_VERSION "999.99"

typedef struct {
    uint32_t pid;
    uint64_t usedGpuMemory;
    uint32_t gpuInstanceId;
    uint32_t computeInstanceId;
} nvmlProcessInfo_t;

typedef struct {
    uint64_t total;
    uint64_t free;
    uint64_t used;
} nvmlMemory_t;

typedef struct {
    uint32_t version;
    uint64_t total;
    uint64_t reserved;
    uint64_t free;
    uint64_t used;
} nvmlMemory_v2_t;

_Static_assert(sizeof(nvmlProcessInfo_t) == 24, "NvmlProcessInfo is 24 bytes");
_Static_assert(sizeof(nvmlMemory_t) == 24, "NvmlMemoryInfo is 24 bytes");
_Static_assert(sizeof(nvmlMemory_v2_t) == 40, "NvmlMemoryInfoV2 is 40 bytes");

/* NVML_STRUCT_VERSION(Memory, 2), as NVML_MEMORY_V2_VERSION in nvml.rs. */
#define MEMORY_V2_VERSION ((uint32_t)sizeof(nvmlMemory_v2_t) | (2u << 24))

typedef void *nvmlDevice_t;

enum kind { WORKING, LOST, NO_PROCS };

struct device {
    enum kind kind;
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
    return value ? (unsigned)strtoul(value, NULL, 10) : NVML_SUCCESS;
}

/* FAKE_NVML_DEVICES, parsed into `devs`; returns the device count. */
static unsigned parse_devices(struct device devs[MAX_DEVICES]) {
    const char *p = getenv("FAKE_NVML_DEVICES");
    unsigned n = 0;
    if (p == NULL || *p == '\0') {
        return 0;
    }
    while (n < MAX_DEVICES) {
        const char *end = strchr(p, ',');
        size_t len = end ? (size_t)(end - p) : strlen(p);
        /* As long as a name: an entry is a name, or a name behind a tag. */
        char entry[NAME_SIZE];
        if (len >= sizeof entry) {
            len = sizeof entry - 1;
        }
        memcpy(entry, p, len);
        entry[len] = '\0';
        struct device *d = &devs[n++];
        if (strcmp(entry, "lost") == 0) {
            d->kind = LOST;
            d->name[0] = '\0';
        } else if (strncmp(entry, "noprocs:", 8) == 0) {
            d->kind = NO_PROCS;
            snprintf(d->name, sizeof d->name, "%s", entry + 8);
        } else {
            d->kind = WORKING;
            snprintf(d->name, sizeof d->name, "%s", entry);
        }
        if (end == NULL) {
            break;
        }
        p = end + 1;
    }
    return n;
}

/* FAKE_NVML_PROCS, parsed into `procs`; returns the row count. */
static unsigned parse_procs(struct proc procs[MAX_PROCS]) {
    const char *p = getenv("FAKE_NVML_PROCS");
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

/* The device a handle names, or NULL for a handle no call gave out. */
static const struct device *device_of(nvmlDevice_t device, struct device devs[MAX_DEVICES],
                                      unsigned *index) {
    unsigned count = parse_devices(devs);
    uintptr_t handle = (uintptr_t)device;
    if (handle == 0 || handle > count || devs[handle - 1].kind == LOST) {
        return NULL;
    }
    *index = (unsigned)(handle - 1);
    return &devs[handle - 1];
}

/* The bytes the processes of device `index` hold, at most what is left
 * after the reserve, so the free figures below never wrap; a row larger
 * than the device (the R570 u64::MAX sentinel included) is left out, as
 * NVML's own device figure would not count it. */
static uint64_t process_bytes(unsigned index) {
    const uint64_t room = TOTAL_BYTES - RESERVED_BYTES;
    struct proc procs[MAX_PROCS];
    unsigned n = parse_procs(procs);
    uint64_t sum = 0;
    for (unsigned i = 0; i < n; i++) {
        if (procs[i].dev == index && procs[i].bytes <= TOTAL_BYTES) {
            sum += procs[i].bytes;
            if (sum > room) {
                sum = room;
            }
        }
    }
    return sum;
}

unsigned nvmlInit_v2(void) {
    return code_from_env("FAKE_NVML_INIT");
}

unsigned nvmlShutdown(void) {
    return NVML_SUCCESS;
}

unsigned nvmlDeviceGetCount_v2(unsigned *count) {
    unsigned code = code_from_env("FAKE_NVML_COUNT");
    struct device devs[MAX_DEVICES];
    if (code != NVML_SUCCESS) {
        return code;
    }
    if (count == NULL) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    *count = parse_devices(devs);
    return NVML_SUCCESS;
}

unsigned nvmlDeviceGetHandleByIndex_v2(unsigned index, nvmlDevice_t *device) {
    struct device devs[MAX_DEVICES];
    unsigned count = parse_devices(devs);
    if (device == NULL || index >= count) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    if (devs[index].kind == LOST) {
        return NVML_ERROR_GPU_IS_LOST;
    }
    *device = (nvmlDevice_t)(uintptr_t)(index + 1);
    return NVML_SUCCESS;
}

unsigned nvmlDeviceGetMemoryInfo(nvmlDevice_t device, nvmlMemory_t *memory) {
    struct device devs[MAX_DEVICES];
    unsigned index;
    if (memory == NULL || device_of(device, devs, &index) == NULL) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    memory->total = TOTAL_BYTES;
    memory->used = RESERVED_BYTES + process_bytes(index);
    memory->free = TOTAL_BYTES - memory->used;
    return NVML_SUCCESS;
}

unsigned nvmlDeviceGetMemoryInfo_v2(nvmlDevice_t device, nvmlMemory_v2_t *memory) {
    struct device devs[MAX_DEVICES];
    unsigned index;
    if (memory == NULL || memory->version != MEMORY_V2_VERSION ||
        device_of(device, devs, &index) == NULL) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    memory->total = TOTAL_BYTES;
    memory->reserved = RESERVED_BYTES;
    memory->used = process_bytes(index);
    memory->free = TOTAL_BYTES - RESERVED_BYTES - memory->used;
    return NVML_SUCCESS;
}

unsigned nvmlDeviceGetName(nvmlDevice_t device, char *name, unsigned length) {
    struct device devs[MAX_DEVICES];
    unsigned index;
    const struct device *d = device_of(device, devs, &index);
    if (name == NULL || d == NULL) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    if (strlen(d->name) + 1 > length) {
        return NVML_ERROR_INSUFFICIENT_SIZE;
    }
    memcpy(name, d->name, strlen(d->name) + 1);
    return NVML_SUCCESS;
}

unsigned nvmlSystemGetDriverVersion(char *version, unsigned length) {
    if (version == NULL) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    if (sizeof DRIVER_VERSION > length) {
        return NVML_ERROR_INSUFFICIENT_SIZE;
    }
    memcpy(version, DRIVER_VERSION, sizeof DRIVER_VERSION);
    return NVML_SUCCESS;
}

/* NVML's contract: with *count smaller than the rows, set *count to the
 * rows and return NVML_ERROR_INSUFFICIENT_SIZE; otherwise write the rows and
 * set *count to how many were written. */
unsigned nvmlDeviceGetComputeRunningProcesses_v3(nvmlDevice_t device, unsigned *count,
                                                  nvmlProcessInfo_t *infos) {
    struct device devs[MAX_DEVICES];
    struct proc procs[MAX_PROCS];
    unsigned index;
    const struct device *d = device_of(device, devs, &index);
    if (count == NULL || d == NULL) {
        return NVML_ERROR_INVALID_ARGUMENT;
    }
    if (d->kind == NO_PROCS) {
        return NVML_ERROR_GPU_IS_LOST;
    }
    unsigned n = parse_procs(procs);
    unsigned rows = 0;
    for (unsigned i = 0; i < n; i++) {
        rows += procs[i].dev == index;
    }
    if (*count < rows || (rows > 0 && infos == NULL)) {
        *count = rows;
        return NVML_ERROR_INSUFFICIENT_SIZE;
    }
    unsigned written = 0;
    for (unsigned i = 0; i < n; i++) {
        if (procs[i].dev == index) {
            infos[written].pid = procs[i].pid;
            infos[written].usedGpuMemory = procs[i].bytes;
            infos[written].gpuInstanceId = 0;
            infos[written].computeInstanceId = 0;
            written++;
        }
    }
    *count = written;
    return NVML_SUCCESS;
}
