#include <stdio.h>
#include <errno.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
#include <sys/sysctl.h>
#include <libproc.h>
extern int ledger(int cmd, void *a1, void *a2, void *a3);
int main(void) {
  int self = getpid();
  errno = 0; int n = proc_listpids(PROC_ALL_PIDS, 0, NULL, 0);
  printf("proc_listpids size probe: %d errno=%s\n", n, n > 0 ? "0" : strerror(errno));
  int mib[4] = {CTL_KERN, KERN_PROC, KERN_PROC_ALL, 0}; size_t sz = 0;
  errno = 0; int r = sysctl(mib, 3, NULL, &sz, NULL, 0);
  printf("sysctl KERN_PROC_ALL size probe: rc=%d errno=%s bytes=%zu\n", r, r ? strerror(errno) : "0", sz);
  if (r == 0) { void *b = malloc(sz + 65536); size_t s2 = sz + 65536; r = sysctl(mib, 3, b, &s2, NULL, 0);
    printf("sysctl KERN_PROC_ALL fill: rc=%d records=%zu\n", r, s2 / sizeof(struct kinfo_proc)); }
  int pids[3] = {0, 1, 393};
  for (int i = 0; i < 3; i++) {
    int m[4] = {CTL_KERN, KERN_PROC, KERN_PROC_PID, pids[i]}; struct kinfo_proc kp; size_t ks = sizeof kp;
    errno = 0; r = sysctl(m, 4, &kp, &ks, NULL, 0);
    printf("KERN_PROC_PID %d: rc=%d len=%zu comm=%s\n", pids[i], r, ks, (r == 0 && ks) ? kp.kp_proc.p_comm : "-");
    char path[4096]; errno = 0; int pl = proc_pidpath(pids[i], path, sizeof path);
    printf("  proc_pidpath: %d %s\n", pl, pl > 0 ? "ok" : strerror(errno));
    char buf[128 * 88]; int cnt = 128; errno = 0;
    int lr = ledger(4, (void *)(long)pids[i], buf, &cnt);
    printf("  ledger: rc=%d %s\n", lr, lr ? strerror(errno) : "ok");
  }
  char buf[128 * 88]; int cnt = 128; errno = 0; int lr = ledger(4, (void *)(long)self, buf, &cnt);
  printf("ledger self: rc=%d %s\n", lr, lr ? strerror(errno) : "ok");
  return 0;
}
