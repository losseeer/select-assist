<#
  列出某个进程名下的所有顶层窗口（HWND + 类名 + 可见性）。
  为什么需要：外壳有两个窗口（chip / panel），PrintWindow 要按 HWND 抓，
  而 panel 是自己建的第二窗口，日志里只有一个 hwnd= 打不出来。
#>
param(
    [string]$Process = 'select-assist-native-win',
    [int]$Pid2 = 0
)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class WinEnum {
    public delegate bool Cb(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Cb f, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [StructLayout(LayoutKind.Sequential)] public struct R { public int l, t, r, b; }
}
'@
$pids = @()
if ($Pid2 -gt 0) {
    $pids += $Pid2
} else {
    $pids += (Get-Process -Name $Process -ErrorAction SilentlyContinue).Id
}
if (-not $pids) { "no process named $Process"; exit 1 }
$found = @()
$callback = [WinEnum+Cb] {
    param($h, $l)
    $p = 0
    [WinEnum]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
    if ($pids -contains $p) {
        $sb = New-Object Text.StringBuilder 128
        [WinEnum]::GetClassName($h, $sb, 128) | Out-Null
        $r = New-Object WinEnum+R
        [WinEnum]::GetWindowRect($h, [ref]$r) | Out-Null
        $script:found += [pscustomobject]@{
            Hwnd    = [int]$h
            Class   = $sb.ToString()
            Visible = [WinEnum]::IsWindowVisible($h)
            Rect    = "{0}x{1}@{2},{3}" -f ($r.r - $r.l), ($r.b - $r.t), $r.l, $r.t
        }
    }
    return $true
}
[WinEnum]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
$found | Format-Table -AutoSize | Out-String -Width 200
$found | ForEach-Object { "{0} {1}" -f $_.Hwnd, $_.Class }
