// 历史记录管理页（F5.7）：列表 / 筛选 / 详情编辑 / Markdown 导入导出 / 命中反馈
import { useEffect, useState } from 'react';
import { getBackend } from '../api/backend';
import { useApp } from '../store';
import { Btn, EmptyState, Field, Modal, Tag } from '../components/ui';
import { IcDownload, IcEye, IcImport, IcMemory, IcSearch, IcThumbsDown, IcThumbsUp, IcTrash } from '../components/icons';
import type { MemoryCase } from '../types';

export function HistoryPage() {
  const hosts = useApp((s) => s.hosts);
  const toast = useApp((s) => s.toast);
  const [cases, setCases] = useState<MemoryCase[]>([]);
  const [hostFilter, setHostFilter] = useState<number | ''>('');
  const [typeFilter, setTypeFilter] = useState('');
  const [verFilter, setVerFilter] = useState<'' | 'true' | 'false'>('');
  const [search, setSearch] = useState('');
  const [detail, setDetail] = useState<MemoryCase | null>(null);
  const [importing, setImporting] = useState(false);
  const [importText, setImportText] = useState('');

  const reload = async () => {
    const b = getBackend();
    const list = await b.memoryList({
      host_id: hostFilter === '' ? undefined : hostFilter,
      problem_type: typeFilter || undefined,
      verified: verFilter === '' ? undefined : verFilter === 'true',
      search: search || undefined,
      limit: 200,
    });
    setCases(list);
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostFilter, typeFilter, verFilter, search]);

  async function doFeedback(id: number, up: boolean) {
    await getBackend().memoryFeedback(id, up);
    toast(up ? 'ok' : 'warn', up ? '👍 +2' : '👎 -1');
    void reload();
  }

  async function doDelete(id: number) {
    if (!window.confirm('删除该历史记录？')) return;
    await getBackend().memoryDelete(id);
    toast('ok', '已删除');
    void reload();
  }

  async function doExport(c: MemoryCase) {
    if (!c.id) return;
    const md = await getBackend().memoryExportMarkdown(c.id);
    const blob = new Blob([md], { type: 'text/markdown' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `ai-ssh-memory-${c.id}.md`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function doImport() {
    if (!importText.trim()) return;
    setImporting(true);
    try {
      const n = await getBackend().memoryImportMarkdown(importText);
      toast('ok', `导入 ${n} 条记录`);
      setImportText('');
      void reload();
    } catch (e) {
      toast('err', (e as Error).message);
    } finally {
      setImporting(false);
    }
  }

  return (
    <div className="scroll-area" style={{ padding: 'var(--space-5)', overflow: 'auto', height: '100%' }}>
      <div style={{ maxWidth: 960, margin: '0 auto' }}>
        <h2 style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
          <IcMemory width={22} height={22} style={{ color: 'var(--acc2)' }} /> 历史问题记录
        </h2>
        <p style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)', marginBottom: 'var(--space-4)' }}>
          本地 SQLite 保存；AI 排查时可检索同主机历史 Top3 注入 Prompt
        </p>

        {/* 筛选行 */}
        <div style={{ display: 'flex', gap: 8, marginBottom: 'var(--space-4)', flexWrap: 'wrap', alignItems: 'center' }}>
          <Field label="">
            <div style={{ position: 'relative' }}>
              <IcSearch width={14} height={14} style={{ position: 'absolute', left: 8, top: 9, color: 'var(--faint)' }} />
              <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="全文搜索" style={{ paddingLeft: 28, width: 180 }} />
            </div>
          </Field>
          <Field label="">
            <select value={hostFilter} onChange={(e) => setHostFilter(e.target.value === '' ? '' : Number(e.target.value))} style={{ width: 140 }}>
              <option value="">全部主机</option>
              {hosts.map((h) => <option key={h.id} value={h.id}>{h.name}</option>)}
            </select>
          </Field>
          <Field label="">
            <select value={typeFilter} onChange={(e) => setTypeFilter(e.target.value)} style={{ width: 120 }}>
              <option value="">全部类型</option>
              <option>network</option>
              <option>web</option>
              <option>database</option>
              <option>disk</option>
              <option>resource_alert</option>
            </select>
          </Field>
          <Field label="">
            <select value={verFilter} onChange={(e) => setVerFilter(e.target.value as '' | 'true' | 'false')} style={{ width: 110 }}>
              <option value="">全部状态</option>
              <option value="true">已验证</option>
              <option value="false">未验证</option>
            </select>
          </Field>
          <span style={{ flex: 1 }} />
          <Btn variant="ghost" onClick={() => setImporting(!importing)}><IcImport width={12} height={12} /> 导入 MD</Btn>
        </div>

        {importing && (
          <div className="card pad" style={{ marginBottom: 'var(--space-4)' }}>
            <Field label="粘贴 Markdown（F5.6 格式，支持多条）" hint="---\ntype: ai-ssh-memory\nos: Ubuntu 22.04\ntags: [nginx, disk]\n---\n## 问题…">
              <textarea rows={6} value={importText} onChange={(e) => setImportText(e.target.value)} placeholder="粘贴记忆文档…" />
            </Field>
            <Btn onClick={doImport} loading={importing}>导入</Btn>
          </div>
        )}

        {/* 列表 */}
        {cases.length === 0 ? (
          <EmptyState icon={<IcMemory width={40} height={40} />} title="没有匹配的记录" desc="AI 修复验证通过或手动保存后会自动沉淀到这里" />
        ) : (
          <div style={{ display: 'grid', gap: 8 }}>
            {cases.map((c) => (
              <div key={c.id} className="card pad" style={{ padding: 'var(--space-3)' }}>
                <div style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: 'flex', gap: 6, marginBottom: 4, flexWrap: 'wrap' }}>
                      <Tag kind={c.verified ? 'ok' : 'warn'}>{c.verified ? '已验证' : '未验证'}</Tag>
                      {c.problem_type && <Tag kind="info">{c.problem_type}</Tag>}
                      {c.source === 'ai_resolved' && <Tag kind="neutral">AI 沉淀</Tag>}
                      {c.source === 'imported' && <Tag kind="neutral">导入</Tag>}
                      {c.failed_for && <Tag kind="err">已失效</Tag>}
                    </div>
                    <div style={{ fontWeight: 600, marginBottom: 2, cursor: 'pointer' }} onClick={() => setDetail(c)}>{c.description}</div>
                    {c.root_cause && <div style={{ color: 'var(--sub)', fontSize: 'var(--fs-caption)' }}>{c.root_cause}</div>}
                    <div style={{ color: 'var(--faint)', fontSize: '0.7rem', marginTop: 4 }}>
                      命中 {c.hit_count} 次 · {c.created_at?.slice(0, 16)}
                      {c.host_id ? ` · ${hosts.find((h) => h.id === c.host_id)?.name ?? `#${c.host_id}`}` : ''}
                    </div>
                  </div>
                  <div style={{ display: 'flex', gap: 4, flexDirection: 'column' }}>
                    <Btn size="sm" variant="ghost" onClick={() => setDetail(c)}><IcEye width={12} height={12} /> 详情</Btn>
                    <Btn size="sm" variant="ghost" onClick={() => c.id && doExport(c)}><IcDownload width={12} height={12} /> 导出</Btn>
                    <div style={{ display: 'flex', gap: 4 }}>
                      <Btn size="sm" variant="ghost" title="有用" onClick={() => c.id && doFeedback(c.id, true)}><IcThumbsUp width={12} height={12} /></Btn>
                      <Btn size="sm" variant="ghost" title="无效" onClick={() => c.id && doFeedback(c.id, false)}><IcThumbsDown width={12} height={12} /></Btn>
                      <Btn size="sm" variant="ghost" title="删除" onClick={() => c.id && doDelete(c.id)}><IcTrash width={12} height={12} /></Btn>
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {detail && <CaseDetailModal c={detail} hosts={hosts} onClose={() => setDetail(null)} onSaved={() => { setDetail(null); void reload(); }} />}
    </div>
  );
}

function CaseDetailModal({ c, hosts, onClose, onSaved }: { c: MemoryCase; hosts: { id: number; name: string }[]; onClose: () => void; onSaved: () => void }) {
  const toast = useApp((s) => s.toast);
  const [f, setF] = useState<MemoryCase>({ ...c });
  const set = (k: keyof MemoryCase, v: unknown) => setF((p) => ({ ...p, [k]: v }));

  async function save() {
    if (!f.id) return;
    try {
      await getBackend().memoryUpdate(f.id, f);
      toast('ok', '已保存');
      onSaved();
    } catch (e) {
      toast('err', (e as Error).message);
    }
  }

  return (
    <Modal title={`记录 #${c.id}`} onClose={onClose} wide>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '0 14px' }}>
        <Field label="主机">
          <select value={f.host_id ?? ''} onChange={(e) => set('host_id', e.target.value ? Number(e.target.value) : null)}>
            <option value="">（无）</option>
            {hosts.map((h) => <option key={h.id} value={h.id}>{h.name}</option>)}
          </select>
        </Field>
        <Field label="问题类型"><input value={f.problem_type ?? ''} onChange={(e) => set('problem_type', e.target.value)} /></Field>
        <Field label="描述（必填）"><input value={f.description} onChange={(e) => set('description', e.target.value)} /></Field>
        <Field label="关键词"><input value={f.keywords ?? ''} onChange={(e) => set('keywords', e.target.value)} /></Field>
        <Field label="OS"><input value={f.os_info ?? ''} onChange={(e) => set('os_info', e.target.value)} /></Field>
        <Field label="verified">
          <select value={String(f.verified)} onChange={(e) => set('verified', e.target.value === 'true')}>
            <option value="false">未验证</option>
            <option value="true">已验证</option>
          </select>
        </Field>
        <Field label="报错片段" hint="可选"><textarea rows={3} value={f.error_snippet ?? ''} onChange={(e) => set('error_snippet', e.target.value)} /></Field>
        <Field label="根因"><textarea rows={3} value={f.root_cause ?? ''} onChange={(e) => set('root_cause', e.target.value)} /></Field>
        <Field label="解决方案（命令，多行）"><textarea rows={4} value={f.solution_cmd ?? ''} onChange={(e) => set('solution_cmd', e.target.value)} /></Field>
        <Field label="解决说明"><textarea rows={4} value={f.solution_text ?? ''} onChange={(e) => set('solution_text', e.target.value)} /></Field>
        <Field label="验证命令"><input value={f.verify_cmd ?? ''} onChange={(e) => set('verify_cmd', e.target.value)} /></Field>
        <Field label="回滚命令"><input value={f.rollback_cmd ?? ''} onChange={(e) => set('rollback_cmd', e.target.value)} /></Field>
      </div>
      <div className="modal-actions">
        <Btn variant="ghost" onClick={onClose}>关闭</Btn>
        <Btn onClick={save}>保存修改</Btn>
      </div>
    </Modal>
  );
}
