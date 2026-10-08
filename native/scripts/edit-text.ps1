<#
  读 / 写面板里第 N 个 Edit 子窗口的文字。
  为什么需要：设置组那三个框是原生 EDIT，文字归控件自己持有，PrintWindow 只能看到
  "长什么样"，看不到"里面到底是什么"——多行框一换行，截图里字段分隔符和折行根本分不开。
  给它一个 -Text 就是写入（模拟用户打字），不给就是读出来。

  已知局限：报出来的 len 对非 ASCII 不可信（实测 46 个 UTF-16 单元的默认模板报成 73，
  33 个单元的串报成 51，像是按本地代码页的字节数在算）。**别拿这个 len 当 Unicode
  正确性的判据** —— 要验中文有没有走样，去看落盘后的 settings.json，或者用 app 自己的
  edits::get()。app 侧的 Unicode 路径另有证据：clip.rs 的测试拿 emoji 过了一遍真剪贴板。
  写入路径本身是对的（写进去的中文在 JSON 里逐字正确），只有回读的长度不准。
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [int]$Parent,
    [Parameter(Mandatory = $true, Position = 1)]
    [int]$Index,
    [string]$Text = ''
)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class Ed {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowEx(IntPtr p, IntPtr c, string cls, string win);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageW")]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageW")]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, string l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    public static IntPtr NthChild(IntPtr parent, int n) {
        IntPtr h = IntPtr.Zero;
        for (int i = 0; i <= n; i++) {
            h = FindWindowEx(parent, h, "Edit", null);
            if (h == IntPtr.Zero) return IntPtr.Zero;
        }
        return h;
    }
}
'@
$p = [IntPtr]$Parent
$h = [Ed]::NthChild($p, $Index)
if ($h -eq [IntPtr]::Zero) { "no Edit child #$Index"; exit 1 }
if ($Text -ne '') {
    [Ed]::SendMessage($h, 0x000C, [IntPtr]::Zero, $Text) | Out-Null  # WM_SETTEXT
    "wrote $($Text.Length) chars -> $([int]$h)"
} else {
    $len = [int][Ed]::SendMessage($h, 0x000E, [IntPtr]::Zero, [IntPtr]::Zero)  # WM_GETTEXTLENGTH
    $sb = New-Object Text.StringBuilder ($len + 2)
    [Ed]::SendMessage($h, 0x000D, [IntPtr]($len + 1), $sb) | Out-Null          # WM_GETTEXT
    "[edit #$Index len=$len]"
    $sb.ToString()
}
