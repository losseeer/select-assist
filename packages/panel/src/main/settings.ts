import * as fs from 'node:fs';
import * as path from 'node:path';

export interface SiteTarget {
  name: string;
  url: string;
}

export interface Settings {
  windowX?: number;
  windowY?: number;
  expanded: boolean;
  /** true = 会话解读 (assemble with transcript), false = 选区直通 (copy raw selection) */
  withContext: boolean;
  chatSites: SiteTarget[];
  directSites: SiteTarget[];
  promptTemplate: string;
  projectPath: string;
  redactPaths: boolean;
  contextTurns: number;
}

export const DEFAULT_PROMPT_TEMPLATE =
  '请根据以下用户与agent的交互记录，解释用户选中的词 / 句子：「{selection}」';

const DEFAULTS: Settings = {
  expanded: false,
  withContext: true,
  chatSites: [
    { name: 'DeepSeek', url: 'https://chat.deepseek.com/' },
    { name: 'ChatGPT', url: 'https://chatgpt.com/' },
    { name: 'Gemini', url: 'https://gemini.google.com/' },
  ],
  directSites: [
    { name: 'Google', url: 'https://www.google.com/' },
    { name: 'Bing', url: 'https://www.bing.com/' },
    { name: 'DeepL', url: 'https://www.deepl.com/translator' },
    { name: '有道', url: 'https://fanyi.youdao.com/#/TextTranslation' },
  ],
  promptTemplate: DEFAULT_PROMPT_TEMPLATE,
  projectPath: '',
  redactPaths: false,
  contextTurns: 8,
};

export class SettingsStore {
  private file: string;
  private cache: Settings;

  constructor(dir: string) {
    this.file = path.join(dir, 'settings.json');
    this.cache = { ...DEFAULTS };
    try {
      const raw = JSON.parse(fs.readFileSync(this.file, 'utf8'));
      // one sites list → per-mode groups (existing user sites belong to 会话解读)
      if (Array.isArray(raw.sites) && !raw.chatSites) {
        raw.chatSites = raw.sites;
        delete raw.sites;
      }
      this.cache = { ...DEFAULTS, ...raw };
      delete (this.cache as Partial<Settings> & { sites?: unknown }).sites;
    } catch {
      /* first run */
    }
  }

  get(): Settings {
    return this.cache;
  }

  patch(partial: Partial<Settings>): Settings {
    this.cache = { ...this.cache, ...partial };
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    fs.writeFileSync(this.file, JSON.stringify(this.cache, null, 2));
    return this.cache;
  }
}
