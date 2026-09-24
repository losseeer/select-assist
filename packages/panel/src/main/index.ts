import { app, BrowserWindow, clipboard, ipcMain, screen, shell } from 'electron';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { SettingsStore } from './settings.js';
import { Capturer } from './capture.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const CHIP = { width: 400, height: 44 }; // same width as PANEL: expand/collapse is a pure height change
const PANEL = { width: 400, height: 350 };

if (!app.requestSingleInstanceLock()) {
  console.error('select-assist panel: 已有一个实例在运行（可能藏在屏幕角落的圆点），本次启动退出。如窗口不可见可执行 pkill -f "select-assist.*Electron" 后重试。');
  app.quit();
}

let settings: SettingsStore;
let capturer: Capturer;
let chipWin: BrowserWindow;
let panelWin: BrowserWindow;
let lastClip = '';

/**
 * Two windows instead of resizing one: the chip stays focusable:false
 * (non-activating) forever, the panel is a normal focusable window that is
 * shown/hidden. This removes both the setFocusable toggle and the transparent
 * resize repaint bug that made the panel "disappear".
 */
function basePrefs(): Electron.WebPreferences {
  return {
    preload: path.join(__dirname, '../preload.js'),
    contextIsolation: true,
    nodeIntegration: false,
    sandbox: false,
  };
}

function topMost(w: BrowserWindow): void {
  w.setAlwaysOnTop(true, 'screen-saver');
  w.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
}

function defaultChipPos(): { x: number; y: number } {
  const s = settings.get();
  const area = screen.getPrimaryDisplay().workArea;
  return {
    x: s.windowX ?? area.x + area.width - CHIP.width - 16,
    y: s.windowY ?? area.y + 60,
  };
}

function createWindows(): void {
  const pos = defaultChipPos();
  chipWin = new BrowserWindow({
    ...pos,
    ...CHIP,
    frame: false,
    transparent: true,
    resizable: false,
    movable: true,
    skipTaskbar: true,
    hasShadow: false,
    fullscreenable: false,
    focusable: false, // clicks must not steal keyboard focus from the source app
    alwaysOnTop: true,
    show: false,
    webPreferences: basePrefs(),
  });
  chipWin.loadFile(path.join(__dirname, '../../static/index.html'), { hash: 'chip' });
  topMost(chipWin);
  chipWin.once('ready-to-show', () => chipWin.showInactive());
  chipWin.on('moved', () => {
    const b = chipWin.getBounds();
    settings.patch({ windowX: b.x, windowY: b.y });
  });

  panelWin = new BrowserWindow({
    x: pos.x - (PANEL.width - CHIP.width),
    y: pos.y,
    ...PANEL,
    frame: false,
    transparent: true,
    resizable: false,
    movable: true,
    skipTaskbar: true,
    hasShadow: false,
    fullscreenable: false,
    focusable: true,
    alwaysOnTop: true,
    show: false,
    webPreferences: basePrefs(),
  });
  panelWin.loadFile(path.join(__dirname, '../../static/index.html'), { hash: 'panel' });
  topMost(panelWin);
  panelWin.on('close', () => {
    const b = chipWin.isVisible() ? chipWin.getBounds() : panelWin.getBounds();
    settings.patch({ windowX: Math.min(b.x, screen.getDisplayNearestPoint(b).workArea.width), windowY: b.y });
  });
}

interface WinBounds { x: number; y: number; width: number; height: number }
function clampToDisplay(b: WinBounds): WinBounds {
  const wa = screen.getDisplayNearestPoint({ x: b.x, y: b.y }).workArea;
  return {
    x: Math.max(wa.x, Math.min(b.x, wa.x + wa.width - b.width - 8)),
    y: Math.max(wa.y, Math.min(b.y, wa.y + wa.height - b.height - 8)),
    width: b.width,
    height: b.height,
  };
}

function expand(): void {
  const c = chipWin.getBounds();
  // keep the right edge anchored so the panel grows leftwards from the chip
  panelWin.setBounds(
    clampToDisplay({ x: c.x + CHIP.width - PANEL.width, y: c.y, width: PANEL.width, height: PANEL.height }),
  );
  chipWin.hide();
  panelWin.showInactive();
  panelWin.webContents.send('win:shown');
}

function collapse(): void {
  const p = panelWin.getBounds();
  chipWin.setBounds(
    clampToDisplay({ x: p.x + PANEL.width - CHIP.width, y: p.y, width: CHIP.width, height: CHIP.height }),
  );
  panelWin.hide();
  chipWin.showInactive();
  const b = chipWin.getBounds();
  settings.patch({ windowX: b.x, windowY: b.y });
}

function wireIpc(): void {
  ipcMain.handle('capture:selection', () => capturer.captureFromClipboard(settings.get()));
  ipcMain.handle('capture:summary', () => capturer.summary());
  ipcMain.handle('capture:context', (_e, opts: { agent: string; turns: number; filePath?: string }) =>
    capturer.attachContext(settings.get(), opts),
  );
  ipcMain.handle('capture:clearContext', () => capturer.clearContext());
  ipcMain.handle('pack:current', () => capturer.currentPayload(settings.get()) ?? null);
  ipcMain.handle('pack:copy', () => capturer.copyToClipboard(settings.get()));
  ipcMain.handle('sessions:browse', () => capturer.browse());

  ipcMain.handle('site:open', (_e, url: string) => {
    const u = new URL(url);
    if (u.protocol !== 'https:' && u.protocol !== 'http:') return false;
    void shell.openExternal(url);
    return true;
  });

  ipcMain.handle('settings:get', () => settings.get());
  ipcMain.handle('settings:patch', (_e, p) => settings.patch(p));
  ipcMain.handle('win:expand', () => expand());
  ipcMain.handle('win:collapse', () => collapse());
  ipcMain.handle('win:focusSelf', () => {
    if (panelWin.isVisible()) panelWin.focus();
  });
  ipcMain.handle('app:quit', () => app.quit());
}

function startClipboardWatch(): void {
  // polling only LIGHTS UP the capture button — it never opens or captures anything
  setInterval(() => {
    if (chipWin.isDestroyed() || panelWin.isDestroyed()) return;
    let text = '';
    try {
      text = clipboard.readText();
    } catch {
      return;
    }
    if (text && text !== lastClip) {
      lastClip = text;
      const payload = { chars: text.length, firstLine: text.split('\n')[0]?.slice(0, 80) };
      for (const w of [chipWin, panelWin]) if (w.isVisible()) w.webContents.send('clipboard:new', payload);
    }
  }, 800);
}

app.whenReady().then(() => {
  const dir = app.getPath('userData');
  settings = new SettingsStore(dir);
  capturer = new Capturer();
  wireIpc();
  createWindows();
  startClipboardWatch();
});

app.on('web-contents-created', (e, contents) => {
  contents.setWindowOpenHandler(({ url }) => {
    try {
      const u = new URL(url);
      if (u.protocol === 'https:' || u.protocol === 'http:') void shell.openExternal(url);
    } catch {
      /* reject */
    }
    return { action: 'deny' };
  });
});

app.on('window-all-closed', () => app.quit());
