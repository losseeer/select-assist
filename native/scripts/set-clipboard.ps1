<#
  把一段 UTF-8 文本放进剪贴板。
  为什么不直接在命令行上给 -Value：bash -> powershell 传参按 OEM 代码页走，中文进得去就是乱码；
  剪贴板本身是 UTF-16，所以内容一律从文件读，编码由 -Encoding 显式指定。
#>
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Path
)
$text = [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
Add-Type -AssemblyName System.Windows.Forms
# Win32 剪贴板会被别的进程短暂占住，重试几次比直接失败有用
for ($i = 0; $i -lt 10; $i++) {
    try {
        if ($text.Length -eq 0) {
            # SetText("") 直接抛异常，清空只能走 Clear
            [System.Windows.Forms.Clipboard]::Clear()
        } else {
            [System.Windows.Forms.Clipboard]::SetText($text)
        }
        "clipboard=$($text.Length) chars"
        exit 0
    } catch {
        Start-Sleep -Milliseconds 150
    }
}
'set-clipboard failed'
exit 1
