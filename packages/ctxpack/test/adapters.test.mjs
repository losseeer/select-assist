import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { claudeCodeAdapter, codexAdapter, workbuddyAdapter, qoderAdapter, pickSession } from '../dist/index.js';
import { makeFixtureHome } from './fixtures.mjs';

let home;
let cwd;

before(() => {
  home = makeFixtureHome();
  cwd = path.join(home, 'Dev', 'prj');
});
after(() => fs.rmSync(home, { recursive: true, force: true }));

test('claude-code: discovery matches cwd and reports basis', async () => {
  const refs = await claudeCodeAdapter.discoverSessions({ cwd, home });
  assert.equal(refs.length, 1);
  assert.match(refs[0].sessionId, /^[0-9a-f-]{36}$/);
  assert.equal(refs[0].projectPath, cwd);
  const pick = pickSession(refs, cwd);
  assert.match(pick.basis, /精确匹配/);
});

test('claude-code: transcript keeps user/assistant text only', async () => {
  const refs = await claudeCodeAdapter.discoverSessions({ cwd, home });
  const { turns, dropped, error } = await claudeCodeAdapter.readTranscript(refs[0]);
  assert.equal(error, undefined);
  const all = JSON.stringify(turns);
  assert.ok(!all.includes('SUBAGENT'), 'sidechain excluded');
  assert.ok(!all.includes('secret internal'), 'thinking excluded');
  assert.ok(!all.includes('huge file'), 'tool results excluded');
  assert.ok(!all.includes('SUMMARY MUST NOT APPEAR'), 'compact-summary excluded');
  assert.deepEqual(turns.map((t) => t.role), ['user', 'assistant']);
  assert.ok(turns[0].text.includes('redis预扣库存'), 'synthetic ide tag stripped, real text kept');
  assert.ok(turns[1].text.includes('可以用超时机制回滚'));
  assert.ok(turns.at(-1).text.includes('waterfall'));
  for (const d of ['tool-results', 'assistant-thinking', 'tool-calls', 'sidechain-turns', 'compact-summary']) {
    assert.ok(dropped.includes(d), `dropped should include ${d}`);
  }
});

test('codex: discovery reads session_meta cwd', async () => {
  const refs = await codexAdapter.discoverSessions({ cwd, home });
  assert.equal(refs.length, 1);
  assert.equal(refs[0].sessionId, 'cx-1');
  assert.equal(refs[0].projectPath, cwd);
});

test('codex: transcript excludes developer/reasoning/tool noise', async () => {
  const refs = await codexAdapter.discoverSessions({ cwd, home });
  const { turns, dropped, error } = await codexAdapter.readTranscript(refs[0]);
  assert.equal(error, undefined);
  const all = JSON.stringify(turns);
  assert.ok(!all.includes('SYSTEM PROMPT'), 'developer instructions excluded');
  assert.ok(!all.includes('INTERNAL REASONING'));
  assert.ok(!all.includes('MUST NOT APPEAR'));
  assert.deepEqual(turns.map((t) => [t.role, t.text]), [
    ['user', '这个skill会token用量过高吗？'],
    ['assistant', '会有，主要瓶颈在检索轮数。'],
  ]);
  assert.ok(dropped.includes('tool-results'));
  assert.ok(dropped.includes('reasoning'));
});

test('pickSession falls back to mtime guess and says so', async () => {
  const refs = await claudeCodeAdapter.discoverSessions({ home });
  const pick = pickSession(refs, '/no/such/dir');
  assert.equal(pick.ref, refs[0]);
  assert.match(pick.basis, /猜测/);
});

test('workbuddy: discovery with cwd match and preview', async () => {
  const refs = await workbuddyAdapter.discoverSessions({ cwd, home });
  assert.equal(refs.length, 1);
  assert.equal(refs[0].projectPath, cwd);
  assert.ok(refs[0].preview.includes('这个 JSON 够不够'), 'user_query tag stripped');
});

test('workbuddy: transcript keeps message text only', async () => {
  const refs = await workbuddyAdapter.discoverSessions({ cwd, home });
  const { turns, dropped, error } = await workbuddyAdapter.readTranscript(refs[0]);
  assert.equal(error, undefined);
  const all = JSON.stringify(turns);
  assert.ok(!all.includes('MUST NOT APPEAR'), 'system-reminder/tool/reasoning excluded');
  assert.deepEqual(turns.map((t) => t.role), ['user', 'assistant']);
  assert.ok(turns[0].text.includes('围绕') || turns[0].text.includes('JSON'));
  assert.ok(dropped.includes('tool-results'));
  assert.ok(dropped.includes('reasoning'));
});

test('qoder: sqlite discovery and transcript', async () => {
  const all = await qoderAdapter.discoverSessions({ cwd, home });
  const refs = all.filter((r) => r.adapter.startsWith('qoderwork'));
  assert.equal(refs.length, 1);
  assert.equal(refs[0].name, '面试准备');
  assert.equal(refs[0].projectPath, cwd);
  const { turns, dropped, error } = await qoderAdapter.readTranscript(refs[0]);
  assert.equal(error, undefined);
  const all2 = JSON.stringify(turns);
  assert.ok(!all2.includes('PRIVATE'), 'tool-Thinking excluded');
  assert.ok(!all2.includes('MUST NOT APPEAR'), 'error parts excluded');
  assert.deepEqual(turns.map((t) => [t.role, t.text]), [
    ['user', '围绕项目向我提问'],
    ['assistant', '好的，第一个问题：'],
  ]);
  assert.ok(dropped.includes('tool-parts'));
  assert.ok(dropped.includes('errors'));
});

test('qoder: CN IDE jsonl discovery and transcript', async () => {
  const all = await qoderAdapter.discoverSessions({ cwd, home });
  const refs = all.filter((r) => r.adapter.startsWith('qoder-cn'));
  assert.equal(refs.length, 1);
  assert.equal(refs[0].projectPath, cwd);
  assert.ok(refs[0].preview.includes('悬浮窗'));
  const { turns, dropped, error } = await qoderAdapter.readTranscript(refs[0]);
  assert.equal(error, undefined);
  const blob = JSON.stringify(turns);
  assert.ok(!blob.includes('MUST NOT APPEAR'), 'system/thinking excluded');
  assert.ok(!blob.includes('PRIVATE'), 'thinking excluded');
  assert.deepEqual(turns.map((t) => [t.role, t.text]), [
    ['user', '这个悬浮窗怎么不抢焦点？'],
    ['assistant', '把 chip 窗设为 focusable:false 就常驻不抢焦点。'],
  ]);
  assert.ok(dropped.includes('assistant-thinking'));
  assert.ok(dropped.includes('tool-calls'));
});

test('corrupt file degrades with error, not throw', async () => {
  const bad = path.join(home, '.claude', 'projects', '-bad', 'deadbeef.jsonl');
  fs.mkdirSync(path.dirname(bad), { recursive: true });
  fs.writeFileSync(bad, 'not json\nat all\n');
  const refs = await claudeCodeAdapter.discoverSessions({ home });
  const ref = refs.find((r) => r.filePath === bad);
  const res = await claudeCodeAdapter.readTranscript(ref);
  assert.ok(res.error, 'must report a reason');
  assert.deepEqual(res.turns, []);
});
