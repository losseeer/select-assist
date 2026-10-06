import {
  assemblePrompt,
  buildPack,
  pickSession,
  render,
  redactPaths,
  claudeCodeAdapter,
  codexAdapter,
  workbuddyAdapter,
  qoderAdapter,
  type CtxPack,
  type SessionRef,
  type TrackBAdapter,
  type TranscriptTurn,
} from '@select-assist/ctxpack';
import { clipboard } from 'electron';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { activePromptTemplate, type Settings } from './settings.js';

const ADAPTERS: Record<string, TrackBAdapter> = {
  'claude-code': claudeCodeAdapter,
  codex: codexAdapter,
  workbuddy: workbuddyAdapter,
  qoder: qoderAdapter,
};

export const AGENT_CHOICES = ['auto', ...Object.keys(ADAPTERS)] as const;

/** Resolve which adapter reads an explicitly chosen file; path layout identifies the agent. */
function adapterForFile(file: string, hint: string): { adapter: TrackBAdapter; agent: string } | undefined {
  if (hint !== 'auto' && ADAPTERS[hint]) return { adapter: ADAPTERS[hint]!, agent: hint };
  const f = file.replace(/\\/g, '/'); // Windows paths arrive with backslashes
  if (f.includes('/.codex/')) return { adapter: ADAPTERS.codex!, agent: 'codex' };
  if (f.includes('/.workbuddy/')) return { adapter: ADAPTERS.workbuddy!, agent: 'workbuddy' };
  if (f.includes('/.qoder-cn/') || f.includes('QoderWork')) return { adapter: ADAPTERS.qoder!, agent: 'qoder' };
  if (f.endsWith('.jsonl')) return { adapter: ADAPTERS['claude-code']!, agent: 'claude-code' };
  return undefined;
}

const PACK_TEMPLATE = 'clean/v1';

/** the round-count choice is the ONLY trimming knob — no char truncation anywhere */
const NO_BUDGET = Number.MAX_SAFE_INTEGER;

export interface SelectionSummary {
  ok: boolean;
  reason?: string;
  firstLine?: string;
  chars?: number;
  captureAt?: string;
  via?: string;
}

export interface ContextSummary {
  agent?: string;
  sessionId?: string;
  basis?: string;
  turnsIncluded?: number;
  error?: string;
}

export interface BrowseEntry {
  agent: string;
  filePath: string;
  sessionId?: string;
  name?: string;
  projectPath?: string;
  preview?: string;
  mtime: string;
}

export class Capturer {
  pack: CtxPack | undefined;
  selectionSummary: SelectionSummary | undefined;
  context: ContextSummary = {};

  /** Track B selection source: the clipboard, must be instant (no parsing here). */
  captureFromClipboard(): SelectionSummary {
    let text: string;
    try {
      text = clipboard.readText();
    } catch (e) {
      const s: SelectionSummary = { ok: false, reason: `读取剪贴板失败：${(e as Error).message}` };
      this.selectionSummary = s;
      return s;
    }
    const summary: SelectionSummary = text.trim()
      ? (() => {
          const at = new Date().toISOString();
          // buildPack requires a non-empty selection.text: store the trimmed copy
          // so a whitespace-only clipboard (e.g. copying a blank line) cannot throw.
          this.pack = buildPack({
            selection: { text: text.trim(), role: 'unknown' },
            capture: { via: 'clipboard', at },
            source: { app: 'desktop' },
            maxChars: NO_BUDGET,
            templateId: PACK_TEMPLATE,
          });
          this.context = {};
          return {
            ok: true,
            firstLine: text.split('\n')[0]?.slice(0, 120) ?? '',
            chars: text.length,
            captureAt: at,
            via: 'clipboard',
          };
        })()
      : { ok: false, reason: '剪贴板为空，请先在源界面复制选中的文本' };
    this.selectionSummary = summary;
    return summary;
  }

  summary(): { selection?: SelectionSummary; context: ContextSummary; hasPack: boolean } {
    return { selection: this.selectionSummary, context: this.context, hasPack: !!this.pack };
  }

  /** Explicit opt-in only; may take hundreds of ms — never called by captureFromClipboard. */
  async attachContext(
    settings: Settings,
    opts: { agent: string; turns: number; filePath?: string; sessionId?: string },
  ): Promise<ContextSummary> {
    if (!this.pack) return (this.context = { error: '请先取入选区' });
    // 'project' entries are cwd hints; other entries are concrete sessions folded in below
    const cwd = settings.sessionPaths.find((p) => p.agent === 'project')?.path || undefined;
    // a project path pointing at a .jsonl file is a direct session pick
    const directFile = opts.filePath ?? (cwd?.endsWith('.jsonl') ? cwd : undefined);

    let chosen: { adapter: TrackBAdapter; ref: SessionRef; basis: string } | undefined;
    if (directFile) {
      if (!fs.existsSync(directFile)) {
        return (this.context = { error: `会话文件不存在：${directFile}` });
      }
      const owner = adapterForFile(directFile, opts.agent);
      if (!owner) return (this.context = { error: `无法判定该文件的 agent 类型：${directFile}` });
      const base = path.basename(directFile).replace(/\.(jsonl|db)$/, '');
      chosen = {
        adapter: owner.adapter,
        ref: {
          agent: owner.agent,
          adapter: owner.adapter.adapter,
          filePath: directFile,
          sessionId: opts.sessionId ?? (/^[0-9a-f-]{36}$/.test(base) ? base : undefined),
          mtimeMs: fs.statSync(directFile).mtimeMs,
        },
        basis: opts.filePath ? '用户手动选择' : '设置指定会话文件',
      };
    } else {
      try {
        const candidates = await this.discover(opts.agent, cwd, settings);
        const pick = pickSession(
          candidates.map((c) => c.ref),
          cwd,
        );
        if (pick?.ref) {
          const owner = candidates.find((c) => c.ref === pick.ref)!;
          chosen = { adapter: owner.adapter, ref: pick.ref, basis: pick.basis };
        }
      } catch (e) {
        return (this.context = { error: `会话发现失败：${(e as Error).message}` });
      }
    }

    if (!chosen) return (this.context = { error: '未发现可用会话文件，将只带选区' });

    const result = await chosen.adapter.readTranscript(chosen.ref);
    const base = this.pack;
    // one 轮 = one user + one assistant exchange; turns <= 0 means 全部（不截取）
    const turns: TranscriptTurn[] =
      opts.turns > 0 ? result.turns.slice(-opts.turns * 2) : result.turns;
    const droppedDefaults = [
      ...(result.error ? [`adapter-error:${result.error}`, ...result.dropped] : result.dropped),
    ];

    this.pack = buildPack({
      selection: base.selection!,
      capture: base.capture,
      source: {
        ...base.source,
        agent: chosen.ref.agent,
        sessionId: chosen.ref.sessionId,
        projectPath: chosen.ref.projectPath ?? cwd,
        adapter: chosen.adapter.adapter,
      },
      transcript: turns,
      maxChars: NO_BUDGET,
      templateId: PACK_TEMPLATE,
      defaultDropped: droppedDefaults,
      generatedAt: base.generatedAt,
    });
    this.context = {
      agent: chosen.ref.agent,
      sessionId: chosen.ref.sessionId,
      basis: result.error ? `${chosen.basis}（解析失败，降级为只带选区）` : chosen.basis,
      turnsIncluded: result.error ? 0 : Math.ceil(turns.length / 2),
      error: result.error,
    };
    return this.context;
  }

  clearContext(): void {
    if (!this.pack) return;
    const base = this.pack;
    this.pack = buildPack({
      selection: base.selection!,
      capture: base.capture,
      source: base.source,
      maxChars: NO_BUDGET,
      templateId: PACK_TEMPLATE,
      generatedAt: base.generatedAt,
    });
    this.context = {};
  }

  /** Cross-agent session browser: exactly what 设置→会话路径 lists, newest first. */
  async browse(settings: Settings, limit = 40): Promise<BrowseEntry[]> {
    const seen = new Set<string>();
    const all = (await this.userCandidates(settings)).filter((c) => {
      const key = `${c.ref.filePath}#${c.ref.sessionId ?? ''}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    all.sort((a, b) => b.ref.mtimeMs - a.ref.mtimeMs);
    return all.slice(0, limit).map(({ ref }) => ({
      agent: ref.agent,
      filePath: ref.filePath,
      sessionId: ref.sessionId,
      name: ref.name,
      projectPath: ref.projectPath,
      preview: ref.preview,
      mtime: new Date(ref.mtimeMs).toISOString(),
    }));
  }

  /**
   * Assembled prompt = instruction line (selection quoted) + clean context pack.
   * No char trimming: the round count is the only size knob; the panel shows
   * the assembled length so the user can dial rounds down for smaller input boxes.
   */
  currentPayload(settings: Settings):
    | { prompt: string; context: string; usedChars: number; dropped: string[] }
    | undefined {
    if (!this.pack) return undefined;
    let pack = this.pack;
    if (settings.redactPaths) {
      pack = redactPaths({ ...pack });
      pack.payload = render(pack, PACK_TEMPLATE, pack.limits?.dropped ?? []);
    }
    // clean/v1 renders selection-only packs AS the selection text — using it as
    // context here would duplicate the selection inside the assembled prompt.
    const context = pack.transcript?.length ? (pack.payload ?? '') : '';
    // 直通模式 = 逐字节原文：脱敏只服务于上下文组装，不碰用户自己复制的选区
    if (!context) {
      const raw = this.pack.selection?.text ?? '';
      return {
        prompt: raw,
        context: '',
        usedChars: raw.length,
        dropped: [...(this.pack.limits?.dropped ?? [])],
      };
    }
    const { prompt, dropped } = assemblePrompt({
      template: activePromptTemplate(settings),
      selection: pack.selection?.text ?? '',
      context,
      maxChars: NO_BUDGET,
    });
    return {
      prompt,
      context,
      usedChars: prompt.length,
      dropped: [...(pack.limits?.dropped ?? []), ...dropped],
    };
  }

  copyToClipboard(settings: Settings): boolean {
    const cur = this.currentPayload(settings);
    if (!cur) return false;
    clipboard.writeText(cur.prompt);
    return true;
  }

  private async discover(
    agent: string,
    cwd: string | undefined,
    settings: Settings,
  ): Promise<{ adapter: TrackBAdapter; ref: SessionRef }[]> {
    let cands = await this.userCandidates(settings, cwd);
    if (agent !== 'auto') cands = cands.filter((c) => c.ref.agent === agent);
    cands.sort((a, b) => b.ref.mtimeMs - a.ref.mtimeMs);
    return cands;
  }

  /**
   * 设置→会话路径 is the ONLY discovery source. A named-agent entry runs that adapter with
   * root=path (so each adapter still parses its own layout: codex date dirs, qoder's sqlite…);
   * an 'auto' entry gets a generic *.jsonl scan classified by path.
   */
  private async userCandidates(
    settings: Settings,
    cwd?: string,
  ): Promise<{ adapter: TrackBAdapter; ref: SessionRef }[]> {
    const out: { adapter: TrackBAdapter; ref: SessionRef }[] = [];
    const seen = new Set<string>();
    const push = (c: { adapter: TrackBAdapter; ref: SessionRef }) => {
      const key = `${c.ref.filePath}#${c.ref.sessionId ?? ''}`;
      if (seen.has(key)) return;
      seen.add(key);
      out.push(c);
    };
    for (const entry of settings.sessionPaths) {
      if (!entry.path || entry.agent === 'project') continue;
      let st: fs.Stats;
      try {
        st = await fs.promises.stat(entry.path);
      } catch {
        continue; // 路径失效：跳过，不打断发现流程
      }
      const named = ADAPTERS[entry.agent];
      if (named) {
        try {
          if (st.isFile() && entry.path.endsWith('.jsonl')) {
            push({ adapter: named, ref: await fileRef(named, entry.path) });
          } else {
            // directories AND .db files: the adapter knows its own layout under this root
            for (const ref of await named.discoverSessions({ root: entry.path, cwd, limit: 15 })) {
              push({ adapter: named, ref });
            }
          }
        } catch {
          /* one broken source must not break discovery */
        }
        continue;
      }
      // 'auto' (or an unknown token): generic scan, classify by path shape
      const files = st.isFile() ? [entry.path] : await scanJsonl(entry.path, 2);
      for (const f of files) {
        const owner = adapterForFile(f, 'auto');
        if (!owner) continue;
        try {
          push({ adapter: owner.adapter, ref: await fileRef(owner.adapter, f, owner.agent) });
        } catch {
          /* skip unreadable file */
        }
      }
    }
    return out;
  }
}

async function fileRef(adapter: TrackBAdapter, file: string, agent?: string): Promise<SessionRef> {
  const base = path.basename(file).replace(/\.(jsonl|db)$/, '');
  return {
    agent: agent ?? adapter.agent,
    adapter: adapter.adapter,
    filePath: file,
    sessionId: /^[0-9a-f-]{36}$/.test(base) ? base : undefined,
    mtimeMs: (await fs.promises.stat(file)).mtimeMs,
  };
}

async function scanJsonl(dir: string, depth: number): Promise<string[]> {
  let entries: fs.Dirent[];
  try {
    entries = await fs.promises.readdir(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  const out: string[] = [];
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isFile() && e.name.endsWith('.jsonl')) out.push(p);
    else if (e.isDirectory() && depth > 0) out.push(...(await scanJsonl(p, depth - 1)));
    if (out.length >= 200) break; // 单条目录上限，防呆
  }
  return out.slice(0, 200);
}
