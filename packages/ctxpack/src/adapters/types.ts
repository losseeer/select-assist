import type { TranscriptTurn } from '../types.js';

export interface SessionRef {
  agent: string;
  adapter: string;
  filePath: string;
  sessionId?: string;
  /** display name when the agent stores one (e.g. Qoder chat title) */
  name?: string;
  /** cwd recorded inside the session file, when available */
  projectPath?: string;
  /** first user message snippet, for the session browser */
  preview?: string;
  mtimeMs: number;
}

export interface TranscriptResult {
  turns: TranscriptTurn[];
  /** privacy defaults dropped regardless of size limits, e.g. "tool-results" */
  dropped: string[];
  /** present when the adapter failed — the caller must degrade to selection-only */
  error?: string;
}

export interface TrackBAdapter {
  agent: string;
  adapter: string;
  /** newest first; filtered/boosted by cwd when given */
  discoverSessions(opts?: { cwd?: string; limit?: number; home?: string }): Promise<SessionRef[]>;
  readTranscript(ref: SessionRef): Promise<TranscriptResult>;
}
