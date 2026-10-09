#!/usr/bin/env python3
"""当前前台应用名 + PID + 激活策略（0=regular 1=accessory 2=prohibited）。
点击 chip 后它必须仍是原来的应用，才说明键盘焦点没被抢走。"""
from AppKit import NSWorkspace

app = NSWorkspace.sharedWorkspace().frontmostApplication()
print(f"{app.localizedName()}\tpid={app.processIdentifier()}\tpolicy={app.activationPolicy()}")
