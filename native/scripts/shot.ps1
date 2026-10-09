<#
  Windows 侧的验证脚手架（对应 mac 的 native/tools/*.py）。
  shot.ps1：截一块屏幕区域，顺带打几个采样点的 RGB，用来自证「画上了没有、画在哪」。
    截图走的是物理像素，直接喂 GetWindowRect 报出来的坐标即可（本进程是 per-monitor v2，
    窗口本身已经按 DPI 放大，不需要再换算）。
#>
param(
    [int]$X = 0,
    [int]$Y = 0,
    [int]$W = 400,
    [int]$H = 44,
    [string]$Out = 'shot.png',
    # 采样点写成字符串再自己切：powershell -File 传 int[] 时 "1,2,3" 会被并成一个数
    [string]$Rows = '1,6,22,41',
    [string]$Cols = '1,8,200,391'
)
Add-Type -AssemblyName System.Drawing
$rowList = @($Rows.Split(',') | ForEach-Object { [int]$_ })
$colList = @($Cols.Split(',') | ForEach-Object { [int]$_ })
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($X, $Y, 0, 0, (New-Object System.Drawing.Size $W, $H))
$bmp.Save([System.IO.Path]::GetFullPath($Out), [System.Drawing.Imaging.ImageFormat]::Png)
foreach ($row in $rowList) {
    $parts = foreach ($col in $colList) {
        $c = $bmp.GetPixel($col, $row)
        "x${col}=$($c.R)/$($c.G)/$($c.B)"
    }
    "y=$row " + ($parts -join ' ')
}
$g.Dispose()
$bmp.Dispose()
