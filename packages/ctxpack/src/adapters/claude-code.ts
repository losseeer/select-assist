import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { TranscriptTurn } from '../types.js';
import type { SessionRef, TrackBAdapter, TranscriptResult } from './types.js';
import { parseJsonLines, stripSynthetic, mergeTurns } from './util.js';

const AGENT = 'claude-code';
const ADAPTER = 'claude-code-jsonl@0';
const HEAD_BYTES = 64 * 1024;
/** auto-injected conversation-continuation summary — agent plumbing, not user speech */
const COMPACT_SUMMARY = /^This session is being continued from a previous conversation/;

function homeDir(home?: string): string {
  return home ?? process.env.HOME ?? '';
}

async function headRecords(file: string): Promise<Record<string, any>[]> {
  const fh = await fs.open(file, 'r');
  try {
    const buf = Buffer.alloc(HEAD_BYTES);
    const { bytesRead } = await fh.read(buf, 0, HEAD_BYTES, 0);
    return parseJsonLines(buf.subarray(0, bytesRead).toString('utf8')).records;
  } finally {
    await fh.close();
  }
}

function cwdOf(records: Record<string, any>[]): string | undefined {
  for (const r of records) {
    if (typeof r.cwd === 'string') return r.cwd;
  }
  return undefined;
}

function firstUserPreview(records: Record<string, any>[]): string | undefined {
  for (const r of records) {
    if (r.type !== 'user' || r.isSidechain === true) continue;
    const content = r.message?.content;
    const blocks: any[] =
      typeof content === 'string' ? [{ type: 'text', text: content }] : Array.isArray(content) ? content : [];
    for (const b of blocks) {
      if (b.type !== 'text') continue;
      const t = stripSynthetic(String(b.text ?? ''));
      if (t && !COMPACT_SUMMARY.test(t)) return t.slice(0, 160);
    }
  }
  return undefined;
}

async function listSessionFiles(projectsDir: string): Promise<string[]> {
  const out: string[] = [];
  let dirs: string[] = [];
  try {
    dirs = await fs.readdir(projectsDir);
  } catch {
    return out;
  }
  for (const d of dirs) {
    const dir = path.join(projectsDir, d);
    let entries: string[];
    try {
      entries = await fs.readdir(dir);
    } catch {
      continue;
    }
    for (const e of entries) {
      if (e.endsWith('.jsonl')) out.push(path.join(dir, e));
    }
  }
  return out;
}

export function makeJsonlAdapter(
  agent: string,
  adapterId: string,
  projectsDirOf: (home?: string) => string
): TrackBAdapter {
  return {
    agent,
    adapter: adapterId,

    async discoverSessions(opts = {}): Promise<SessionRef[]> {
      const { cwd, limit = 20, home } = opts;
      const projectsDir = projectsDirOf(home);
      const refs: SessionRef[] = [];
      for (const file of await listSessionFiles(projectsDir)) {
        let stat;
        try {
          stat = await fs.stat(file);
        } catch {
          continue;
        }
        if (!stat.isFile()) continue;
        let projectPath: string | undefined;
        let preview: string | undefined;
        try {
          const records = await headRecords(file);
          projectPath = cwdOf(records);
          preview = firstUserPreview(records);
        } catch {
          /* discovery must not fail on one unreadable file */
        }
        const base = path.basename(file, '.jsonl');
        refs.push({
          agent,
          adapter: adapterId,
          filePath: file,
          sessionId: /^[0-9a-f-]{36}$/.test(base) ? base : undefined,
          projectPath,
          preview,
          mtimeMs: stat.mtimeMs,
        });
      }
      refs.sort((a, b) => b.mtimeMs - a.mtimeMs);
      const matched = cwd
        ? refs.filter((r) => r.projectPath === cwd || r.projectPath?.startsWith(cwd + path.sep))
        : refs;
      return (matched.length > 0 ? matched : refs).slice(0, limit);
    },

    async readTranscript(ref: SessionRef): Promise<TranscriptResult> {
      let text: string;
      try {
        text = await fs.readFile(ref.filePath, 'utf8');
      } catch (e) {
        return { turns: [], dropped: [], error: `无法读取会话文件: ${(e as Error).message}` };
      }
      const { records, bad } = parseJsonLines(text);
      if (records.length === 0) {
        return { turns: [], dropped: [], error: '会话文件为空或全部无法解析' };
      }
      const dropped = new Set<string>();
      if (bad > 0) dropped.add(`unparsable-lines:${bad}`);

      const raw: { role: 'user' | 'assistant'; text: string }[] = [];
      for (const r of records) {
        // sub-agent sidechains are not this conversation's surface text
        if (r.isSidechain === true) {
          dropped.add('sidechain-turns');
          continue;
        }
        const content = r.message?.content;
        const blocks: any[] =
          typeof content === 'string' ? [{ type: 'text', text: content }] : Array.isArray(content) ? content : [];
        if (r.type === 'user') {
          for (const b of blocks) {
            if (b.type === 'text') {
              const t = stripSynthetic(String(b.text ?? ''));
              if (!t) continue;
              if (COMPACT_SUMMARY.test(t)) {
                dropped.add('compact-summary');
                continue;
              }
              raw.push({ role: 'user', text: t });
            } else if (b.type === 'tool_result') {
              dropped.add('tool-results');
            }
          }
        } else if (r.type === 'assistant') {
          for (const b of blocks) {
            if (b.type === 'text' && typeof b.text === 'string' && b.text.trim()) {
              raw.push({ role: 'assistant', text: b.text.trim() });
            } else if (b.type === 'thinking' || b.type === 'redacted_thinking') {
              dropped.add('assistant-thinking');
            } else if (b.type === 'tool_use') {
              dropped.add('tool-calls');
            }
          }
        }
      }
      const turns: TranscriptTurn[] = mergeTurns(raw);
      return { turns, dropped: [...dropped] };
    },
  };
}

export const claudeCodeAdapter: TrackBAdapter = makeJsonlAdapter(
  AGENT,
  ADAPTER,
  (home) => path.join(homeDir(home), '.claude', 'projects')
);
