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

# 系统自带的是 bash 3.2，空数组在 set -u 下展开会直接报 unbound variable
if [[ $profile == release ]]; then
  cargo build --release
else
  cargo build
fi

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -1)
[[ -n "$version" ]] || { echo "Cargo.toml 里读不到 version" >&2; exit 1; }

app=dist/select-assist-native.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "target/$profile/select-assist-native" "$app/Contents/MacOS/select-assist-native"
cp Info.plist "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$version" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

codesign --force --sign - "$app"
codesign --verify --strict "$app"

printf '%s  %s  v%s  %s\n' "$app" "$profile" "$version" "$(du -sh "$app" | cut -f1)"
