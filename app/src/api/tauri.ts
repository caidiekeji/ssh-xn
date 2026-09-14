// Tauri IPC 实现：桥接 Rust 后端（命令签名与 PRD 第 8 节一致；全类型标注）
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Backend, Unsub } from './backend';
import type {
  AlertRecord, CommandResult, DiagnosisResult, Host, HostInput, MemoryCase,
  MetricsSnapshot, MonitorSettings, Provider, SafetyVerdict, SeriesPoint,
} from '../types';

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export class TauriBackend implements Backend {
  isTauri() { return true; }

  // ===== 主机 =====
  listHosts(): Promise<Host[]> { return invoke<Host[]>('list_hosts'); }
  saveHost(input: HostInput, id?: number): Promise<Host> {
    return invoke<Host>('save_host', { input, id }).then(() => ({ ...input, id: id ?? Date.now() } as Host));
  }
  deleteHost(id: number): Promise<void> { return invoke<void>('delete_host', { id }); }
  testHostConnection(id: number): Promise<{ ok: boolean; message: string }> {
    return invoke<{ ok: boolean; message: string }>('test_host_connection', { id });
  }
  sshConnect(hostId: number): Promise<string> { return invoke<string>('ssh_connect', { hostId }); }
  sshClose(sessionId: string): Promise<void> { return invoke<void>('ssh_close', { sessionId }); }
  sshWrite(sessionId: string, data: string): Promise<void> { return invoke<void>('ssh_write', { sessionId, data }); }
  sshResize(sessionId: string, cols: number, rows: number): Promise<void> {
    return invoke<void>('ssh_resize', { sessionId, cols, rows });
  }

  onTerminalOutput(cb: (s: string, d: string) => void): Unsub {
    const p = listen<string>('terminal_output', (e) => {
      const payload = e.payload as unknown as { session_id: string; data: string };
      cb(payload.session_id, payload.data);
    });
    return () => { void p.then((u) => u()); };
  }
  onTerminalExit(cb: (s: string, code: number | null) => void): Unsub {
    const p = listen<string>('terminal_exit', (e) => {
      const payload = e.payload as unknown as { session_id: string; code: number | null };
      cb(payload.session_id, payload.code);
    });
    return () => { void p.then((u) => u()); };
  }
  onErrorDetected(cb: (s: string, snippet: string) => void): Unsub {
    const p = listen<string>('error_detected', (e) => {
      const payload = e.payload as unknown as { session_id: string; snippet: string };
      cb(payload.session_id, payload.snippet);
    });
    return () => { void p.then((u) => u()); };
  }

  // ===== LLM（SSE 流式，前端逐 token 渲染） =====
  listProviders(): Promise<Provider[]> { return invoke<Provider[]>('llm_list_providers'); }
  saveProvider(p: Provider, id?: number): Promise<Provider> {
    return invoke<number>('llm_save_provider', { provider: p, id }).then((nid) => ({ ...p, id: id ?? nid }));
  }
  testProvider(id: number): Promise<{ ok: boolean; latency_ms: number; reply: string }> {
    return invoke<{ ok: boolean; latency_ms: number; reply: string }>('llm_test_provider', { providerId: id });
  }
  generateCommand(sessionId: string, input: string, onChunk: (t: string) => void, signal: AbortSignal): Promise<CommandResult> {
    return this.streamInvoke<CommandResult>('llm_generate_command', { sessionId, userInput: input }, onChunk, signal);
  }
  analyzeError(sessionId: string, text: string, onChunk: (t: string) => void, signal: AbortSignal): Promise<DiagnosisResult> {
    return this.streamInvoke<DiagnosisResult>('llm_analyze_error', { sessionId, errorText: text }, onChunk, signal);
  }
  analyzeAlert(hostId: number, alertId: number, onChunk: (t: string) => void, signal: AbortSignal): Promise<DiagnosisResult> {
    return this.streamInvoke<DiagnosisResult>('llm_analyze_alert', { hostId, alertId }, onChunk, signal);
  }

  private streamInvoke<T>(cmd: string, args: any, onChunk: (t: string) => void, signal: AbortSignal): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      let done = false;
      const unsubs: Unsub[] = [];
      const cleanup = () => {
        if (done) return;
        done = true;
        signal.removeEventListener('abort', abort);
        unsubs.forEach((u) => u());
      };
      const abort = () => { cleanup(); reject(new Error('已取消')); };
      signal.addEventListener('abort', abort, { once: true });

      void listen<string>(`${cmd}_chunk`, (e) => {
        const p = e.payload as unknown as { chunk: string };
        onChunk(p.chunk);
      }).then((un) => unsubs.push(un));

      void listen<string>(`${cmd}_done`, (e) => {
        const p = e.payload as unknown as { result: string };
        cleanup();
        resolve(JSON.parse(p.result) as T);
      }).then((un) => unsubs.push(un));

      void listen<string>(`${cmd}_error`, (e) => {
        const p = e.payload as unknown as { message: string };
        cleanup();
        reject(new Error(p.message));
      }).then((un) => unsubs.push(un));

      void invoke(cmd, args).catch((err) => {
        cleanup();
        reject(err);
      });
    });
  }

  // ===== 记忆 =====
  memorySearch(hostId: number | null, query: string): Promise<MemoryCase[]> {
    return invoke<MemoryCase[]>('memory_search', { hostId, query });
  }
  memorySave(case_: MemoryCase): Promise<number> { return invoke<number>('memory_save', { case: case_ }); }
  memoryList(filter: any): Promise<MemoryCase[]> { return invoke<MemoryCase[]>('memory_list', { filter }); }
  memoryUpdate(id: number, patch: Partial<MemoryCase>): Promise<void> { return invoke<void>('memory_update', { id, patch }); }
  memoryDelete(id: number): Promise<void> { return invoke<void>('memory_delete', { id }); }
  memoryFeedback(id: number, up: boolean): Promise<void> { return invoke<void>('memory_feedback', { id, up }); }
  memoryImportMarkdown(text: string): Promise<number> { return invoke<number>('memory_import_markdown', { text }); }
  memoryExportMarkdown(id: number): Promise<string> { return invoke<string>('memory_export_markdown', { id }); }

  // ===== 监控 =====
  monitorStart(hostId: number): Promise<void> { return invoke<void>('monitor_start', { hostId }); }
  monitorStop(hostId: number): Promise<void> { return invoke<void>('monitor_stop', { hostId }); }
  monitorGetSnapshot(hostId: number): Promise<MetricsSnapshot | null> {
    return invoke<MetricsSnapshot>('monitor_get_snapshot', { hostId }).catch(() => null);
  }
  monitorGetHistory(hostId: number, metric: string, range: string): Promise<SeriesPoint[]> {
    return invoke<SeriesPoint[]>('monitor_get_history', { hostId, metric, range });
  }
  monitorListAlerts(filter: { host_id?: number; only_active?: boolean }): Promise<AlertRecord[]> {
    return invoke<AlertRecord[]>('monitor_list_alerts', { filter });
  }
  monitorMuteAlert(id: number, durationSecs?: number): Promise<void> {
    return invoke<void>('monitor_mute_alert', { id, durationSecs });
  }
  monitorGetSettings(): Promise<MonitorSettings> { return invoke<MonitorSettings>('monitor_get_settings'); }
  monitorUpdateSettings(s: MonitorSettings): Promise<void> { return invoke<void>('monitor_update_settings', { settings: s }); }
  onMonitorMetrics(cb: (s: MetricsSnapshot) => void): Unsub {
    const p = listen<MetricsSnapshot>('monitor_metrics', (e) => cb(e.payload));
    return () => { void p.then((u) => u()); };
  }
  onMonitorAlert(cb: (a: AlertRecord) => void): Unsub {
    const p = listen<AlertRecord>('monitor_alert', (e) => cb(e.payload));
    return () => { void p.then((u) => u()); };
  }

  // ===== 安全/审计 =====
  safetyCheck(command: string): Promise<SafetyVerdict> { return invoke<SafetyVerdict>('safety_check', { command }); }
  auditExportCsv(): Promise<string> { return invoke<string>('audit_export_csv'); }

  // ===== 执行 =====
  async executeCommand(sessionId: string, command: string, options?: {
    audit?: { user_input?: string; risk?: string };
    confirmed?: boolean;
  }): Promise<number | null> {
    const r = await invoke<{ exit_code: number | null }>('ssh_execute', {
      sessionId, command, audit: options?.audit ?? null, confirmed: options?.confirmed ?? false,
    });
    return r?.exit_code ?? null;
  }
  verifyAndSave(sessionId: string, hostId: number, case_: {
    description: string; error_snippet?: string; solution_cmd: string; verify_cmd?: string;
    rollback_cmd?: string; root_cause?: string; keywords?: string;
  }): Promise<{ verified: boolean; saved: boolean; id?: number }> {
    return invoke<{ verified: boolean; saved: boolean; id?: number }>('memory_auto_save', { sessionId, hostId, case: case_ });
  }

  // ===== SFTP =====
  sftpList(sessionId: string, path: string): Promise<{ name: string; is_dir: boolean; size: number }[]> {
    return invoke<{ name: string; is_dir: boolean; size: number }[]>('sftp_list', { sessionId, path });
  }
}
