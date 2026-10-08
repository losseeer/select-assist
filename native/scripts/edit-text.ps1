<#
  读 / 写面板里第 N 个 Edit 子窗口的文字。
  为什么需要：设置组那三个框是原生 EDIT，文字归控件自己持有，PrintWindow 只能看到
  "长什么样"，看不到"里面到底是什么"——多行框一换行，截图里字段分隔符和折行根本分不开。
  给它一个 -Text 就是写入（模拟用户打字），不给就是读出来。
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
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
