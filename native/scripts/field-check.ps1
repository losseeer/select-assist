<#
  一条命令抓现场：设置组的文字"看不见"时，把三个原生 EDIT 的真实状态和一张 PrintWindow 图一起收走。
  为什么需要：这个现象是间歇性的，只在用户自己的会话里出现，维护者这边复现不出来；
  而"看不见"至少有四种成因（控件没建 / 被藏或零尺寸 / 没字体 / 颜色答错），截图分不出来。
  全程只读：不点、不打字、不改配置。

  用法：
    field-check.ps1              # 自动找在跑的 native 实例
    field-check.ps1 -ProcId 1234 # 指定 pid
#>
param(
    [int]$ProcId = 0,
    [string]$Out = "$env:TEMP\field-check.png"
)

if (-not $ProcId) {
    $p = Get-Process -Name 'select-assist*' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $p) { '没有 native 实例在跑，先把程序开起来'; exit 1 }
    $ProcId = $p.Id
}
"pid = $ProcId"

# windows.ps1 的表格是 Format-Table | Out-String 一次性吐出来的**一个多行字符串**，
# 不是逐行对象，所以 Select-String 之前要先按行拆开
$rows = ((& "$PSScriptRoot\windows.ps1" -Pid2 $ProcId 2>$null | Out-String) -split "`r?`n")
$line = $rows | Where-Object { $_ -match 'SelectAssistNativePanel' } | Select-Object -First 1
if (-not $line) { '面板没开着：点 chip 最左边那个圆点展开，打开「设置」，再跑一次'; exit 1 }
$hwnd = [int](($line.Trim() -split '\s+')[0])
"panel hwnd = $hwnd"

& "$PSScriptRoot\edit-probe.ps1" $hwnd
& "$PSScriptRoot\pwshot.ps1" -Hwnd $hwnd -Out $Out
"截图 -> $Out"
