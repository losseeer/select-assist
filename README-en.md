# select-assist

[![license](https://img.shields.io/github/license/losseeer/select-assist)](LICENSE)
[![release](https://img.shields.io/github/v/release/losseeer/select-assist?include_prereleases)](https://github.com/losseeer/select-assist/releases)
[![stars](https://img.shields.io/github/stars/losseeer/select-assist?style=social)](https://github.com/losseeer/select-assist)
![PRs](https://img.shields.io/badge/PRs-welcome-brightgreen)

[简体中文](README.md) | English

Select text in any agent UI → one click to pull it into a floating panel: assemble it with your agent conversation context into a prompt for free web models (**Explain**), or get the byte-exact original text for translating, searching, or pasting anywhere (**Passthrough**). Pull-only, visible and controllable — never pollutes the original session, never spends your configured API keys.

## Features

- **Pull-only interaction**: the persistent floating bar reads the clipboard only when you click "Take selection" — it never pops up on its own and never steals keyboard focus; expand/collapse is just a height change of the same title bar.
- **Two modes** (segmented switch, each with its own site group and settings):
  - **Explain** — optionally attaches agent session history: browse sessions across agents (Claude Code / Codex / WorkBuddy / Qoder), listed by last-modified as "agent · session-id prefix · project · first-message preview"; click one to fill the context. Which stores get scanned is driven entirely by the settings "Session paths" list — it ships pre-filled with the 5 built-in locations (including QoderWork's agents.db); edit a line to relocate a source, delete one to stop scanning it, and a `project|path` line feeds the auto-detection hint. What you copy is an assembled prompt — keep several named instructions (name + template with a `{selection}` placeholder) and pick one from the dropdown before copying; the only trimming knob is "rounds" (last N rounds or All, full text preserved, no character truncation), with the assembled length shown live.
  - **Passthrough** — zero assembly: the copied text is the byte-exact selection (no template, no redaction), made for translation, search, and paste-anywhere workflows.
- **Privacy boundaries**: transcripts contain user/assistant plain text only; tool results, thinking, sub-agents, and system injections (including session-continuation summaries) are dropped by default, every omission is surfaced in the panel — silent truncation is forbidden; disk reads happen only in Explain mode.

## Getting started

### Option 1: download a release

Grab the current v0.1.0-beta.1 from [GitHub Releases](https://github.com/losseeer/select-assist/releases):

| Platform | File | Notes |
| --- | --- | --- |
| Windows x64 | `select-assist.Setup.<ver>.exe` | NSIS installer (per-user, no admin required) |
| Windows x64 | `select-assist.<ver>.exe` | Portable — double-click and go |
| macOS (Apple Silicon) | `select-assist-<ver>-arm64.dmg` / `-arm64-mac.zip` | Drag into Applications |

Builds are unsigned and not notarized: on Windows, click "More info → Run anyway" at SmartScreen; on macOS, right-click (or ⌘-click) → "Open" on first launch. Settings live in `%APPDATA%\select-assist\settings.json` and `~/Library/Application Support/select-assist/settings.json` respectively.

### Option 2: build from source

```bash
pnpm install
pnpm panel                          # build and run the floating bar
pnpm --filter @select-assist/panel dist   # (optional) package an installer for this machine (electron-builder)
```

### Daily workflow

1. Select the text you want to ask about in the agent UI and copy it (⌘C / Ctrl+C);
2. Click "Take selection" on the floating bar (an indicator lights up when there is a new copy);
3. Explain mode: context is auto-detected, or pick a session via "Browse sessions"; Passthrough mode: skip this step;
4. Click "Copy Prompt" / "Copy selection", then click a target-site button (LLM chat sites or translation/search sites — one group per mode, editable in settings) and paste.

## Repository layout

- `packages/ctxpack` — the `ctxpack/0` data contract, render templates, trimming policy, and per-agent session adapters (library)
- `packages/panel` — the Electron floating panel (`pnpm --filter @select-assist/panel dist` produces installers)
- `packages/bridge-ext` — P1 placeholder: a browser extension reading page selections directly

Development: `pnpm test` (contract and adapter tests).

## TODO

- [ ] Session adapters for more agents: Trae (its `ai-agent/database.db` is encrypted; clipboard-only for now), Zcode, DeepSeek harness, and more
