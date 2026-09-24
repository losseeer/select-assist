import { contextBridge, ipcRenderer } from 'electron';
import type { IpcRendererEvent } from 'electron';

function on<T>(channel: string, cb: (data: T) => void): () => void {
  const listener = (_e: IpcRendererEvent, data: T) => cb(data);
  ipcRenderer.on(channel, listener);
  return () => ipcRenderer.removeListener(channel, listener);
}

contextBridge.exposeInMainWorld('api', {
  captureSelection: () => ipcRenderer.invoke('capture:selection'),
  captureSummary: () => ipcRenderer.invoke('capture:summary'),
  attachContext: (opts: { agent: string; turns: number; filePath?: string }) =>
    ipcRenderer.invoke('capture:context', opts),
  clearContext: () => ipcRenderer.invoke('capture:clearContext'),
  browseSessions: () => ipcRenderer.invoke('sessions:browse'),
  packCurrent: () => ipcRenderer.invoke('pack:current'),
  copyPack: () => ipcRenderer.invoke('pack:copy'),
  openSite: (url: string) => ipcRenderer.invoke('site:open', url),
  getSettings: () => ipcRenderer.invoke('settings:get'),
  patchSettings: (p: Record<string, unknown>) => ipcRenderer.invoke('settings:patch', p),
  expand: () => ipcRenderer.invoke('win:expand'),
  collapse: () => ipcRenderer.invoke('win:collapse'),
  focusSelf: () => ipcRenderer.invoke('win:focusSelf'),
  quit: () => ipcRenderer.invoke('app:quit'),
  onClipboardNew: (cb: (d: { chars: number; firstLine: string }) => void) =>
    on('clipboard:new', cb),
  onWinShown: (cb: () => void) => on('win:shown', cb),
});
