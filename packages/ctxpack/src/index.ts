export * from './types.js';
export { buildPack, DEFAULT_MAX_CHARS, type BuildInput } from './build.js';
export { render, registerTemplate, listTemplates, type Template } from './render.js';
export { validatePack, type ValidationResult } from './validate.js';
export { redactPaths } from './redact.js';
export { assemblePrompt, type AssembleInput, type Assembled } from './prompt.js';
export type { SessionRef, TrackBAdapter, TranscriptResult } from './adapters/types.js';
export { claudeCodeAdapter } from './adapters/claude-code.js';
export { codexAdapter } from './adapters/codex.js';
export { workbuddyAdapter } from './adapters/workbuddy.js';
export { qoderAdapter } from './adapters/qoder.js';
export { stripSynthetic } from './adapters/util.js';

import * as path from 'node:path';
import type { SessionRef } from './adapters/types.js';

/**
 * Session attribution: prefer exact cwd match, then sessions under cwd,
 * finally most-recent mtime. Returns the basis string the panel must show.
 */
export function pickSession(
  refs: SessionRef[],
  cwd?: string,
): { ref: SessionRef; basis: string } | undefined {
  if (refs.length === 0) return undefined;
  if (cwd) {
    const exact = refs.find((r) => r.projectPath === cwd);
    if (exact) return { ref: exact, basis: `cwd 精确匹配 ${cwd}` };
    const under = refs.find((r) => r.projectPath?.startsWith(cwd + path.sep));
    if (under) return { ref: under, basis: `cwd 前缀匹配 ${under.projectPath}` };
  }
  return {
    ref: refs[0]!,
    basis: cwd
      ? `未匹配 cwd（${cwd}），按最近修改时间猜测`
      : '未提供项目路径，按最近修改时间猜测',
  };
}
