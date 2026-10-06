import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { qoderWorkDbPath } from '@select-assist/ctxpack';

export interface SiteTarget {
  name: string;
  url: string;
}

export interface PromptTemplate {
  name: string;
  template: string;
}

/** user-maintained session source; agent accepts an adapter name, 'auto', or 'project' (cwd hint for 自动判定) */
export interface SessionPath {
  agent: string;
  path: string;
}

export interface Settings {
  windowX?: number;
  windowY?: number;
  expanded: boolean;
  /** true = 会话解读 (assemble with transcript), false = 选区直通 (copy raw selection) */
  withContext: boolean;
  chatSites: SiteTarget[];
  directSites: SiteTarget[];
  prompts: PromptTemplate[];
  activePrompt: number;
  sessionPaths: SessionPath[];
  redactPaths: boolean;
  contextTurns: number;
}

export const DEFAULT_PROMPT_TEMPLATE =
  '请根据以下用户与agent的交互记录，解释用户选中的词 / 句子：「{selection}」';

/** the known agent stores, pre-filled into 会话路径 so discovery is settings-driven (delete a line = stop scanning it) */
export function defaultSessionPaths(): SessionPath[] {
  const home = os.homedir();
  return [
    { agent: 'claude-code', path: path.join(home, '.claude', 'projects') },
    { agent: 'codex', path: path.join(home, '.codex', 'sessions') },
    { agent: 'workbuddy', path: path.join(home, '.workbuddy', 'projects') },
    { agent: 'qoder', path: path.join(home, '.qoder-cn', 'projects') },
    { agent: 'qoder', path: qoderWorkDbPath() },
  ];
}

export function activePromptTemplate(s: Settings): string {
  return s.prompts[s.activePrompt]?.template || s.prompts[0]?.template || DEFAULT_PROMPT_TEMPLATE;
}

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
  prompts: [{ name: '解释选区', template: DEFAULT_PROMPT_TEMPLATE }],
  activePrompt: 0,
  sessionPaths: defaultSessionPaths(),
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
      // single template → named prompt set (the user's own template keeps its name slot)
      if (typeof raw.promptTemplate === 'string' && !Array.isArray(raw.prompts)) {
        raw.prompts = [{ name: '解释选区', template: raw.promptTemplate }];
        if (raw.activePrompt === undefined) raw.activePrompt = 0;
        delete raw.promptTemplate;
      }
      // discovery is settings-driven: an absent OR empty list means "never configured" → seed built-ins
      if (!Array.isArray(raw.sessionPaths) || raw.sessionPaths.length === 0) {
        raw.sessionPaths = raw.projectPath
          ? [{ agent: 'project', path: raw.projectPath }, ...defaultSessionPaths()]
          : defaultSessionPaths();
        delete raw.projectPath;
      } else if (typeof raw.projectPath === 'string' && raw.projectPath) {
        // legacy single projectPath becomes a 'project' cwd-hint line ahead of the list
        raw.sessionPaths = [{ agent: 'project', path: raw.projectPath }, ...raw.sessionPaths];
        delete raw.projectPath;
      }
      this.cache = { ...DEFAULTS, ...raw };
      const stale = this.cache as Settings & { sites?: unknown; promptTemplate?: unknown; projectPath?: unknown };
      delete stale.sites;
      delete stale.promptTemplate;
      delete stale.projectPath;
      if (this.cache.activePrompt >= this.cache.prompts.length) this.cache.activePrompt = 0;
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
