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
      $('chip-badge').classList.remove('on');
    } finally {
      btn.classList.remove('busy');
      // always open the panel — failures surface there instead of dying silently
      api.expand();
    }
  });
  // payload null means "state cleared" — toggle so a capture in either window lights out both
  api.onClipboardNew((d) => $('chip-badge').classList.toggle('on', !!d));
}

// ================= panel window =================
if (MODE === 'panel') {
  let settings = null;
  let lastCtx = null;
  let browsing = null; // selected filePath from browser

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
      if (r && r.ok !== false) btn.classList.remove('newclip');
      await refreshSummary();
    } finally {
      btn.classList.remove('busy');
    }
  });
  api.onClipboardNew((d) => $('head-capture').classList.toggle('newclip', !!d));

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
  api.onWinShown(async () => {
    await refreshSummary();
    reportHeight();
  });

  function renderSessionLine() {
    const el = $('sum-session');
    if (!$('with-ctx').checked) { el.textContent = ''; return; }
    if (!lastCtx || (!lastCtx.agent && !lastCtx.error)) { el.textContent = '上下文未填充'; return; }
    if (lastCtx.error) { el.textContent = `上下文：${lastCtx.error}`; return; }
    el.textContent =
      `判定会话：${lastCtx.agent} / ${(lastCtx.sessionId ?? '').slice(0, 12)} · ${lastCtx.turnsIncluded} 轮 · 判据：${lastCtx.basis}`;
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
      sessionId: browsing?.sessionId,
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
    const wrap = $('browser-list');
    const pathEl = $('browser-path');
    wrap.innerHTML = '<div class="br-loading">正在发现会话…</div>';
    const list = await api.browseSessions();
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
      const nm = e.name || (e.projectPath ? e.projectPath.split('/').pop() : (e.sessionId ?? '').slice(0, 8));
      const sid = e.sessionId ? `#${e.sessionId.slice(0, 8)}` : '';
      div.innerHTML = `<div class="l1"><span class="ag">${e.agent}${sid}</span><span class="nm"></span><span class="mt">${new Date(e.mtime).toLocaleString()}</span></div><div class="pv"></div>`;
      div.querySelector('.nm').textContent = nm + (e.projectPath ? ` · ${e.projectPath}` : '');
      div.querySelector('.pv').textContent = e.preview ?? '';
      // hover path goes to its own line — ctx-status stays reserved for loading/errors
      div.addEventListener('mouseenter', () => { pathEl.textContent = e.filePath; });
      div.addEventListener('mouseleave', () => { pathEl.textContent = ''; });
      const choose = async () => {
        wrap.querySelectorAll('.sel').forEach((x) => x.classList.remove('sel'));
        div.classList.add('sel');
        browsing = { agent: e.agent, filePath: e.filePath, sessionId: e.sessionId };
        $('ctx-agent').value = e.agent;
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
    $('set-redact').checked = !!settings.redactPaths;
    $('set-sites').value = settings.sites.map((s) => `${s.name}|${s.url}`).join('\n');
    refreshSummary();
  }
  $('settings').addEventListener('input', () => $('set-save').classList.add('dirty'));
  $('set-save').addEventListener('click', async () => {
    const sites = [];
    const bad = [];
    $('set-sites').value.split('\n').forEach((line, i) => {
      if (!line.trim()) return;
      const p = line.split('|');
      const name = p[0]?.trim();
      const url = p[1]?.trim();
      if (p.length >= 2 && name && url && /^https?:\/\//i.test(url)) sites.push({ name, url });
      else bad.push(i + 1);
    });
    settings = await api.patchSettings({
      promptTemplate: $('set-prompt').value.trim(),
      projectPath: $('set-project').value.trim(),
      redactPaths: $('set-redact').checked,
      sites: sites.length ? sites : settings.sites,
    });
    renderSites();
    $('set-save').classList.remove('dirty');
    if (bad.length) flashStatus(`目标站第 ${bad.join('、')} 行无效（需 名称|http(s)://URL），该行未生效`, true);
    else flashStatus('已保存');
  });

  init();
}
