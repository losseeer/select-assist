param(
    [hashtable]$Env = @{},
    [int]$TimeoutSec = 30
)
$exe = 'D:\Projects\others\select-assist\native\target\debug\select-assist-native-win.exe'
foreach ($k in $Env.Keys) { [Environment]::SetEnvironmentVariable($k, [string]$Env[$k]) }
$p = Start-Process -FilePath $exe -NoNewWindow -PassThru -Wait
"exit=0x{0:X8} ({0})" -f $p.ExitCode
