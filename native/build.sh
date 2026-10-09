#!/usr/bin/env bash
# §2 打包：cargo 产物组装成 select-assist-native.app，bundle id 区别于 Electron 版，ad-hoc 签名。
# 版本号只认 Cargo.toml 一处（写进 Info.plist）；签名或 plist 校验失败就以非零码退出，dist/ 里那个包别再用。
set -euo pipefail
cd "$(dirname "$0")"
# 非交互 shell 里 rustup 的 PATH 追加还没生效
command -v cargo >/dev/null || PATH="$HOME/.cargo/bin:$PATH"

case "${1:-}" in
  ""|--release) profile=release ;;
  --debug)      profile=debug ;;
  *) echo "usage: ./build.sh [--debug|--release]" >&2; exit 2 ;;
esac

if [[ $profile == release ]]; then
  cargo build --release -p select-assist-native
else
  cargo build -p select-assist-native
fi

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' crates/app-mac/Cargo.toml | head -1)
[[ -n "$version" ]] || { echo "Cargo.toml 里读不到 version" >&2; exit 1; }

app=dist/select-assist-native.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "target/$profile/select-assist-native" "$app/Contents/MacOS/select-assist-native"
cp Info.plist "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
# CFBundleVersion 是构建序号，归 Info.plist 自己管（发一次改一次）；
# 以前这里也拿 $version 覆盖它，于是 "0.1.0" 被当成 build number 写进去了
plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

codesign --force --sign - "$app"
codesign --verify --strict "$app"

# 分发产物：ditto 而不是 zip —— zip 不保留 bundle 的扩展属性和符号链接，
# 解出来的 .app 会丢签名
arch=$(uname -m)
name="select-assist-native-mac-$arch-$version"
zip="dist/$name.zip"
rm -f "$zip" "$zip.sha256"
ditto -c -k --keepParent "$app" "$zip"
shasum -a 256 "$zip" > "$zip.sha256"

printf '%s  %s  v%s  %s\n' "$app" "$profile" "$version" "$(du -sh "$app" | cut -f1)"
printf '%s  %s  ad-hoc 签名，未公证\n' "$zip" "$(du -sh "$zip" | cut -f1)"
