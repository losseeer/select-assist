# select-assist

选中任意 agent 界面里的一段输出 → 打包成可见、可控的上下文包（ctxpack）→ 粘到另一个网页模型问一句，不污染原会话、不消耗已配置的推理 API key。

v0.1 · 状态：P0 已实现（Track B 直采 + 常驻面板 + ctxpack 契约）。

## 包

| 包 | 说明 | 状态 |
|---|---|---|
| [`packages/ctxpack`](packages/ctxpack) | `ctxpack/0` 数据契约、校验、渲染模板、截断策略 + Track B 适配器（Claude Code / Codex / WorkBuddy / Qoder） | ✅ P0 |
| [`packages/panel`](packages/panel) | Electron 常驻悬浮面板（chip/panel 双窗口、不抢焦点、剪贴板取用、跨 agent 会话浏览、一键复制组装 Prompt、打开目标站） | ✅ P0 |
| [`packages/bridge-ext`](packages/bridge-ext) | Track A 浏览器扩展 + 本机接线（127.0.0.1 随机端口 + pairing token） | ⏳ P1 |

## 交互模型：拉取，不推送

无任何自动触发。面板常驻折叠态（圆点 + 「取入选区」按钮），只有在用户点击「取入选区」时才读剪贴板并产出只带选区的 pack（瞬时返回）。「附带会话上下文」默认关闭，勾选后才异步读盘解析会话；每次取用都显示来源（剪贴板/页面选区）、时间、字符数与判定会话及判据。

隐私边界：transcript 只收 user/assistant 纯文本；tool 结果、thinking/reasoning、子代理侧链、developer/system 指令、会话续接摘要默认丢弃并记入 `limits.dropped`，超限截断可见、禁止静默。

输出形态：「复制 Prompt」一键产出组装后的提问 = 指令行（默认：`请根据以下用户与agent的交互记录，解释用户选中的词 / 句子：「{selection}」`，设置里可改）+ 纯净上下文包（`clean/v1` 模板，只有 `用户> / 助手>` 记录，无「上下文包」「选区原文」等指引句）。面板按钮下方实时显示组装字数与已省略项。

会话发现不需要知道存储路径：勾选「附带会话上下文」后可点「浏览会话」，面板会跨 agent（Claude Code / Codex / WorkBuddy / Qoder）按最近修改列出会话（agent · 项目 · 名称 · 首条内容预览），点一条即填充，判据显示为「用户手动选择」。Qoder 同时覆盖两处存储：Qoder CN IDE 的 `~/.qoder-cn/projects/*.jsonl`（Claude 风格）与 QoderWork 的 `agents.db`（sqlite）。Trae 的会话库（ai-agent/database.db）为加密存储，暂不支持，只能走剪贴板选区。

## 开发

```bash
pnpm install        # 首次需允许 electron 构建脚本（已在 pnpm.onlyBuiltDependencies 声明）
pnpm build          # 全量构建
pnpm test           # ctxpack 契约与适配器测试
pnpm panel          # 启动悬浮面板
```

## 已知待核实（对应设计文档 §待核实）

- 双窗口方案（chip 窗永久 `focusable:false`，panel 窗独立 `focusable:true`，展开/折叠用 show/hide 而非同窗 resize）在各类终端前台时的选区/滚动保持表现——需真机长期使用验证；
- MV3 扩展长期轮询本机端口的后台休眠限制（P1）；
- dsh web URL/DOM 里稳定取 sessionId（P1）；
- 各目标站输入框上限（当前默认 maxChars=8000，面板内可调）。
