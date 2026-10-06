# 原生 Rust + AppKit 重构方案（分支实验，不替换 Electron 版）

## 1. 分支与仓库布局

```bash
git checkout -b native/rust-mac main
```

- 新代码全部放 `native/`（独立 Cargo 包），**不触碰 `packages/`**；`native/target/` 加入 .gitignore。
- Electron 版继续在 main 演进，两分支互不干扰；`docs/native-rust-mac.md`（本文件）随分支携带。

## 2. 工具链准备（本机当前缺）

```bash
brew install rust            # 或 curl https://sh.rustup.rs -sSf | sh（推荐 rustup，可锁 stable）
xcode-select -p              # 已有 CLT：/Library/Developer/CommandLineTools
```

依赖 crate（Cargo.toml）：

```toml
[dependencies]
objc2 = "0.6"
objc2-app-kit = { version = "0.3", features = ["std", "NSWindow", "NSPanel", "NSVisualEffectView", "NSSwitch", "NSSegmentedControl", "NSPopUpButton", "NSPasteboard", "NSWorkspace", "..."] }
objc2-foundation = { version = "0.3", features = [...] }
block2 = "0.6"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.37", features = ["bundled"] }   # QoderWork agents.db
dirs = "6"                                                 # home dir
```

打包：手写 `build.sh` 生成 `select-assist-native.app`（Contents/MacOS + Info.plist，bundle id 用 `dev.select-assist.native` **区别于 Electron 版**），`codesign --force --sign - ` ad-hoc 签名。

## 3. 功能 → AppKit API 映射（对照 Electron 现状）

| Electron 现状 | 原生等价物 |
| --- | --- |
| chipWin `focusable:false` 非激活悬浮 | `NSPanel` + `.nonactivatingPanel`，不 `makeKey`；level = `.floating` |
| panelWin `focusable:true` show/hide | 第二个 `NSPanel`（可 key），`orderOut/orderFront` |
| `vibrancy:'hud'` | `NSVisualEffectView` material `.hudWindow`，`state = .active` |
| 圆角+透明窗口 | `isOpaque=false` + `backgroundColor=.clear` + `hasShadow=true` |
| expand/collapse 同 x/y 高度切换 | 两窗口同 origin，`setFrame(display:animate:)` |
| `alwaysOnTop` + topMost 重申 | level 监听 `NSApplication.didBecomeActive` 重设 |
| 无边框拖动（app-region: drag） | `isMovableByWindowBackground = true`，控件区自动不拖 |
| 800ms 剪贴板轮询 | `NSPasteboard.general.changeCount` 定时器（同逻辑：lastClip/clipUnread 状态机照搬） |
| NSSwitch=「会话上下文」旧开关 | 本项目用 `NSSegmentedControl`（会话解读/选区直通），与现 UI 语义一致 |
| select 下拉 | `NSPopUpButton` |
| tooltip | `NSView.toolTip`（原生悬浮提示，替代自绘 tooltip——注意 alwaysOnTop 遮挡问题在原生里不存在） |
| details 折叠设置 | `NSStackView` + `NSButton` 手风琴或 `NSDisclosureBezel` |
| `shell.openExternal` | `NSWorkspace.shared.open(URL)` |
| `app.requestSingleInstanceLock` | bundle id 不同 → 与 Electron 版**天然不互斥**；原生版内部用 `NSRunningApplication` 按 bundle id 查重 |
| settings.json | 读写**同一个** `~/Library/Application Support/select-assist/settings.json`（schema 已含 withContext/chatSites/directSites/promptTemplate/projectPath/redactPaths）。⚠️ 开发与调试时先退出 Electron 版，避免双写覆盖（两程序都 patch 该文件） |

## 4. ctxpack 移植（纯逻辑，Rust 直译 + 单测）

`native/src/ctxpack/`：

- `types.rs` — CtxPack/TranscriptTurn/SiteTarget 等（serde 对齐 JSON 字段名）
- `render.rs` — clean/v1 渲染（`用户> / 助手>` 行、分隔线）
- `prompt.rs` — assemblePrompt（`{selection}` 填充、maxChars 裁剪顺序：先丢最旧轮→再截 selection→hard-trim，dropped 语义逐条对齐）
- `redact.rs` — home→`~`、`/Users/x`→`~/user`、`C:\Users\x`→`~\user`
- `adapters/{claude_code,codex,workbuddy,qoder}.rs` — 移植时**以 ctxpack 测试用例为验收标准**：把 `packages/ctxpack/test/` 的 fixture JSONL 复制到 `native/fixtures/`，Rust 单测断言输出一致（28 个测试是行为规格书）
- 直通模式规则照搬：无 transcript ⇒ 复制逐字节原文（不套模板、不脱敏）

## 5. 里程碑（每个都有可验证验收）

- **M0 骨架**：cargo 编译过、.app 双击启动、chip 窗口显示（400×44、hudWindow 毛玻璃、圆角、浮于全屏之上、点击不抢他应用键盘焦点）。验收：`screencapture -l` 截图 + 在别的编辑器里打字确认焦点未丢。
- **M1 双窗**：expand/collapse（同位置高度切换、面板自适应内容高度）、拖动、位置持久化到 settings.json、启动时 clampToDisplay（离屏显示器保护，逻辑照搬 main/index.ts）。
- **M2 直通闭环**：剪贴板轮询 + 未读小红点 + 「取入选区」→「复制选区原文」逐字节回写 + 站点按钮 `NSWorkspace.open`。**此时已可作为日常工具替换使用**。
- **M3 ctxpack**：Rust 单测全绿（对齐 TS fixture），会话发现（4 adapter 目录扫描 + mtime 排序 + cwd 匹配）。
- **M4 会话解读 UI**：模式分段、agent/轮数下拉、浏览会话列表（可滚动、点选填充）、组装 Prompt 复制、dropped 明示（禁止静默截断）、设置编辑器（模板/项目路径/脱敏/双站点组，按模式显隐）。
- **M5 内存与打磨**：`ps -o rss` 空闲实测（目标 ≤20MB，冲 10MB）、动画（入场 fade+rise 180ms、按钮按下态）、`prefers-reduced-motion` 等价（系统 Reduce Motion 检测）。
- **M6 交付**：build.sh 出 .app、README 增加"原生版（实验分支）"一节、切回 main 的合并策略说明。

## 6. 验证手段（无 CDP，沿用本机既有工作流）

- 窗口真值：python3 Quartz `CGWindowListCopyWindowInfo` 按 PID 过滤（Electron 版验证同款脚本可直接复用，owner 名会变成 `select-assist-native`）。
- 截图：`screencapture -o -x -l<windowID>`。
- 交互：合成点击 `/tmp/click.py`（注意 frameless 窗口拖拽区会把窗口点飞——原生版窗口用 isMovableByWindowBackground 后**同样有此坑**，每次点击前重新查 bounds）。
- 内存：`ps -o pid,rss,comm -p <pid>` 采样 60s 均值。
- 行为回归：把本仓库 `packages/panel` 里已验证过的场景清单（capture→badge→copy 逐字节、模式切换显隐、redact 只作用于解读、离屏位置回拉、双显示器负坐标）逐条在原生版重放。

## 7. 风险与止损

- **objc2 unsafe 样板**导致进度慢：M2 完成前投入约等于重写半个产品；若 M2 卡壳超预算，止损=停在分支，main 不受影响。
- AppKit 细节坑：非激活面板的首击语义、`NSPanel` 默认关窗即释放（`isReleasedWhenAnimated=false`）、vibrancy 下自绘文字对比度（沿用现有 `--bg` 遮罩思路加半透明层）。
- 双版本共存写同一 settings.json：开发期约定"跑原生版前先退 Electron 版"；若长期共存，再加文件锁或分文件。
- Windows 原生分支**暂不启动**，等 macOS 分支到 M4 验证路线成立后再立项。
