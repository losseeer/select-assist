<#
  盯住设置组那三个原生 EDIT：**当"控件里有字"但"屏幕上看不见"的那一刻**自动留证据。
  为什么需要：这个现象是间歇性的，而且一观察就没了 —— 手工跑 field-check 抓不到，
  只能让脚本一直在旁边量。每轮做两件事：
    1) 问控件本身：文字长度、字体句柄、可见性、屏幕矩形（只读 SendMessage）
    2) 从屏幕那块矩形里数亮点子（真屏幕，不是 PrintWindow —— 用户看见的是这个）
  文字长度 > 0 而亮点子接近 0，就是"看不见"的那一刻，把截图 + 体检结果一起写盘后退出。

  用法：
    watch-fields.ps1                 # 一直盯（Ctrl+C 停）
    watch-fields.ps1 -Seconds 0.5    # 采样更密
  锁屏时 CopyFromScreen 会失败，脚本会跳过那一轮而不是误报。
#>
param(
    [double]$Seconds = 1.5,
    [int]$ProcId = 0,
    [int]$Polls = 0,
    [switch]$Heartbeat,
    [string]$OutDir = "$PSScriptRoot\..\target\uitest\catch"
)

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class Watch {
    [StructLayout(LayoutKind.Sequential)] public struct R { public int l, t, r, b; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowEx(IntPtr p, IntPtr c, string cls, string win);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageW")]
    public static extern IntPtr Send(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out int p);
    public const uint WM_GETFONT = 0x0031, WM_GETTEXTLENGTH = 0x000E;
}
'@

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

if (-not $ProcId) {
    $p = Get-Process -Name 'select-assist*' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $p) { '没有 native 实例在跑'; exit 1 }
    $ProcId = $p.Id
}
"盯着 pid=$ProcId 的 EDIT；每 $Seconds 秒一轮，抓到就写 $OutDir"

function Get-Panel([int]$procId) {
    $rows = ((& "$PSScriptRoot\windows.ps1" -Pid2 $procId 2>$null | Out-String) -split "`r?`n")
    $pl = $rows | Where-Object { $_ -match 'SelectAssistNativePanel' } | Select-Object -First 1
    if (-not $pl) { return $null }
    return [int](($pl.Trim() -split '\s+')[0])
}

$round = 0
while ($true) {
    $round++
    $panel = Get-Panel $ProcId
    if (-not $panel) { Start-Sleep -Seconds $Seconds; continue }

    $cur = [IntPtr]::Zero
    $n = 0
    $report = @()
    $caught = $null
    while ($true) {
        $cur = [Watch]::FindWindowEx([IntPtr]$panel, $cur, 'Edit', $null)
        if ($cur -eq [IntPtr]::Zero) { break }
        $r = New-Object Watch+R
        [Watch]::GetWindowRect($cur, [ref]$r) | Out-Null
        # 常量在 C# 那侧，写成 $WM_... 会被 PowerShell 当不存在的变量、按 0（WM_NULL）发出去，
        # 于是每个框都"报" len=0 —— 检测器自己瞎了，比误报更糟
        $len = [int][Watch]::Send($cur, [Watch]::WM_GETTEXTLENGTH, [IntPtr]::Zero, [IntPtr]::Zero)
        $font = [Watch]::Send($cur, [Watch]::WM_GETFONT, [IntPtr]::Zero, [IntPtr]::Zero)
        $vis = [Watch]::IsWindowVisible($cur)
        $w = $r.r - $r.l; $h = $r.b - $r.t
        $bright = -1
        if ($w -gt 4 -and $h -gt 4) {
            try {
                $bmp = New-Object Drawing.Bitmap $w, $h, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
                $g = [Drawing.Graphics]::FromImage($bmp)
                $g.CopyFromScreen($r.l, $r.t, 0, 0, (New-Object Drawing.Size $w, $h))
                $g.Dispose()
                $bright = 0
                for ($y = 2; $y -lt $h - 2; $y += 2) {
                    for ($x = 6; $x -lt $w - 20; $x += 2) {
                        $c = $bmp.GetPixel($x, $y)
                        if ($c.R -gt 100 -or $c.G -gt 100 -or $c.B -gt 100) { $bright++ }
                    }
                }
                $bmp.Dispose()
            } catch { $bright = -1 }   # 锁屏 / 区域不可见，跳过这一轮
        }
        $line = "Edit #$n len=$len font=0x$($font.ToInt64().ToString('x')) visible=$vis size=${w}x${h} bright=$bright"
        $report += $line
        if ($len -gt 0 -and $bright -ge 0 -and $bright -lt 20) {
            $caught = @{ index = $n; line = $line; rect = $r }
        }
        $n++
        if ($n -gt 8) { break }
    }

    if ($caught) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $png = Join-Path $OutDir "panel-$stamp.png"
        & "$PSScriptRoot\pwshot.ps1" -Hwnd $panel -Out $png | Out-Null
        $txt = Join-Path $OutDir "catch-$stamp.txt"
        ("时间 $stamp  面板 hwnd=$panel`n" + ($report -join "`n") + "`n--> 第 $($caught.index) 个框：有文字但屏幕上没有亮点子") |
            Set-Content -Path $txt -Encoding UTF8
        "抓到了！" + $caught.line
        "证据 -> $txt`n证据 -> $png"
        exit 2
    }
    if ($Heartbeat) { "round $round : " + ($report -join '  |  ') }
    if ($Polls -gt 0 -and $round -ge $Polls) { "跑了 $round 轮，没抓到" ; exit 0 }
    Start-Sleep -Milliseconds ([int]($Seconds * 1000))
}
