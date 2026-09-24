export interface AssembleInput {
  /** must contain {selection}; appended with a blank line when it does not */
  template: string;
  selection: string;
  /** rendered context pack ('' when no transcript attached) */
  context: string;
  maxChars: number;
}

export interface Assembled {
  prompt: string;
  /** trimming the assembled prompt produced these, e.g. "selection:truncated" */
  dropped: string[];
}

/**
 * Whole-prompt budget, never silent: when the assembled prompt exceeds
 * maxChars, oldest transcript turns are dropped first (context is '\n\n'
 * -joined turns), then the selection is cut with an explicit ellipsis.
 */
export function assemblePrompt(input: AssembleInput): Assembled {
  const { selection, maxChars } = input;
  const dropped: string[] = [];
  const tpl = input.template.includes('{selection}') ? input.template : `${input.template}\n\n{selection}`;
  const fixed = tpl.split('{selection}').join('').length;
  const fill = (sel: string, ctx: string): string => {
    const instruction = tpl.split('{selection}').join(sel);
    return ctx ? `${instruction}\n\n${ctx}` : instruction;
  };

  let ctx = input.context;
  let sel = selection;
  if (fill(sel, ctx).length > maxChars) {
    const turns = ctx.split('\n\n').filter(Boolean);
    while (turns.length > 1 && fill(sel, turns.join('\n\n')).length > maxChars) {
      turns.shift();
      if (!dropped.includes('context:trimmed-oldest')) dropped.push('context:trimmed-oldest');
    }
    ctx = turns.join('\n\n');
    const over = fill(sel, ctx).length - maxChars;
    if (over > 0) {
      const keep = Math.max(0, sel.length - over - 1); // 1 for the ellipsis
      sel = keep > 0 ? `${sel.slice(0, keep)}…` : '…';
      dropped.push('selection:truncated');
    }
  }
  let prompt = fill(sel, ctx);
  if (prompt.length > maxChars) {
    // degenerate budget (single huge turn / tiny maxChars): last-resort hard cut
    prompt = `${prompt.slice(0, Math.max(0, maxChars - 1))}…`;
    dropped.push('final:hard-trim');
  }
  return { prompt, dropped };
}
