#!/usr/bin/env python3
"""按 PID 列出去不了的窗口真值（§6）：`windows.py <pid>` 或 `windows.py <pid> --all`（含隐藏）"""
import json
import sys

import Quartz

args = [a for a in sys.argv[1:] if not a.startswith("-")]
show_all = "--all" in sys.argv
if not args:
    sys.exit("usage: windows.py <pid> [--all]")
pid = int(args[0])

options = Quartz.kCGWindowListOptionAll if show_all else Quartz.kCGWindowListOptionOnScreenOnly
rows = []
for win in Quartz.CGWindowListCopyWindowInfo(options, Quartz.kCGNullWindowID) or []:
    if win.get("kCGWindowOwnerPID") != pid:
        continue
    bounds = win["kCGWindowBounds"]
    rows.append(
        {
            "id": win["kCGWindowNumber"],
            "layer": win["kCGWindowLayer"],
            "name": win.get("kCGWindowName", ""),
            "owner": win.get("kCGWindowOwnerName", ""),
            "x": bounds["X"],
            "y": bounds["Y"],
            "w": bounds["Width"],
            "h": bounds["Height"],
        }
    )
print(json.dumps(rows, ensure_ascii=False, indent=2))
