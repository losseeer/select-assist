/* global api */
'use strict';

const $ = (id) => document.getElementById(id);
const MODE = document.documentElement.dataset.mode; // 'chip' | 'panel'

// ================= custom tooltip (both windows) =================
// native title is unusable: alwaysOnTop windows cover their own tooltips
const tooltip = (() => {
  const el = document.createElement('div');
  el.id = 'tooltip';
  document.body.appendChild(el);
  let target = null;
  let timer;

  const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const setTip = (text) => { el.innerHTML = esc(text).replace(/`([^`]+)`/g, '<code>$1</code>'); };

  function place(t) {
    const r = t.getBoundingClientRect();
    el.style.visibility = 'hidden';
    el.classList.add('show');
    const tr = el.getBoundingClientRect();
    const gap = 6;
    const pad = 4;
    const fits = (x, y) => x >= pad && x + tr.width <= window.innerWidth - pad && y >= pad && y + tr.height <= window.innerHeight - pad;
    const cx = (x) => Math.max(pad, Math.min(x, window.innerWidth - pad - tr.width));
    let x = cx(r.left + r.width / 2 - tr.width / 2);
    let y = r.bottom + gap;
    if (!fits(x, y)) y = r.top - tr.height - gap; // flip above
    if (!fits(x, y)) {
      // too short a window (e.g. the 44px chip) for below/above — sit beside the target
      y = Math.max(pad, Math.min(r.top + r.height / 2 - tr.height / 2, window.innerHeight - pad - tr.height));
      x = r.right + gap;
      if (!fits(x, y)) x = r.left - tr.width - gap;
    }
    if (!fits(x, y)) { x = cx(r.left + r.width / 2 - tr.width / 2); y = Math.max(pad, r.bottom + gap); }
    el.style.left = `${Math.round(x)}px`;
    el.style.top = `${Math.round(y)}px`;
    el.style.visibility = '';
  }
  function show(t) {
    target = t;
    setTip(t.dataset.tip);
    place(t);
  }
  function hide() {
    clearTimeout(timer);
    target = null;
    el.classList.remove('show');
  }
  document.addEventListener('mouseover', (e) => {
    const t = e.target.closest ? e.target.closest('[data-tip]') : null;
    if (t === target) return;
    clearTimeout(timer);
    if (!t) { hide(); return; }
    timer = setTimeout(() => show(t), 400);
  });
  document.addEventListener('mouseout', (e) => {
    if (!target) return;
    const t = e.target.closest ? e.target.closest('[data-tip]') : null;
    if (t === target && !(e.relatedTarget && target.contains(e.relatedTarget))) hide();
  });
  document.addEventListener('focusin', (e) => {
    const t = e.target.closest ? e.target.closest('[data-tip]') : null;
    if (t) show(t); else hide();
  });
  document.addEventListener('focusout', hide);
  window.addEventListener('blur', hide);
  return { hide };
})();

// ================= chip window =================
if (MODE === 'chip') {
  $('dot').addEventListener('click', () => api.expand());

  async function refreshChipStatus() {
    const s = await api.captureSummary();
    const st = $('chip-status');
    st.classList.remove('err');
    if (s.selection?.ok) {
      st.textContent = s.selection.firstLine || '已取入选区';
      st.dataset.tip = `来源：剪贴板 · ${new Date(s.selection.captureAt).toLocaleTimeString()} · ${s.selection.chars} 字`;
    } else {
      st.textContent = '还没有选区';
      st.dataset.tip = '在源界面复制，再点「取入选区」';
    }
  }

  $('chip-capture').addEventListener('click', async () => {
    const btn = $('chip-capture');
    btn.classList.add('busy');
    try {
      const r = await api.captureSelection();
      if (r && r.ok === false) {
        const st = $('chip-status');
        st.textContent = r.reason ?? '取入失败';
        st.classList.add('err');
        setTimeout(refreshChipStatus, 3000);
      }
      $('chip-badge').classList.remove('on');
    } finally {
      btn.classList.remove('busy');
      // always open the panel — failures surface there instead of dying silently
      api.expand();
    }
  });
  api.onClipboardNew((d) => $('chip-badge').classList.toggle('on', !!d));
  api.onWinShown(refreshChipStatus);
  refreshChipStatus();
}

// ================= panel window =================
if (MODE === 'panel') {
  let settings = null;
  let lastCtx = null;
  let browsing = null; // selected filePath from browser
  // 'read' = 会话解读 (assemble with context), 'direct' = 选区直通 (raw selection).
  // Persisted as the withContext boolean; the segmented control is its semantic face.
  let mode = 'read';

  function keyActivate(el, fn) {
    el.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        fn(e);
      }
    });
  }

  // panel window height follows content (review #8): report natural content height upward
  function reportHeight() {
    const panel = $('panel');
    const last = panel.lastElementChild;
    if (!last) return;
    const pad = parseFloat(getComputedStyle(panel).paddingBottom) || 0;
    api.autoHeight(Math.ceil(last.getBoundingClientRect().bottom + pad) + 4);
  }
  let heightTimer;
  new MutationObserver(() => {
    clearTimeout(heightTimer);
    heightTimer = setTimeout(reportHeight, 120);
  }).observe($('panel'), { childList: true, subtree: true, attributes: true, characterData: true });

  // while content animates (details open/close), the WINDOW must follow every frame —
  // one-shot setBounds after the debounce is what made the settings expand look janky
  function chaseHeight(duration = 260) {
    const start = performance.now();
    const step = (now) => {
      reportHeight();
      if (now - start < duration) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }
  $('settings').addEventListener('toggle', () => chaseHeight());

  document.addEventListener('mousedown', (e) => {
    const t = e.target;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT') {
      api.focusSelf();
    }
  });

  $('collapse').addEventListener('click', () => api.collapse());
  keyActivate($('collapse'), () => api.collapse());
  $('quit').addEventListener('click', () => api.quit());
  keyActivate($('quit'), () => api.quit());
  $('head-capture').addEventListener('click', async () => {
    const btn = $('head-capture');
    btn.classList.add('busy');
    try {
      const r = await api.captureSelection();
      if (r && r.ok === false) flashStatus(r.reason ?? '取入失败', true);
      await refreshSummary();
    } finally {
      btn.classList.remove('busy');
    }
  });
  api.onClipboardNew((d) => $('head-capture').classList.toggle('newclip', !!d));

  async function refreshSummary() {
    const s = await api.captureSummary();
    const st = $('head-status');
    if (s.selection?.ok) {
      st.textContent = s.selection.firstLine || '已取入选区';
      st.dataset.tip = `来源：剪贴板 · ${new Date(s.selection.captureAt).toLocaleTimeString()} · ${s.selection.chars} 字`;
    } else {
      st.textContent = '还没有选区';
      st.dataset.tip = '在源界面复制，再点「取入选区」';
    }
    lastCtx = s.context;
    renderSessionLine();
    refreshPackMeta();
    // a new capture resets the pack — re-fill context when in 会话解读 mode
    if (s.selection?.ok && mode === 'read' && !s.context.agent && !s.context.error) {
      await refreshContext();
    }
  }
  api.onWinShown(async () => {
    // replay entrance animation on every expand (window show does not reload the page)
    const p = $('panel');
    p.classList.remove('enter');
    void p.offsetWidth;
    p.classList.add('enter');
    await refreshSummary();
    reportHeight();
  });

  function renderSessionLine() {
    const el = $('sum-session');
    if (mode !== 'read') { el.textContent = ''; return; }
    if (!lastCtx || (!lastCtx.agent && !lastCtx.error)) { el.textContent = '上下文未填充'; return; }
    if (lastCtx.error) { el.textContent = `上下文：${lastCtx.error}`; return; }
    el.textContent = `判定会话：${lastCtx.agent} · ${lastCtx.turnsIncluded} 轮`;
    el.dataset.tip = `会话 id：${lastCtx.sessionId ?? '—'}\n判据：${lastCtx.basis}`;
  }

  // ---------- mode (会话解读 / 选区直通) ----------
  function applyMode() {
    const read = mode === 'read';
    $('mode-read').setAttribute('aria-selected', String(read));
    $('mode-direct').setAttribute('aria-selected', String(!read));
    // visibility (not display): the ctx row keeps its slot so nothing jumps on switch
    $('ctx-selects').classList.toggle('off', !read);
    // 直通模式没有上下文区：彻底移除（display:none）而不是占位隐藏，否则面板中部出现大片空白
    $('ctx-actions').classList.toggle('hidden', !read);
    $('sum-session').classList.toggle('hidden', !read);
    // the picker must not hold a slot in 直通: the whole actions row differs per mode anyway
    $('prompt-pick').classList.toggle('hidden', !read);
    $('copy').textContent = read ? '复制 Prompt' : '复制选区原文';
    if (read) $('copy').dataset.tip = '指令 + 选区 + 历史，一次复制';
    else delete $('copy').dataset.tip;
    document.querySelectorAll('#settings .ctx-only').forEach((el) => el.classList.toggle('hidden', !read));
    // each mode only configures its own site group; the hidden editor keeps its value for save
    document.querySelectorAll('#settings .site-edit').forEach((el) => {
      el.classList.toggle('hidden', el.dataset.for === 'read' ? !read : read);
    });
  }

  async function setMode(next) {
    if (next === mode) return;
    mode = next;
    settings = await api.patchSettings({ withContext: mode === 'read' });
    if (mode === 'read') {
      await refreshContext();
    } else {
      ctxSeq++; // invalidate any in-flight attach so it cannot repopulate after clearing
      $('browser').classList.add('hidden');
      lastCtx = null;
      browsing = null;
      await api.clearContext();
      renderSessionLine();
      refreshPackMeta();
    }
    applyMode();
    renderSites();
    chaseHeight();
  }
  $('mode-read').addEventListener('click', () => setMode('read'));
  $('mode-direct').addEventListener('click', () => setMode('direct'));
  keyActivate($('mode-read'), () => setMode('read'));
  keyActivate($('mode-direct'), () => setMode('direct'));

  // ---------- context ----------
  let ctxSeq = 0; // last-write-wins guard: a slow attach must not clobber a newer one
  $('ctx-refresh').addEventListener('click', () => refreshContext());

  async function refreshContext() {
    if (mode !== 'read') return;
    const seq = ++ctxSeq;
    flashStatus('正在填充上下文…');
    const r = await api.attachContext({
      agent: browsing ? browsing.agent : $('ctx-agent').value,
      turns: $('ctx-turns').value === 'all' ? 0 : Number($('ctx-turns').value),
      filePath: browsing?.filePath,
      sessionId: browsing?.sessionId,
    });
    if (seq !== ctxSeq) return; // a newer refresh already owns the state
    lastCtx = r;
    flashStatus(r.error ?? '', !!r.error);
    renderSessionLine();
    refreshPackMeta();
  }

  // ---------- session browser (solves "I can't find where sessions live") ----------
  $('ctx-browse').addEventListener('click', async () => {
    const sec = $('browser');
    if (!sec.classList.contains('hidden')) { sec.classList.add('hidden'); return; }
    sec.classList.remove('hidden');
    const wrap = $('browser-list');
    wrap.innerHTML = '<div class="br-loading">正在发现会话…</div>';
    let list = [];
    try {
      list = await api.browseSessions();
    } catch (e) {
      wrap.innerHTML = '';
      const div = document.createElement('div');
      div.className = 'br-loading';
      div.textContent = `会话发现失败：${e.message ?? e}`;
      wrap.appendChild(div);
      return;
    }
    wrap.innerHTML = '';
    if (list.length === 0) {
      wrap.innerHTML = '<div class="br-item"><span class="pv">未发现任何可解析的会话（Trae 数据库加密暂不支持）</span></div>';
      return;
    }
    for (const e of list) {
      const div = document.createElement('div');
      div.className = 'br-item';
      div.tabIndex = 0;
      div.setAttribute('role', 'option');
      div.dataset.tip = e.filePath;
      const nm = e.name || (e.projectPath ? e.projectPath.split('/').pop() : (e.sessionId ?? '').slice(0, 8));
      const sid = e.sessionId ? `#${e.sessionId.slice(0, 8)}` : '';
      div.innerHTML = `<div class="l1"><span class="ag">${e.agent}${sid}</span><span class="nm"></span><span class="mt">${new Date(e.mtime).toLocaleString()}</span></div><div class="pv"></div>`;
      div.querySelector('.nm').textContent = nm + (e.projectPath ? ` · ${e.projectPath}` : '');
      div.querySelector('.pv').textContent = e.preview ?? '';
      const choose = async () => {
        wrap.querySelectorAll('.sel').forEach((x) => x.classList.remove('sel'));
        div.classList.add('sel');
        browsing = { agent: e.agent, filePath: e.filePath, sessionId: e.sessionId };
        $('ctx-agent').value = e.agent;
        tooltip.hide();
        await refreshContext();
        sec.classList.add('hidden');
      };
      div.addEventListener('click', choose);
      keyActivate(div, choose);
      wrap.appendChild(div);
    }
  });

  function flashStatus(msg, isErr = false) {
    const el = $('ctx-status');
    el.textContent = msg;
    el.classList.toggle('err', isErr);
    if (msg && !isErr) setTimeout(() => { if (el.textContent === msg) el.textContent = ''; }, 4000);
  }

  // ---------- copy & open ----------
  async function refreshPackMeta() {
    const el = $('pack-meta');
    const cur = await api.packCurrent();
    if (!cur) { el.textContent = ''; return; }
    // no context ⇒ the copied text IS the raw selection (nothing was assembled)
    const noun = cur.context ? '组装后' : '选区原文';
    if (cur.dropped.length) {
      el.textContent = `${noun} ${cur.usedChars} 字 · 已省略 ${cur.dropped.length} 类`;
      el.dataset.tip = `已省略：${cur.dropped.join('、')}`;
    } else {
      el.textContent = `${noun} ${cur.usedChars} 字`;
      el.removeAttribute('data-tip');
    }
  }

  $('copy').addEventListener('click', async () => {
    const ok = await api.copyPack();
    if (!ok) { flashStatus('还没有取入选区', true); return; }
    const btn = $('copy');
    btn.classList.add('done');
    btn.textContent = '已复制 ✓';
    setTimeout(() => { btn.classList.remove('done'); applyMode(); }, 1200);
    refreshPackMeta();
  });

  function renderSites() {
    const wrap = $('sites');
    wrap.innerHTML = '';
    for (const s of mode === 'read' ? settings.chatSites : settings.directSites) {
      const b = document.createElement('button');
      b.textContent = s.name;
      b.dataset.tip = s.url;
      b.addEventListener('click', () => api.openSite(s.url));
      wrap.appendChild(b);
    }
  }

  // ---------- 提问指令：主视图选择器 ----------
  function renderPromptPick() {
    const sel = $('prompt-pick');
    sel.innerHTML = '';
    settings.prompts.forEach((p, i) => {
      const o = document.createElement('option');
      o.value = String(i);
      o.textContent = p.name || `指令 ${i + 1}`;
      sel.appendChild(o);
    });
    sel.value = String(Math.min(settings.activePrompt, settings.prompts.length - 1));
  }
  $('prompt-pick').addEventListener('change', async () => {
    settings = await api.patchSettings({ activePrompt: Number($('prompt-pick').value) });
    refreshPackMeta();
  });

  // ---------- 提问指令编辑器：下拉选 + 单个模板框（草稿随保存写回） ----------
  let promptDraft = [];
  let editSel = 0;

  function renderPromptEdit() {
    const sel = $('pe-pick');
    sel.innerHTML = '';
    promptDraft.forEach((p, i) => {
      const o = document.createElement('option');
      o.value = String(i);
      o.textContent = p.name;
      sel.appendChild(o);
    });
    sel.value = String(editSel);
    $('pe-tpl').value = promptDraft[editSel]?.template ?? '';
    $('pe-count').textContent = promptDraft.length ? `${editSel + 1} / ${promptDraft.length}` : '';
  }
  function commitTpl() {
    const cur = promptDraft[editSel];
    if (cur) cur.template = $('pe-tpl').value.trim();
  }
  function loadPromptEdit() {
    promptDraft = settings.prompts.map((p) => ({ ...p }));
    editSel = Math.min(Math.max(settings.activePrompt, 0), promptDraft.length - 1);
    renderPromptEdit();
  }
  $('pe-pick').addEventListener('change', () => {
    commitTpl();
    editSel = Number($('pe-pick').value);
    renderPromptEdit();
  });
  $('pe-tpl').addEventListener('input', commitTpl);
  $('pe-new').addEventListener('click', () => {
    commitTpl();
    let n = promptDraft.length + 1;
    while (promptDraft.some((p) => p.name === `指令 ${n}`)) n++;
    promptDraft.push({ name: `指令 ${n}`, template: '{selection}' });
    editSel = promptDraft.length - 1;
    renderPromptEdit();
    $('set-save').classList.add('dirty');
    $('pe-tpl').focus();
  });
  $('pe-del').addEventListener('click', () => {
    if (promptDraft.length <= 1) { flashStatus('至少保留一条指令', true); return; }
    promptDraft.splice(editSel, 1);
    editSel = Math.max(0, editSel - 1);
    renderPromptEdit();
    $('set-save').classList.add('dirty');
  });

  // ---------- settings ----------
  const AGENT_TOKENS = ['auto', 'claude-code', 'codex', 'workbuddy', 'qoder', 'project'];
  async function init() {
    settings = await api.getSettings();
    mode = settings.withContext === false ? 'direct' : 'read';
    applyMode();
    renderSites();
    renderPromptPick();
    loadPromptEdit();
    $('set-chat-sites').value = settings.chatSites.map((s) => `${s.name}|${s.url}`).join('\n');
    $('set-direct-sites').value = settings.directSites.map((s) => `${s.name}|${s.url}`).join('\n');
    $('set-sessions').value = settings.sessionPaths.map((s) => `${s.agent}|${s.path}`).join('\n');
    $('set-redact').checked = !!settings.redactPaths;
    refreshSummary();
  }
  $('settings').addEventListener('input', () => $('set-save').classList.add('dirty'));
  $('set-save').addEventListener('click', async () => {
    commitTpl();
    const prompts = promptDraft.filter((p) => p.name && p.template);
    const sessionPaths = [];
    const badSess = [];
    $('set-sessions').value.split('\n').forEach((line, i) => {
      if (!line.trim()) return;
      const p = line.split('|');
      const agent = p[0]?.trim().toLowerCase();
      const path = p[1]?.trim();
      if (p.length >= 2 && AGENT_TOKENS.includes(agent) && path) sessionPaths.push({ agent, path });
      else badSess.push(i + 1);
    });
    const parseSites = (text) => {
      const sites = [];
      const bad = [];
      text.split('\n').forEach((line, i) => {
        if (!line.trim()) return;
        const p = line.split('|');
        const name = p[0]?.trim();
        const url = p[1]?.trim();
        if (p.length >= 2 && name && url && /^https?:\/\//i.test(url)) sites.push({ name, url });
        else bad.push(i + 1);
      });
      return { sites, bad };
    };
    const chat = parseSites($('set-chat-sites').value);
    const direct = parseSites($('set-direct-sites').value);
    settings = await api.patchSettings({
      prompts: prompts.length ? prompts : settings.prompts,
      activePrompt: prompts.length ? Math.min(settings.activePrompt, prompts.length - 1) : 0,
      sessionPaths,
      redactPaths: $('set-redact').checked,
      chatSites: chat.sites.length ? chat.sites : settings.chatSites,
      directSites: direct.sites.length ? direct.sites : settings.directSites,
    });
    renderSites();
    renderPromptPick();
    loadPromptEdit();
    $('set-save').classList.remove('dirty');
    const warn = [];
    if (chat.bad.length) warn.push(`会话解读站点第 ${chat.bad.join('、')} 行`);
    if (direct.bad.length) warn.push(`直通站点第 ${direct.bad.join('、')} 行`);
    if (badSess.length) warn.push(`会话路径第 ${badSess.join('、')} 行`);
    if (warn.length) flashStatus(`无效行（站点需 名称|http(s)://URL，路径需 ${AGENT_TOKENS.join('/')}|路径）：${warn.join('，')}，未生效`, true);
    else flashStatus('已保存');
    refreshPackMeta(); // 指令集可能变了
  });

  init();
}
