import * as os from 'node:os';
import type { CtxPack } from './types.js';

/**
 * Privacy aid: the pack may carry absolute paths (project path, file paths in
 * pasted text). This returns a copy with the home directory replaced by "~".
 */
export function redactPaths(pack: CtxPack, home = os.homedir()): CtxPack {
  const sub = (s: string): string =>
    s.split(home).join('~').replace(/\/Users\/[^/\s"]+/g, '~/user');
  const out: CtxPack = JSON.parse(JSON.stringify(pack));
  if (typeof out.payload === 'string') out.payload = sub(out.payload);
  if (out.source?.projectPath) out.source.projectPath = sub(out.source.projectPath);
  if (out.selection?.text) out.selection.text = sub(out.selection.text);
  out.transcript = (out.transcript ?? []).map((t) => ({ ...t, text: sub(t.text) }));
  return out;
}
