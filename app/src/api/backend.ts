// 后端能力接口：Tauri 实现 ↔ 浏览器 Mock 实现共用同一契约
import type {
  AlertRecord, CommandResult, DiagnosisResult, Host, HostInput, MemoryCase,
  MetricsSnapshot, MonitorSettings, Provider, SafetyVerdict, SeriesPoint,
} from '../types';

export type Unsub = () => void;

export interface Backend {
  isTauri(): boolean;

  // ===== 主机 =====
  listHosts(): Promise<Host[]>;
  saveHost(input: HostInput, id?: number): Promise<Host>;
  deleteHost(id: number): Promise<void>;
  testHostConnection(id: number): Promise<{ ok: boolean; message: string }>;
  sshConnect(hostId: number): Promise<string>; // -> sessionId
  sshClose(sessionId: string): Promise<void>;
  sshWrite(sessionId: string, data: string): Promise<void>;
  sshResize(sessionId: string, cols: number, rows: number): Promise<void>;
  onTerminalOutput(cb: (sessionId: string, data: string) => void): Unsub;
  onTerminalExit(cb: (sessionId: string, code: number | null) => void): Unsub;
  onErrorDetected(cb: (sessionId: string, snippet: string) => void): Unsub;

  // ===== LLM =====
  listProviders(): Promise<Provider[]>;
  saveProvider(p: Provider, id?: number): Promise<Provider>;
  testProvider(id: number): Promise<{ ok: boolean; latency_ms: number; reply: string }>;
  generateCommand(sessionId: string, input: string, onChunk: (t: string) => void, signal: AbortSignal): Promise<CommandResult>;
  analyzeError(sessionId: string, text: string, onChunk: (t: string) => void, signal: AbortSignal): Promise<DiagnosisResult>;
  analyzeAlert(hostId: number, alertId: number, onChunk: (t: string) => void, signal: AbortSignal): Promise<DiagnosisResult>;

  // ===== 历史记忆 =====
  memorySearch(hostId: number | null, query: string): Promise<MemoryCase[]>;
  memorySave(case_: MemoryCase): Promise<number>;
  memoryList(filter: { host_id?: number; problem_type?: string; verified?: boolean; search?: string; limit?: number; offset?: number }): Promise<MemoryCase[]>;
  memoryUpdate(id: number, patch: Partial<MemoryCase>): Promise<void>;
  memoryDelete(id: number): Promise<void>;
  memoryFeedback(id: number, up: boolean): Promise<void>;
  memoryImportMarkdown(text: string): Promise<number>;
  memoryExportMarkdown(id: number): Promise<string>;

  // ===== 监控 =====
  monitorStart(hostId: number): Promise<void>;
  monitorStop(hostId: number): Promise<void>;
  monitorGetSnapshot(hostId: number): Promise<MetricsSnapshot | null>;
  monitorGetHistory(hostId: number, metric: string, range: string): Promise<SeriesPoint[]>;
  monitorListAlerts(filter: { host_id?: number; only_active?: boolean }): Promise<AlertRecord[]>;
  monitorMuteAlert(id: number, durationSecs?: number): Promise<void>;
  monitorGetSettings(): Promise<MonitorSettings>;
  monitorUpdateSettings(s: MonitorSettings): Promise<void>;
  onMonitorMetrics(cb: (snap: MetricsSnapshot) => void): Unsub;
  onMonitorAlert(cb: (alert: AlertRecord) => void): Unsub;

  // ===== 安全/审计 =====
  safetyCheck(command: string): Promise<SafetyVerdict>;
  auditExportCsv(): Promise<string>;

  // ===== 执行 =====
  executeCommand(sessionId: string, command: string, options?: {
    audit?: { user_input?: string; risk?: string };
    confirmed?: boolean;
  }): Promise<number | null>;
  verifyAndSave(sessionId: string, hostId: number, case_: {
    description: string; error_snippet?: string; solution_cmd: string; verify_cmd?: string;
    rollback_cmd?: string; root_cause?: string; keywords?: string;
  }): Promise<{ verified: boolean; saved: boolean; id?: number }>;

  // ===== SFTP（F1.4） =====
  sftpList(sessionId: string, path: string): Promise<{ name: string; is_dir: boolean; size: number }[]>;
}

let backend: Backend | null = null;

export function setBackend(b: Backend) {
  backend = b;
}

export function getBackend(): Backend {
  if (!backend) throw new Error('backend 未初始化');
  return backend;
}
