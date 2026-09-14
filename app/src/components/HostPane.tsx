// 左侧主机面板（F1 主机 CRUD + 连接）+ 主机编辑弹窗
import { useState } from 'react';
import { getBackend } from '../api/backend';
import { useApp } from '../store';
import { Btn, Field, Modal, Tag } from './ui';
import { IcEdit, IcPlus, IcShield, IcTerminal, IcTrash } from './icons';
import type { Host, HostInput } from '../types';

export function HostPane() {
  const hosts = useApp((s) => s.hosts);
  const tabs = useApp((s) => s.tabs);
  const activeSessionId = useApp((s) => s.activeSessionId);
  const sessionMeta = useApp((s) => s.sessionMeta);
  const connectHost = useApp((s) => s.connectHost);
  const closeTab = useApp((s) => s.closeTab);
  const activateTab = useApp((s) => s.activateTab);
  const toast = useApp((s) => s.toast);
  const [editing, setEditing] = useState<Host | 'new' | null>(null);

  const groups = new Map<string, Host[]>();
  for (const h of hosts) {
    const g = h.group_name || '默认';
    groups.set(g, [...(groups.get(g) ?? []), h]);
  }

  async function remove(h: Host) {
    const b = getBackend();
    if (!window.confirm(`删除主机「${h.name}」？凭据将一并删除`)) return;
    try {
      await b.deleteHost(h.id);
      toast('ok', '已删除');
      // 若正在连接则断开
      const sid = Object.entries(sessionMeta).find(([, m]) => m.hostId === h.id)?.[0];
      if (sid) await closeTab(sid);
      window.dispatchEvent(new Event('ai-ssh:hosts-changed'));
    } catch (e) {
      toast('err', (e as Error).message);
    }
  }

  return (
    <div className="host-pane">
      <div className="pane-title">
        <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <IcShield width={13} height={13} style={{ color: 'var(--acc2)' }} /> 主机
        </span>
        <Btn size="sm" onClick={() => setEditing('new')} title="新增主机">
          <IcPlus width={12} height={12} /> 新建
        </Btn>
      </div>

      <div className="scroll-area">
        {hosts.length === 0 && (
          <div style={{ padding: 'var(--space-4)', color: 'var(--faint)', fontSize: 'var(--fs-caption)', textAlign: 'center' }}>
            还没有主机，点右上角新建
          </div>
        )}
        {[...groups.entries()].map(([g, list]) => (
          <div key={g}>
            <div style={{ padding: '6px 12px 2px', color: 'var(--faint)', fontSize: '0.68rem', letterSpacing: '0.08em', textTransform: 'uppercase' }}>{g}</div>
            {list.map((h) => {
              const sid = Object.entries(sessionMeta).find(([, m]) => m.hostId === h.id)?.[0];
              const on = sid === activeSessionId;
              return (
                <div key={h.id} className={`host-card ${on ? 'on' : ''}`} onClick={() => (sid ? activateTab(sid) : void connectHost(h.id))}>
                  <div className="hc-top">
                    <Tag kind={h.connected ? 'ok' : 'neutral'}><i className="dot" />{h.connected ? '已连接' : '离线'}</Tag>
                    <span className="hc-name" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{h.name}</span>
                  </div>
                  <div className="hc-meta">{h.username}@{h.host}:{h.port}</div>
                  <div className="hc-actions">
                    <Btn size="sm" variant="ghost" title="编辑" onClick={(e) => { e?.stopPropagation(); setEditing(h); }}>
                      <IcEdit width={12} height={12} />
                    </Btn>
                    <Btn size="sm" variant="ghost" title="删除" onClick={(e) => { e?.stopPropagation(); void remove(h); }}>
                      <IcTrash width={12} height={12} />
                    </Btn>
                  </div>
                </div>
              );
            })}
          </div>
        ))}
      </div>

      {/* 标签页条 */}
      {tabs.length > 0 && (
        <div style={{ borderTop: '1px solid var(--line)', padding: 6, display: 'flex', gap: 4, flexWrap: 'wrap' }}>
          {tabs.map((sid) => {
            const meta = sessionMeta[sid];
            const name = hosts.find((h) => h.id === meta?.hostId)?.name ?? sid.slice(0, 8);
            return (
              <span
                key={sid}
                className={`tag neutral clickable ${sid === activeSessionId ? 'info' : ''}`}
                onClick={() => activateTab(sid)}
                title="点击切换 · 右键关闭"
                onContextMenu={(e) => { e.preventDefault(); void closeTab(sid); }}
              >
                <IcTerminal width={11} height={11} />
                {name}
              </span>
            );
          })}
        </div>
      )}

      {editing && <HostModal host={editing === 'new' ? null : editing} onClose={() => setEditing(null)} />}
    </div>
  );
}

export function HostModal({ host, onClose }: { host: Host | null; onClose: () => void }) {
  const toast = useApp((s) => s.toast);
  const [f, setF] = useState<HostInput>({
    name: host?.name ?? '',
    host: host?.host ?? '',
    port: host?.port ?? 22,
    username: host?.username ?? 'root',
    auth_type: host?.auth_type ?? 'password',
    secret: '',
    key_path: host?.key_path ?? '',
    passphrase: '',
    jump_host_id: null,
    group_name: host?.group_name ?? '',
    tags: host?.tags ?? '',
    notes: host?.notes ?? '',
    memory_enabled: host?.memory_enabled ?? true,
    monitor_enabled: host?.monitor_enabled ?? true,
  });
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string>('');
  const set = (k: keyof HostInput, v: unknown) => setF((p) => ({ ...p, [k]: v }));

  async function save() {
    if (!f.name.trim() || !f.host.trim() || !f.username.trim()) {
      toast('warn', '名称、地址、用户名必填');
      return;
    }
    const b = getBackend();
    setSaving(true);
    try {
      const saved = await b.saveHost(f, host?.id);
      toast('ok', host ? '已更新' : `已创建 ${saved.name}`);
      window.dispatchEvent(new Event('ai-ssh:hosts-changed'));
      onClose();
    } catch (e) {
      toast('err', (e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  async function test() {
    if (!host) return toast('warn', '请先保存主机');
    setTesting(true);
    setTestResult('');
    try {
      const r = await getBackend().testHostConnection(host.id);
      setTestResult(r.ok ? `✓ ${r.message}` : `✕ ${r.message}`);
      toast(r.ok ? 'ok' : 'err', r.message);
    } catch (e) {
      setTestResult(`✕ ${(e as Error).message}`);
    } finally {
      setTesting(false);
    }
  }

  return (
    <Modal title={host ? `编辑 ${host.name}` : '新建主机'} onClose={onClose} wide>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 14px' }}>
        <Field label="名称"><input value={f.name} onChange={(e) => set('name', e.target.value)} placeholder="如 web-01" /></Field>
        <Field label="地址"><input value={f.host} onChange={(e) => set('host', e.target.value)} placeholder="10.0.0.11" /></Field>
        <Field label="端口"><input type="number" value={f.port} onChange={(e) => set('port', Number(e.target.value))} /></Field>
        <Field label="用户名"><input value={f.username} onChange={(e) => set('username', e.target.value)} placeholder="root" /></Field>
        <Field label="认证方式">
          <select value={f.auth_type} onChange={(e) => set('auth_type', e.target.value)}>
            <option value="password">密码</option>
            <option value="private_key">私钥</option>
          </select>
        </Field>
        {f.auth_type === 'password' ? (
          <Field label="密码" hint="AES-256-GCM 加密存储"><input type="password" value={f.secret} onChange={(e) => set('secret', e.target.value)} placeholder={host ? '留空保持不变' : ''} /></Field>
        ) : (
          <>
            <Field label="私钥路径"><input value={f.key_path ?? ''} onChange={(e) => set('key_path', e.target.value)} placeholder="~/.ssh/id_ed25519" /></Field>
            <Field label="Passphrase（可选）"><input type="password" value={f.passphrase ?? ''} onChange={(e) => set('passphrase', e.target.value)} /></Field>
          </>
        )}
        <Field label="跳板机 ID（可选）"><input type="number" value={f.jump_host_id ?? ''} onChange={(e) => set('jump_host_id', e.target.value ? Number(e.target.value) : null)} placeholder="ProxyJump 主机 id" /></Field>
        <Field label="分组"><input value={f.group_name ?? ''} onChange={(e) => set('group_name', e.target.value)} placeholder="生产 / 开发" /></Field>
        <Field label="标签"><input value={f.tags ?? ''} onChange={(e) => set('tags', e.target.value)} placeholder="nginx,prod" /></Field>
        <Field label="备注"><input value={f.notes ?? ''} onChange={(e) => set('notes', e.target.value)} /></Field>
      </div>

      <div style={{ display: 'flex', gap: 'var(--space-4)', marginTop: 4 }}>
        <label className="check-row"><input type="checkbox" checked={f.memory_enabled} onChange={(e) => set('memory_enabled', e.target.checked)} />启用历史记录（F5）</label>
        <label className="check-row"><input type="checkbox" checked={f.monitor_enabled} onChange={(e) => set('monitor_enabled', e.target.checked)} />启用资源监控（F8）</label>
      </div>

      {testResult && (
        <div style={{ marginTop: 8, fontSize: 'var(--fs-caption)', color: testResult.startsWith('✓') ? 'var(--ok)' : 'var(--err)' }}>{testResult}</div>
      )}

      <div className="modal-actions">
        <Btn variant="ghost" onClick={onClose}>取消</Btn>
        {host && <Btn variant="ghost" loading={testing} onClick={test}>测试连接</Btn>}
        <Btn onClick={save} loading={saving}>保存</Btn>
      </div>
    </Modal>
  );
}
