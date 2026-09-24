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
  sites: SiteTarget[];
  maxChars: number;
  promptTemplate: string;
  projectPath: string;
  redactPaths: boolean;
  contextTurns: number;
}

export const DEFAULT_PROMPT_TEMPLATE =
  '请根据以下用户与agent的交互记录，解释用户选中的词 / 句子：「{selection}」';

const DEFAULTS: Settings = {
  expanded: false,
  sites: [
    { name: 'DeepSeek', url: 'https://chat.deepseek.com/' },
    { name: 'ChatGPT', url: 'https://chatgpt.com/' },
    { name: 'Gemini', url: 'https://gemini.google.com/' },
  ],
  maxChars: 8000,
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
      this.cache = { ...DEFAULTS, ...raw };
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
