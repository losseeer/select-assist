import * as fs from 'node:fs';
import * as path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import type { TranscriptTurn } from '../types.js';
import type { SessionRef, TrackBAdapter, TranscriptResult } from './types.js';
import { stripSynthetic, mergeTurns } from './util.js';
import { makeJsonlAdapter } from './claude-code.js';

const AGENT = 'qoder';
const ADAPTER = 'qoder-composite@0';
const SQLITE_ADAPTER = 'qoderwork-sqlite@0';

/** Qoder CN IDE stores Claude-style JSONL transcripts under ~/.qoder-cn/projects */
const qoderCn: TrackBAdapter = makeJsonlAdapter(
  AGENT,
  'qoder-cn-jsonl@0',
  (home) => path.join(home ?? process.env.HOME ?? '', '.qoder-cn', 'projects')
);

function dbPath(home?: string): string {
  const base = home ?? process.env.HOME ?? '';
  return path.join(base, 'Library', 'Application Support', 'QoderWork', 'data', 'agents.db');
}

/** timestamps come as epoch seconds (or ms in some builds) */
function toMs(v: unknown): number {
  const n = Number(v) || 0;
  return n > 1e12 ? n : n * 1000;
}

function firstText(partsJson: string | null): { text?: string; nonText: number } {
  let nonText = 0;
  if (!partsJson) return { nonText };
  try {
    const parts = JSON.parse(partsJson) as any[];
    for (const p of parts) {
      if (p?.type === 'text' && typeof p.text === 'string') {
        const t = stripSynthetic(p.text);
        if (t) return { text: t, nonText };
      } else {
        nonText++;
      }
    }
  } catch {
    /* malformed parts: ignore */
  }
  return { nonText };
}

const qoderWork: TrackBAdapter = {
  agent: AGENT,
  adapter: SQLITE_ADAPTER,

  async discoverSessions(opts = {}): Promise<SessionRef[]> {
    const { cwd, limit = 20, home } = opts;
    const file = dbPath(home);
    if (!fs.existsSync(file)) return [];
    const refs: SessionRef[] = [];
    let db: DatabaseSync;
    try {
      db = new DatabaseSync(file, { readOnly: true });
    } catch (e) {
      return Promise.reject(new Error(`无法打开 Qoder 数据库: ${(e as Error).message}`));
    }
    try {
      const rows = db
        .prepare(
          `select c.id, c.name, c.updated_at, p.path as project_path
             from chats c left join projects p on p.id = c.project_id
            where c.deleted_at is null
            order by c.updated_at desc limit 60`,
        )
        .all() as any[];
      const previewStmt = db.prepare(
        `select parts from messages where chat_id = ? and role = 'user' order by sequence limit 1`,
      );
      for (const r of rows) {
        let preview: string | undefined;
        try {
          const p = previewStmt.get(r.id) as any;
          preview = firstText(p?.parts ?? null).text?.slice(0, 160);
        } catch {
          /* preview is best-effort */
        }
        refs.push({
          agent: AGENT,
          adapter: SQLITE_ADAPTER,
          filePath: file,
          sessionId: String(r.id),
          name: r.name ?? undefined,
          projectPath: r.project_path ?? undefined,
          preview,
          mtimeMs: toMs(r.updated_at),
        });
      }
    } finally {
      db.close();
    }
    const matched = cwd
      ? refs.filter((r) => r.projectPath === cwd || r.projectPath?.startsWith(cwd + path.sep))
      : refs;
    return (matched.length > 0 ? matched : refs).slice(0, limit);
  },

  async readTranscript(ref: SessionRef): Promise<TranscriptResult> {
    let db: DatabaseSync;
    try {
      db = new DatabaseSync(ref.filePath, { readOnly: true });
    } catch (e) {
      return { turns: [], dropped: [], error: `无法打开 Qoder 数据库: ${(e as Error).message}` };
    }
    const dropped = new Set<string>();
    try {
      const rows = db
        .prepare(
          `select role, parts from messages where chat_id = ? order by sub_chat_id, sequence`,
        )
        .all(ref.sessionId ?? '') as any[];
      if (rows.length === 0) {
        return { turns: [], dropped: [...dropped], error: '该会话没有消息' };
      }
      const raw: { role: 'user' | 'assistant'; text: string }[] = [];
      for (const r of rows) {
        if (r.role !== 'user' && r.role !== 'assistant') continue;
        let parts: any[] = [];
        try {
          parts = JSON.parse(r.parts || '[]');
        } catch {
          dropped.add('unparsable-parts');
          continue;
        }
        for (const p of parts) {
          if (p?.type === 'text' && typeof p.text === 'string') {
            const t = stripSynthetic(p.text);
            if (t) raw.push({ role: r.role, text: t });
          } else if (p?.type === 'error') {
            dropped.add('errors');
          } else {
            dropped.add('tool-parts');
          }
        }
      }
      return { turns: mergeTurns(raw), dropped: [...dropped] };
    } finally {
      db.close();
    }
  },
};

export const qoderAdapter: TrackBAdapter = {
  agent: AGENT,
  adapter: ADAPTER,

  async discoverSessions(opts = {}): Promise<SessionRef[]> {
    const { limit = 20 } = opts;
    const [cn, work] = await Promise.all([
      qoderCn.discoverSessions({ ...opts, limit }).catch(() => [] as SessionRef[]),
      qoderWork.discoverSessions({ ...opts, limit }).catch(() => [] as SessionRef[]),
    ]);
    return [...cn, ...work].sort((a, b) => b.mtimeMs - a.mtimeMs).slice(0, limit);
  },

  readTranscript(ref: SessionRef): Promise<TranscriptResult> {
    return ref.filePath.endsWith('.jsonl') ? qoderCn.readTranscript(ref) : qoderWork.readTranscript(ref);
  },
};
