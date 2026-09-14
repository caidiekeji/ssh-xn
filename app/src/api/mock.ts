// 浏览器 Mock 后端：无 Tauri/SSH 时模拟全流程，用于界面预览与联调
import type { Backend, Unsub } from './backend';
import type {
  AlertRecord, CommandResult, DiagnosisResult, Host, MemoryCase, MetricsSnapshot,
  MonitorSettings, Provider, SafetyVerdict, SeriesPoint,
} from '../types';

const DEMO_HOSTS: Host[] = [
  { id: 1, name: 'web-01', host: '10.0.0.11', port: 22, username: 'root', auth_type: 'password', memory_enabled: true, monitor_enabled: true, connected: false, group_name: '生产', tags: 'nginx,prod' },
  { id: 2, name: 'db-01', host: '10.0.0.12', port: 22, username: 'deploy', auth_type: 'private_key', key_path: '~/.ssh/id_ed25519', memory_enabled: true, monitor_enabled: true, connected: false, group_name: '生产', tags: 'mysql,prod' },
  { id: 3, name: 'dev-box', host: '192.168.1.20', port: 2222, username: 'dev', auth_type: 'password', memory_enabled: false, monitor_enabled: true, connected: false, group_name: '开发', tags: 'dev' },
];

const DEMO_MEMORY: MemoryCase[] = [
  {
    id: 1, host_id: 1, description: 'nginx 502 Bad Gateway（磁盘满导致日志写不进去）', keywords: 'nginx 502 disk',
    root_cause: '磁盘使用率 100%，/var/log/nginx 无法写入', solution_cmd: 'rm /var/log/nginx/*.log\nsystemctl restart nginx',
    verify_cmd: 'curl -I http://localhost', hit_count: 12, verified: true, source: 'ai_resolved',
    created_at: '2026-09-01 10:00:00', problem_type: 'web',
  },
  {
    id: 2, host_id: 2, description: 'MySQL 连接数耗尽 (Too many connections)', keywords: 'mysql connections pool',
    root_cause: 'max_connections 过低且存在未释放连接', solution_cmd: 'SET GLOBAL max_connections=500;',
    verify_cmd: 'SHOW STATUS LIKE "Threads_connected";', hit_count: 8, verified: true, source: 'user_manual',
    created_at: '2026-08-20 14:00:00', problem_type: 'database',
  },
  {
    id: 3, host_id: 1, description: '端口 80 被占用', keywords: 'port 80 occupied',
    root_cause: '残留 nginx 进程', solution_cmd: 'ss -ltnp | grep :80\nkill $(ss -ltnp | grep :80 | grep -oP "pid=\\K\\d+")',
    verify_cmd: 'ss -ltn | grep :80', hit_count: 5, verified: false, source: 'user_manual',
    created_at: '2026-09-10 09:00:00', problem_type: 'network',
  },
];

function makeSnapshot(hostId: number, t: number, prev?: MetricsSnapshot): MetricsSnapshot {
  const walk = (base: number, amp: number) => base + (Math.random() - 0.5) * amp;
  const cpu = prev ? Math.min(99, Math.max(2, walk(prev.cpu.total_pct, 14))) : 23 + Math.random() * 10;
  const mem = prev ? Math.min(98, Math.max(10, walk(prev.memory.pct, 3))) : 62 + Math.random() * 6;
  const swap = prev ? Math.min(60, Math.max(0, walk(prev.memory.swap_pct, 2))) : 4;
  const disk = 74 + Math.random() * 4;
  const rx = (prev ? Math.max(0, walk(prev.network[0]?.rx_bps ?? 120000, 60000)) : 120000 + Math.random() * 40000);
  const tx = (prev ? Math.max(0, walk(prev.network[0]?.tx_bps ?? 30000, 20000)) : 30000 + Math.random() * 15000);
  return {
    host_id: hostId,
    timestamp: t,
    cpu: { total_pct: cpu, user_pct: cpu * 0.7, sys_pct: cpu * 0.2, iowait_pct: cpu * 0.1, cores: 8 },
    load: { load1: cpu / 12, load5: cpu / 14, load15: cpu / 16 },
    memory: { total_mb: 16384, used_mb: (mem / 100) * 16384, available_mb: 16384 - (mem / 100) * 16384, pct: mem, buffers_cached_mb: 2048, swap_total_mb: 4096, swap_used_mb: (swap / 100) * 4096, swap_pct: swap },
    disks: [
      { mount: '/', total_gb: 100, used_gb: (disk / 100) * 100, pct: disk },
      { mount: '/data', total_gb: 500, used_gb: 312, pct: 62.4 },
    ],
    network: [
      { iface: 'eth0', rx_bps: rx, tx_bps: tx, rx_bytes_total: 2 ** 32 * 3, tx_bytes_total: 2 ** 32, errors: 0, dropped: Math.floor(Math.random() * 3) },
    ],
    processes: {
      by_cpu: [
        { pid: 4123, user: 'www', cpu_pct: 31.2, mem_pct: 12.4, command: 'node /srv/app/server.js' },
        { pid: 873, user: 'root', cpu_pct: 9.1, mem_pct: 1.2, command: 'nginx: worker process' },
        { pid: 2145, user: 'mysql', cpu_pct: 5.0, mem_pct: 28.6, command: 'mysqld --datadir=/var/lib/mysql' },
        { pid: 1, user: 'root', cpu_pct: 0.3, mem_pct: 0.4, command: 'systemd' },
      ],
      by_mem: [],
    },
    uptime_sec: 864000 + Math.floor(Math.random() * 1000),
  };
}

export class MockBackend implements Backend {
  private hosts: Host[] = DEMO_HOSTS.map((h) => ({ ...h }));
  private memory: MemoryCase[] = DEMO_MEMORY.map((c) => ({ ...c }));
  private snapshots = new Map<number, MetricsSnapshot>();
  private monitorTimers = new Map<number, ReturnType<typeof setInterval>>();
  private monitorCallbacks = new Set<(s: MetricsSnapshot) => void>();
  private alertCallbacks = new Set<(a: AlertRecord) => void>();
  private alerts: AlertRecord[] = [
    { id: 1, host_id: 1, alert_type: 'cpu', value: 96.2, threshold: 90, triggered_at: '2026-09-14 02:11:00', muted: false },
    { id: 2, host_id: 2, alert_type: 'disk', value: 96.8, threshold: 95, triggered_at: '2026-09-13 23:40:00', recovered_at: '2026-09-14 00:12:00', muted: false },
  ];
  private sessions = new Set<string>();
  private termCallbacks = new Set<(s: string, d: string) => void>();
  private errCallbacks = new Set<(s: string, snippet: string) => void>();
  private settings: MonitorSettings = { interval_sec: 3, cpu_alert: 90, mem_alert: 90, swap_alert: 80, disk_alert: 95, notification_mode: 'app' };

  isTauri() { return false; }

  // ===== 主机 =====
  async listHosts() { return this.hosts.map((h) => ({ ...h })); }
  async saveHost(input: any, id?: number) {
    if (id) {
      const h = this.hosts.find((x) => x.id === id);
      if (!h) throw new Error('主机不存在');
      Object.assign(h, input, { id });
      return { ...h };
    }
    const h: Host = { id: Date.now(), ...input } as Host;
    this.hosts.push(h);
    return { ...h };
  }
  async deleteHost(id: number) { this.hosts = this.hosts.filter((h) => h.id !== id); }
  async testHostConnection(id: number) {
    const h = this.hosts.find((x) => x.id === id);
    if (!h) return { ok: false, message: '主机不存在' };
    await delay(600);
    return { ok: true, message: `已连通 ${h.host}:${h.port}（演示模式）` };
  }
  async sshConnect(hostId: number) {
    const h = this.hosts.find((x) => x.id === hostId);
    if (!h) throw new Error('主机不存在');
    h.connected = true;
    const sid = `demo-${hostId}-${Date.now()}`;
    this.sessions.add(sid);
    await delay(500);
    this.emitTerm(sid, `\x1b[32m连接到 ${h.host}\x1b[0m\r\nLast login: Sun Sep 14 02:00:00 2026 from 10.0.0.1\r\n`);
    this.emitTerm(sid, `\x1b[36m${h.username}@${h.name}\x1b[0m:~$ `);
    return sid;
  }
  async sshClose(sessionId: string) { this.sessions.delete(sessionId); }
  async sshWrite(sessionId: string, data: string) {
    if (data === '\r') return;
    if (data === '\u0003') { this.emitTerm(sessionId, '^C\r\n'); return; }
    // 简化：按行执行
    const line = data.replace(/[\r\n]/g, '');
    if (line.trim()) this.runDemoCommand(sessionId, line);
  }
  async sshResize(_s: string, _c: number, _r: number) {}

  private runDemoCommand(sessionId: string, cmd: string) {
    this.emitTerm(sessionId, `${cmd}\r\n`);
    const lower = cmd.toLowerCase();
    const outputs: Record<string, string> = {
      ls: 'app.log  config.yml  deploy.sh  logs  src\r\n',
      pwd: '/home/deploy\r\n',
      'ss -ltnp': 'State   Recv-Q  Send-Q  Local Address:Port   Peer Address:Port   Process\r\nLISTEN  0       511     0.0.0.0:80              0.0.0.0:*        users:(("nginx",pid=873,fd=6))\r\nLISTEN  0       128     0.0.0.0:3306            0.0.0.0:*        users:(("mysqld",pid=2145,fd=22))\r\n',
      'df -h': 'Filesystem      Size  Used Avail Use% Mounted on\r\n/dev/sda1        98G   74G   19G  80% /\r\n/dev/sdb1       492G  312G  155G  67% /data\r\n',
      free: '              total        used        free      shared  buff/cache   available\r\nMem:           15Gi        9.8Gi       1.2Gi       120Mi       4.0Gi       4.9Gi\r\nSwap:          4.0Gi        96Mi       3.9Gi\r\n',
      uptime: ' 02:15:00 up 10 days,  0:30,  1 user,  load average: 1.92, 1.71, 1.60\r\n',
      whoami: 'root\r\n',
      uname: 'Linux web-01 6.6.95 #1 SMP x86_64 GNU/Linux\r\n',
    };
    const key = Object.keys(outputs).find((k) => lower.startsWith(k));
    const out = key ? outputs[key] : this.fakeOutput(cmd);
    setTimeout(() => {
      this.emitTerm(sessionId, out);
      // 报错检测（F4.2 规则层）
      if (/error|failed|fatal|panic|exception|denied|refused|traceback|segmentation fault/i.test(out)) {
        const snippet = out;
        this.errCallbacks.forEach((cb) => cb(sessionId, snippet));
      }
      this.emitTerm(sessionId, '\x1b[36mroot@demo\x1b[0m:~$ ');
    }, 400 + Math.random() * 300);
  }

  private fakeOutput(cmd: string): string {
    if (/systemctl restart nginx/.test(cmd)) return 'Job for nginx.service started.\r\n';
    if (/curl/.test(cmd)) return 'HTTP/1.1 200 OK\r\n';
    return `bash: ${cmd}: 演示环境不执行真实命令\r\n`;
  }

  private emitTerm(s: string, d: string) {
    this.termCallbacks.forEach((cb) => cb(s, d));
  }

  onTerminalOutput(cb: (s: string, d: string) => void): Unsub {
    this.termCallbacks.add(cb);
    return () => this.termCallbacks.delete(cb);
  }
  onTerminalExit(_cb: (s: string, code: number | null) => void): Unsub {
    // mock 不主动断线
    return () => {};
  }
  onErrorDetected(cb: (s: string, snippet: string) => void): Unsub {
    this.errCallbacks.add(cb);
    return () => this.errCallbacks.delete(cb);
  }

  // ===== LLM =====
  private providers: Provider[] = [
    { id: 1, name: 'Ollama（本地）', protocol: 'ollama', base_url: 'http://localhost:11434', api_key: '', model_name: 'qwen2.5:7b', is_local: true, temperature: 0.2, max_tokens: 2048, extra_system_prompt: '', enabled: true },
    { id: 2, name: 'DeepSeek', protocol: 'openai_compatible', base_url: 'https://api.deepseek.com/v1', api_key: 'sk-demo', model_name: 'deepseek-chat', is_local: false, temperature: 0.2, max_tokens: 2048, extra_system_prompt: '', enabled: true },
  ];
  async listProviders() { return this.providers.map((p) => ({ ...p, api_key: p.api_key ? '••••' : '' })); }
  async saveProvider(p: Provider, id?: number) {
    if (id) {
      const i = this.providers.findIndex((x) => x.id === id);
      if (i >= 0) this.providers[i] = { ...p, id };
      return { ...this.providers[i] };
    }
    const np = { ...p, id: Date.now() };
    this.providers.push(np);
    return np;
  }
  async testProvider(_id: number) {
    await delay(700);
    return { ok: true, latency_ms: 320 + Math.floor(Math.random() * 200), reply: 'pong（演示模型）' };
  }

  async generateCommand(_s: string, input: string, onChunk: (t: string) => void, signal: AbortSignal): Promise<CommandResult> {
    await delay(300);
    const result = this.buildCommandResult(input);
    await streamJson(result, onChunk, signal, 40);
    return result;
  }

  private buildCommandResult(input: string): CommandResult {
    const i = input.toLowerCase();
    if (i.includes('端口') || i.includes('80')) {
      return {
        commands: ['ss -ltnp | grep :80'], explanation: ['查看 80 端口监听进程（-p 显示 pid）'],
        risk: 'low', risk_reason: '', notes: '无需 sudo 即可查看监听信息',
      };
    }
    if (i.includes('磁盘') || i.includes('空间')) {
      return {
        commands: ['df -h', 'du -sh /var/log/* 2>/dev/null | sort -rh | head -5'],
        explanation: ['查看各挂载点使用率', '定位日志目录中占用最大的文件'],
        risk: 'low', risk_reason: '', notes: '',
      };
    }
    if (i.includes('内存')) {
      return {
        commands: ['free -h', 'ps -eo pid,user,pmem,comm --sort=-pmem | head -8'],
        explanation: ['查看内存总量与可用量', '列出内存占用最高的前 8 个进程'],
        risk: 'low', risk_reason: '', notes: '',
      };
    }
    if (i.includes('重启') || i.includes('restart')) {
      return {
        commands: ['systemctl restart nginx'],
        explanation: ['重启 nginx 服务'],
        risk: 'medium', risk_reason: '会中断正在进行的请求，建议在低峰期执行', notes: '重启后可用 curl -I 验证',
      };
    }
    return {
      commands: ['ls -la'],
      explanation: ['列出当前目录所有文件（含隐藏文件）'],
      risk: 'low', risk_reason: '', notes: '',
    };
  }

  async analyzeError(_s: string, _text: string, onChunk: (t: string) => void, signal: AbortSignal): Promise<DiagnosisResult> {
    await delay(300);
    const d: DiagnosisResult = {
      diagnosis: '目标服务未监听该端口，连接被拒绝。',
      root_cause: 'nginx 未启动，或启动后崩溃（常见：配置文件错误、端口被占用、磁盘满）。',
      fix_steps: [
        { cmd: 'systemctl status nginx', desc: '查看 nginx 运行状态' },
        { cmd: 'journalctl -u nginx --no-pager -n 30', desc: '查看最近 30 行日志定位原因' },
        { cmd: 'ss -ltnp | grep :80', desc: '确认端口是否被其他进程占用' },
      ],
      verify_cmd: 'curl -I http://localhost',
      rollback_cmd: '',
      ref_memory: ['1'],
    };
    await streamJson(d, onChunk, signal, 40);
    return d;
  }

  async analyzeAlert(_h: number, _a: number, onChunk: (t: string) => void, signal: AbortSignal): Promise<DiagnosisResult> {
    return this.analyzeError('', 'CPU 使用率持续超过 90%', onChunk, signal);
  }

  // ===== 记忆 =====
  async memorySearch(hostId: number | null, query: string) {
    const q = query.toLowerCase();
    return this.memory
      .filter((c) => (hostId ? c.host_id === hostId : true))
      .filter((c) => !q || (c.description + ' ' + (c.keywords ?? '')).toLowerCase().includes(q))
      .slice(0, 3);
  }
  async memorySave(case_: MemoryCase) {
    if (case_.id) {
      const i = this.memory.findIndex((c) => c.id === case_.id);
      if (i >= 0) this.memory[i] = { ...case_ };
      return case_.id;
    }
    const id = Date.now();
    this.memory.unshift({ ...case_, id, created_at: new Date().toISOString().slice(0, 19).replace('T', ' ') });
    return id;
  }
  async memoryList(filter: any) {
    return this.memory.filter((c) => {
      if (filter?.host_id && c.host_id !== filter.host_id) return false;
      if (filter?.search && !(c.description + (c.keywords ?? '')).toLowerCase().includes(filter.search.toLowerCase())) return false;
      if (filter?.verified !== undefined && c.verified !== filter.verified) return false;
      return true;
    });
  }
  async memoryUpdate(id: number, patch: any) {
    const c = this.memory.find((x) => x.id === id);
    if (c) Object.assign(c, patch);
  }
  async memoryDelete(id: number) { this.memory = this.memory.filter((c) => c.id !== id); }
  async memoryFeedback(id: number, up: boolean) {
    const c = this.memory.find((x) => x.id === id);
    if (c) c.hit_count = Math.max(0, c.hit_count + (up ? 2 : -1));
  }
  async memoryImportMarkdown(text: string) {
    const count = (text.match(/## 问题/g) || []).length || 1;
    return count;
  }
  async memoryExportMarkdown(id: number) {
    const c = this.memory.find((x) => x.id === id);
    if (!c) return '';
    return `---
type: ai-ssh-memory
tags: [${(c.keywords ?? '').split(' ').join(', ')}]
---
## 问题
${c.description}
## 根因
${c.root_cause ?? ''}
## 解决
\`\`\`bash
${c.solution_cmd ?? ''}
\`\`\`
验证
\`\`\`bash
${c.verify_cmd ?? ''}
\`\`\`
`;
  }

  // ===== 监控 =====
  async monitorStart(hostId: number) {
    const emit = () => {
      const prev = this.snapshots.get(hostId);
      const s = makeSnapshot(hostId, Date.now(), prev);
      this.snapshots.set(hostId, s);
      this.monitorCallbacks.forEach((cb) => cb(s));
    };
    emit();
    const timer = setInterval(emit, this.settings.interval_sec * 1000);
    this.monitorTimers.set(hostId, timer);
  }
  async monitorStop(hostId: number) {
    const t = this.monitorTimers.get(hostId);
    if (t) clearInterval(t);
    this.monitorTimers.delete(hostId);
  }
  async monitorGetSnapshot(hostId: number) {
    return this.snapshots.get(hostId) ?? makeSnapshot(hostId, Date.now());
  }
  async monitorGetHistory(_hostId: number, metric: string, range: string): Promise<SeriesPoint[]> {
    const points: SeriesPoint[] = [];
    const step = range === '1h' ? 10 : range === '24h' ? 15 : range === '7d' ? 120 : 600;
    const count = range === '1h' ? 12 : range === '24h' ? 48 : range === '7d' ? 84 : 72;
    const now = Date.now() / 1000;
    let base = metric === 'mem' ? 62 : metric === 'load1' ? 1.7 : metric === 'net_rx' ? 120000 : 30;
    for (let i = count; i >= 0; i--) {
      base += (Math.random() - 0.48) * (metric === 'net_rx' ? 30000 : metric === 'load1' ? 0.4 : 6);
      base = Math.max(2, base);
      points.push({ ts: now - i * step, avg: base, max: base * 1.08 });
    }
    return points;
  }
  async monitorListAlerts(filter: any) {
    return this.alerts.filter((a) => {
      if (filter?.host_id && a.host_id !== filter.host_id) return false;
      if (filter?.only_active && a.recovered_at) return false;
      return true;
    });
  }
  async monitorMuteAlert(id: number) {
    const a = this.alerts.find((x) => x.id === id);
    if (a) a.muted = true;
  }
  async monitorGetSettings() { return { ...this.settings }; }
  async monitorUpdateSettings(s: MonitorSettings) { this.settings = { ...s }; }
  onMonitorMetrics(cb: (s: MetricsSnapshot) => void): Unsub {
    this.monitorCallbacks.add(cb);
    return () => this.monitorCallbacks.delete(cb);
  }
  onMonitorAlert(cb: (a: AlertRecord) => void): Unsub {
    this.alertCallbacks.add(cb);
    return () => this.alertCallbacks.delete(cb);
  }

  // ===== 安全 =====
  async safetyCheck(command: string): Promise<SafetyVerdict> {
    const c = command.trim();
    const reasons: string[] = [];
    let level: 'low' | 'medium' | 'high' = 'low';
    if (/rm\s+(-[a-z]*r[a-z]*f|-fr|-rf)\s+/.test(c) && /(\/|\/\*|etc|boot|root)\s*$/.test(c)) {
      level = 'high'; reasons.push('递归强制删除根目录或系统关键目录');
    } else if (/rm\s+-r/.test(c) || /chmod\s+777/.test(c) || /kill\s+-9/.test(c) || /systemctl\s+stop/.test(c)) {
      level = 'medium'; reasons.push('删除/权限/强杀/停服类操作');
    }
    return { level, reasons, matched_rules: [] };
  }
  async auditExportCsv() {
    return 'id,created_at,host_id,user_input,generated_cmd,risk_level,executed\n1,2026-09-14 02:00:00,1,查看磁盘,df -h,low,1\n';
  }

  // ===== 执行 =====
  async executeCommand(sessionId: string, command: string, options?: any): Promise<number | null> {
    // 与后端 ssh_execute 同契约：medium/high 必须 confirmed
    const v = await this.safetyCheck(command);
    if ((v.level === 'medium' || v.level === 'high') && !options?.confirmed) {
      throw new Error(`命令需确认后执行：${v.reasons.join('; ')}`);
    }
    this.emitTerm(sessionId, `\r\n\x1b[33m$ ${command}\x1b[0m\r\n`);
    this.runDemoCommand(sessionId, command);
    return 0;
  }
  async verifyAndSave(_sessionId: string, hostId: number, case_: any) {
    // mock：验证命令直接视为 exit 0
    await delay(500);
    await this.memorySave({
      description: case_.description,
      error_snippet: case_.error_snippet,
      solution_cmd: case_.solution_cmd,
      verify_cmd: case_.verify_cmd,
      rollback_cmd: case_.rollback_cmd,
      root_cause: case_.root_cause,
      keywords: case_.keywords,
      host_id: hostId,
      hit_count: 0,
      verified: true,
      source: 'ai_resolved',
    } as MemoryCase);
    return { verified: true, saved: true, id: Date.now() };
  }

  // ===== SFTP =====
  async sftpList(_s: string, path: string) {
    if (path === '/') {
      return [
        { name: 'etc', is_dir: true, size: 4096 },
        { name: 'home', is_dir: true, size: 4096 },
        { name: 'var', is_dir: true, size: 4096 },
        { name: 'deploy.sh', is_dir: false, size: 2048 },
      ];
    }
    return [{ name: '..', is_dir: true, size: 0 }, { name: 'config.yml', is_dir: false, size: 512 }];
  }
}

async function streamJson<T>(obj: T, onChunk: (t: string) => void, signal: AbortSignal, stepMs: number) {
  const json = JSON.stringify(obj);
  for (let i = 0; i < json.length; i += 24) {
    if (signal.aborted) throw new Error('已取消');
    onChunk(json.slice(i, i + 24));
    await delay(stepMs);
  }
}

function delay(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}
