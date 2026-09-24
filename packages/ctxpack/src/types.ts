export const PACK_FORMAT = 'ctxpack/0';

export type CaptureVia = 'page-selection' | 'clipboard';

export interface Capture {
  via: CaptureVia;
  /** ISO 8601, UTC recommended */
  at: string;
}

export interface Source {
  agent?: string;
  app?: string;
  sessionId?: string;
  projectPath?: string;
  adapter?: string;
}

export type AnchorKind = 'event' | 'message' | 'ambiguous' | 'none';

export interface Anchor {
  kind: AnchorKind;
  seq?: number;
  messageId?: string;
}

export interface Selection {
  text: string;
  role?: 'user' | 'assistant' | 'unknown';
  anchor?: Anchor;
}

export interface TranscriptTurn {
  role: 'user' | 'assistant';
  text: string;
  seq?: number;
}

export interface Limits {
  maxChars?: number;
  usedChars?: number;
  /** e.g. "tool-results", "turns:0-31" — never truncate silently */
  dropped?: string[];
}

export interface CtxPack {
  pack: string;
  generatedAt: string;
  capture: Capture;
  source?: Source;
  selection?: Selection;
  transcript?: TranscriptTurn[];
  limits?: Limits;
  payload?: string;
  [key: string]: unknown;
}
