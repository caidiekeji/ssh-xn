// 资源监控面板（F8.3）：概览卡 + 实时折线（ECharts 增量更新）+ 磁盘 + 进程 Top + 告警
import { useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import * as echarts from 'echarts';
import { useApp } from '../store';
import { Btn, EmptyState, Tag } from './ui';
import { IcAi, IcCpu, IcMem, IcNet, IcSignal, IcWave } from './icons';
import type { MetricsSnapshot } from '../types';

// 轻量 ECharts 封装：增量 setOption，禁止全量重建实例（实现注意事项 #15）
function useChart(option: (el: HTMLElement) => echarts.EChartsOption | null, deps: unknown[]) {
  const ref = useRef<HTMLDivElement>(null);
  const inst = useRef<echarts.ECharts | null>(null);
  useEffect(() => {
    if (!ref.current) return;
    if (!inst.current) {
      inst.current = echarts.init(ref.current);
    }
    const opt = option(ref.current);
    if (opt) inst.current.setOption(opt, { notMerge: false });
    return () => {
      // 组件卸载时才销毁
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(() => {
    return () => {
      inst.current?.dispose();
      inst.current = null;
    };
  }, []);
  return ref;
}

const AXIS = {
  axisLine: { lineStyle: { color: 'rgba(179,168,152,0.25)' } },
  axisLabel: { color: 'var(--faint)', fontSize: 10 },
  splitLine: { lineStyle: { color: 'rgba(59,51,42,0.5)' } },
};

export function MonitorPanel() {
  const activeSessionId = useApp((s) => s.activeSessionId);
  const sessionMeta = useApp((s) => s.sessionMeta);
  const snapshots = useApp((s) => s.snapshots);
  const alerts = useApp((s) => s.alerts);
  const hostId = activeSessionId ? sessionMeta[activeSessionId]?.hostId : undefined;
  const snap = hostId ? snapshots[hostId] : undefined;
  const host = useApp((s) => s.hosts.find((h) => h.id === hostId));

  // 最近 5 分钟滑动窗口（3s 间隔 ≈ 100 点）
  const [window_, setWindow_] = useState<MetricsSnapshot[]>([]);
  useEffect(() => {
    if (!snap) return;
    setWindow_((w) => {
      const next = [...w, snap];
      while (next.length > 100) next.shift();
      return next;
    });
  }, [snap?.timestamp]);

  const ts = window_.map((s) => s.timestamp);
  const cpu = window_.map((s) => s.cpu.total_pct);
  const mem = window_.map((s) => s.memory.pct);
  const rx = window_.map((s) => (s.network[0]?.rx_bps ?? 0) / 1024);
  const tx = window_.map((s) => (s.network[0]?.tx_bps ?? 0) / 1024);

  const cpuRef = useChart(() => lineOption('CPU %', ts, cpu, '#4D8DFF'), [window_.length]);
  const memRef = useChart(() => lineOption('内存 %', ts, mem, '#22D48C'), [window_.length]);
  const netRef = useChart(() => lineOption('网络 KB/s', ts, [rx, tx], ['#4D8DFF', '#A855F7']), [window_.length]);

  const [procTab, setProcTab] = useState<'cpu' | 'mem'>('cpu');
  const hostAlerts = useMemo(() => alerts.filter((a) => a.host_id === hostId).slice(0, 5), [alerts, hostId]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <div className="pane-title" style={{ justifyContent: 'flex-start', gap: 8 }}>
        <IcWave width={14} height={14} style={{ color: 'var(--acc2)' }} />
        实时监控 · {host?.name ?? hostId ?? '未连接'}
        <span style={{ flex: 1 }} />
        {snap && <Tag kind="ok"><i className="dot" /> {fmtUptime(snap.uptime_sec)}</Tag>}
        {hostAlerts.filter((a) => !a.recovered_at).length > 0 && (
          <Tag kind="err"><i className="dot" /> {hostAlerts.filter((a) => !a.recovered_at).length} 告警中</Tag>
        )}
      </div>

      {!hostId || !snap ? (
        <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <EmptyState icon={<IcSignal width={36} height={36} />} title="未在监控" desc="连接主机后自动开始采集（默认 3s）" />
        </div>
      ) : (
        <div className="scroll-area" style={{ padding: 'var(--space-3)' }}>
          {/* 概览卡片行 */}
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(110px, 1fr))', gap: 8, marginBottom: 8 }}>
            <StatCard icon={<IcCpu width={14} height={14} />} label="CPU" value={`${snap.cpu.total_pct.toFixed(1)}%`} danger={snap.cpu.total_pct > 90} sub={`负载 ${snap.load.load1.toFixed(2)}`} />
            <StatCard icon={<IcMem width={14} height={14} />} label="内存" value={`${snap.memory.pct.toFixed(1)}%`} danger={snap.memory.pct > 90} sub={`${(snap.memory.used_mb / 1024).toFixed(1)}/${(snap.memory.total_mb / 1024).toFixed(0)}G`} />
            <StatCard icon={<IcMem width={14} height={14} />} label="Swap" value={`${snap.memory.swap_pct.toFixed(1)}%`} danger={snap.memory.swap_pct > 80} sub={`${(snap.memory.swap_used_mb / 1024).toFixed(1)}G`} />
            <StatCard icon={<IcNet width={14} height={14} />} label="网络" value={`${fmtRate(snap.network[0]?.rx_bps ?? 0)} ↓`} sub={`${fmtRate(snap.network[0]?.tx_bps ?? 0)} ↑`} />
            <StatCard icon={<IcWave width={14} height={14} />} label="核心" value={`${snap.cpu.cores}`} sub={`iowait ${snap.cpu.iowait_pct.toFixed(1)}%`} />
          </div>

          {/* 实时折线 */}
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 8, marginBottom: 8 }}>
            <MiniChart ref_={cpuRef} title="CPU" />
            <MiniChart ref_={memRef} title="内存" />
            <MiniChart ref_={netRef} title="网络" />
          </div>

          {/* 磁盘 + 进程 */}
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
            <div className="card pad" style={{ padding: 'var(--space-3)' }}>
              <div className="pane-title" style={{ padding: 0, border: 'none', marginBottom: 6 }}>磁盘使用</div>
              {snap.disks.map((d) => (
                <div key={d.mount} style={{ marginBottom: 8 }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 'var(--fs-caption)', marginBottom: 3 }}>
                    <span style={{ color: 'var(--sub)' }}>{d.mount}</span>
                    <span style={{ color: d.pct > 95 ? 'var(--err)' : d.pct > 85 ? 'var(--warn)' : 'var(--faint)' }}>
                      {d.pct.toFixed(1)}% · {(d.used_gb).toFixed(0)}/{d.total_gb.toFixed(0)}G
                    </span>
                  </div>
                  <div className="bar"><i className={d.pct > 95 ? 'err' : d.pct > 85 ? 'warn' : ''} style={{ width: `${Math.min(100, d.pct)}%` }} /></div>
                </div>
              ))}
              <DiskAIButton hostId={hostId} snap={snap} />
            </div>

            <div className="card pad" style={{ padding: 'var(--space-3)' }}>
              <div className="pane-title" style={{ padding: 0, border: 'none', marginBottom: 6, display: 'flex', gap: 8 }}>
                <button className={`btn ghost sm ${procTab === 'cpu' ? 'on' : ''}`} onClick={() => setProcTab('cpu')}>按 CPU</button>
                <button className={`btn ghost sm ${procTab === 'mem' ? 'on' : ''}`} onClick={() => setProcTab('mem')}>按内存</button>
              </div>
              <table className="tbl">
                <thead><tr><th>PID</th><th>用户</th><th>CPU%</th><th>MEM%</th><th>命令</th></tr></thead>
                <tbody>
                  {(procTab === 'cpu' ? snap.processes.by_cpu : snap.processes.by_mem).map((p) => (
                    <tr key={`${p.pid}-${p.command}`}>
                      <td><button className="link-like" onClick={() => window.dispatchEvent(new CustomEvent('ai-ssh:analyze-process', { detail: { pid: p.pid, hostId } }))} style={{ background: 'none', border: 'none', color: 'var(--acc)', padding: 0, fontSize: 'inherit' }}>{p.pid}</button></td>
                      <td>{p.user}</td>
                      <td>{p.cpu_pct.toFixed(1)}</td>
                      <td>{p.mem_pct.toFixed(1)}</td>
                      <td style={{ maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{p.command}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          {/* 最近告警 */}
          {hostAlerts.length > 0 && (
            <div style={{ marginTop: 8 }}>
              {hostAlerts.map((a) => (
                <div key={a.id} className="list-row" style={{ border: '1px solid rgba(229,72,77,.35)', borderRadius: 'var(--radius-md)', marginBottom: 4, background: 'rgba(229,72,77,.06)' }}>
                  <Tag kind={a.recovered_at ? 'ok' : 'err'}>{a.alert_type.toUpperCase()}</Tag>
                  <div className="lr-main">
                    <div className="lr-title">{a.value.toFixed(1)}% &gt; {a.threshold}% 阈值</div>
                    <div className="lr-sub">{a.triggered_at}{a.recovered_at ? ` · ${a.recovered_at} 恢复` : ' · 进行中'}{a.muted ? ' · 已静音' : ''}</div>
                  </div>
                  {!a.recovered_at && (
                    <Btn size="sm" onClick={() => window.dispatchEvent(new CustomEvent('ai-ssh:analyze-alert', { detail: { alertId: a.id, hostId } }))}>
                      <IcAi width={12} height={12} /> AI 分析
                    </Btn>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function DiskAIButton({ hostId, snap }: { hostId: number; snap: MetricsSnapshot }) {
  const diskMax = snap.disks.reduce((m, d) => Math.max(m, d.pct), 0);
  return (
    <Btn size="sm" variant="ghost" style={{ marginTop: 4 }} onClick={() => window.dispatchEvent(new CustomEvent('ai-ssh:analyze-alert', { detail: { alertId: 0, hostId } }))} disabled={diskMax < 80}>
      <IcAi width={12} height={12} /> 磁盘异常 AI 诊断
    </Btn>
  );
}

function StatCard({ icon, label, value, sub, danger }: { icon: React.ReactNode; label: string; value: string; sub: string; danger?: boolean }) {
  return (
    <div className="card pad" style={{ padding: 'var(--space-2) var(--space-3)', borderColor: danger ? 'var(--err)' : 'var(--line)', display: 'grid', gap: 2 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, color: 'var(--faint)', fontSize: 'var(--fs-caption)', letterSpacing: '0.05em' }}>{icon}{label}</div>
      <div style={{ fontSize: 'var(--fs-display)', lineHeight: 1.1, fontWeight: 600, color: danger ? 'var(--err)' : 'var(--ink)', fontFamily: 'var(--font-display)' }}>{value}</div>
      <div style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>{sub}</div>
    </div>
  );
}

function MiniChart({ ref_, title }: { ref_: RefObject<HTMLDivElement>; title: string }) {
  return (
    <div className="card pad" style={{ padding: 'var(--space-2) var(--space-3)' }}>
      <div style={{ fontSize: 'var(--fs-caption)', color: 'var(--faint)', letterSpacing: '0.05em', marginBottom: 2 }}>{title}</div>
      <div ref={ref_} style={{ height: 64 }} />
    </div>
  );
}

function lineOption(title: string, ts: number[], series: (number[] | number)[], colors: string | string[]): echarts.EChartsOption {
  const data = Array.isArray(series[0]) ? series as number[][] : [series as number[]];
  const names = title === '网络 KB/s' ? ['收', '发'] : data.map((_, i) => `${title}${i + 1}`);
  const colorArr = Array.isArray(colors) ? colors : [colors];
  return {
    grid: { left: 4, right: 4, top: 6, bottom: 4, containLabel: true },
    tooltip: { trigger: 'axis', backgroundColor: 'var(--paper)', borderColor: 'var(--line)', textStyle: { color: 'var(--ink)', fontSize: 10 } },
    xAxis: { type: 'category', show: false, data: ts.map((t) => new Date(t).toLocaleTimeString()) },
    yAxis: { type: 'value', show: false, ...AXIS },
    series: data.map((d, i) => ({
      name: names[i],
      type: 'line',
      data: d,
      smooth: true,
      symbol: 'none',
      lineStyle: { width: 1.5, color: colorArr[i % colorArr.length] },
      areaStyle: { opacity: 0.08 },
      animation: false,
    })),
  } as echarts.EChartsOption;
}

function fmtRate(bps: number): string {
  if (bps >= 1024 * 1024) return `${(bps / 1024 / 1024).toFixed(1)}M/s`;
  if (bps >= 1024) return `${(bps / 1024).toFixed(0)}K/s`;
  return `${bps.toFixed(0)}B/s`;
}

function fmtUptime(sec: number): string {
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  return d > 0 ? `运行 ${d}天${h}时` : `运行 ${h}时${m}分`;
}
