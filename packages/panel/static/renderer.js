/* global api */
'use strict';

const $ = (id) => document.getElementById(id);
const MODE = document.documentElement.dataset.mode; // 'chip' | 'panel'

// ================= chip window =================
if (MODE === 'chip') {
  $('dot').addEventListener('click', () => api.expand());
  $('chip-capture').addEventListener('click', async () => {
    const btn = $('chip-capture');
    btn.classList.add('busy');
    try {
      await api.captureSelection();
      $('chip-badge').classList.add('hidden');
      api.expand();
    } finally {
      btn.classList.remove('busy');
    }
  });
  api.onClipboardNew(({ chars, firstLine }) => {
    const b = $('chip-badge');
    b.classList.remove('hidden');
    b.title = `剪贴板 ${chars} 字：${firstLine}`;
  });
}

// ================= panel window =================
if (MODE === 'panel') {
  let settings = null;
  let lastCtx = null;
  let browsing = null; // selected filePath from browser

  document.addEventListener('mousedown', (e) => {
    const t = e.target;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT') {
      api.focusSelf();
    }
  });

  $('collapse').addEventListener('click', () => api.collapse());
  $('quit').addEventListener('click', () => api.quit());
  $('head-capture').addEventListener('click', async () => {
    const btn = $('head-capture');
    btn.classList.add('busy');
    try {
      await api.captureSelection();
      await refreshSummary();
    } finally {
      btn.classList.remove('busy');
    }
  });

  async function refreshSummary() {
    const s = await api.captureSummary();
    if (s.selection?.ok) {
      $('summary-empty').classList.add('hidden');
      $('summary-body').classList.remove('hidden');
      $('sum-first').textContent = s.selection.firstLine || '（无首行）';
      $('sum-meta').textContent =
        `来源：剪贴板 · ${new Date(s.selection.captureAt).toLocaleTimeString()} · ${s.selection.chars} 字`;
    } else if (s.selection && !s.selection.ok) {
      $('summary-empty').textContent = s.selection.reason ?? '';
    }
    lastCtx = s.context;
    renderSessionLine();
    refreshPackMeta();
  }
  api.onWinShown(() => refreshSummary());

  function renderSessionLine() {
    const el = $('sum-session');
    if (!$('with-ctx').checked) { el.textContent = ''; return; }
    if (!lastCtx || (!lastCtx.agent && !lastCtx.error)) { el.textContent = '上下文未填充'; return; }
    if (lastCtx.error) { el.textContent = `上下文：${lastCtx.error}`; return; }
    el.textContent =
      `判定会话：${lastCtx.agent} / ${(lastCtx.sessionId ?? '').slice(0, 12)} · ${lastCtx.turnsIncluded} 条 · 判据：${lastCtx.basis}`;
  }

  // ---------- context ----------
  $('with-ctx').addEventListener('change', async (e) => {
    $('ctx-refresh').classList.toggle('hidden', !e.target.checked);
    $('ctx-browse').classList.toggle('hidden', !e.target.checked);
    if (!e.target.checked) {
      $('browser').classList.add('hidden');
      lastCtx = null;
      browsing = null;
      await api.clearContext();
      renderSessionLine();
    } else {
      await refreshContext();
    }
  });
  $('ctx-refresh').addEventListener('click', () => refreshContext());

  async function refreshContext() {
    if (!$('with-ctx').checked) return;
    flashStatus('正在填充上下文…');
    const r = await api.attachContext({
      agent: browsing ? browsing.agent : $('ctx-agent').value,
      turns: Number($('ctx-turns').value),
      filePath: browsing?.filePath,
    });
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
    const list = await api.browseSessions();
    const wrap = $('browser-list');
    wrap.innerHTML = '';
    if (list.length === 0) {
      wrap.innerHTML = '<div class="br-item"><span class="pv">未发现任何可解析的会话（Trae 数据库加密暂不支持）</span></div>';
      return;
    }
    for (const e of list) {
      const div = document.createElement('div');
      div.className = 'br-item';
      const nm = e.name || (e.projectPath ? e.projectPath.split('/').pop() : (e.sessionId ?? '').slice(0, 8));
      div.innerHTML = `<div class="l1"><span class="ag">${e.agent}</span><span class="nm"></span><span class="mt">${new Date(e.mtime).toLocaleString()}</span></div><div class="pv"></div>`;
      div.querySelector('.nm').textContent = nm + (e.projectPath ? ` · ${e.projectPath}` : '');
      div.querySelector('.pv').textContent = e.preview ?? '';
      div.title = e.filePath;
      div.addEventListener('click', async () => {
        wrap.querySelectorAll('.sel').forEach((x) => x.classList.remove('sel'));
        div.classList.add('sel');
        browsing = { agent: e.agent, filePath: e.filePath };
        $('ctx-agent').value = e.agent;
        await refreshContext();
        sec.classList.add('hidden');
      });
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
    const dropNote = cur.dropped.length ? ` · 已省略 ${cur.dropped.join('、')}` : '';
    el.textContent = `组装后 ${cur.usedChars} 字${dropNote}`;
  }

  $('copy').addEventListener('click', async () => {
    const ok = await api.copyPack();
    if (!ok) { flashStatus('还没有取入选区', true); return; }
    const btn = $('copy');
    btn.classList.add('done');
    btn.textContent = '已复制 ✓';
    setTimeout(() => { btn.classList.remove('done'); btn.textContent = '复制 Prompt'; }, 1200);
    refreshPackMeta();
  });

  function renderSites() {
    const wrap = $('sites');
    wrap.innerHTML = '';
    for (const s of settings.sites) {
      const b = document.createElement('button');
      b.textContent = s.name;
      b.title = s.url;
      b.addEventListener('click', () => api.openSite(s.url));
      wrap.appendChild(b);
    }
  }

  // ---------- settings ----------
  async function init() {
    settings = await api.getSettings();
    renderSites();
    $('set-prompt').value = settings.promptTemplate ?? '';
    $('set-project').value = settings.projectPath ?? '';
    $('set-max').value = settings.maxChars;
    $('set-redact').checked = !!settings.redactPaths;
    $('set-sites').value = settings.sites.map((s) => `${s.name}|${s.url}`).join('\n');
    refreshSummary();
  }
  $('set-save').addEventListener('click', async () => {
    const sites = $('set-sites').value
      .split('\n')
      .map((l) => l.split('|'))
      .filter((p) => p.length >= 2 && p[0].trim() && p[1].trim())
      .map(([name, url]) => ({ name: name.trim(), url: url.trim() }));
    settings = await api.patchSettings({
      promptTemplate: $('set-prompt').value.trim(),
      projectPath: $('set-project').value.trim(),
      maxChars: Number($('set-max').value) || 8000,
      redactPaths: $('set-redact').checked,
      sites: sites.length ? sites : settings.sites,
    });
    renderSites();
    flashStatus('已保存');
  });

  init();
}
