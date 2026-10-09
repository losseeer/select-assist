#!/usr/bin/env python3
"""按住不放看按下态：`hold.py <windowId> <x> <y> <out.png>`。
mouseDown 之后、mouseUp 之前截一帧，用来对比 AppKit 有没有给按钮原生反馈。"""
import subprocess
import sys
import time

import Quartz

if len(sys.argv) != 5:
    sys.exit("usage: hold.py <windowId> <x> <y> <out.png>")
wid, x, y, out = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
point = Quartz.CGPointMake(x, y)


def post(kind):
    Quartz.CGEventPost(
        Quartz.kCGHIDEventTap, Quartz.CGEventCreateMouseEvent(None, kind, point, 0)
    )


for _ in range(20):
    Quartz.CGWarpMouseCursorPosition(point)
    time.sleep(0.03)
    loc = Quartz.CGEventGetLocation(Quartz.CGEventCreate(None))
    if abs(loc.x - x) < 1.5 and abs(loc.y - y) < 1.5:
        break

post(Quartz.kCGEventMouseMoved)
time.sleep(0.05)
post(Quartz.kCGEventLeftMouseDown)
time.sleep(0.25)
subprocess.run(["screencapture", "-o", "-x", f"-l{wid}", out], check=True)
post(Quartz.kCGEventLeftMouseUp)
print(out)
