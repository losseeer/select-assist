import { app, BrowserWindow, clipboard, ipcMain, screen, shell } from 'electron';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { SettingsStore } from './settings.js';
import { Capturer } from './capture.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const CHIP = { width: 400, height: 44 }; // same width as PANEL: expand/collapse is a pure height change
const PANEL = { width: 400, height: 350 }; // height is the default; autoHeight() adapts it to content

// DWM system backdrops (acrylic/mica/tabbed) are Windows 11 only (build 22000+); on Windows 10
// the option degrades silently, so that path keeps the plain transparent window it had before.
// A backdrop also enforces a 64 physical-px minimum height on a transparent:true window: the
// 44px chip would report 44 to Electron while the OS window is 64, and the extra 20px paints as
// a light band under the bar. Making the window opaque lifts that minimum while the backdrop
// still shows through the page's alpha — but Electron then paints any backgroundColor over the
// backdrop, so none may be set. Corner clipping comes from Electron's default
// roundedCorners:true, which is what keeps the card's corners from turning into black wedges,
// so it must not be switched off here.
const hasBackdrop = process.platform === 'win32' && Number(/(\d+)\.(\d+)\.(\d+)/.exec(os.release())?.[3]) >= 22000;
const MATERIAL: Partial<Electron.BrowserWindowConstructorOptions> =
  process.platform === 'darwin'
    ? { vibrancy: 'hud' }
    : hasBackdrop
      ? { transparent: false, backgroundMaterial: 'acrylic' }
      : {};

// dev and packaged builds share one userData dir (productName "select-assist"),
// so settings and the single-instance lock behave identically
app.setName('select-assist');

if (!app.requestSingleInstanceLock()) {
  console.error('select-assist panel: 已有一个实例在运行（可能藏在屏幕角落的圆点），本次启动退出。如窗口不可见可执行 pkill -f "select-assist.*Electron" 后重试。');
  app.quit();
}

let settings: SettingsStore;
let capturer: Capturer;
let chipWin: BrowserWindow;
let panelWin: BrowserWindow;
let lastClip = '';
let panelHeight = PANEL.height;
// unread copy survives expand/collapse: a copy made while the chip is hidden must still light it up later
interface ClipNote { chars: number; firstLine: string }
let clipUnread: ClipNote | null = null;

function notifyClip(): void {
  // always (re)send, including null — that is how a cleared state reaches visible windows
  safeSend(chipWin, 'clipboard:new', clipUnread);
  safeSend(panelWin, 'clipboard:new', clipUnread);
}

// a send racing a reload/teardown throws "Render frame was disposed"; an uncaught
// throw inside the clipboard poll interval would crash the whole app
function safeSend(w: BrowserWindow | undefined, channel: string, ...args: unknown[]): void {
  try {
    if (w && !w.isDestroyed() && w.isVisible()) w.webContents.send(channel, ...args);
  } catch {
    /* window vanished mid-send; state replays on next expand/collapse */
  }
}

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
  const raw = {
    x: s.windowX ?? area.x + area.width - CHIP.width - 16,
    y: s.windowY ?? area.y + 60,
  };
  // a saved position on a now-disconnected display must not park the chip offscreen forever
  const clamped = clampToDisplay({ ...raw, width: CHIP.width, height: CHIP.height });
  return { x: clamped.x, y: clamped.y };
}

/**
 * Two windows instead of resizing one: the chip stays focusable:false (non-activating)
 * forever, the panel is a normal focusable window that is shown/hidden. That removes both
 * the setFocusable toggle and the transparent-resize repaint bug that made the panel
 * "disappear" when macOS grew its window.
 */
function createWindows(): void {
  const pos = defaultChipPos();
  chipWin = new BrowserWindow({
    ...pos,
    ...CHIP,
    frame: false,
    transparent: true,
    ...MATERIAL, // macOS HUD material behind the translucent page; --bg veil keeps text contrast
    resizable: false,
    movable: true,
    skipTaskbar: true,
    hasShadow: true,
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
    ...MATERIAL,
    resizable: false,
    movable: true,
    skipTaskbar: true,
    hasShadow: true,
    fullscreenable: false,
    focusable: true,
    alwaysOnTop: true,
    show: false,
    webPreferences: basePrefs(),
  });
  panelWin.loadFile(path.join(__dirname, '../../static/index.html'), { hash: 'panel' });
  topMost(panelWin);
  panelWin.on('close', () => {
    // store the raw position; clampToDisplay() at startup keeps it on-screen
    // (Math.min against workArea.width here would break negative-x secondary displays)
    const b = chipWin.isVisible() ? chipWin.getBounds() : panelWin.getBounds();
    settings.patch({ windowX: b.x, windowY: b.y });
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
    clampToDisplay({ x: c.x + CHIP.width - PANEL.width, y: c.y, width: PANEL.width, height: panelHeight }),
  );
  chipWin.hide();
  panelWin.showInactive();
  safeSend(panelWin, 'win:shown'); // refresh status line from main
  notifyClip(); // then replay the unread-clipboard badge
}

function collapse(): void {
  const p = panelWin.getBounds();
  chipWin.setBounds(
    clampToDisplay({ x: p.x + PANEL.width - CHIP.width, y: p.y, width: CHIP.width, height: CHIP.height }),
  );
  panelWin.hide();
  chipWin.showInactive();
  safeSend(chipWin, 'win:shown'); // chip status line refreshes like the panel does
  notifyClip();
  const b = chipWin.getBounds();
  settings.patch({ windowX: b.x, windowY: b.y });
}

function wireIpc(): void {
  ipcMain.handle('capture:selection', async () => {
    const r = await capturer.captureFromClipboard();
    if (r.ok) clipUnread = null;
    notifyClip();
    return r;
  });
  ipcMain.handle('capture:summary', () => capturer.summary());
  ipcMain.handle(
    'capture:context',
    (_e, opts: { agent: string; turns: number; filePath?: string; sessionId?: string }) =>
      capturer.attachContext(settings.get(), opts),
  );
  ipcMain.handle('capture:clearContext', () => capturer.clearContext());
  ipcMain.handle('pack:current', () => capturer.currentPayload(settings.get()) ?? null);
  ipcMain.handle('pack:copy', () => {
    const ok = capturer.copyToClipboard(settings.get());
    // our own write must not look like a fresh user copy to the watcher (would light the unread dot)
    if (ok) lastClip = clipboard.readText();
    return ok;
  });
  ipcMain.handle('sessions:browse', () => capturer.browse());

  ipcMain.handle('site:open', (_e, url: string) => {
    try {
      const u = new URL(String(url));
      if (u.protocol !== 'https:' && u.protocol !== 'http:') return false;
      void shell.openExternal(url);
      return true;
    } catch {
      return false;
    }
  });

  ipcMain.handle('settings:get', () => settings.get());
  ipcMain.handle('settings:patch', (_e, p) => settings.patch(p));
  ipcMain.handle('win:expand', () => expand());
  ipcMain.handle('win:collapse', () => collapse());
  ipcMain.handle('win:focusSelf', () => {
    if (panelWin.isVisible()) panelWin.focus();
  });
  // panel height follows its content (review #8): renderer reports natural height
  ipcMain.on('win:autoHeight', (_e, h: number) => {
    if (panelWin.isDestroyed() || !panelWin.isVisible()) return;
    const b = panelWin.getBounds();
    const wa = screen.getDisplayNearestPoint({ x: b.x, y: b.y }).workArea;
    const max = wa.y + wa.height - b.y - 8;
    const next = Math.max(240, Math.min(Math.ceil(h), max));
    if (Math.abs(next - b.height) <= 2) return;
    panelHeight = next;
    panelWin.setBounds({ ...b, height: next });
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
      clipUnread = { chars: text.length, firstLine: text.split('\n')[0]?.slice(0, 80) ?? '' };
      notifyClip();
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
