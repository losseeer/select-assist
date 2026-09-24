import type { CtxPack } from './types.js';

export interface ValidationResult {
  ok: boolean;
  errors: string[];
}

const CAPTURE_VIAS = new Set(['page-selection', 'clipboard']);
const ANCHOR_KINDS = new Set(['event', 'message', 'ambiguous', 'none']);
const ROLES = new Set(['user', 'assistant']);

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/**
 * Contract rule: selection.text and capture are required; everything else may
 * be absent and consumers must not assume its presence. Unknown fields ignored.
 */
export function validatePack(value: unknown): ValidationResult {
  const errors: string[] = [];
  if (!isObj(value)) return { ok: false, errors: ['pack must be an object'] };
  const p = value as Partial<CtxPack> & Record<string, unknown>;

  if (typeof p.pack !== 'string' || !/^ctxpack\/\d+$/.test(p.pack)) {
    errors.push('field "pack" must look like "ctxpack/<major>"');
  }
  if (typeof p.generatedAt !== 'string') errors.push('field "generatedAt" must be a string');

  if (!isObj(p.capture)) {
    errors.push('field "capture" is required');
  } else {
    const c = p.capture as Record<string, unknown>;
    if (typeof c.via !== 'string' || !CAPTURE_VIAS.has(c.via)) {
      errors.push('capture.via must be "page-selection" | "clipboard"');
    }
    if (typeof c.at !== 'string') errors.push('capture.at must be a string');
  }

  if (!isObj(p.selection) || typeof (p.selection as Record<string, unknown>).text !== 'string') {
    errors.push('field "selection.text" is required and must be a string');
  } else {
    const s = p.selection as Record<string, unknown>;
    if (s.role !== undefined && s.role !== 'user' && s.role !== 'assistant' && s.role !== 'unknown') {
      errors.push('selection.role must be user|assistant|unknown');
    }
    if (s.anchor !== undefined) {
      if (!isObj(s.anchor)) errors.push('selection.anchor must be an object');
      else if (typeof (s.anchor as Record<string, unknown>).kind !== 'string' ||
        !ANCHOR_KINDS.has(String((s.anchor as Record<string, unknown>).kind))) {
        errors.push('selection.anchor.kind must be event|message|ambiguous|none');
      }
    }
  }

  if (p.transcript !== undefined) {
    if (!Array.isArray(p.transcript)) {
      errors.push('field "transcript" must be an array');
    } else {
      p.transcript.forEach((t, i) => {
        if (!isObj(t)) errors.push(`transcript[${i}] must be an object`);
        else {
          const tt = t as Record<string, unknown>;
          if (typeof tt.text !== 'string') errors.push(`transcript[${i}].text must be a string`);
          if (typeof tt.role !== 'string' || !ROLES.has(tt.role)) {
            errors.push(`transcript[${i}].role must be user|assistant`);
          }
        }
      });
    }
  }

  if (p.limits !== undefined && !isObj(p.limits)) errors.push('field "limits" must be an object');
  if (p.source !== undefined && !isObj(p.source)) errors.push('field "source" must be an object');

  return { ok: errors.length === 0, errors };
}
