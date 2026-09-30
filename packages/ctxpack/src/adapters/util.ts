import type { SessionRef } from './types.js';

export function parseJsonLines(text: string): { records: Record<string, any>[]; bad: number } {
  const records: Record<string, any>[] = [];
  let bad = 0;
  for (const line of text.split('\n')) {
    const t = line.trim();
    if (!t) continue;
    try {
      records.push(JSON.parse(t));
    } catch {
      bad++;
    }
  }
  return { records, bad };
}

/**
 * Harnesses inject synthetic blocks into user messages (IDE context, reminders,
 * slash-command wrappers). They are not what the user typed.
 * DROP removes the whole block; UNWRAP removes the tags but keeps the payload
 * (e.g. <user_query>…</user_query> wraps the real question).
 */
const DROP_TAGS = [
  'system-reminder',
  'ide_opened_file',
  'ide_selection',
  'environment_context',
  'command-name',
  'command-message',
  'command-args',
  'local-command-stdout',
  'local-command-caveat',
  'user-prompt-submit-hook',
  'skills_instructions',
  'collaboration_mode',
  'permissions',
];

const UNWRAP_TAGS = ['user_query', 'user_message'];

export function stripSynthetic(text: string): string {
  let out = text;
  for (const tag of DROP_TAGS) {
    const esc = tag.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    out = out.replace(new RegExp(`<${esc}[^>]*>[\\s\\S]*?</${esc}[^>]*>`, 'g'), '');
    out = out.replace(new RegExp(`<${esc}[^>]*/>`, 'g'), '');
  }
  for (const tag of UNWRAP_TAGS) {
    const esc = tag.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    out = out.replace(new RegExp(`<${esc}[^>]*>([\\s\\S]*?)</${esc}[^>]*>`, 'g'), '$1');
  }
  return out.trim();
}

export function mergeTurns(
  raw: { role: 'user' | 'assistant'; text: string }[],
): { role: 'user' | 'assistant'; text: string; seq: number }[] {
  const merged: { role: 'user' | 'assistant'; text: string; seq: number }[] = [];
  for (const t of raw) {
    const last = merged[merged.length - 1];
    if (last && last.role === t.role) last.text += '\n' + t.text;
    else merged.push({ role: t.role, text: t.text, seq: merged.length });
  }
  return merged;
}

export type CwdMatch = 'exact' | 'under';

/**
 * Does a session's recorded project path belong to `cwd`?
 *
 * Windows logs the same directory two ways: agents write a lowercase drive
 * (`d:\prj`) while users type `D:\prj`, and either separator shows up. The
 * drive letter — not process.platform — decides whether to fold case, so POSIX
 * paths stay case-sensitive and one rule holds on every OS.
 */
export function matchCwd(projectPath: string | undefined, cwd?: string): CwdMatch | undefined {
  if (!cwd || !projectPath) return undefined;
  const canon = (p: string): string => {
    let s = p.replace(/\\/g, '/');
    if (s.length > 1) s = s.replace(/\/+$/, '');
    return /^[a-z]:\//i.test(s) ? s.toLowerCase() : s;
  };
  const [p, c] = [canon(projectPath), canon(cwd)];
  if (p === c) return 'exact';
  return p.startsWith(c + '/') ? 'under' : undefined;
}

/**
 * Newest-first refs narrowed to `cwd`. Discovery stays generous: a session file
 * that predates a moved or renamed project must still surface rather than return
 * nothing, so a miss falls back to the full list.
 */
export function byCwd(refs: SessionRef[], cwd: string | undefined, limit: number): SessionRef[] {
  const matched = cwd ? refs.filter((r) => matchCwd(r.projectPath, cwd)) : refs;
  return (matched.length > 0 ? matched : refs).slice(0, limit);
}
