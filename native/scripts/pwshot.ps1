<#
  用 PrintWindow 抓某个 HWND 的自身内容，而不是抓屏幕。
  为什么不用 shot.ps1 的 CopyFromScreen：锁屏 / 显示器休眠时整屏捕获会失败或全黑，
  而 PrintWindow 是让窗口自己画一遍到内存 DC，跟显示状态无关 —— 无人值守的验证只能靠它。
  局限：只对 GDI 绘制的内容可靠（我们的 chip 正是）；D3D/WebView 的 surface 可能抓出来是空的。
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [int]$Hwnd,
    [Parameter(Mandatory = $true, Position = 1)]
    [string]$Out
)
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class PW {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, int flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
}
'@
$h = [IntPtr]$Hwnd
$r = New-Object PW+RECT
if (-not [PW]::GetWindowRect($h, [ref]$r)) { "GetWindowRect failed"; exit 1 }
$w = $r.right - $r.left
$hh = $r.bottom - $r.top
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$dc = $g.GetHdc()
# 2 = PW_RENDERFULLCONTENT：不带它，DWM 托管的内容会缺
$ok = [PW]::PrintWindow($h, $dc, 2)
$g.ReleaseHdc($dc)
$bmp.Save([System.IO.Path]::GetFullPath($Out), [System.Drawing.Imaging.ImageFormat]::Png)
"visible=$([PW]::IsWindowVisible($h)) print=$ok rect=${w}x${hh}"
$g.Dispose()
$bmp.Dispose()
