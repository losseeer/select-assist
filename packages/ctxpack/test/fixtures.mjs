// Shared fixtures mirroring the real on-disk formats sampled from
// ~/.claude/projects and ~/.codex/sessions (2026-09).
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

export function makeFixtureHome() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'ctxpack-test-'));
  const cwd = path.join(home, 'Dev', 'prj');

  // ---- Claude Code ----
  const sessionUuid = '11111111-2222-3333-4444-555555555555';
  const projDir = path.join(home, '.claude', 'projects', '-Users-x-Dev-prj');
  fs.mkdirSync(projDir, { recursive: true });
  const cc = [
    { type: 'queue-operation', operation: 'enqueue', timestamp: '2026-09-22T09:07:59.904Z', sessionId: 's-1' },
    {
      type: 'user', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'user', content: [
        { type: 'text', text: '<ide_opened_file>The user opened the file X.java.</ide_opened_file>' },
        { type: 'text', text: 'redis预扣库存后消息丢了怎么办？' },
      ] },
    },
    {
      type: 'assistant', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'assistant', content: [{ type: 'thinking', thinking: 'secret internal chain' }] },
    },
    {
      type: 'assistant', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'assistant', content: [{ type: 'text', text: '可以用超时机制回滚。' }] },
    },
    {
      type: 'assistant', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'assistant', content: [{ type: 'tool_use', name: 'Read', id: 't1' }] },
    },
    {
      type: 'user', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 't1', content: 'huge file contents' }] },
    },
    {
      type: 'assistant', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'assistant', content: [{ type: 'text', text: 'waterfall 监听器必须调用 next()' }] },
    },
    {
      type: 'assistant', sessionId: 's-1', cwd, isSidechain: true,
      message: { role: 'assistant', content: [{ type: 'text', text: 'SUBAGENT TEXT MUST NOT APPEAR' }] },
    },
    {
      type: 'user', sessionId: 's-1', cwd, isSidechain: false,
      message: { role: 'user', content: [{ type: 'text', text: 'This session is being continued from a previous conversation. SUMMARY MUST NOT APPEAR' }] },
    },
  ];
  fs.writeFileSync(path.join(projDir, sessionUuid + '.jsonl'), cc.map((r) => JSON.stringify(r)).join('\n') + '\n');

  // ---- Codex ----
  const cxDir = path.join(home, '.codex', 'sessions', '2026', '09', '11');
  fs.mkdirSync(cxDir, { recursive: true });
  const cx = [
    { type: 'session_meta', ordinal: 0, payload: { session_id: 'cx-1', cwd, originator: 'codex-tui' } },
    { type: 'response_item', payload: { type: 'message', role: 'developer', content: [{ type: 'input_text', text: '<permissions instructions>SYSTEM PROMPT MUST NOT APPEAR</permissions instructions>' }] } },
    { type: 'response_item', payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text: '<environment_context><cwd>' + cwd + '</cwd></environment_context>' }] } },
    { type: 'response_item', payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text: '这个skill会token用量过高吗？' }] } },
    { type: 'response_item', payload: { type: 'reasoning', summary: [{ type: 'summary_text', text: 'INTERNAL REASONING MUST NOT APPEAR' }] } },
    { type: 'response_item', payload: { type: 'custom_tool_call', name: 'exec', input: 'ls' } },
    { type: 'response_item', payload: { type: 'custom_tool_call_output', output: 'MUST NOT APPEAR' } },
    { type: 'response_item', payload: { type: 'message', role: 'assistant', content: [{ type: 'output_text', text: '会有，主要瓶颈在检索轮数。' }] } },
  ];
  fs.writeFileSync(
    path.join(cxDir, 'rollout-2026-09-11T10-15-33-cx-1.jsonl'),
    cx.map((r) => JSON.stringify(r)).join('\n') + '\n',
  );

  // ---- WorkBuddy ----
  const wbDir = path.join(home, '.workbuddy', 'projects', 'Users-x-Dev-prj');
  fs.mkdirSync(wbDir, { recursive: true });
  const wb = [
    { id: 'm1', timestamp: 1789649854432, type: 'message', role: 'user', cwd,
      content: [{ type: 'input_text', text: '<system-reminder data-role="user-context">ENV MUST NOT APPEAR</system-reminder><user_query>这个 JSON 够不够？</user_query>' }] },
    { id: 'r1', type: 'reasoning', content: [{ type: 'reasoning', text: 'PRIVATE MUST NOT APPEAR' }] },
    { id: 'fc1', type: 'function_call', name: 'read_file' },
    { id: 'fr1', type: 'function_call_result', output: 'MUST NOT APPEAR' },
    { id: 'm2', type: 'message', role: 'assistant', status: 'completed', cwd,
      content: [{ type: 'output_text', text: '我先读一遍实际代码，再判断。' }] },
  ];
  fs.writeFileSync(path.join(wbDir, 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl'), wb.map((r) => JSON.stringify(r)).join('\n') + '\n');

  // ---- Qoder (QoderWork agents.db, sqlite) ----
  const qDir = path.join(home, 'Library', 'Application Support', 'QoderWork', 'data');
  fs.mkdirSync(qDir, { recursive: true });
  const db = new DatabaseSync(path.join(qDir, 'agents.db'));
  db.exec(`
    create table projects (id text primary key, name text, path text, created_at integer, updated_at integer);
    create table chats (id text primary key, name text, project_id text, created_at integer, updated_at integer, deleted_at integer);
    create table messages (id text primary key, message_id text, chat_id text, sub_chat_id text, sequence integer, role text, parts text, created_at integer);
  `);
  const nowSec = Math.floor(Date.now() / 1000);
  db.prepare('insert into projects values (?,?,?,?,?)').run('p1', 'prj', cwd, nowSec, nowSec);
  db.prepare('insert into chats values (?,?,?,?,?,?)').run('chat1', '面试准备', 'p1', nowSec - 100, nowSec, null);
  const ins = db.prepare('insert into messages values (?,?,?,?,?,?,?,?)');
  ins.run('m1', 'mm1', 'chat1', 's1', 1, 'user', JSON.stringify([{ type: 'text', text: '围绕项目向我提问' }]), nowSec);
  ins.run('m2', 'mm2', 'chat1', 's1', 2, 'assistant', JSON.stringify([{ type: 'tool-Thinking', input: { text: 'PRIVATE' } }, { type: 'text', text: '好的，第一个问题：' }]), nowSec);
  ins.run('m3', 'mm3', 'chat1', 's1', 3, 'assistant', JSON.stringify([{ type: 'error', text: 'MUST NOT APPEAR' }]), nowSec);
  db.close();

  // ---- Qoder CN IDE (~/.qoder-cn/projects, Claude-style JSONL) ----
  const qcDir = path.join(home, '.qoder-cn', 'projects', '-Users-x-Dev-prj');
  fs.mkdirSync(qcDir, { recursive: true });
  const qcUuid = '11111111-2222-3333-4444-555555555555';
  const qc = [
    { type: 'workspace-directories', sessionId: qcUuid },
    { type: 'user', cwd, sessionId: qcUuid, isSidechain: false,
      message: { content: [{ type: 'text', text: '<system-reminder>ENV MUST NOT APPEAR</system-reminder>这个悬浮窗怎么不抢焦点？' }] } },
    { type: 'assistant', cwd, sessionId: qcUuid,
      message: { content: [{ type: 'thinking', text: 'PRIVATE MUST NOT APPEAR' }] } },
    { type: 'assistant', cwd, sessionId: qcUuid,
      message: { content: [{ type: 'tool_use', name: 'read_file' }] } },
    { type: 'assistant', cwd, sessionId: qcUuid,
      message: { content: [{ type: 'text', text: '把 chip 窗设为 focusable:false 就常驻不抢焦点。' }] } },
    { type: 'active-leaf', sessionId: qcUuid },
    { type: 'system', text: 'MUST NOT APPEAR' },
  ];
  fs.writeFileSync(path.join(qcDir, qcUuid + '.jsonl'), qc.map((r) => JSON.stringify(r)).join('\n') + '\n');

  return home;
}
