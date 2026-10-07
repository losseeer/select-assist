#!/usr/bin/env python3
"""合成点击（§6）：`click.py <x> <y>`。窗口是 isMovableByWindowBackground 的，点拖拽区会把窗口点飞，
每次点击前先用 windows.py 重新查 bounds。"""
import sys
import time

import Quartz

if len(sys.argv) != 3:
    sys.exit("usage: click.py <x> <y>")
x, y = float(sys.argv[1]), float(sys.argv[2])
point = Quartz.CGPointMake(x, y)


def post(kind):
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, Quartz.CGEventCreateMouseEvent(None, kind, point, 0))


# 光标先落到真实位置：CGEvent 的坐标只作用于事件，hover 态仍取决于系统光标
Quartz.CGWarpMouseCursorPosition(point)
time.sleep(0.05)
post(Quartz.kCGEventMouseMoved)
time.sleep(0.05)
post(Quartz.kCGEventLeftMouseDown)
time.sleep(0.05)
post(Quartz.kCGEventLeftMouseUp)
