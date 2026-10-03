import ctypes, ctypes.util, os, errno
lp = ctypes.CDLL(ctypes.util.find_library('proc'), use_errno=True)
for pid in [393,1,332,99998,999999,os.getpid(),0,-1,2561]:
    buf = ctypes.create_string_buffer(4096)
    ctypes.set_errno(0)
    n = lp.proc_pidpath(ctypes.c_int(pid), buf, ctypes.c_uint32(4096))
    e = ctypes.get_errno()
    print(pid, 'ret', n, 'errno', e, errno.errorcode.get(e), buf.value[-40:] if n>0 else '')
