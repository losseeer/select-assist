#!/usr/bin/env bash
# §2 打包：cargo 产物组装成 select-assist-native.app，bundle id 区别于 Electron 版，ad-hoc 签名
set -euo pipefail
cd "$(dirname "$0")"
# 非交互 shell 里 rustup 的 PATH 追加还没生效
command -v cargo >/dev/null || PATH="$HOME/.cargo/bin:$PATH"

if [[ ${1:-} == --debug ]]; then
  cargo build
  bin=target/debug/select-assist-native
else
  cargo build --release
  bin=target/release/select-assist-native
fi

app=dist/select-assist-native.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/select-assist-native"
cp Info.plist "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
codesign --force --sign - "$app"
echo "$app"
