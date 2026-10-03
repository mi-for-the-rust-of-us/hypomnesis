import ctypes, errno
libc = ctypes.CDLL(None, use_errno=True)
for pid in (0, 99998, 393):
    mib=(ctypes.c_int*4)(1,14,1,pid); buf=ctypes.create_string_buffer(648); sz=ctypes.c_size_t(648); ctypes.set_errno(0)
    r=libc.sysctl(mib,4,buf,ctypes.byref(sz),None,0)
    print('kern.proc.pid', pid, 'rc', r, errno.errorcode.get(ctypes.get_errno()), 'len', sz.value, buf.raw[243:260].split(b'\0')[0] if sz.value else b'')
