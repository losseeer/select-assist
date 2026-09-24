import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { TranscriptTurn } from '../types.js';
import type { SessionRef, TrackBAdapter, TranscriptResult } from './types.js';
import { parseJsonLines, stripSynthetic, mergeTurns } from './util.js';

const AGENT = 'workbuddy';
const ADAPTER = 'workbuddy-jsonl@0';
const HEAD_BYTES = 64 * 1024;

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

function textsOf(rec: Record<string, any>): string[] {
  const content = rec.content;
  const blocks: any[] =
    typeof content === 'string' ? [{ type: 'text', text: content }] : Array.isArray(content) ? content : [];
  const out: string[] = [];
  for (const b of blocks) {
    if (b.type === 'input_text' || b.type === 'output_text' || b.type === 'text') {
      const t = stripSynthetic(String(b.text ?? ''));
      if (t) out.push(t);
    }
  }
  return out;
}

export const workbuddyAdapter: TrackBAdapter = {
  agent: AGENT,
  adapter: ADAPTER,

  async discoverSessions(opts = {}): Promise<SessionRef[]> {
    const { cwd, limit = 20, home } = opts;
    const projectsDir = path.join(homeDir(home), '.workbuddy', 'projects');
    const refs: SessionRef[] = [];
    let dirs: string[] = [];
    try {
      dirs = await fs.readdir(projectsDir);
    } catch {
      return refs;
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
        if (!e.endsWith('.jsonl')) continue;
        const file = path.join(dir, e);
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
          for (const r of records) {
            if (!projectPath && typeof r.cwd === 'string') projectPath = r.cwd;
            if (!preview && r.type === 'message' && r.role === 'user') {
              preview = textsOf(r)[0]?.slice(0, 160);
            }
            if (projectPath && preview) break;
          }
        } catch {
          /* discovery must not fail on one unreadable file */
        }
        const base = path.basename(file, '.jsonl');
        refs.push({
          agent: AGENT,
          adapter: ADAPTER,
          filePath: file,
          sessionId: /^[0-9a-f-]{36}$/.test(base) ? base : undefined,
          projectPath,
          preview,
          mtimeMs: stat.mtimeMs,
        });
      }
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
      if (r.type === 'message' && (r.role === 'user' || r.role === 'assistant')) {
        for (const t of textsOf(r)) raw.push({ role: r.role, text: t });
      } else if (r.type === 'reasoning') {
        dropped.add('reasoning');
      } else if (r.type === 'function_call' || r.type === 'function_call_result' || r.type === 'tool-call' || r.type === 'tool-result') {
        dropped.add('tool-results');
      }
    }
    const turns: TranscriptTurn[] = mergeTurns(raw);
    return { turns, dropped: [...dropped] };
  },
};
