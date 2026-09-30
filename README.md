# select-assist

在任意 agent 界面选中文本 → 点一下取入悬浮面板：可以带上会话上下文组装成 Prompt 去问免费网页模型（**会话解读**），也可以拿到逐字节原文直接翻译、搜索或粘贴到任何地方（**选区直通**）。拉取式、可见可控，不污染原会话，不消耗已配置的 API key。

## 功能

- **拉取式交互**：常驻悬浮条只在点击「取入选区」时读剪贴板，永不自动弹出、不抢键盘焦点；展开/折叠是同一标题栏的高度变化。
- **两种模式**（分段切换，各自独立的站点组与设置项）：
  - **会话解读**——可选附带 agent 会话历史：跨 agent 浏览会话（Claude Code / Codex / WorkBuddy / Qoder），按最近修改列出「agent · 会话 id 片段 · 项目 · 首条预览」，点一条即填充，不需要知道任何存储路径；复制的是组装 Prompt（指令行 `{selection}` 占位 + 纯净上下文）。唯一的裁剪手段是「轮数」（取最近 N 轮，全文保留不做字符截断），组装总字数实时显示。
  - **选区直通**——零组装：复制的就是选区的逐字节原文（不套模板、不脱敏），面向翻译、搜索、任意粘贴场景。
- **隐私边界**：transcript 只收 user/assistant 纯文本；tool 结果、thinking、子代理、系统注入（含会话续接摘要）默认丢弃，省略项在面板上明示，禁止静默截断；读盘只发生在会话解读模式。

## 使用

### 方式一：下载发布版

到 [GitHub Releases](https://github.com/losseeer/select-assist/releases) 下载当前 v0.1.0-beta.1：

| 平台 | 文件 | 说明 |
| --- | --- | --- |
| Windows x64 | `select-assist.Setup.<ver>.exe` | NSIS 安装器（当前用户，免管理员） |
| Windows x64 | `select-assist.<ver>.exe` | Portable，双击即用 |
| macOS (Apple Silicon) | `select-assist-<ver>-arm64.dmg` / `-arm64-mac.zip` | 拖入「应用程序」 |

产物未做代码签名/公证：Windows 遇 SmartScreen 点「更多信息 → 仍要运行」；macOS 首次启动右键（或 ⌘ 点击）→「打开」。设置分别存于 `%APPDATA%\select-assist\settings.json` 与 `~/Library/Application Support/select-assist/settings.json`。

### 方式二：源码构建

```bash
pnpm install
pnpm panel                          # 构建并启动悬浮条
pnpm --filter @select-assist/panel dist   # （可选 ）打包本机安装包（electron-builder）
```

### 日常流程

1. 在 agent 界面选中要问的文字，复制（⌘C / Ctrl+C）；
2. 点悬浮条「取入选区」（有新复制时提示点亮）；
3. 会话解读：自动判定或「浏览会话」手选一条填充上下文；选区直通：跳过此步；
4. 点「复制 Prompt」/「复制选区原文」，再点目标站按钮（LLM 对话站或翻译/搜索站，按模式各一组，设置可编辑）粘贴提问。

## 仓库结构

- `packages/ctxpack` — `ctxpack/0` 数据契约、渲染模板、截断策略、各 agent 会话适配器（库）
- `packages/panel` — Electron 悬浮面板（`pnpm --filter panel dist` 出安装包）
- `packages/bridge-ext` — P1 占位：浏览器扩展直读页面选区

开发：`pnpm test`（契约与适配器测试）。

## TODO

- [ ] 更多 agent 的会话适配：Trae（`ai-agent/database.db` 加密，暂只能走剪贴板）、Zcode、DeepSeek harness 等
