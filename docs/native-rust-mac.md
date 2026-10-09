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

`native/crates/ctxpack/`（独立 crate，不依赖任何 AppKit 类型 —— 这样 Windows 外壳能直接复用同一份逻辑，不必再维护第二套盘符折叠和 %APPDATA% 推导）：

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

## 8. 交付状态与合并策略（M6，2026-10-07）

### 里程碑验收结果

| 里程碑 | 状态 | 验收方式 |
| --- | --- | --- |
| M0 骨架 | ✅ | `screencapture -l` 截图 + Finder ⌘⇧G 探针证明键盘焦点没被抢 |
| M1 双窗 | ✅ | 同锚点高度切换、`drag.py` 拖动、位置回写 settings.json、启动 clamp（含负坐标副屏单测） |
| M2 直通闭环 | ✅ | 800ms `changeCount` 轮询 + 未读点；取入→复制逐字节比对（含首尾空白与内部双空格） |
| M3 ctxpack | ✅ | ctxpack 46 项 Rust 单测（全 crate 76 项），其中 `matches_typescript_reference_output` 与 TS 输出逐字比对（golden 文件 `native/fixtures/parity-ts.json`） |
| M4 会话解读 UI | ✅ | 真实会话（qoder / workbuddy）浏览—点选—填充—组装—复制全链路截图验收；设置编辑器按模式显隐 + 保存回写 |
| M5 内存与打磨 | ✅ | 入场动画连拍帧取证；Reduce Motion 打开后无中间态；按下态像素对照 |
| M6 交付 | ✅ | `build.sh` 出包 + 签名校验；本文 §8；README「原生版」一节 |

### 与计划的偏差（都是实测逼出来的，不是随手改的）

- **panel 窗口形态**：§3 写的「Titled 可 key」不成立 —— AppKit 在 order-front 时对 titled 窗口跑 `constrainFrameRect`，会把算好的锚点改掉（实测 x<221 一律被推到 221）。改为无边框 `NSPanel` 子类 + `canBecomeKeyWindow`，并在 `sendEvent:` 里复刻 renderer.js「mousedown 命中输入控件才 focusSelf」的语义。
- **内存目标**：§5 的「≤20MB、冲 10MB」低于本机 AppKit 下限。§6 的 `ps rss` 口径还把共享框架页算进来（实测 76–113MB）。真实口径（`footprint` / phys_footprint）：**空闲 36MB、展开 37MB、最重 42MB**；同机参照 WallpaperAgent 18MB（无窗口）、Dock 57MB、ControlCenter 75MB。快速展开/折叠会瞬时到 71MB，但 20/30 次不再涨、静置 15s 回落到 34MB —— 是 WindowServer 的有界 backing-store 池，不是泄漏。
  - **UI 走查重做后我又量了一次，结论是「重设计不费内存」，先前那句「+7MB」是我自己的口径错了**：拿跑了很久、开过设置与会话列表的进程，去比 §M5 那个刚启动的数。同机同状态对照（`git archive` 出两份源码分别构建）：`51acf28`（重做前）35.2MB、`be41648`（重做后）33.5MB、本轮修改后 34.5MB —— 差异在噪声里。
  - **真正的内存大户是 `maskImage`，已换掉**：圆角原先靠给 `NSVisualEffectView` 挂一张与窗口等大的遮罩位图，`arrange` 每次改高度都要重画一张（入场动画 10 帧 = 10 张）。改成图层 `cornerRadius + masksToBounds` 后：**空闲 22MB，30 次展开/折叠后仍是 22.0MB 不涨**（原先 34.5 → 37.6，瞬时峰值见过 80MB）。圆角与尺寸无关，也就不存在「动画中途被拉成椭圆」的问题。§5 的 ≤20MB 目标现在只差 2MB。
- **高度动画的驱动方式**：原先每帧重挂一个 one-shot `NSTimer`，入列开销 ~1.3ms 把帧距从 16.7ms 顶到 18.5ms，对不上 60Hz 的 vsync；而且一次切模式会连着调 `relayout` 三遍（`reload_sites` → `attach` → `refresh`），每遍各挂一条链，实测同一帧被驱动两次、`arrange` 开销翻倍。改成**整段动画只挂一条 repeating 计时器**（`relayout` 只改终点，不另起链），缓出按「已经走了多久」算而不是按第几帧算。实测：一拍一帧、帧距 17.4ms、单帧成本从 2.4ms 降到 ~0.6ms。
- **NSBox 的内容缩进**：全代码库都拿 `host.bounds()` 给盒子内的子视图定位，而 NSBox 会把 `contentView` 摆到 (6,6)、四边各缩掉 6pt（`setContentInsets:` 在 objc2-app-kit 0.3 里没绑，直接 msg_send 会抛 ObjC 异常 —— Rust 侧接不住，进程当场 abort）。后果是**胶囊里居中的标题比盒子中心高 5.75pt**（「取入选区」顶在蓝底上半截）、模式轨道的滑块被压成 10pt 薄片。修法是 `views::face()` 把缩进补回来，并且在**每次摆放时重算**：盒子没排过布局时 `contentView` 的缩进还不是真值，只在构造时算会拿到没缩进的面（实测「复制 Prompt」那颗因此反过来偏低 5.75pt）。
  两处补充：① `views::face()` 要按 contentView 的**真实原点**算，不能假设四边对称地缩；② 模式轨道的滑块与两段标签最终**不放进盒子的 contentView**，改为与轨道同为「行」的子孙 —— NSBox 会在自己布局时挪 contentView，已经躺在里面的子视图跟着整体平移，实测整组比轨道中线高 5pt、右端顶出盒子（胶囊那边靠「每次摆放重算」就够了，因为它只有被子视图自己用的一个坐标面）。
- **窗口投影关掉了**（Electron 那侧 `hasShadow: true`）：无边框窗口的投影按**矩形**内容轮廓算，四角外缘留下一圈没被投影盖到的亮直角。三条路都试过：图层 `cornerRadius` 只裁绘制、改不了投影形状；`NSWindow.setContentShape:` 在这台系统上直接抛 ObjC 异常（Rust 接不住，当场 abort）；退回从前的 `maskImage` 也没用 —— 拿 `git archive HEAD` 另建一份逐像素比对，四角同样有。关掉后靠 1px 发丝描边 + veil 仍然分得清层次。**要还原投影的话唯一正路是自绘**：容器层不裁、挂 `shadowRadius/shadowColor`，毛玻璃层单独裁圆角。
- **脏点判定不许回写草稿**：`sync_dirty()` 一开始写成「先 `commit_template()` 把编辑框折回草稿再比」，而启动时 `sync_from_state` 跑在 `reload_prompt_editor` 之前，那一刻编辑框还是空的 —— 于是把草稿里真实的模板抹成空串，表现为**模板框整片空白**，点保存还会把用户的指令一起抹掉。现在改成只读：把编辑框当前文字代进一份本地草稿副本再比。附带一个与 Electron 的语义差：事件计数式脏标记「改回去也还是脏」，值比较式会回到干净。
- **依赖**：§2 的清单外多了 `regex`（redact 的两条模式）与 `serde_json` 的 `preserve_order`（写回 settings.json 必须保持用户键序）。
- **§2 的 feature 名**（`NSApplicationConstants`/`NSGeometry`/`NSWindowConstants`）在 objc2-app-kit 0.3 里不存在；几何类型来自 `objc2_foundation`。
- **单实例保护**：§3 要求「原生版内部按 bundle id 查重」，M6 落地为 `main.rs::another_instance_is_running()`；裸二进制（无 bundle id）放行，沙箱并排调试不受影响。

### UI 走查：native 先改了，Electron 未跟（要同步就照这份清单反向移植）

范围按约定只动 `native/`，`packages/` 一个文件没碰。下面每条都是**行为或观感上的分歧点**，不是实现细节。

- **间距改成四档 4pt 网格**：S1 4（光学微调）/ S2 8（组内）/ S3 12（组间）/ S4 16（面板边距）。Electron 是 `padding: 8px 12px 12px` + 组内 gap 8 + 组间 gap 12；native 把左右与底部从 12 提到 16，400pt 宽的内容不再顶到圆角。**移植点**：`style.css` 的 `#panel` padding。
- **行距分「组内 8 / 组间 12」两档**：native 之前一律 12，把 `.line-slot` 空槽放大成了一个洞；发丝行两侧按组间算。Electron 本来就是这两档，无需改。
- **`.icon-btn` 22×22 → 24×24**，与 24pt 控件行高对齐；小图标原来的可点面积小于视觉预期。
- **模式开关换成自绘「轨道 + 滑块 + 裸标签」**。原因不是审美：`NSSegmentedControl` 的选中段（含 `setSelectedSegmentBezelColor`）只在 App 处于激活态时上色，而本面板按设计永不激活，于是「我在哪个模式」这个最重要的信息在真实使用态里读不出来。Electron 用 `aria-selected` + CSS 背景，没这个毛病 —— **这条是 native 的实现性偏差，不建议反向移植**。
- **设置披露行由实心胶囊改为裸字 + 三角（▸ / ▾ 随开合换字形）**，对齐 CSS `#settings summary` 的注释「plain text + chevron, fill only on hover」。分组标题不该和组里的站点按钮同一个视觉重量。
- **面板头的 ▾ 去掉底槽**，与右侧同为裸字形的 ✕ 配成一对（chip 的 ▸ 保留底槽：小条上它是唯一的动作入口，两个窗口不同处理是有意的）。
- **「保存设置」贴右**（CSS `#set-save { margin-left: auto }`：读起来是一个动作，不是又一块草稿框）。native 之前左对齐。
- **正文高度不再有 240pt 地板**：内容多高就多高，切模式时的高度差交给既有的高度动画。Electron 的 `autoHeight` 从来就没有地板。
- **组装失败的原因只写一处**：只留会话行的「上下文：<原因>」，状态行不再抄一遍（原来是两行同义的橙色）。**移植点**：删掉 `renderer.js` 里 attach 失败那次 `flashStatus(err, true)`。
- **提示收回**：非错误 4s、错误 3s，两者都会把状态行清空。Electron 的 `flashStatus` 只清非错误，错误文本会一直挂在 `#ctx-status` 上直到下一次写入。

- **保存按钮的脏点 ●**（CSS: `#set-save.dirty::after`）：native 已实现，槽位常年留着，亮起来不推按钮。判定方式与 Electron 不同 —— Electron 是 `#settings` 上的 `input` 事件一响就永久置脏，native 每次把「现在点保存会写出去的那份」按 `save_settings` 同样的取舍现算一遍再与已存值比，所以**改了又改回去会回到干净**。要同步的话得在 renderer 里做同样的比较，而不只是清 class。

native **缺**、Electron 有的：所有 hover 态（AppKit 侧没做 tracking area，只有按下态反馈）。

### 合并回 main 的三条路

`native/` 是独立 Cargo 包、不碰 `packages/`、不碰 `pnpm` 工作区，所以合并成本与顺序无关：

1. **保持实验分支（默认）** —— main 继续只演 Electron 版。适合「想看内存/体积收益，但产品形态还没定」。分支已随本文件携带全部上下文，随时可续。
2. **整体并入 main（推荐在 Electron 版仍为主发布通道时采用）** —— 一次性 squash，把 `native/` 与本文一起落到 main：
   ```bash
   git switch main && git merge --squash native/rust-mac && git commit -m "feat(native): add Rust + AppKit experimental macOS build"
   ```
   之所以用 squash 而不是 merge commit：分支里有按里程碑切分的过程性提交（含 M4 的返工），对 main 的历史没有信息量。合并后 Electron 版发布流程完全不变，`native/` 只是仓库里多出来的第二个可构建目标。
3. **反向替换（暂不建议）** —— 用原生版取代 Electron 版作为 mac 发布物。缺的前置条件：Windows 原生分支还没立项（§7 明确等 mac 路线验证后再说）、未做代码签名/公证（现网用户拿到的是 ad-hoc 包）、以及 §5 的「日常可替换」只在 mac 上成立。

**共同前提**：两版共用一份 settings.json，长期双跑要先解决双写（文件锁或分文件，§7 已列）。合并动作由维护者本人执行，本仓库约定 agent 不代为提交/推送。
