<#
  驱动 UI 输入的统一入口。同一个动作有两种发法，这条区别就是被测对象的一部分：

    -Post   PostMessage 直达窗口过程。不打扰用户，锁屏也能跑，但**绕过了 DWM 的
            hit-test 和系统的输入管线** —— 所以它测不到拖拽、测不到 hover 的真实
            触发路径、也测不到焦点/Tab 顺序/IME。
    -Real   SendInput 走系统输入队列。光标真的会动、当前窗口真的会失焦。
            这是唯一能验"用户实际会遇到的那条路"的方式，代价是会打断你手上的事。

  用法：
    input.ps1 -Hwnd 12345 -Action click  -X 324 -Y 20 [-Real]
    input.ps1 -Hwnd 12345 -Action move   -X 40  -Y 20            # hover，只有 -Real 有意义
    input.ps1 -Hwnd 12345 -Action drag   -X 200 -Y 20 -ToX 400 -ToY 260 [-Real]
    input.ps1 -Hwnd 12345 -Action wheel  -X 200 -Y 300 -Clicks -3 [-Real]
    input.ps1 -Hwnd 12345 -Action hittest -X 200 -Y 20            # 问 WM_NCHITTEST 要一个答案
    input.ps1             -Action keys -Text "abc中文"            # 打给当前焦点窗口
    input.ps1             -Action keys -Keys "^a"                 # SendKeys 语法（^=Ctrl +=Shift %=Alt）

  注意：-Real 会移动光标并在结束时移回去，但中途你动鼠标会和它抢。
#>
[CmdletBinding()]
param(
    [int]$Hwnd = 0,
    [ValidateSet('click', 'move', 'drag', 'wheel', 'hittest', 'keys')]
    [string]$Action = 'click',
    [int]$X = 0,
    [int]$Y = 0,
    [int]$ToX = 0,
    [int]$ToY = 0,
    [int]$Clicks = 0,
    [string]$Text = '',
    [string]$Keys = '',
    [switch]$Real
)

Add-Type -AssemblyName System.Windows.Forms

$src = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class Input {
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT {
        public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT {
        public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct HARDWAREINPUT { public uint uMsg, lParam1, lParam2; }

    [StructLayout(LayoutKind.Explicit)]
    public struct INPUTUNION {
        [FieldOffset(0)] public MOUSEINPUT mi;
        [FieldOffset(0)] public KEYBDINPUT ki;
        [FieldOffset(0)] public HARDWAREINPUT hi;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public INPUTUNION u; }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint n, INPUT[] inputs, int cbSize);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out P pt);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref P pt);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(P pt);
    // SM_CXSCREEN / SM_CYSCREEN：主屏像素尺寸。用 P/Invoke 而不是 WinForms，
    // 因为 Add-Type -TypeDefinition 不会把会话里已加载的 WinForms.dll 带进编译引用
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [StructLayout(LayoutKind.Sequential)] public struct P { public int x, y; }

    const uint INPUT_MOUSE = 0, INPUT_KEYBOARD = 1;
    const uint MOUSEEVENTF_MOVE = 0x0001, LEFTDOWN = 0x0002, LEFTUP = 0x0004;
    const uint WHEEL = 0x0800, VIRTUALDESK = 0x4000;
    const uint KEYDOWN = 0x0000, KEYUP = 0x0002, UNICODE = 0x0004;

    static INPUT Mouse(uint flags, int dx, int dy, uint data) {
        INPUT i = new INPUT();
        i.type = INPUT_MOUSE;
        i.u.mi.dwFlags = flags; i.u.mi.dx = dx; i.u.mi.dy = dy; i.u.mi.mouseData = data;
        return i;
    }

    /// 绝对屏幕坐标要归一化到 0..65535，否则 -Real 的点击会落在别处
    static void Absolute(ref INPUT i, int x, int y) {
        i.u.mi.dx = (int)(((long)x * 65535) / (GetSystemMetrics(0) - 1));
        i.u.mi.dy = (int)(((long)y * 65535) / (GetSystemMetrics(1) - 1));
        i.u.mi.dwFlags |= VIRTUALDESK;
    }

    public static void RealMove(int x, int y) {
        INPUT[] a = new INPUT[1];
        a[0] = Mouse(MOUSEEVENTF_MOVE, 0, 0, 0);
        Absolute(ref a[0], x, y);
        SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void RealDown() {
        INPUT[] a = new INPUT[1]; a[0] = Mouse(LEFTDOWN, 0, 0, 0);
        SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void RealUp() {
        INPUT[] a = new INPUT[1]; a[0] = Mouse(LEFTUP, 0, 0, 0);
        SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
    }
    /// 滚轮的 mouseData 是带符号的格数（负数向下），不是像素
    public static void RealWheel(int notches) {
        INPUT[] a = new INPUT[1];
        a[0] = Mouse(WHEEL, 0, 0, (uint)(notches * 120));
        SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
    }
    /// VK_PACKET：把一个 UTF-16 码元当"按键"送进去，中文这样才进得去焦点窗口
    public static void TypeChar(char c) {
        INPUT[] a = new INPUT[2];
        a[0] = new INPUT(); a[0].type = INPUT_KEYBOARD;
        a[0].u.ki.wVk = 0xE7; a[0].u.ki.wScan = (ushort)c; a[0].u.ki.dwFlags = UNICODE;
        a[1] = new INPUT(); a[1].type = INPUT_KEYBOARD;
        a[1].u.ki.wVk = 0xE7; a[1].u.ki.wScan = (ushort)c; a[1].u.ki.dwFlags = UNICODE | KEYUP;
        SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
    }
    public static bool TryPoint(IntPtr h, int cx, int cy, out int sx, out int sy) {
        P p = new P(); p.x = cx; p.y = cy;
        sx = 0; sy = 0;
        if (!ClientToScreen(h, ref p)) return false;
        sx = p.x; sy = p.y; return true;
    }
    public static IntPtr Send(IntPtr h, uint msg, IntPtr w, IntPtr l) { return SendMessage(h, msg, w, l); }
    public static bool Post(IntPtr h, uint msg, IntPtr w, IntPtr l) { return PostMessage(h, msg, w, l); }
    public static IntPtr TopAt(int x, int y) { P p = new P(); p.x = x; p.y = y; return WindowFromPoint(p); }
    public static void Get(out int x, out int y) { P p; GetCursorPos(out p); x = p.x; y = p.y; }
}
'@
Add-Type -TypeDefinition $src

function Get-ScreenPoint([int]$h, [int]$cx, [int]$cy) {
    # 不能 [ref] 绑定 $p.X / $p.Y：System.Drawing.Point 的那两个是只读属性，
    # 这样传 ClientToScreen 会把结果丢掉、原样返回客户区坐标，
    # 于是 hittest 把客户区当屏幕坐标发出去，到处都报 HTCAPTION。
    $sx = 0; $sy = 0
    if (-not [Input]::TryPoint([IntPtr]$h, $cx, $cy, [ref]$sx, [ref]$sy)) { throw "ClientToScreen 失败" }
    return New-Object System.Drawing.Point $sx, $sy
}
function Pack-Client([int]$cx, [int]$cy) {
    # lParam：低 16 位 x、高 16 位 y，客户区坐标
    [IntPtr](($cy -shl 16) -band 0xFFFF0000 -bor ($cx -band 0xFFFF))
}
function Pack-Screen([int]$sx, [int]$sy) { [IntPtr](($sy -shl 16) -band 0xFFFF0000 -bor ($sx -band 0xFFFF)) }

$WM_MOUSEMOVE = 0x0200; $WM_LBUTTONDOWN = 0x0201; $WM_LBUTTONUP = 0x0202
$WM_MOUSEWHEEL = 0x020A; $WM_NCHITTEST = 0x0084
$MK_LBUTTON = 1

if ($Action -ne 'keys' -and $Hwnd -eq 0) { '这个动作要 -Hwnd'; exit 2 }
$before = New-Object System.Drawing.Point
[Input]::Get([ref]$before.x, [ref]$before.y) | Out-Null

try {
    switch ($Action) {
        'move' {
            if ($Real) {
                $sp = Get-ScreenPoint $Hwnd $X $Y
                [Input]::RealMove($sp.x, $sp.y)
                "real move -> screen $($sp.x),$($sp.y)"
            } else {
                [Input]::Post([IntPtr]$Hwnd, $WM_MOUSEMOVE, [IntPtr]::Zero, (Pack-Client $X $Y)) | Out-Null
                "posted WM_MOUSEMOVE $X,$Y"
            }
        }
        'click' {
            if ($Real) {
                $sp = Get-ScreenPoint $Hwnd $X $Y
                [Input]::RealMove($sp.x, $sp.y); Start-Sleep -Milliseconds 120
                [Input]::RealDown(); Start-Sleep -Milliseconds 60; [Input]::RealUp()
                $top = [int][Input]::TopAt($sp.x, $sp.y)
                "real click -> screen $($sp.x),$($sp.y) 命中 hwnd=$top（若不是 $Hwnd，说明有东西盖在上面）"
            } else {
                $lp = Pack-Client $X $Y
                $h = [IntPtr]$Hwnd
                [Input]::Post($h, $WM_LBUTTONDOWN, [IntPtr]$MK_LBUTTON, $lp) | Out-Null
                Start-Sleep -Milliseconds 60
                [Input]::Post($h, $WM_LBUTTONUP, [IntPtr]::Zero, $lp) | Out-Null
                "posted click $X,$Y"
            }
        }
        'drag' {
            if ($Real) {
                $a = Get-ScreenPoint $Hwnd $X $Y
                $b = Get-ScreenPoint $Hwnd $ToX $ToY
                [Input]::RealMove($a.x, $a.y); Start-Sleep -Milliseconds 120
                [Input]::RealDown(); Start-Sleep -Milliseconds 60
                # 分步移动：一步到位会被系统当成点击而不是拖动
                for ($i = 1; $i -le 12; $i++) {
                    $px = [int]($a.x + ($b.x - $a.x) * $i / 12)
                    $py = [int]($a.y + ($b.y - $a.y) * $i / 12)
                    [Input]::RealMove($px, $py); Start-Sleep -Milliseconds 25
                }
                [Input]::RealUp()
                "real drag $($a.x),$($a.y) -> $($b.x),$($b.y)"
            } else {
                # 非真实输入下"拖"只能验 hit-test 说这是不是标题区，移动本身是 DWM 做的
                $sp = Get-ScreenPoint $Hwnd $X $Y
                $r = [Input]::Send([IntPtr]$Hwnd, $WM_NCHITTEST, [IntPtr]::Zero, (Pack-Screen $sp.x $sp.y))
                "drag 需要 -Real；这里只报 WM_NCHITTEST($X,$Y) = $([int]$r)（2=HTCAPTION 可拖，1=HTCLIENT 不可拖）"
            }
        }
        'wheel' {
            if ($Clicks -eq 0) { 'wheel 要 -Clicks（负数向下）'; exit 2 }
            if ($Real) {
                $sp = Get-ScreenPoint $Hwnd $X $Y
                [Input]::RealMove($sp.x, $sp.y); Start-Sleep -Milliseconds 120
                [Input]::RealWheel($Clicks)
                "real wheel $Clicks 格 @ $($sp.x),$($sp.y)"
            } else {
                $sp = Get-ScreenPoint $Hwnd $X $Y
                $wp = [IntPtr]((($Clicks * 120) -shl 16) -band 0xFFFF0000)
                [Input]::Post([IntPtr]$Hwnd, $WM_MOUSEWHEEL, $wp, (Pack-Screen $sp.x $sp.y)) | Out-Null
                "posted WM_MOUSEWHEEL $Clicks"
            }
        }
        'hittest' {
            $sp = Get-ScreenPoint $Hwnd $X $Y
            $r = [int][Input]::Send([IntPtr]$Hwnd, $WM_NCHITTEST, [IntPtr]::Zero, (Pack-Screen $sp.x $sp.y))
            $name = switch ($r) { 1 { 'HTCLIENT' } 2 { 'HTCAPTION' } default { "code $r" } }
                "hittest($X,$Y) = $name"
        }
        'keys' {
            if ($Text -eq '' -and $Keys -eq '') { 'keys 要 -Text 或 -Keys'; exit 2 }
            if ($Keys -ne '') {
                [System.Windows.Forms.SendKeys]::SendWait($Keys)
                "sent keys: $Keys"
            } else {
                foreach ($c in $Text.ToCharArray()) { [Input]::TypeChar($c); Start-Sleep -Milliseconds 12 }
                "typed $($Text.Length) chars into the focused window"
            }
        }
    }
} finally {
    if ($Real) {
        # 把光标还回去。不还原的话，用户回来会发现鼠标停在应用窗口上
        [Input]::RealMove($before.x, $before.y) | Out-Null
        "cursor restored -> $($before.x),$($before.y)"
    }
}
