<#
  只读地把面板下面那几个原生 EDIT 的真实状态打出来，用来分"框里没内容"和"有内容但画不出来"。
  为什么需要：PrintWindow 只能看到像素，看不到控件自己的状态；而"文字看不见"至少有四种成因，
  光看截图分不出来 —— 控件根本没建出来 / 建出来但被 ShowWindow(SW_HIDE) 藏了或零尺寸 /
  没拿到字体 / 父窗口把 WM_CTLCOLOREDIT 答成了背景色。这个脚本逐条量一遍。
  它不改任何东西、不注入输入，所以对正在复现问题的实例可以直接跑。

  用法：
    windows.ps1 -Pid2 <pid>                     # 先拿面板句柄
    edit-probe.ps1 <面板 hwnd>                  # 打每个 Edit 的状态
    edit-probe.ps1 <面板 hwnd> <pid>            # 顺带打当前焦点在哪个句柄上
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [int]$Parent,
    [int]$ProcId = 0
)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class Probe {
    [StructLayout(LayoutKind.Sequential)] public struct R { public int l, t, r, b; }
    [StructLayout(LayoutKind.Sequential)] public struct PT { public int x, y; }
    [StructLayout(LayoutKind.Sequential)]
    public struct GI {
        public uint cbSize; public uint flags; public IntPtr active; public IntPtr focus;
        public IntPtr trans; public IntPtr caret; public uint caretW; public uint caretH;
        public int caretX; public int caretY; public int clipL, clipT, clipR, clipB; public int charW, charH;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowEx(IntPtr p, IntPtr c, string cls, string win);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out int p);
    [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint tid, ref GI info);
    [DllImport("user32.dll")] public static extern IntPtr GetFocus();

    public const uint WM_GETFONT = 0x0031, EM_GETLINECOUNT = 0x00BA, WM_GETTEXTLENGTH = 0x000E;

    public static string Report(IntPtr h) {
        if (!IsWindow(h)) return "hwnd 已失效";
        R wr; GetWindowRect(h, out wr);
        int style = GetWindowLong(h, -16);
        IntPtr fnt = SendMessage(h, WM_GETFONT, IntPtr.Zero, IntPtr.Zero);
        int lines = SendMessage(h, EM_GETLINECOUNT, IntPtr.Zero, IntPtr.Zero).ToInt32();
        int len = SendMessage(h, WM_GETTEXTLENGTH, IntPtr.Zero, IntPtr.Zero).ToInt32();
        string s = "";
        s += "size=" + (wr.r - wr.l) + "x" + (wr.b - wr.t) + "@" + wr.l + "," + wr.t;
        s += " visible=" + ((style & unchecked((int)0x10000000)) != 0);
        s += " IsWinVis=" + IsWindowVisible(h);
        s += " disabled=" + ((style & unchecked((int)0x08000000)) != 0);
        s += " multiline=" + ((style & 0x4) != 0);
        s += " vscroll=" + ((style & 0x200000) != 0);
        s += " font=0x" + fnt.ToInt64().ToString("x");
        s += " textLen=" + len + " lines=" + lines;
        return s;
    }
}
'@

$p = [IntPtr]$Parent
if (-not [Probe]::IsWindow($p)) { "父窗口句柄无效：$Parent"; exit 1 }
# FindWindowEx 用前一个句柄当 hwndChildAfter 往后翻
$cur = [IntPtr]::Zero
$n = 0
while ($true) {
    $cur = [Probe]::FindWindowEx($p, $cur, 'Edit', $null)
    if ($cur -eq [IntPtr]::Zero) { break }
    "Edit #$n  " + [Probe]::Report($cur)
    $n++
    if ($n -gt 8) { break }
}
if ($n -eq 0) { "面板下没有 Edit 子窗口 —— 设置组的框全是自绘的底，文字当然不会有" }
