// 与 Rust core schema 对齐的前端类型

export type RiskLevel = 'low' | 'medium' | 'high';

export interface CommandResult {
  commands: string[];
  explanation: string[];
  risk: RiskLevel;
  risk_reason: string;
  notes: string;
}

export interface FixStep {
  cmd: string;
  desc: string;
}

export interface DiagnosisResult {
  diagnosis: string;
  root_cause: string;
  fix_steps: FixStep[];
  verify_cmd: string;
  rollback_cmd: string;
  ref_memory: string[];
}

export interface SafetyVerdict {
  level: RiskLevel;
  reasons: string[];
  matched_rules: string[];
}

export interface Host {
  id: number;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: 'password' | 'private_key';
  key_path?: string | null;
  jump_host_id?: number | null;
  group_name?: string | null;
  tags?: string | null;
  notes?: string | null;
  memory_enabled: boolean;
  monitor_enabled: boolean;
  connected?: boolean;
}

export interface HostInput {
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: 'password' | 'private_key';
  secret?: string;
  key_path?: string;
  passphrase?: string;
  jump_host_id?: number | null;
  group_name?: string;
  tags?: string;
  notes?: string;
  memory_enabled: boolean;
  monitor_enabled: boolean;
}

export type LlmProtocol = 'openai_compatible' | 'anthropic' | 'ollama';

export interface Provider {
  id?: number;
  name: string;
  protocol: LlmProtocol;
  base_url: string;
  api_key?: string;
  model_name: string;
  is_local: boolean;
  temperature: number;
  max_tokens: number;
  extra_system_prompt: string;
  enabled: boolean;
}

export interface MemoryCase {
  id?: number;
  created_at?: string;
  updated_at?: string;
  host_id?: number | null;
  os_info?: string | null;
  problem_type?: string | null;
  error_snippet?: string | null;
  description: string;
  keywords?: string | null;
  root_cause?: string | null;
  solution_cmd?: string | null;
  solution_text?: string | null;
  verify_cmd?: string | null;
  rollback_cmd?: string | null;
  hit_count: number;
  verified: boolean;
  source: 'ai_resolved' | 'user_manual' | 'imported';
  failed_for?: string | null;
}

export interface CpuMetrics {
  total_pct: number;
  user_pct: number;
  sys_pct: number;
  iowait_pct: number;
  cores: number;
}
export interface LoadMetrics { load1: number; load5: number; load15: number }
export interface MemMetrics {
  total_mb: number; used_mb: number; available_mb: number; pct: number;
  buffers_cached_mb: number; swap_total_mb: number; swap_used_mb: number; swap_pct: number;
}
export interface DiskMetrics { mount: string; total_gb: number; used_gb: number; pct: number }
export interface NetMetrics {
  iface: string; rx_bps: number; tx_bps: number;
  rx_bytes_total: number; tx_bytes_total: number; errors: number; dropped: number;
}
export interface ProcMetrics { pid: number; user: string; cpu_pct: number; mem_pct: number; command: string }

export interface MetricsSnapshot {
  host_id: number;
  timestamp: number;
  cpu: CpuMetrics;
  load: LoadMetrics;
  memory: MemMetrics;
  disks: DiskMetrics[];
  network: NetMetrics[];
  processes: { by_cpu: ProcMetrics[]; by_mem: ProcMetrics[] };
  uptime_sec: number;
}

export interface AlertRecord {
  id: number;
  host_id: number;
  alert_type: 'cpu' | 'memory' | 'swap' | 'disk';
  value: number;
  threshold: number;
  triggered_at: string;
  recovered_at?: string | null;
  muted: boolean;
  analysis_log?: string | null;
}

export interface SeriesPoint { ts: number; avg: number; max: number }

export interface ChatMsg {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  kind: 'text' | 'command' | 'diagnosis';
  command?: CommandResult;
  diagnosis?: DiagnosisResult;
  streaming?: boolean;
  refMemory?: boolean;
  error?: string;
}

export interface ToastMsg {
  id: number;
  kind: 'ok' | 'warn' | 'err' | 'info';
  text: string;
}

export interface MonitorSettings {
  interval_sec: number;
  cpu_alert: number;
  mem_alert: number;
  swap_alert: number;
  disk_alert: number;
  notification_mode: string;
}
