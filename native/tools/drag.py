#!/usr/bin/env python3
"""合成拖动（§6）：`drag.py <x0> <y0> <x1> <y1>`。
isMovableByWindowBackground 的窗口会被拖走，所以起点要选在非控件区，且每次点击前重新查 bounds。"""
import sys
import time

import Quartz

if len(sys.argv) != 5:
    sys.exit("usage: drag.py <x0> <y0> <x1> <y1>")
x0, y0, x1, y1 = (float(a) for a in sys.argv[1:5])


def post(kind, x, y):
    point = Quartz.CGPointMake(x, y)
    event = Quartz.CGEventCreateMouseEvent(None, kind, point, 0)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)


Quartz.CGWarpMouseCursorPosition(Quartz.CGPointMake(x0, y0))
time.sleep(0.05)
post(Quartz.kCGEventMouseMoved, x0, y0)
post(Quartz.kCGEventLeftMouseDown, x0, y0)
time.sleep(0.05)

# 分多步移动：一次跳到位会被系统当成点击而不是拖动
steps = 12
for i in range(1, steps + 1):
    t = i / steps
    post(Quartz.kCGEventLeftMouseDragged, x0 + (x1 - x0) * t, y0 + (y1 - y0) * t)
    time.sleep(0.02)

post(Quartz.kCGEventLeftMouseUp, x1, y1)
