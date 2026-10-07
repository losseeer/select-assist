#!/usr/bin/env python3
"""合成点击（§6）：`click.py <x> <y>`。窗口是 isMovableByWindowBackground 的，
光标没真正落位就发 mouseDown 会被 AppKit 当成拖拽起点，把窗口点飞，所以先等 CGWarp 生效。"""
import sys
import time

import Quartz

if len(sys.argv) != 3:
    sys.exit("usage: click.py <x> <y>")
x, y = float(sys.argv[1]), float(sys.argv[2])
point = Quartz.CGPointMake(x, y)


def cursor():
    return Quartz.CGEventGetLocation(Quartz.CGEventCreate(None))


def post(kind):
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, Quartz.CGEventCreateMouseEvent(None, kind, point, 0))


# 光标先落到真实位置：CGEvent 的坐标只作用于事件，hover 态仍取决于系统光标
for _ in range(20):
    Quartz.CGWarpMouseCursorPosition(point)
    time.sleep(0.03)
    cx, cy = cursor()
    if abs(cx - x) < 1.5 and abs(cy - y) < 1.5:
        break
else:
    sys.exit(f"光标没落到 {x},{y}（现在在 {cursor()}）")

post(Quartz.kCGEventMouseMoved)
time.sleep(0.05)
post(Quartz.kCGEventLeftMouseDown)
time.sleep(0.05)
post(Quartz.kCGEventLeftMouseUp)
time.sleep(0.05)
print(f"clicked {x},{y}  cursor={cursor()}")
