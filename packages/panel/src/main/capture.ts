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
  DEFAULT_MAX_CHARS,
  type CtxPack,
  type SessionRef,
  type TrackBAdapter,
  type TranscriptTurn,
} from '@select-assist/ctxpack';
import { clipboard } from 'electron';
import type { Settings } from './settings.js';

const ADAPTERS: Record<string, TrackBAdapter> = {
  'claude-code': claudeCodeAdapter,
  codex: codexAdapter,
  workbuddy: workbuddyAdapter,
  qoder: qoderAdapter,
};

export const AGENT_CHOICES = ['auto', ...Object.keys(ADAPTERS)] as const;

/** pure record, no scaffolding sentences — the prompt template provides the framing */
const PACK_TEMPLATE = 'clean/v1';

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
  captureFromClipboard(settings: Settings): SelectionSummary {
    const text = clipboard.readText();
    const summary: SelectionSummary = text.trim()
      ? (() => {
          const at = new Date().toISOString();
          this.pack = buildPack({
            selection: { text, role: 'unknown' },
            capture: { via: 'clipboard', at },
            source: { app: 'desktop' },
            maxChars: settings.maxChars,
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
      : { ok: false, reason: '剪贴板为空，请先在源界面 ⌘C 复制选中的文本' };
    this.selectionSummary = summary;
    return summary;
  }

  summary(): { selection?: SelectionSummary; context: ContextSummary; hasPack: boolean } {
    return { selection: this.selectionSummary, context: this.context, hasPack: !!this.pack };
  }

  /** Explicit opt-in only; may take hundreds of ms — never called by captureFromClipboard. */
  async attachContext(
    settings: Settings,
    opts: { agent: string; turns: number; filePath?: string },
  ): Promise<ContextSummary> {
    if (!this.pack) return (this.context = { error: '请先取入选区' });
    const cwd = settings.projectPath || undefined;

    let chosen: { adapter: TrackBAdapter; ref: SessionRef; basis: string } | undefined;
    try {
      const candidates = await this.discover(opts.agent, opts.filePath ? undefined : cwd);
      if (opts.filePath) {
        const own = candidates.find((c) => c.ref.filePath === opts.filePath);
        if (!own) return (this.context = { error: '所选会话已不可见（被移动或删除）' });
        chosen = { ...own, basis: '用户手动选择' };
      } else {
        const pick = pickSession(
          candidates.map((c) => c.ref),
          cwd,
        );
        if (pick?.ref) {
          const owner = candidates.find((c) => c.ref === pick.ref)!;
          chosen = { adapter: owner.adapter, ref: pick.ref, basis: pick.basis };
        }
      }
    } catch (e) {
      return (this.context = { error: `会话发现失败：${(e as Error).message}` });
    }

    if (!chosen) return (this.context = { error: '未发现可用会话文件，将只带选区' });

    const result = await chosen.adapter.readTranscript(chosen.ref);
    const base = this.pack;
    const turns: TranscriptTurn[] = result.turns.slice(-Math.max(1, opts.turns));
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
      maxChars: settings.maxChars,
      templateId: PACK_TEMPLATE,
      defaultDropped: droppedDefaults,
      generatedAt: base.generatedAt,
    });
    this.context = {
      agent: chosen.ref.agent,
      sessionId: chosen.ref.sessionId,
      basis: result.error ? `${chosen.basis}（解析失败，降级为只带选区）` : chosen.basis,
      turnsIncluded: result.error ? 0 : turns.length,
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
      maxChars: base.limits?.maxChars ?? DEFAULT_MAX_CHARS,
      templateId: PACK_TEMPLATE,
      generatedAt: base.generatedAt,
    });
    this.context = {};
  }

  /** Cross-agent session browser: everything discoverable, newest first. */
  async browse(limit = 40): Promise<BrowseEntry[]> {
    const all: { ref: SessionRef }[] = [];
    for (const adapter of Object.values(ADAPTERS)) {
      try {
        for (const ref of await adapter.discoverSessions({ limit: 15 })) all.push({ ref });
      } catch {
        /* one broken agent must not break the browser */
      }
    }
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
   * Assembled prompt = instruction line (selection quoted) + clean context pack,
   * hard-capped at settings.maxChars as a whole (see ctxpack assemblePrompt).
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
    const context = pack.transcript?.length ? (pack.payload ?? '') : '';
    const { prompt, dropped } = assemblePrompt({
      template: settings.promptTemplate,
      selection: pack.selection?.text ?? '',
      context,
      maxChars: settings.maxChars,
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
    cwd?: string,
  ): Promise<{ adapter: TrackBAdapter; ref: SessionRef }[]> {
    const names = agent === 'auto' ? Object.keys(ADAPTERS) : [agent];
    const out: { adapter: TrackBAdapter; ref: SessionRef }[] = [];
    for (const n of names) {
      const adapter = ADAPTERS[n];
      if (!adapter) continue;
      try {
        for (const ref of await adapter.discoverSessions({ cwd, limit: 10 })) out.push({ adapter, ref });
      } catch {
        /* agent home dir missing etc. — try the next one */
      }
    }
    out.sort((a, b) => b.ref.mtimeMs - a.ref.mtimeMs);
    return out;
  }
}
