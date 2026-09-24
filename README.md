# select-assist

在任意 agent 界面选中文本 → 取入一个可见可控的上下文包 → 一键复制组装好的 Prompt 去问免费网页模型。不污染原会话，不消耗已配置的 API key。

## 功能

- **拉取式交互**：常驻悬浮条只在点击「取入选区」时读剪贴板，永不自动弹出、不抢键盘焦点；展开/折叠是同一标题栏的高度变化。
- **会话上下文（可选、显式勾选才读盘）**：跨 agent 浏览会话（Claude Code / Codex / WorkBuddy / Qoder），按最近修改列出「agent · 项目 · 首条预览」，点一条即填充——不需要知道任何存储路径。
- **一键复制 Prompt**：指令行（`{selection}` 占位，设置可改）+ 纯净上下文（只有 `用户> / 助手>` 记录，无指引句）；整条受字符上限约束，超出先丢最旧轮、再截选区，均明示于面板。
- **隐私边界**：transcript 只收 user/assistant 纯文本；tool 结果、thinking、子代理、系统注入（含会话续接摘要）默认丢弃，省略项在面板上明示，禁止静默截断。

## 使用

```bash
pnpm install && pnpm build
pnpm panel                     # 启动悬浮条（屏幕右上）
```

1. 在 agent 界面选中要问的文字，`⌘C`；
2. 点悬浮条「取入选区」（有新复制时右侧会亮提示）；
3. 需要背景时勾选「附带会话上下文」，自动判定或点「浏览会话」手选；
4. 点「复制 Prompt」，去 DeepSeek / ChatGPT / Gemini（面板内一键打开）粘贴提问。

设置（字符上限、指令模板、项目路径、路径脱敏）在面板底部「设置」里改。

## 仓库结构

- `packages/ctxpack` — `ctxpack/0` 数据契约、渲染模板、截断策略、各 agent 会话适配器（库）
- `packages/panel` — Electron 悬浮面板
- `packages/bridge-ext` — P1 占位：浏览器扩展直读页面选区

开发：`pnpm test`（契约与适配器测试）、`pnpm panel` 启动。

## TODO

- [ ] P1（低优先）：浏览器扩展 + 本机接线（页面选区免复制直取）
- [ ] Trae 会话支持（其 `ai-agent/database.db` 加密，暂只能走剪贴板）
- [ ] Windows 适配：核心逻辑跨平台，主要是会话存储路径映射（`%USERPROFILE%\...`、`%APPDATA%\QoderWork`）与悬浮窗行为验证
