<#
  Windows 侧的打包，对应 mac 的 build.sh。
  产出：dist/select-assist-native-win-x64-<版本>.exe 与同名 zip，外加一份对 zip 的 .sha256。

  三处和 mac 不一样，都是有意的：
  · 没有 .app 那种目录结构，也没有 plist —— 版本号写不进裸 exe（要写就得拉一个资源
    编译器进依赖链，而这台机器上没有签名需求，收益只是属性页上多一行字）。
    所以版本只体现在文件名里。
  · 不签名。Electron 那两条 Windows 产物同样是 NotSigned，SmartScreen 的"未知发布者"
    提示两版都有，不是 native 版新引入的问题。
  · 单文件就是全部运行时：静态链接 CRT，目标机器不需要装 VC++ redist。
#>
# 不写 [CmdletBinding()]：它自带的 -Debug/-Verbose 会跟下面的 -Debug 撞名，
# 而我们只需要一个开关
param(
    [switch]$Dev
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSCommandPath)

$env:PATH += ";$env:USERPROFILE\.cargo\bin"
$profile_ = if ($Dev) { 'debug' } else { 'release' }
& cargo build $(if ($Dev) { @() } else { @('--release') }) -p app-win
if ($LASTEXITCODE -ne 0) { "cargo build 失败"; exit $LASTEXITCODE }

$version = (Select-String -Path crates/app-win/Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { '读不到 Cargo.toml 里的 version'; exit 1 }

$exe = "target/$profile_/select-assist-native-win.exe"
$dist = 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
# 名字里带 x64：与 mac 侧的 select-assist-native-mac-arm64-<版本>.zip 同一套命名
$name = "select-assist-native-win-x64-$version"
$out = Join-Path $dist "$name.exe"
Copy-Item $exe $out -Force
$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $out -DestinationPath $zip

# 校验和对准 mac 侧：是给 **zip** 的，文件名 <zip>.sha256，放一起就能直接
# `sha256sum -c select-assist-native-win-x64-<版本>.zip.sha256`
# 两处都得留意：里面只写基名（不写 dist/ 前缀，否则换目录就验不了）；
# 行尾必须是 LF —— Set-Content 默认写 CRLF，那个 \r 会被算进文件名，
# sha256sum 报的是"FAILED open or read"，看着像文件丢了而不是格式错了
$zipHash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
[IO.File]::WriteAllText((Join-Path (Get-Location) "$zip.sha256"), "$zipHash  $name.zip`n")
$exeHash = (Get-FileHash -Algorithm SHA256 $out).Hash.ToLower()

$mb = [math]::Round((Get-Item $out).Length / 1MB, 2)
$zmb = [math]::Round((Get-Item $zip).Length / 1MB, 2)
"{0}  v{1}  exe={2}MB  zip={3}MB" -f $out, $version, $mb, $zmb
"zip    sha256={0}" -f $zipHash
"exe    sha256={0}" -f $exeHash
