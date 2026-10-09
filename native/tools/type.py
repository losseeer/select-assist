#!/usr/bin/env python3
"""往当前焦点控件打字（§6）：`type.py [--cmd-a] <文本>`。
用 CGEventKeyboardSetUnicodeString 注入字符，绕开键盘布局，中文也能验。"""
import sys
import time

import Quartz

args = sys.argv[1:]
select_all = False
if args and args[0] == "--cmd-a":
    select_all = True
    args = args[1:]
if len(args) != 1:
    sys.exit("usage: type.py [--cmd-a] <text>")


def post(keycode, down, flags=0, text=None):
    event = Quartz.CGEventCreateKeyboardEvent(None, keycode, down)
    if flags:
        Quartz.CGEventSetFlags(event, flags)
    if text is not None:
        Quartz.CGEventKeyboardSetUnicodeString(event, len(text), text)
    Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)


if select_all:
    post(0x00, True, Quartz.kCGEventFlagMaskCommand)  # ANSI_A
    post(0x00, False, Quartz.kCGEventFlagMaskCommand)
    time.sleep(0.1)

text = args[0]
post(0, True, text=text)
post(0, False, text=text)
time.sleep(0.1)
print(f"typed {len(text)} chars")
