#!/usr/bin/env python3
"""跑一轮动作再回窗口真值：`state.py <pid>`（列窗口）或 `state.py <pid> <x> <y>`（先点一下）"""
import json
import os
import subprocess
import sys
import time

import Quartz

args = sys.argv[1:]
pid = int(args[0])
if len(args) >= 3:
    # 按脚本自身位置找 click.py：从仓库根或 native/ 调用都成立
    here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "click.py")
    subprocess.run(["python3", here, args[1], args[2]], check=True)
    time.sleep(0.6)


def rows(show_all=True):
    options = Quartz.kCGWindowListOptionAll if show_all else Quartz.kCGWindowListOptionOnScreenOnly
    out = []
    for win in Quartz.CGWindowListCopyWindowInfo(options, Quartz.kCGNullWindowID) or []:
        if win.get("kCGWindowOwnerPID") != pid:
            continue
        b = win["kCGWindowBounds"]
        if b["Width"] < 100:
            continue
        out.append({"id": win["kCGWindowNumber"], "x": b["X"], "y": b["Y"], "w": b["Width"], "h": b["Height"]})
    return out


print(json.dumps(rows(), ensure_ascii=False))
