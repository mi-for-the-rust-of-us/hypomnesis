import ctypes, errno, os, struct
libc = ctypes.CDLL(None, use_errno=True)
led = libc.ledger
led.restype = ctypes.c_int
led.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
# template to find index
tb = ctypes.create_string_buffer(128*64)  # template info row size? probe
# Use template size: 3 x 32 byte fields = 96
rowsz=96
tb = ctypes.create_string_buffer(128*rowsz)
cnt = ctypes.c_int(128)
ctypes.set_errno(0)
rc = led(2, ctypes.addressof(tb), ctypes.addressof(cnt), None)
print('template rc',rc,'count',cnt.value,'errno',ctypes.get_errno())
idx=None
for i in range(cnt.value):
    nm = tb.raw[i*rowsz:i*rowsz+32].split(b'\0')[0]
    if nm==b'graphics_footprint': idx=i
print('idx',idx)
for pid in [393,1,332,2561,os.getpid(),99998,999999]:
    buf = ctypes.create_string_buffer(128*88)
    c = ctypes.c_int(128)
    ctypes.set_errno(0)
    rc = led(4, ctypes.c_void_p(pid), ctypes.addressof(buf), ctypes.addressof(c))
    e = ctypes.get_errno()
    bal=None
    if rc==0 and idx is not None and c.value>idx:
        bal = struct.unpack_from('<q', buf.raw, idx*88+32)[0]  # lei_balance offset guess
    print(pid,'rc',rc,'errno',e,errno.errorcode.get(e),'count',c.value,'bal?',bal)
print('--- scan all pids')
lp = ctypes.CDLL(ctypes.util.find_library('proc'), use_errno=True) if False else None
import subprocess
out = subprocess.check_output(['ps','-axo','pid=,user=']).decode().split('\n')
from collections import Counter
errs=Counter(); nz=[]
me=os.environ.get('USER')
for l in out:
    if not l.strip(): continue
    pid,user=l.split(None,1); pid=int(pid); user=user.strip()
    buf = ctypes.create_string_buffer(128*88); c = ctypes.c_int(128)
    ctypes.set_errno(0)
    rc = led(4, ctypes.c_void_p(pid), ctypes.addressof(buf), ctypes.addressof(c))
    e=ctypes.get_errno()
    errs[(user==me, rc, e)]+=1
    if rc==0:
        bal=struct.unpack_from('<q',buf.raw,idx*88)[0]
        if bal>0 and user!=me: nz.append((pid,user,bal>>20))
print(dict(errs)); print('cross-user nonzero', nz)
