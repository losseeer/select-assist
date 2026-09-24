import type { CtxPack } from './types.js';

export interface RenderOptions {
  dropped?: string[];
}

export type Template = (pack: CtxPack, dropped: string[]) => string;

function header(pack: CtxPack): string {
  const s = pack.source ?? {};
  const parts = [
    s.agent ?? 'unknown agent',
    s.app ?? '?',
  ];
  if (s.sessionId) parts.push(`session ${s.sessionId}`);
  return `【上下文包】选中内容来自 ${parts.join(' · ')}，抓取于 ${pack.capture.at}（${pack.capture.via === 'clipboard' ? '剪贴板' : '页面选区'}）`;
}

function contextBlock(pack: CtxPack, heading: (t: string) => string): string {
  const turns = pack.transcript ?? [];
  if (turns.length === 0) return '';
  const lines = turns.map((t) => `${t.role === 'user' ? '用户' : '助手'}> ${t.text}`);
  return `\n\n${heading(`会话摘录（最近 ${turns.length} 条）`)}\n\n${lines.join('\n\n')}`;
}

function droppedBlock(dropped: string[], heading: (t: string) => string): string {
  if (dropped.length === 0) return '';
  return `\n\n${heading('为控制体积，本包已省略')}\n\n${dropped.map((d) => `- ${d}`).join('\n')}`;
}

const plainV1: Template = (pack, dropped) => {
  const h = (t: string) => `—— ${t} ——`;
  return (
    `${header(pack)}\n\n${h('选区原文')}\n\n${pack.selection?.text ?? ''}` +
    contextBlock(pack, h) +
    droppedBlock(dropped, h) +
    `\n\n${h('选区原文结束，以下是我的问题')}`
  );
};

const markdownV1: Template = (pack, dropped) => {
  const h = (t: string) => `### ${t}`;
  return (
    `${header(pack)}\n\n${h('选区原文')}\n\n> ${(pack.selection?.text ?? '').replace(/\n/g, '\n> ')}` +
    contextBlock(pack, (t) => `\n${h(t)}`).replace(/\n用户> /g, '\n**用户**: ').replace(/\n助手> /g, '\n**助手**: ') +
    droppedBlock(dropped, h) +
    `\n\n${h('以上为参考上下文，我的问题是：')}`
  );
};

/** Pure record: transcript lines only (selection is quoted by the prompt itself). */
const cleanV1: Template = (pack) => {
  const turns = pack.transcript ?? [];
  if (turns.length === 0) return pack.selection?.text ?? '';
  return turns.map((t) => `${t.role === 'user' ? '用户' : '助手'}> ${t.text}`).join('\n\n');
};

const templates: Record<string, Template> = {
  'plain/v1': plainV1,
  'markdown/v1': markdownV1,
  'clean/v1': cleanV1,
};

export function registerTemplate(id: string, fn: Template): void {
  templates[id] = fn;
}

export function render(pack: CtxPack, templateId = 'plain/v1', dropped: string[] = []): string {
  const fn = templates[templateId];
  if (!fn) throw new Error(`unknown ctxpack template: ${templateId}`);
  return fn(pack, dropped);
}

export function listTemplates(): string[] {
  return Object.keys(templates);
}
