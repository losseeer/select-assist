import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { TranscriptTurn } from '../types.js';
import type { SessionRef, TrackBAdapter, TranscriptResult } from './types.js';
import { parseJsonLines, stripSynthetic, mergeTurns } from './util.js';

const AGENT = 'codex';
const ADAPTER = 'codex-jsonl@0';
const HEAD_BYTES = 64 * 1024;

function homeDir(home?: string): string {
  return home ?? process.env.HOME ?? '';
}

async function walkDateDirs(root: string): Promise<string[]> {
  const files: string[] = [];
  async function walk(dir: string, depth: number): Promise<void> {
    if (depth > 4) return;
    let entries: import('node:fs').Dirent[];
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) await walk(p, depth + 1);
      else if (e.isFile() && e.name.startsWith('rollout-') && e.name.endsWith('.jsonl')) files.push(p);
    }
  }
  await walk(root, 0);
  return files;
}

async function metaOf(file: string): Promise<{ cwd?: string; sessionId?: string; preview?: string }> {
  const fh = await fs.open(file, 'r');
  try {
    const buf = Buffer.alloc(HEAD_BYTES);
    const { bytesRead } = await fh.read(buf, 0, HEAD_BYTES, 0);
    let cwd: string | undefined;
    let sessionId: string | undefined;
    let preview: string | undefined;
    for (const r of parseJsonLines(buf.subarray(0, bytesRead).toString('utf8')).records) {
      if (r.type === 'session_meta') {
        cwd = typeof r.payload?.cwd === 'string' ? r.payload.cwd : undefined;
        sessionId = typeof r.payload?.session_id === 'string' ? r.payload.session_id : undefined;
      } else if (!preview && r.type === 'response_item' && r.payload?.type === 'message' && r.payload.role === 'user') {
        for (const b of r.payload.content ?? []) {
          if (b.type !== 'input_text') continue;
          const t = stripSynthetic(String(b.text ?? ''));
          if (t) {
            preview = t.slice(0, 160);
            break;
          }
        }
      }
      if (cwd && sessionId && preview) break;
    }
    return { cwd, sessionId, preview };
  } finally {
    await fh.close();
  }
}

export const codexAdapter: TrackBAdapter = {
  agent: AGENT,
  adapter: ADAPTER,

  async discoverSessions(opts = {}): Promise<SessionRef[]> {
    const { cwd, limit = 20, home } = opts;
    const codexHome = home
      ? path.join(home, '.codex')
      : process.env.CODEX_HOME ?? path.join(homeDir(), '.codex');
    const sessionsRoot = path.join(codexHome, 'sessions');
    const refs: SessionRef[] = [];
    for (const file of await walkDateDirs(sessionsRoot)) {
      let stat;
      try {
        stat = await fs.stat(file);
      } catch {
        continue;
      }
      let meta: { cwd?: string; sessionId?: string; preview?: string } = {};
      try {
        meta = await metaOf(file);
      } catch {
        /* one unreadable file must not fail discovery */
      }
      refs.push({
        agent: AGENT,
        adapter: ADAPTER,
        filePath: file,
        sessionId: meta.sessionId,
        projectPath: meta.cwd,
        preview: meta.preview,
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
      if (r.type !== 'response_item' || !r.payload) continue;
      const p = r.payload;
      if (p.type === 'message') {
        // developer instructions are system-prompt material: never enter a pack
        if (p.role !== 'user' && p.role !== 'assistant') continue;
        const items: any[] = Array.isArray(p.content) ? p.content : [];
        for (const b of items) {
          if (b.type !== 'input_text' && b.type !== 'output_text' && b.type !== 'text') continue;
          const t = stripSynthetic(String(b.text ?? ''));
          if (t) raw.push({ role: p.role, text: t });
        }
      } else if (p.type === 'reasoning') {
        dropped.add('reasoning');
      } else if (
        p.type === 'function_call' || p.type === 'function_call_output' ||
        p.type === 'custom_tool_call' || p.type === 'custom_tool_call_output' ||
        p.type === 'web_search_call'
      ) {
        dropped.add('tool-results');
      }
    }
    const turns: TranscriptTurn[] = mergeTurns(raw);
    return { turns, dropped: [...dropped] };
  },
};
