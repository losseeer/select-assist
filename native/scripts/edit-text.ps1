<#
  读 / 写面板里第 N 个 Edit 子窗口的文字。
  为什么需要：设置组那三个框是原生 EDIT，文字归控件自己持有，PrintWindow 只能看到
  "长什么样"，看不到"里面到底是什么"——多行框一换行，截图里字段分隔符和折行根本分不开。
  给它一个 -Text 就是写入（模拟用户打字），不给就是读出来。

  一个把整轮测试带偏过的坑，记在这里：len 曾经按本地代码页的**字节数**报（"中文abc"
  5 个 UTF-16 单元报 7，因为中文在 GBK 里是 4 字节），看起来像 app 把中文弄坏了。
  原因是 WM_GETTEXTLENGTH 那个重载没写 CharSet，.NET 按 ANSI 默认解析成了 SendMessageA，
  而 EDIT 控件对 ANSI  flavor 的消息回的就是字节数。文字本身一直是对的（同一个脚本
  读出来的正文逐字正确），错的只有那个长度。现在重载钉死在 SendMessageW 上。
  教训：拿工具的数字给 app 定罪之前，先把正文本身读出来看。
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "SendMessageW")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
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
