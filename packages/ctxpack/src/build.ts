import { PACK_FORMAT } from './types.js';
import type {
  Capture,
  CtxPack,
  Limits,
  Selection,
  Source,
  TranscriptTurn,
} from './types.js';
import { render } from './render.js';

export const DEFAULT_MAX_CHARS = 8000;

export interface BuildInput {
  selection: Selection;
  capture: Capture;
  source?: Source;
  transcript?: TranscriptTurn[];
  maxChars?: number;
  templateId?: string;
  /** defaults recorded by adapters, e.g. "tool-results", "assistant-thinking" */
  defaultDropped?: string[];
  generatedAt?: string;
}

function uniq(records: string[]): string[] {
  return [...new Set(records)];
}

function rangeLabel(from: number, to: number): string {
  return from === to ? `turns:${from}` : `turns:${from}-${to}`;
}

/**
 * Never truncate silently: every omission is recorded in limits.dropped
 * and rendered visibly in the payload.
 */
export function buildPack(input: BuildInput): CtxPack {
  const {
    selection,
    capture,
    source,
    transcript = [],
    maxChars = DEFAULT_MAX_CHARS,
    templateId = 'plain/v1',
    defaultDropped = [],
    generatedAt = new Date().toISOString(),
  } = input;

  if (!selection.text) throw new Error('ctxpack: selection.text is required');
  if (!capture?.via || !capture?.at) throw new Error('ctxpack: capture.via/at are required');

  const numbered: TranscriptTurn[] = transcript.map((t, i) => ({ ...t, seq: t.seq ?? i }));

  // Try dropping turns from the oldest (index 0..dropFrom-1) until payload fits.
  for (let dropFrom = 0; dropFrom <= numbered.length; dropFrom++) {
    const kept = numbered.slice(dropFrom);
    const dropped = defaultDropped.slice();
    if (dropFrom > 0) dropped.push(rangeLabel(0, dropFrom - 1));
    const draft: CtxPack = {
      pack: PACK_FORMAT,
      generatedAt,
      capture,
      ...(source ? { source } : {}),
      selection,
      transcript: kept,
      limits: { maxChars, usedChars: 0, dropped } satisfies Limits,
    };
    const payload = render(draft, templateId, dropped);
    if (payload.length <= maxChars) {
      draft.limits = { maxChars, usedChars: payload.length, dropped };
      draft.payload = payload;
      return draft;
    }
  }

  // Selection alone exceeds the budget: explicit, visible truncation.
  const dropped = uniq([...defaultDropped, rangeLabel(0, numbered.length - 1), 'selection:truncated']);
  const empty: CtxPack = {
    pack: PACK_FORMAT,
    generatedAt,
    capture,
    ...(source ? { source } : {}),
    selection,
    transcript: [],
    limits: { maxChars, usedChars: 0, dropped },
  };
  const overhead = render({ ...empty, selection: { ...empty.selection, text: '' } }, templateId, dropped).length;
  const room = Math.max(0, maxChars - overhead - 1); // 1 for the ellipsis marker
  const text = room > 1 ? selection.text.slice(0, room - 1) + '…' : selection.text.slice(0, room);
  empty.selection = { ...empty.selection, text };
  empty.payload = render(empty, templateId, dropped);
  empty.limits = { maxChars, usedChars: empty.payload.length, dropped };
  return empty;
}
