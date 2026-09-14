// 设置页（F2 LLM 配置 + F7.4 通用设置 + F8.7 监控设置 + 脱敏/安全）
import { useEffect, useState } from 'react';
import { getBackend } from '../api/backend';
import { useApp } from '../store';
import { Btn, Field, Modal, Tag } from '../components/ui';
import { IcGear, IcLock, IcPlus } from '../components/icons';
import type { MonitorSettings, Provider } from '../types';

const PRESETS: { name: string; protocol: Provider['protocol']; base_url: string; model_name: string; is_local: boolean }[] = [
  { name: 'OpenAI', protocol: 'openai_compatible', base_url: 'https://api.openai.com/v1', model_name: 'gpt-4o-mini', is_local: false },
  { name: 'Anthropic', protocol: 'anthropic', base_url: 'https://api.anthropic.com', model_name: 'claude-3-5-haiku-latest', is_local: false },
  { name: 'DeepSeek', protocol: 'openai_compatible', base_url: 'https://api.deepseek.com/v1', model_name: 'deepseek-chat', is_local: false },
  { name: 'GLM', protocol: 'openai_compatible', base_url: 'https://open.bigmodel.cn/api/paas/v4', model_name: 'glm-4-flash', is_local: false },
  { name: 'Ollama（本地）', protocol: 'ollama', base_url: 'http://localhost:11434', model_name: 'qwen2.5:7b', is_local: true },
];

const SCENES = [
  { key: 'command_gen', label: '命令生成' },
  { key: 'error_analysis', label: '错误分析' },
  { key: 'metrics_analysis', label: '资源异常分析' },
];

export function SettingsPage() {
  const toast = useApp((s) => s.toast);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [editing, setEditing] = useState<Provider | 'new' | null>(null);
  const [ms, setMs] = useState<MonitorSettings>(useApp.getState().monitorSettings);
  const [routing, setRouting] = useState<Record<string, number>>({});
  const [maskIp, setMaskIp] = useState(true);
  const [localTrusted, setLocalTrusted] = useState(true);

  const reload = async () => {
    const b = getBackend();
    const [ps, s] = await Promise.all([b.listProviders?.() ?? Promise.resolve([] as Provider[]), b.monitorGetSettings()]);
    setProviders(ps);
    setMs(s);
    useApp.getState().setMonitorSettings(s);
  };

  useEffect(() => {
    void reload();
  }, []);

  async function saveMonitor() {
    try {
      await getBackend().monitorUpdateSettings(ms);
      useApp.getState().setMonitorSettings(ms);
      toast('ok', '监控设置已保存');
    } catch (e) {
      toast('err', (e as Error).message);
    }
  }

  return (
    <div className="scroll-area" style={{ padding: 'var(--space-5)', overflow: 'auto', height: '100%' }}>
      <div style={{ maxWidth: 860, margin: '0 auto', display: 'grid', gap: 'var(--space-5)' }}>
        <div>
          <h2 style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
            <IcGear width={20} height={20} style={{ color: 'var(--acc2)' }} /> 设置
          </h2>
          <p style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>凭据 AES-256-GCM 加密存储；LLM 可指向本地 Ollama 全离线可用</p>
        </div>

        {/* ===== LLM Provider ===== */}
        <section className="card pad">
          <div className="pane-title" style={{ padding: 0, border: 'none', marginBottom: 'var(--space-3)' }}>
            <span>LLM Provider（F2）</span>
            <Btn size="sm" onClick={() => setEditing('new')}><IcPlus width={12} height={12} /> 新增</Btn>
          </div>
          {providers.length === 0 && (
            <div style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)', marginBottom: 12 }}>
              尚未配置 Provider——添加一个（或使用预设模板），否则 AI 功能不可用但 SSH/监控不受影响（AC-6）。
            </div>
          )}
          <div style={{ display: 'grid', gap: 8 }}>
            {providers.map((p) => (
              <ProviderRow key={p.id} p={p} onEdit={() => setEditing(p)} onChanged={reload} />
            ))}
          </div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 12 }}>
            {PRESETS.map((pre) => (
              <Tag key={pre.name} kind="neutral" onClick={() => {
                setEditing({ ...pre, api_key: '', temperature: 0.2, max_tokens: 2048, extra_system_prompt: '', enabled: true });
              }} title={`用「${pre.name}」模板新建`}>+ {pre.name}</Tag>
            ))}
          </div>
        </section>

        {/* ===== 场景路由 ===== */}
        <section className="card pad">
          <div className="pane-title" style={{ padding: 0, border: 'none', marginBottom: 'var(--space-3)' }}>
            <span>场景路由与降级（F2.2）</span>
          </div>
          <table className="tbl">
            <thead><tr><th>场景</th><th>模型</th></tr></thead>
            <tbody>
              {SCENES.map((s) => (
                <tr key={s.key}>
                  <td>{s.label}</td>
                  <td>
                    <select value={routing[s.key] ?? ''} onChange={(e) => setRouting({ ...routing, [s.key]: Number(e.target.value) })}>
                      <option value="">（跟随默认）</option>
                      {providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)', marginTop: 8 }}>
            场景模型失败将依次尝试 fallback 列表，全部失败提示「AI 暂不可用」
          </div>
        </section>

        {/* ===== 脱敏 ===== */}
        <section className="card pad">
          <div className="pane-title" style={{ padding: 0, border: 'none', marginBottom: 'var(--space-3)' }}>
            <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}><IcLock width={13} height={13} /> 脱敏（F6.3）</span>
          </div>
          <label className="check-row"><input type="checkbox" checked={maskIp} onChange={(e) => setMaskIp(e.target.checked)} />发送云端前对 IP 中间段打码</label>
          <label className="check-row"><input type="checkbox" checked={localTrusted} onChange={(e) => setLocalTrusted(e.target.checked)} />本地 Provider（localhost/Ollama）默认不脱敏</label>
          <div style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>
            密码、token、私钥内容始终在发送前替换为 [REDACTED]
          </div>
        </section>

        {/* ===== 监控设置 ===== */}
        <section className="card pad">
          <div className="pane-title" style={{ padding: 0, border: 'none', marginBottom: 'var(--space-3)' }}>
            <span>监控与告警（F8.7 / F8.5）</span>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit,minmax(150px,1fr))', gap: 12 }}>
            <Field label="采集间隔（秒）"><input type="number" min={1} max={60} value={ms.interval_sec} onChange={(e) => setMs({ ...ms, interval_sec: Number(e.target.value) })} /></Field>
            <Field label="CPU 告警阈值 %"><input type="number" value={ms.cpu_alert} onChange={(e) => setMs({ ...ms, cpu_alert: Number(e.target.value) })} /></Field>
            <Field label="内存告警阈值 %"><input type="number" value={ms.mem_alert} onChange={(e) => setMs({ ...ms, mem_alert: Number(e.target.value) })} /></Field>
            <Field label="Swap 告警阈值 %"><input type="number" value={ms.swap_alert} onChange={(e) => setMs({ ...ms, swap_alert: Number(e.target.value) })} /></Field>
            <Field label="磁盘告警阈值 %"><input type="number" value={ms.disk_alert} onChange={(e) => setMs({ ...ms, disk_alert: Number(e.target.value) })} /></Field>
          </div>
          <div style={{ marginTop: 8 }}>
            <Btn onClick={saveMonitor}>保存监控设置</Btn>
            <span style={{ marginLeft: 12, color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>CPU 需持续超阈值 60s；恢复 5 分钟后允许再次告警</span>
          </div>
        </section>
      </div>

      {editing && <ProviderModal p={editing === 'new' ? null : editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); void reload(); }} />}
    </div>
  );
}

function ProviderRow({ p, onEdit, onChanged }: { p: Provider; onEdit: () => void; onChanged: () => void }) {
  const toast = useApp((s) => s.toast);
  const [testing, setTesting] = useState(false);
  const [latency, setLatency] = useState<number | null>(null);

  async function test() {
    if (!p.id) return;
    setTesting(true);
    try {
      const r = await getBackend().testProvider(p.id);
      setLatency(r.latency_ms);
      toast(r.ok ? 'ok' : 'err', `延迟 ${r.latency_ms}ms · ${r.reply.slice(0, 40)}`);
    } catch (e) {
      toast('err', (e as Error).message);
    } finally {
      setTesting(false);
    }
  }

  async function toggle() {
    if (!p.id) return;
    await getBackend().saveProvider?.({ ...p, enabled: !p.enabled }, p.id).catch(() => {});
    onChanged();
  }

  return (
    <div className="list-row">
      <Tag kind={p.enabled ? 'ok' : 'neutral'}><i className="dot" />{p.enabled ? '启用' : '停用'}</Tag>
      <div className="lr-main">
        <div className="lr-title">{p.name} · {p.model_name}</div>
        <div className="lr-sub">{p.protocol} · {p.base_url}{p.is_local ? ' · 本地' : ''}</div>
      </div>
      {latency !== null && <Tag kind="info">{latency}ms</Tag>}
      <Btn size="sm" variant="ghost" loading={testing} onClick={test}>测试</Btn>
      <Btn size="sm" variant="ghost" onClick={toggle}>{p.enabled ? '停用' : '启用'}</Btn>
      <Btn size="sm" variant="ghost" onClick={onEdit}>编辑</Btn>
    </div>
  );
}

function ProviderModal({ p, onClose, onSaved }: { p: Provider | null; onClose: () => void; onSaved: () => void }) {
  const toast = useApp((s) => s.toast);
  const [f, setF] = useState<Provider>(
    p ?? { name: '', protocol: 'openai_compatible', base_url: '', api_key: '', model_name: '', is_local: false, temperature: 0.2, max_tokens: 2048, extra_system_prompt: '', enabled: true },
  );
  const [saving, setSaving] = useState(false);
  const set = (k: keyof Provider, v: unknown) => setF((prev) => ({ ...prev, [k]: v }));

  async function save() {
    if (!f.name.trim() || !f.base_url.trim() || !f.model_name.trim()) {
      toast('warn', '名称、base_url、模型名必填');
      return;
    }
    setSaving(true);
    try {
      const b = getBackend();
      if (b.saveProvider) {
        await b.saveProvider(f, p?.id);
      } else {
        toast('warn', '演示模式不支持持久化，请运行桌面端配置');
      }
      toast('ok', p ? '已更新' : '已添加');
      onSaved();
    } catch (e) {
      toast('err', (e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal title={p ? `编辑 ${p.name}` : '新增 Provider'} onClose={onClose} wide>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 14px' }}>
        <Field label="名称"><input value={f.name} onChange={(e) => set('name', e.target.value)} placeholder="如 DeepSeek 主用" /></Field>
        <Field label="协议">
          <select value={f.protocol} onChange={(e) => set('protocol', e.target.value)}>
            <option value="openai_compatible">OpenAI 兼容</option>
            <option value="anthropic">Anthropic</option>
            <option value="ollama">Ollama</option>
          </select>
        </Field>
        <Field label="Base URL"><input value={f.base_url} onChange={(e) => set('base_url', e.target.value)} placeholder="https://api.openai.com/v1" /></Field>
        <Field label="模型名"><input value={f.model_name} onChange={(e) => set('model_name', e.target.value)} placeholder="gpt-4o-mini" /></Field>
        <Field label="API Key" hint="仅加密存储在本机"><input type="password" value={f.api_key ?? ''} onChange={(e) => set('api_key', e.target.value)} /></Field>
        <Field label="Temperature"><input type="number" step={0.1} min={0} max={2} value={f.temperature} onChange={(e) => set('temperature', Number(e.target.value))} /></Field>
        <Field label="Max Tokens"><input type="number" value={f.max_tokens} onChange={(e) => set('max_tokens', Number(e.target.value))} /></Field>
        <Field label="自定义 System Prompt 追加段"><input value={f.extra_system_prompt} onChange={(e) => set('extra_system_prompt', e.target.value)} placeholder="可选" /></Field>
      </div>
      <label className="check-row" style={{ marginTop: 4 }}>
        <input type="checkbox" checked={f.is_local} onChange={(e) => set('is_local', e.target.checked)} />
        本地 Provider（默认不脱敏，可全离线）
      </label>
      <div className="modal-actions">
        <Btn variant="ghost" onClick={onClose}>取消</Btn>
        <Btn onClick={save} loading={saving}>保存</Btn>
      </div>
    </Modal>
  );
}
