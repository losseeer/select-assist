import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildPack, validatePack, render, redactPaths } from '../dist/index.js';

const capture = { via: 'clipboard', at: '2026-09-24T10:12:01Z' };

test('minimal pack: selection + capture only, no transcript', () => {
  const pack = buildPack({
    selection: { text: 'waterfall 监听器必须调用 next()' },
    capture,
  });
  assert.equal(pack.pack, 'ctxpack/0');
  assert.deepEqual(pack.transcript, []);
  assert.ok(pack.payload.includes('waterfall 监听器必须调用 next()'));
  assert.ok(!pack.payload.includes('会话摘录'));
  assert.ok(validatePack(pack).ok);
});

test('pack with transcript validates and renders context block', () => {
  const pack = buildPack({
    selection: { text: 'sel' },
    capture,
    source: { agent: 'deepseek-harness', app: 'web', sessionId: 's-8f2', adapter: 'session-export-zip@0' },
    transcript: [
      { role: 'user', text: '第一条', seq: 12 },
      { role: 'assistant', text: '第二条', seq: 13 },
    ],
  });
  assert.ok(validatePack(pack).ok);
  assert.ok(pack.payload.includes('会话摘录'));
  assert.ok(pack.payload.includes('用户> 第一条'));
  assert.equal(pack.limits.usedChars, pack.payload.length);
  assert.ok(pack.limits.usedChars <= pack.limits.maxChars);
});

test('overflow drops oldest turns first, recorded in limits.dropped (never silent)', () => {
  const turns = Array.from({ length: 40 }, (_, i) => ({
    role: i % 2 === 0 ? 'user' : 'assistant',
    text: `turn-${i} ${'x'.repeat(100)}`,
    seq: i,
  }));
  const pack = buildPack({ selection: { text: 'S'.repeat(200) }, capture, transcript: turns, maxChars: 1200 });
  assert.ok(pack.payload.length <= 1200);
  assert.ok(pack.limits.dropped.some((d) => /^turns:0-\d+$/.test(d)));
  const last = turns[turns.length - 1].text.slice(0, 12);
  assert.ok(pack.payload.includes(last), 'newest turn must survive');
  // dropped ranges must be visible in the payload itself
  assert.ok(pack.payload.includes('为控制体积，本包已省略'));
  assert.ok(validatePack(pack).ok);
});

test('oversized selection is truncated explicitly, marked in dropped', () => {
  const pack = buildPack({ selection: { text: 'y'.repeat(5000) }, capture, maxChars: 500 });
  assert.ok(pack.payload.length <= 500);
  assert.ok(pack.limits.dropped.includes('selection:truncated'));
});

test('adapter default drops (tool-results etc.) flow into payload', () => {
  const pack = buildPack({
    selection: { text: 'a' },
    capture,
    defaultDropped: ['tool-results', 'assistant-thinking'],
  });
  assert.ok(pack.limits.dropped.includes('tool-results'));
  assert.ok(pack.payload.includes('tool-results'));
});

test('validate rejects bad packs', () => {
  assert.equal(validatePack(null).ok, false);
  assert.equal(validatePack({}).ok, false);
  assert.equal(validatePack({ pack: 'ctxpack/0', generatedAt: 'x', capture: { via: 'clipboard', at: 'x' }, selection: { text: 'ok' } }).ok, true);
  const bad = validatePack({ pack: 'ctxpack/0', generatedAt: 'x', capture: { via: 'telepathy', at: 'x' }, selection: { text: 'ok' }, transcript: [{ role: 'tool', text: 'x' }] });
  assert.equal(bad.ok, false);
  assert.ok(bad.errors.some((e) => e.includes('capture.via')));
  assert.ok(bad.errors.some((e) => e.includes('transcript[0].role')));
});

test('unknown fields are tolerated (forward minor versions)', () => {
  const pack = {
    pack: 'ctxpack/1', generatedAt: 'x', capture: { via: 'clipboard', at: 'x' },
    selection: { text: 'ok' }, someFutureField: 42,
  };
  assert.ok(validatePack(pack).ok);
});

test('render: markdown template selectable', () => {
  const pack = buildPack({ selection: { text: 'a\nb' }, capture, templateId: 'markdown/v1' });
  assert.ok(pack.payload.startsWith('【上下文包】'));
  assert.ok(pack.payload.includes('> a\n> b'));
});

test('render: clean/v1 is a pure record without scaffolding', () => {
  const pack = buildPack({
    selection: { text: '选中的词' },
    capture,
    templateId: 'clean/v1',
    transcript: [
      { role: 'user', text: '这个怎么理解？' },
      { role: 'assistant', text: '分两层看。' },
    ],
  });
  assert.equal(pack.payload, '用户> 这个怎么理解？\n\n助手> 分两层看。');
  assert.ok(!pack.payload.includes('上下文包'));
  assert.ok(!pack.payload.includes('选区原文'));
  // selection-only fallback keeps the text itself
  const solo = buildPack({ selection: { text: '只有选区' }, capture, templateId: 'clean/v1' });
  assert.equal(solo.payload, '只有选区');
});

test('redactPaths hides home dir in payload and source', () => {
  const pack = buildPack({
    selection: { text: `see ${process.env.HOME}/secret/file.ts` },
    capture,
    source: { projectPath: `${process.env.HOME}/secret` },
  });
  const r = redactPaths(pack);
  assert.ok(!r.payload.includes(process.env.HOME));
  assert.equal(r.source.projectPath, '~/secret');
});
