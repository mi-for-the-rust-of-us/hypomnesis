# `kinfo_proc` layout: which architectures it was checked on

The v0.2.14 `sysctl kern.proc` fallback reads `kinfo_proc` records as fixed 648-byte arrays with
named offsets. This file records where that layout was checked, and how. Asked for in the
maintainer's reply on issue #3.

## Environment

```
$ uname -m
arm64
$ sw_vers
ProductName:		macOS
ProductVersion:		26.6.2
BuildVersion:		25G83
$ xcrun --show-sdk-version
26.2
$ clang --version | head -1
Apple clang version 17.0.0 (clang-1700.6.3.2)
$ sysctl -n machdep.cpu.brand_string
Apple M3 Pro
date: 2026-10-03
```

## Programs

`layout.c` prints the size and offsets the parser relies on, from the SDK header:

```c
#include <stdio.h>
#include <stddef.h>
#include <sys/sysctl.h>
int main(void) {
    struct kinfo_proc kp;
    printf("sizeof(kinfo_proc)=%zu offsetof(kp_proc.p_pid)=%zu offsetof(kp_proc.p_comm)=%zu sizeof(p_comm)=%zu\n",
           sizeof(struct kinfo_proc),
           offsetof(struct kinfo_proc, kp_proc.p_pid),
           offsetof(struct kinfo_proc, kp_proc.p_comm),
           sizeof kp.kp_proc.p_comm);
    return 0;
}
```

`live.c` asks the running kernel: one `KERN_PROC_PID` read of PID 1, and the `KERN_PROC_ALL` size
probe, checked for a whole number of records. `proc_translated` is 1 when the process runs under
Rosetta 2.

```c
#include <stdio.h>
#include <errno.h>
#include <sys/sysctl.h>

int main(void) {
    int translated = 0;
    size_t tlen = sizeof translated;
    if (sysctlbyname("sysctl.proc_translated", &translated, &tlen, NULL, 0) != 0) translated = -1;
    printf("proc_translated=%d\n", translated);

    int pid_mib[4] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, 1};
    struct kinfo_proc kp;
    size_t len = sizeof kp;
    int rc = sysctl(pid_mib, 4, &kp, &len, NULL, 0);
    printf("KERN_PROC_PID 1: rc=%d len=%zu comm=%s\n", rc, len, rc == 0 ? kp.kp_proc.p_comm : "-");

    int all_mib[3] = {CTL_KERN, KERN_PROC, KERN_PROC_ALL};
    size_t size = 0;
    rc = sysctl(all_mib, 3, NULL, &size, NULL, 0);
    printf("KERN_PROC_ALL size probe: rc=%d errno=%d whole_records=%s\n",
           rc, rc == 0 ? 0 : errno, size % sizeof(struct kinfo_proc) == 0 ? "yes" : "no");
    return 0;
}
```

## arm64, native

```
$ clang -arch arm64 -o layout_arm64 layout.c && ./layout_arm64
sizeof(kinfo_proc)=648 offsetof(kp_proc.p_pid)=40 offsetof(kp_proc.p_comm)=243 sizeof(p_comm)=17
$ clang -arch arm64 -o live_arm64 live.c && ./live_arm64
proc_translated=0
KERN_PROC_PID 1: rc=0 len=648 comm=launchd
KERN_PROC_ALL size probe: rc=0 errno=0 whole_records=yes
```

## x86_64, against the SDK header and under Rosetta 2

```
$ clang -arch x86_64 -o layout_x86_64 layout.c && ./layout_x86_64
sizeof(kinfo_proc)=648 offsetof(kp_proc.p_pid)=40 offsetof(kp_proc.p_comm)=243 sizeof(p_comm)=17
$ clang -arch x86_64 -o live_x86_64 live.c && ./live_x86_64
proc_translated=1
KERN_PROC_PID 1: rc=0 len=648 comm=launchd
KERN_PROC_ALL size probe: rc=0 errno=0 whole_records=yes
```

## Reading

- Both architectures compile the header to 648 bytes, with `p_pid` at 40 and `p_comm` at 243
  (17 bytes with the NUL).
- The live reads give one 648-byte record named `launchd` for PID 1, and a whole number of records
  for `KERN_PROC_ALL`. The x86_64 run is translated (`proc_translated=1`), so it shows what
  x86_64 userland sees on the arm64 kernel, not what an Intel kernel returns.
- The earlier `probes/kern_proc_pid_probe.py` had already read `len 648` on arm64.

**Not run:** native Intel Mac. No Intel-Mac hardware is available (`ROADMAP.md`, Principle 3).
