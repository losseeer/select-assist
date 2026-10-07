#!/usr/bin/env python3
"""内存采样（§6）：`mem.py <pid> [秒数]`。

同时给两个口径：
  * `ps -o rss` —— 计划文档点名的算法，但它把 dyld 共享缓存的共享页也算进来，
    任何 AppKit 程序都会虚高几十 MB；
  * phys_footprint —— 活动监视器「内存」那一列 / jetsam 用的口径，才是「这个进程
    真的吃掉多少内存」。
"""
import ctypes
import subprocess
import sys
import time

pid = int(sys.argv[1])
seconds = int(sys.argv[2]) if len(sys.argv) > 2 else 60

LIBC = ctypes.CDLL("/usr/lib/libproc.dylib")
RUSAGE_INFO_V2 = 2


class RusageInfoV2(ctypes.Structure):
    # 顺序照 sys/proc_info.h 的 rusage_info_v2，偏移错了就全是垃圾值
    _fields_ = [
        ("ri_uuid", ctypes.c_uint8 * 16),
        ("ri_user_time", ctypes.c_uint64),
        ("ri_system_time", ctypes.c_uint64),
        ("ri_pkg_idle_wkups", ctypes.c_uint64),
        ("ri_interrupt_wkups", ctypes.c_uint64),
        ("ri_pageins", ctypes.c_uint64),
        ("ri_wired_size", ctypes.c_uint64),
        ("ri_resident_size", ctypes.c_uint64),
        ("ri_phys_footprint", ctypes.c_uint64),
        ("ri_proc_start_abstime", ctypes.c_uint64),
        ("ri_proc_exit_abstime", ctypes.c_uint64),
        # 内核会按真实的 rusage_info_v2 长度回写，结构体给小了会踩坏堆（实测脚本退出时 SIGSEGV）
        ("_pad", ctypes.c_uint64 * 40),
    ]


def footprint(pid):
    info = RusageInfoV2()
    n = LIBC.proc_pid_rusage(ctypes.c_int(pid), ctypes.c_int(RUSAGE_INFO_V2), ctypes.byref(info))
    return None if n else info.ri_phys_footprint


rss, fp = [], []
deadline = time.time() + seconds
while time.time() < deadline:
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    if not out:
        break
    rss.append(int(out) * 1024)
    got = footprint(pid)
    if got:
        fp.append(got)
    time.sleep(1.0)

mb = 1024 * 1024
if not rss:
    sys.exit("进程不在了")
print(f"样本 {len(rss)} 次 / 每 1s")
print(f"ps rss           : 均值 {sum(rss)/len(rss)/mb:7.1f} MB   最小 {min(rss)/mb:6.1f}  最大 {max(rss)/mb:6.1f}")
if fp:
    print(f"phys_footprint   : 均值 {sum(fp)/len(fp)/mb:7.1f} MB   最小 {min(fp)/mb:6.1f}  最大 {max(fp)/mb:6.1f}")
