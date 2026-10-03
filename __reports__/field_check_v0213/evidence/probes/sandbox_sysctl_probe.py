import ctypes, errno, sys
libc = ctypes.CDLL(None, use_errno=True)
led = libc.ledger; led.restype=ctypes.c_int; led.argtypes=[ctypes.c_int,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p]
for pid in map(int, sys.argv[1:]):
    buf=ctypes.create_string_buffer(128*88); c=ctypes.c_int(128); ctypes.set_errno(0)
    rc=led(4, ctypes.c_void_p(pid), ctypes.addressof(buf), ctypes.addressof(c)); e=ctypes.get_errno()
    print('ledger', pid, rc, errno.errorcode.get(e,e))
# sysctl CTL_KERN=1, KERN_PROC=14, KERN_PROC_ALL=0
mib=(ctypes.c_int*3)(1,14,0); sz=ctypes.c_size_t(0); ctypes.set_errno(0)
r=libc.sysctl(mib,3,None,ctypes.byref(sz),None,0); print('sysctl kern.proc.all size probe', r, errno.errorcode.get(ctypes.get_errno()), sz.value)
if r==0:
    buf=ctypes.create_string_buffer(sz.value+65536); sz2=ctypes.c_size_t(len(buf)); ctypes.set_errno(0)
    r=libc.sysctl(mib,3,buf,ctypes.byref(sz2),None,0); print('fill', r, errno.errorcode.get(ctypes.get_errno()), 'kinfo_proc count', sz2.value//648)
