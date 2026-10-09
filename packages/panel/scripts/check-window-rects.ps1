# Prints the REAL Win32 rect of every select-assist window — dev builds run as electron.exe,
# packaged ones as select-assist.exe — so a mismatch with the sizes in src/main/index.ts is
# visible. Needed because Electron keeps reporting the size it asked for: a transparent window
# with a DWM backdrop is silently grown to a 64 physical-px minimum height (getBounds() still
# says 44), and the extra pixels paint as a light band under the chip.
# Usage: powershell -File check-window-rects.ps1   (compare the chip against CHIP.height and
# the panel against PANEL.height / the autoHeight value).
$src = @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
}
'@
Add-Type -TypeDefinition $src

$rows = New-Object System.Collections.ArrayList
$cb = [W+EnumProc] {
  param($h, $l)
  $p = 0
  [void][W]::GetWindowThreadProcessId($h, [ref]$p)
  $proc = Get-Process -Id $p -ErrorAction SilentlyContinue
  if ($proc -and ($proc.ProcessName -eq 'electron' -or $proc.ProcessName -eq 'select-assist')) {
    $r = New-Object W+R
    [void][W]::GetWindowRect($h, [ref]$r)
    $cn = New-Object System.Text.StringBuilder 256
    [void][W]::GetClassName($h, $cn, 256)
    if ($cn.ToString() -eq 'Chrome_WidgetWin_1') {
      $w = $r.Rt - $r.L
      if ($w -ge 100) {
        $line = "pid={0} vis={1} rect={2}x{3}@{4},{5}" -f $p, [W]::IsWindowVisible($h), $w, ($r.B - $r.T), $r.L, $r.T
        [void]$rows.Add($line)
      }
    }
  }
  return $true
}
[void][W]::EnumWindows($cb, [IntPtr]::Zero)
foreach ($x in $rows) { Write-Output $x }
