#!/usr/bin/env python3
"""截某个窗口的图（§6）：`shot.py <window-id> <out.png>`，用 -o 去掉阴影，方便肉眼比对。"""
import subprocess
import sys

if len(sys.argv) != 3:
    sys.exit("usage: shot.py <windowId> <out.png>")
subprocess.run(["screencapture", "-o", "-x", f"-l{sys.argv[1]}", sys.argv[2]], check=True)
print(sys.argv[2])
