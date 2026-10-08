<#
  往指定 HWND 的客户区坐标发一次左键点击（WM_LBUTTONDOWN + WM_LBUTTONUP）。
  为什么不用 mouse_event：那个走的是系统输入队列，光标要真的落位、窗口要在最前、
  屏幕还不能锁 —— 无人值守验证时这些条件经常不成立。PostMessage 直接打到窗口过程，
  验的是我们自己的命中测试与动作分派，跟显示状态无关。
  局限：绕过了 DWM 的 hit-test，所以测不到 WM_NCHITTEST 的拖拽路径。
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [int]$Hwnd,
    [Parameter(Mandatory = $true, Position = 1)]
    [int]$X,
    [Parameter(Mandatory = $true, Position = 2)]
    [int]$Y
)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class Clicker {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref P p);
    [StructLayout(LayoutKind.Sequential)] public struct P { public int x, y; }
}
'@
$h = [IntPtr]$Hwnd
$mk = [IntPtr]1 # MK_LBUTTON
$lp = [IntPtr](($Y -shl 16) -bor ($X -band 0xFFFF))
[Clicker]::PostMessage($h, 0x0201, $mk, $lp) | Out-Null  # WM_LBUTTONDOWN
Start-Sleep -Milliseconds 60
[Clicker]::PostMessage($h, 0x0202, [IntPtr]::Zero, $lp) | Out-Null  # WM_LBUTTONUP
"posted click $X,$Y -> $Hwnd"
