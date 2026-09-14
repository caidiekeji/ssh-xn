import { create } from 'zustand';
import { getBackend } from './api/backend';
import type {
  AlertRecord, ChatMsg, Host, MetricsSnapshot, MonitorSettings, ToastMsg,
} from './types';

let toastSeq = 0;

interface SessionMeta {
  hostId: number;
  connected: boolean;
  error?: string;
  errorSnippet?: string;
}

interface AppState {
  ready: boolean;
  view: 'workspace' | 'history' | 'settings';
  hosts: Host[];
  tabs: string[];
  activeSessionId: string | null;
  sessionMeta: Record<string, SessionMeta>;
  snapshots: Record<number, MetricsSnapshot>;
  alerts: AlertRecord[];
  monitorSettings: MonitorSettings;
  aiBySession: Record<string, ChatMsg[]>;
  aiBusyBySession: Record<string, boolean>;
  toasts: ToastMsg[];

  setView(v: AppState['view']): void;
  init(): Promise<void>;
  setHosts(hosts: Host[]): void;
  connectHost(hostId: number): Promise<void>;
  closeTab(sessionId: string): Promise<void>;
  activateTab(sessionId: string): void;
  setSessionError(sessionId: string, snippet: string): void;
  clearSessionError(sessionId: string): void;
  pushSnapshot(s: MetricsSnapshot): void;
  pushAlert(a: AlertRecord): void;
  setMonitorSettings(s: MonitorSettings): void;
  pushAiMsg(sessionId: string, m: ChatMsg): void;
  patchAiMsg(sessionId: string, id: string, patch: Partial<ChatMsg>): void;
  appendAiChunk(sessionId: string, id: string, chunk: string): void;
  setAiBusy(sessionId: string, busy: boolean): void;
  resetAi(sessionId: string): void;
  toast(kind: ToastMsg['kind'], text: string): void;
  dismissToast(id: number): void;
}

export const useApp = create<AppState>((set, get) => ({
  ready: false,
  view: 'workspace',
  hosts: [],
  tabs: [],
  activeSessionId: null,
  sessionMeta: {},
  snapshots: {},
  alerts: [],
  monitorSettings: { interval_sec: 3, cpu_alert: 90, mem_alert: 90, swap_alert: 80, disk_alert: 95, notification_mode: 'app' },
  aiBySession: {},
  aiBusyBySession: {},
  toasts: [],

  setView(v) { set({ view: v }); },

  setHosts(hosts) { set({ hosts }); },

  async init() {
    const b = getBackend();
    const [hosts, alerts, ms] = await Promise.all([
      b.listHosts(),
      b.monitorListAlerts({}).catch(() => []),
      b.monitorGetSettings().catch(() => get().monitorSettings),
    ]);
    set({ hosts, alerts, monitorSettings: ms, ready: true });
    // 全局事件：监控推送
    b.onMonitorMetrics((s) => get().pushSnapshot(s));
    b.onMonitorAlert((a) => get().pushAlert(a));
    // 报错检测（F4.2）：非侵入提示条，不自动调用 LLM
    b.onErrorDetected((sessionId, snippet) => {
      get().setSessionError(sessionId, snippet);
    });
  },

  async connectHost(hostId) {
    const b = getBackend();
    try {
      const sessionId = await b.sshConnect(hostId);
      const host = get().hosts.find((h) => h.id === hostId);
      set((s) => ({
        tabs: [...s.tabs, sessionId],
        activeSessionId: sessionId,
        sessionMeta: { ...s.sessionMeta, [sessionId]: { hostId, connected: true } },
        hosts: s.hosts.map((h) => (h.id === hostId ? { ...h, connected: true } : h)),
        aiBySession: { ...s.aiBySession, [sessionId]: [] },
        aiBusyBySession: { ...s.aiBusyBySession, [sessionId]: false },
      }));
      // 监控联动（F8.1）：连接后自动启动
      const hostMeta = host ?? { monitor_enabled: true } as Host;
      if (hostMeta.monitor_enabled) {
        void b.monitorStart(hostId).catch(() => {});
      }
      get().toast('ok', `已连接 ${host?.name ?? hostId}`);
    } catch (e) {
      get().toast('err', `连接失败: ${(e as Error).message}`);
    }
  },

  async closeTab(sessionId) {
    const b = getBackend();
    void b.sshClose(sessionId).catch(() => {});
    const meta = get().sessionMeta[sessionId];
    if (meta) {
      void b.monitorStop(meta.hostId).catch(() => {});
    }
    set((s) => {
      const tabs = s.tabs.filter((t) => t !== sessionId);
      const activeSessionId = s.activeSessionId === sessionId ? (tabs[tabs.length - 1] ?? null) : s.activeSessionId;
      const sessionMeta = { ...s.sessionMeta };
      delete sessionMeta[sessionId];
      const aiBySession = { ...s.aiBySession };
      delete aiBySession[sessionId];
      const aiBusyBySession = { ...s.aiBusyBySession };
      delete aiBusyBySession[sessionId];
      const hosts = s.hosts.map((h) =>
        h.id === meta?.hostId ? { ...h, connected: false } : h,
      );
      return { tabs, activeSessionId, sessionMeta, aiBySession, aiBusyBySession, hosts };
    });
  },

  activateTab(sessionId) { set({ activeSessionId: sessionId }); },

  setSessionError(sessionId, snippet) {
    set((s) => ({
      sessionMeta: { ...s.sessionMeta, [sessionId]: { ...(s.sessionMeta[sessionId] ?? { hostId: 0, connected: true }), errorSnippet: snippet } },
    }));
  },

  clearSessionError(sessionId) {
    set((s) => ({
      sessionMeta: s.sessionMeta[sessionId]
        ? { ...s.sessionMeta, [sessionId]: { ...s.sessionMeta[sessionId], errorSnippet: undefined } }
        : s.sessionMeta,
    }));
  },

  pushSnapshot(s) {
    set((st) => ({ snapshots: { ...st.snapshots, [s.host_id]: s } }));
  },

  pushAlert(a) {
    set((st) => ({
      alerts: [a, ...st.alerts.filter((x) => !(x.host_id === a.host_id && x.alert_type === a.alert_type && !x.recovered_at))].slice(0, 200),
    }));
  },

  setMonitorSettings(s) { set({ monitorSettings: s }); },

  pushAiMsg(sessionId, m) {
    set((s) => ({ aiBySession: { ...s.aiBySession, [sessionId]: [...(s.aiBySession[sessionId] ?? []), m] } }));
  },
  patchAiMsg(sessionId, id, patch) {
    set((s) => ({
      aiBySession: {
        ...s.aiBySession,
        [sessionId]: (s.aiBySession[sessionId] ?? []).map((m) => (m.id === id ? { ...m, ...patch } : m)),
      },
    }));
  },
  appendAiChunk(sessionId, id, chunk) {
    set((s) => ({
      aiBySession: {
        ...s.aiBySession,
        [sessionId]: (s.aiBySession[sessionId] ?? []).map((m) => (m.id === id ? { ...m, content: m.content + chunk } : m)),
      },
    }));
  },
  setAiBusy(sessionId, busy) {
    set((s) => ({ aiBusyBySession: { ...s.aiBusyBySession, [sessionId]: busy } }));
  },
  resetAi(sessionId) { set((s) => ({ aiBySession: { ...s.aiBySession, [sessionId]: [] } })); },

  toast(kind, text) {
    const id = ++toastSeq;
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    setTimeout(() => get().dismissToast(id), 4200);
  },
  dismissToast(id) { set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })); },
}));
