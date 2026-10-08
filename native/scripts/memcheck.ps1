<#
  量一个进程的内存与句柄数，按"空闲 chip / 展开面板 / 展开设置组"三档分别取。
  为什么报两个数：mac 那边比的是 phys_footprint，Windows 上最接近的是 Private Bytes
  （WorkingSet 会被系统回收，页面缓存一进来就虚高）。只报 WorkingSet 会得出
  "原生版比 Electron 还费内存"的荒唐结论。
#>
param(
    [int]$Pid2,
    [string[]]$Labels = @('chip')
)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class M {
    [StructLayout(LayoutKind.Sequential)]
    public class PSS : IDisposable {
        public int cb = Marshal.SizeOf(typeof(PSS));
        public int flags, cnt, hs, ws, qu, us, pr, wc, gp, def, paged, wsPeak, quPeak, pagedPeak;
        public IntPtr priv;
        public void Dispose() { }
    }
    [DllImport("psapi.dll", SetLastError = true)]
    public static extern bool GetProcessMemoryInfo(IntPtr h, PSS pmi, int cb);
}
'@
$p = Get-Process -Id $Pid2
$pmc = [M+PSS]::new()
$ok = [M]::GetProcessMemoryInfo($p.Handle, $pmc, $pmc.cb)
$priv = if ($ok) { [math]::Round($pmc.priv / 1MB, 1) } else { 'n/a' }
$ws = [math]::Round($p.WorkingSet64 / 1MB, 1)
"{0,-10} pid={1} threads={2} handles={3} WorkingSet={4}MB PrivateBytes={5}MB" -f `
    ($Labels[0]), $Pid2, $p.Threads.Count, $p.HandleCount, $ws, $priv
