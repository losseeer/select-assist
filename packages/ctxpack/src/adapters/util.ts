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
