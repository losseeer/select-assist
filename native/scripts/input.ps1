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
  急停：物理按住 Esc，下一次调用会在注入任何输入之前退出（drag 会先松开左键再退），
  所以按住不放就能让一整轮停下来。检查点在每次调用开头、drag 的每一步、每个字符之前。
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
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [StructLayout(LayoutKind.Sequential)] public struct P { public int x, y; }

    const uint INPUT_MOUSE = 0, INPUT_KEYBOARD = 1;
    const uint MOUSEEVENTF_MOVE = 0x0001, LEFTDOWN = 0x0002, LEFTUP = 0x0004;
    const uint WHEEL = 0x0800, VIRTUALDESK = 0x4000;
    // 0x8000。少了它，dx/dy 是**相对位移**而不是归一化坐标：往 (320,142) "移动"
    // 实际发出去的是"向右 1 万像素"，光标被钉在屏幕右下角，而 SendInput 照样返回成功。
    // 这就是这轮真实输入一开始全点空、以及 shell.rs 里"本机合成鼠标不可靠"那句话的成因。
    const uint ABSOLUTE = 0x8000;
    const uint KEYDOWN = 0x0000, KEYUP = 0x0002, UNICODE = 0x0004;

    static INPUT Mouse(uint flags, int dx, int dy, uint data) {
        INPUT i = new INPUT();
        i.type = INPUT_MOUSE;
        i.u.mi.dwFlags = flags; i.u.mi.dx = dx; i.u.mi.dy = dy; i.u.mi.mouseData = data;
        return i;
    }

    /// 绝对屏幕坐标要归一化到 0..65535，并且分母要用**虚拟桌面**（所有显示器拼起来
    /// 那张）的尺寸与原点，不是主屏。这台机器是 1920+1920 横排，用主屏尺寸会把落点整体放大一倍。
    static void Absolute(ref INPUT i, int x, int y) {
        i.u.mi.dx = (int)(((long)(x - vx()) * 65535) / (vw() - 1));
        i.u.mi.dy = (int)(((long)(y - vy()) * 65535) / (vh() - 1));
        i.u.mi.dwFlags |= ABSOLUTE | VIRTUALDESK;
    }
    static int vx() { return GetSystemMetrics(76); }
    static int vy() { return GetSystemMetrics(77); }
    static int vw() { return GetSystemMetrics(78); }
    static int vh() { return GetSystemMetrics(79); }

    public static void RealMove(int x, int y) {
        INPUT[] a = new INPUT[1];
        a[0] = Mouse(MOUSEEVENTF_MOVE, 0, 0, 0);
        Absolute(ref a[0], x, y);
        uint n = SendInput(1, a, Marshal.SizeOf(typeof(INPUT)));
        if (n != 1) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
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
    /// 发一个 Unicode 字符。wVk 必须是 0：填 VK_PACKET(0xE7) 是**收**那一侧的 wParam，
    /// 发的时候填它，系统会改走键盘布局的翻译，非 ASCII 只剩低字节 ——
    /// 实测 "测试"(U+6D4B U+8BD5) 打出来是 "KÕ"(U+4B U+D5)，看着像 app 把中文弄坏了。
    public static void TypeChar(char c) {
        INPUT[] a = new INPUT[2];
        a[0] = new INPUT(); a[0].type = INPUT_KEYBOARD;
        a[0].u.ki.wVk = 0; a[0].u.ki.wScan = (ushort)c; a[0].u.ki.dwFlags = UNICODE;
        a[1] = new INPUT(); a[1].type = INPUT_KEYBOARD;
        a[1].u.ki.wVk = 0; a[1].u.ki.wScan = (ushort)c; a[1].u.ki.dwFlags = UNICODE | KEYUP;
        uint n = SendInput(2, a, Marshal.SizeOf(typeof(INPUT)));
        if (n != 2) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    }
    public static bool TryPoint(IntPtr h, int cx, int cy, out int sx, out int sy) {
        P p = new P(); p.x = cx; p.y = cy;
        sx = 0; sy = 0;
        if (!ClientToScreen(h, ref p)) return false;
        sx = p.x; sy = p.y; return true;
    }
    public static IntPtr Send(IntPtr h, uint msg, IntPtr w, IntPtr l) { return SendMessage(h, msg, w, l); }
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
    public static bool Post(IntPtr h, uint msg, IntPtr w, IntPtr l) { return PostMessage(h, msg, w, l); }
    public static IntPtr TopAt(int x, int y) { P p = new P(); p.x = x; p.y = y; return WindowFromPoint(p); }
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    public static IntPtr Fg() { return GetForegroundWindow(); }
    public static void Get(out int x, out int y) { P p; GetCursorPos(out p); x = p.x; y = p.y; }
    /// 按值返回而不是 out：PowerShell 拿 [ref] 绑结构体的**字段**会拷一份走，
    /// 读回来的永远是 0,0（原来的 -Real 就是这么"还原"光标的，等于每步都把光标甩到左上角）
    public static P Cur() { P p; GetCursorPos(out p); return p; }
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

# 真实移动之后要回读光标实际落在哪："我以为点的是 x,y"和"系统真把光标放到了 x,y"
# 是两件事，而只有后者决定命中测试的结果。偏得多就直接失败，不许继续装作成功。
function Move-Real([int]$tx, [int]$ty) {
    [Input]::RealMove($tx, $ty)
    $c = [Input]::Cur()
    if ([Math]::Abs($c.x - $tx) -gt 2 -or [Math]::Abs($c.y - $ty) -gt 2) {
        throw "真实移动落点不符：期望 $tx,$ty，实际 $($c.x),$($c.y)"
    }
    return $c
}

$WM_MOUSEMOVE = 0x0200; $WM_LBUTTONDOWN = 0x0201; $WM_LBUTTONUP = 0x0202
$WM_MOUSEWHEEL = 0x020A; $WM_NCHITTEST = 0x0084
$MK_LBUTTON = 1

if ($Action -ne 'keys' -and $Hwnd -eq 0) { '这个动作要 -Hwnd'; exit 2 }

# -Real 会抢走鼠标键盘，所以留一条人能按得动的退出路径：按住 Esc 再触发下一步，
# 这一步就不注入输入并以 3 退出。轮子之间也会检查，按住不放能让整轮停下来。
function Test-Abort {
    if ([Input]::GetAsyncKeyState(0x1B) -lt 0) {
        'ABORT: Esc 被按住，未注入任何输入'
        exit 3
    }
}
Test-Abort

$before = [Input]::Cur()

try {
    switch ($Action) {
        'move' {
            if ($Real) {
                $sp = Get-ScreenPoint $Hwnd $X $Y
                $c = Move-Real $sp.x $sp.y
                "real move -> 实际光标 $($c.x),$($c.y)（目标 $($sp.x),$($sp.y)）"
            } else {
                [Input]::Post([IntPtr]$Hwnd, $WM_MOUSEMOVE, [IntPtr]::Zero, (Pack-Client $X $Y)) | Out-Null
                "posted WM_MOUSEMOVE $X,$Y"
            }
        }
        'click' {
            if ($Real) {
                $sp = Get-ScreenPoint $Hwnd $X $Y
                $c = Move-Real $sp.x $sp.y; Start-Sleep -Milliseconds 120
                # 命中要用**实际**光标位置去问，用期望位置问等于自己给自己编一个成功
                $top = [int][Input]::TopAt($c.x, $c.y)
                [Input]::RealDown(); Start-Sleep -Milliseconds 60; [Input]::RealUp()
                "real click -> 实际 $($c.x),$($c.y) 命中 hwnd=$top 目标 hwnd=$Hwnd 前台=$([int][Input]::Fg())" +
                    $(if ($top -ne $Hwnd) { '  <-- 不是目标窗口，这一步的结论无效' } else { '' })
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
                $start = Move-Real $a.x $a.y; Start-Sleep -Milliseconds 120
                [Input]::RealDown(); Start-Sleep -Milliseconds 60
                $aborted = $false
                # 分步移动：一步到位会被系统当成点击而不是拖动
                for ($i = 1; $i -le 12; $i++) {
                    $px = [int]($a.x + ($b.x - $a.x) * $i / 12)
                    $py = [int]($a.y + ($b.y - $a.y) * $i / 12)
                    [Input]::RealMove($px, $py); Start-Sleep -Milliseconds 25
                    # 中途按 Esc 只中断循环，不在左键还按着的时候 exit：
                    # 那样会跳过 RealUp，用户回来发现左键一直是按下状态
                    if ([Input]::GetAsyncKeyState(0x1B) -lt 0) { $aborted = $true; break }
                }
                [Input]::RealUp()
                $end = [Input]::Cur()
                if ($aborted) { "ABORT: Esc，已松开左键后停止"; exit 3 }
                # 报实际起止点：拖动的位移要按真实落点算，按期望算会把结论推歪
                "real drag 实际 $($start.x),$($start.y) -> $($end.x),$($end.y)（期望终点 $($b.x),$($b.y)）"
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
                $c = Move-Real $sp.x $sp.y; Start-Sleep -Milliseconds 120
                [Input]::RealWheel($Clicks)
                "real wheel $Clicks 格 @ 实际 $($c.x),$($c.y) 下方 hwnd=$([int][Input]::TopAt($c.x, $c.y))（目标 $Hwnd）"
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
                foreach ($c in $Text.ToCharArray()) { Test-Abort; [Input]::TypeChar($c); Start-Sleep -Milliseconds 12 }
                "typed $($Text.Length) chars into the focused window"
            }
        }
    }
} finally {
    if ($Real) {
        # 把光标还回去。不还原的话，用户回来会发现鼠标停在应用窗口上
        [Input]::RealMove($before.x, $before.y) | Out-Null
        "cursor restored -> $($before.x),$($before.y)"
    }}
