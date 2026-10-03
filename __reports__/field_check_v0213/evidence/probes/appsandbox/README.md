# App Sandbox probe

Runs `proc_listpids`, `sysctl(KERN_PROC_ALL)`, `sysctl(KERN_PROC_PID)`, `proc_pidpath` and
`ledger` inside a real App Sandbox. A CLI tool needs an embedded `Info.plist` and the
`com.apple.security.app-sandbox` entitlement to be sandboxed at launch.

```sh
clang -O1 -o probe appsandbox_probe.c -Wl,-sectcreate,__TEXT,__info_plist,Info.plist
codesign -s - -f --entitlements entitlements.plist ./probe
./probe
```

The same `-sectcreate` link argument (`RUSTFLAGS="-C link-arg=-Wl,-sectcreate,__TEXT,__info_plist,<path>"`)
and the same signing step produce an App-Sandboxed `hmn`.

Observed on 2026-10-02 (M3 Pro, macOS 26.6.2):

- `proc_listpids` → `EPERM`.
- `KERN_PROC_ALL` → 823 records.
- `KERN_PROC_PID` 0/1/393 → `kernel_task`, `launchd`, `WindowServer`.
- `proc_pidpath` 1/393 → ok; for 0 → `ESRCH`.
- `ledger` 0/1/393 → `EPERM`; for self → ok.
