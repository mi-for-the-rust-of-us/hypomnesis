import ctypes, errno, os, sys
libc = ctypes.CDLL(None, use_errno=True)
# responsible process (who TCC attributes our requests to)
try:
    f = libc.responsibility_get_pid_responsible_for_pid; f.restype=ctypes.c_int; f.argtypes=[ctypes.c_int]
    r = f(os.getpid())
    import subprocess
    print("responsible pid", r)
except AttributeError as e: print('no responsibility API', e)
led = libc.ledger; led.restype=ctypes.c_int; led.argtypes=[ctypes.c_int,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p]
lp = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
for pid in [393, 1, 2561, os.getpid()]:
    buf = ctypes.create_string_buffer(128*88); c = ctypes.c_int(128); ctypes.set_errno(0)
    rc = led(4, ctypes.c_void_p(pid), ctypes.addressof(buf), ctypes.addressof(c)); e = ctypes.get_errno()
    pb = ctypes.create_string_buffer(4096); ctypes.set_errno(0)
    n = lp.proc_pidpath(ctypes.c_int(pid), pb, ctypes.c_uint32(4096)); e2 = ctypes.get_errno()
    print(f'pid {pid}: ledger rc={rc} errno={errno.errorcode.get(e,e)}  proc_pidpath ret={n} errno={errno.errorcode.get(e2,e2)}')
sb = ctypes.CDLL('/usr/lib/libsandbox.1.dylib')
sb.sandbox_check.restype = ctypes.c_int; sb.sandbox_check.argtypes=[ctypes.c_int, ctypes.c_char_p, ctypes.c_int]
print('sandboxed:', sb.sandbox_check(os.getpid(), None, 0))
