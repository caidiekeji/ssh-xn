// AI 对话面板（F3 命令生成 / F4 错误分析 / F5 历史注入标注）
import { useEffect, useRef, useState } from 'react';
import { getBackend } from '../api/backend';
import { useApp } from '../store';
import { Btn, EmptyState, RiskTag, Tag } from './ui';
import { IcAi, IcCopy, IcMemory, IcRollback, IcSave, IcThumbsDown, IcThumbsUp } from './icons';
import type { ChatMsg, CommandResult, DiagnosisResult, MemoryCase } from '../types';

let msgSeq = 0;
const nid = () => `m${++msgSeq}`;

export function AiPanel() {
  const activeSessionId = useApp((s) => s.activeSessionId);
  const sessionMeta = useApp((s) => s.sessionMeta);
  const msgs = useApp((s) => (s.activeSessionId ? s.aiBySession[s.activeSessionId] ?? [] : []));
  const busy = useApp((s) => (s.activeSessionId ? s.aiBusyBySession[s.activeSessionId] ?? false : false));
  const pushAiMsg = useApp((s) => s.pushAiMsg);
  const patchAiMsg = useApp((s) => s.patchAiMsg);
  const appendAiChunk = useApp((s) => s.appendAiChunk);
  const setAiBusy = useApp((s) => s.setAiBusy);
  const toast = useApp((s) => s.toast);
  const [input, setInput] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  // 自动滚动
  useEffect(() => {
    const el = listRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [msgs, busy]);

  // Ctrl+K 聚焦 / 报错条分析事件 / 告警联动 AI 分析（F8.6） / 进程分析
  useEffect(() => {
    const onFocus = () => inputRef.current?.focus();
    const onAnalyze = (e: Event) => {
      const detail = (e as CustomEvent).detail as { sessionId: string; snippet: string };
      if (detail.sessionId === activeSessionId) {
        void sendAnalysis(detail.snippet);
      }
    };
    const onAlert = (e: Event) => {
      const detail = (e as CustomEvent).detail as { alertId: number; hostId: number };
      const meta = sessionMeta[activeSessionId ?? ''];
      if (meta && meta.hostId === detail.hostId && activeSessionId) {
        void sendAlertAnalysis(detail.alertId);
      }
    };
    const onProcess = (e: Event) => {
      const detail = (e as CustomEvent).detail as { pid: number; hostId: number };
      const meta = sessionMeta[activeSessionId ?? ''];
      if (meta && meta.hostId === detail.hostId && activeSessionId) {
        void sendAnalysis(`进程 ${detail.pid} 占用资源异常，请分析原因与处理方案`);
      }
    };
    window.addEventListener('ai-ssh:focus-input', onFocus);
    window.addEventListener('ai-ssh:analyze-snippet', onAnalyze);
    window.addEventListener('ai-ssh:analyze-alert', onAlert);
    window.addEventListener('ai-ssh:analyze-process', onProcess);
    return () => {
      window.removeEventListener('ai-ssh:focus-input', onFocus);
      window.removeEventListener('ai-ssh:analyze-snippet', onAnalyze);
      window.removeEventListener('ai-ssh:analyze-alert', onAlert);
      window.removeEventListener('ai-ssh:analyze-process', onProcess);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSessionId, sessionMeta]);

  async function sendAlertAnalysis(alertId: number) {
    if (!activeSessionId) return;
    const meta = sessionMeta[activeSessionId];
    if (!meta) return;
    const b = getBackend();
    const id = nid();
    pushAiMsg(activeSessionId, { id, role: 'assistant', content: '', kind: 'diagnosis', streaming: true });
    setAiBusy(activeSessionId, true);
    const ac = new AbortController();
    abortRef.current = ac;
    try {
      const d = await b.analyzeAlert(meta.hostId, alertId, (chunk) => {
        appendAiChunk(activeSessionId, id, chunk);
      }, ac.signal);
      patchAiMsg(activeSessionId, id, { content: '', kind: 'diagnosis', diagnosis: d, streaming: false, refMemory: d.ref_memory.length > 0 });
      if (d.ref_memory.length) toast('info', `参考了 ${d.ref_memory.length} 条历史记录`);
    } catch (e) {
      patchAiMsg(activeSessionId, id, { streaming: false, error: (e as Error).message });
    } finally {
      setAiBusy(activeSessionId, false);
    }
  }

  async function sendAnalysis(text: string) {
    if (!activeSessionId) return;
    const b = getBackend();
    const id = nid();
    pushAiMsg(activeSessionId, { id, role: 'assistant', content: '', kind: 'diagnosis', streaming: true });
    setAiBusy(activeSessionId, true);
    const ac = new AbortController();
    abortRef.current = ac;
    try {
      const d = await b.analyzeError(activeSessionId, text, (chunk) => {
        appendAiChunk(activeSessionId, id, chunk);
      }, ac.signal);
      patchAiMsg(activeSessionId, id, { content: '', kind: 'diagnosis', diagnosis: d, streaming: false, refMemory: d.ref_memory.length > 0 });
      if (d.ref_memory.length) toast('info', `参考了 ${d.ref_memory.length} 条历史记录`);
    } catch (e) {
      patchAiMsg(activeSessionId, id, { streaming: false, error: (e as Error).message });
    } finally {
      setAiBusy(activeSessionId, false);
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || !activeSessionId) return;
    setInput('');
    const b = getBackend();
    pushAiMsg(activeSessionId, { id: nid(), role: 'user', content: text, kind: 'text' });
    const id = nid();
    pushAiMsg(activeSessionId, { id, role: 'assistant', content: '', kind: 'command', streaming: true });
    setAiBusy(activeSessionId, true);
    const ac = new AbortController();
    abortRef.current = ac;
    try {
      const r = await b.generateCommand(activeSessionId, text, (chunk) => {
        appendAiChunk(activeSessionId, id, chunk);
      }, ac.signal);
      patchAiMsg(activeSessionId, id, { content: '', kind: 'command', command: r, streaming: false });
    } catch (e) {
      patchAiMsg(activeSessionId, id, { streaming: false, error: (e as Error).message });
      if ((e as Error).message !== '已取消') toast('err', `AI 不可用：${(e as Error).message}`);
    } finally {
      setAiBusy(activeSessionId, false);
    }
  }

  const hostId = activeSessionId ? sessionMeta[activeSessionId]?.hostId : undefined;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <div className="pane-title">
        <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <IcAi width={14} height={14} style={{ color: 'var(--acc2)' }} /> AI 副驾
        </span>
        <Tag kind={busy ? 'warn' : 'ok'}>{busy ? '生成中' : '就绪'}</Tag>
      </div>

      <div className="scroll-area" ref={listRef} style={{ padding: 'var(--space-3)', display: 'flex', flexDirection: 'column', gap: 'var(--space-3)' }}>
        {msgs.length === 0 && (
          <EmptyState
            icon={<IcAi width={42} height={42} />}
            title="还没有对话"
            desc="输入自然语言生成命令，或选中终端文本让我分析报错"
          />
        )}
        {msgs.map((m) => (
          <MsgView key={m.id} m={m} hostId={hostId} sessionId={activeSessionId ?? ''} />
        ))}
        {busy && <div style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>▍</div>}
      </div>

      <div style={{ borderTop: '1px solid var(--line)', padding: 'var(--space-3)', display: 'flex', gap: 8, background: 'var(--bg2)' }}>
        <input
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && !busy && send()}
          placeholder="用自然语言说需求，如：查看 80 端口占用"
          style={{ flex: 1, background: 'var(--deep)', border: '1px solid var(--line)', borderRadius: 'var(--radius-md)', color: 'var(--ink)', padding: '8px 10px', fontSize: 'var(--fs-body)' }}
          aria-label="AI 命令输入"
        />
        <Btn onClick={send} loading={busy}>
          <IcAi width={14} height={14} />
        </Btn>
      </div>
    </div>
  );
}

function MsgView({ m, hostId, sessionId }: { m: ChatMsg; hostId?: number; sessionId: string }) {
  if (m.role === 'user') {
    return (
      <div style={{ alignSelf: 'flex-end', maxWidth: '92%', background: 'var(--surface)', border: '1px solid var(--line)', borderRadius: 'var(--radius-md) var(--radius-md) 2px var(--radius-md)', padding: '8px 12px', fontSize: 'var(--fs-body)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
        {m.content}
      </div>
    );
  }
  if (m.streaming) {
    return (
      <div style={{ alignSelf: 'flex-start', maxWidth: '100%', fontSize: 'var(--fs-caption)', color: 'var(--faint)', fontFamily: 'var(--font-body)', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
        <span style={{ color: 'var(--acc2)', animation: 'cursor-blink 1s steps(1) infinite' }}>█</span> {m.content}
      </div>
    );
  }
  if (m.error) {
    return (
      <div className="card pad" style={{ alignSelf: 'flex-start', borderColor: 'var(--err)', color: 'var(--tag-err-fg)', fontSize: 'var(--fs-caption)' }}>
        ⚠ {m.error}
      </div>
    );
  }
  if (m.kind === 'command' && m.command) {
    return <CommandCard result={m.command} sessionId={sessionId} hostId={hostId} />;
  }
  if (m.kind === 'diagnosis' && m.diagnosis) {
    return <DiagnosisCard d={m.diagnosis} sessionId={sessionId} hostId={hostId} snippet={m.content} />;
  }
  return null;
}

// ===== 命令确认卡片（F3.2：命令块 + 逐行解释 + 风险标记 + 执行/编辑/复制/取消） =====
function CommandCard({ result, sessionId, hostId }: { result: CommandResult; sessionId: string; hostId?: number }) {
  const toast = useApp((s) => s.toast);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(result.commands.join('\n'));
  const [confirming, setConfirming] = useState<null | 'medium' | 'high'>(null);
  const [running, setRunning] = useState(false);
  const [hostConfirm, setHostConfirm] = useState('');

  async function doExecute() {
    const b = getBackend();
    setRunning(true);
    try {
      const v = await b.safetyCheck(text);
      const level = v.level;
      if (level === 'high') { setConfirming('high'); setRunning(false); return; }
      if (level === 'medium') { setConfirming('medium'); setRunning(false); return; }
      await run(text);
    } catch (e) {
      toast('err', `安全检查失败: ${(e as Error).message}`);
      setRunning(false);
    }
  }

  async function run(cmdText: string) {
    const b = getBackend();
    setRunning(true);
    try {
      // 危险确认已在前端弹窗完成；服务端仍会强制复检（F6.1 无绕过路径）
      await b.executeCommand(sessionId, cmdText, { audit: { user_input: '', risk: result.risk }, confirmed: true });
      toast('ok', '命令已发送到终端');
      // F5.4 自动沉淀：验证命令返回 exit 0 后写入历史
      if (result.commands.length && hostId) {
        const r = await b.verifyAndSave(sessionId, hostId, {
          description: `AI 命令: ${cmdText.split('\n')[0].slice(0, 80)}`,
          solution_cmd: cmdText,
          verify_cmd: '',
          keywords: '',
          root_cause: '',
        }).catch(() => null);
        if (r?.saved) toast('ok', '已沉淀到历史记录');
      }
    } catch (e) {
      toast('err', `执行失败: ${(e as Error).message}`);
    } finally {
      setRunning(false);
      setConfirming(null);
    }
  }

  const confirmModal = confirming && (
    <ConfirmDangerModal
      level={confirming}
      commands={text}
      hostId={hostId}
      hostConfirm={hostConfirm}
      setHostConfirm={setHostConfirm}
      onCancel={() => setConfirming(null)}
      onConfirm={() => void run(text)}
    />
  );

  return (
    <div className="card pad" style={{ alignSelf: 'flex-start', width: '100%', borderColor: result.risk === 'high' ? 'var(--err)' : result.risk === 'medium' ? 'var(--warn)' : 'var(--line)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <Tag kind="info">命令</Tag>
        <RiskTag level={result.risk} />
        {result.risk !== 'low' && <span style={{ color: 'var(--tag-warn-fg)', fontSize: 'var(--fs-caption)' }}>{result.risk_reason}</span>}
      </div>

      {editing ? (
        <textarea value={text} onChange={(e) => setText(e.target.value)} rows={Math.max(2, text.split('\n').length)} style={{ width: '100%' }} aria-label="编辑命令" />
      ) : (
        <div className="cmd-block">{[...result.commands].map((c, i) => (
          <div key={i} style={{ marginBottom: 4 }}>
            <span className="ps">$ </span>{c}
          </div>
        ))}</div>
      )}

      <ol style={{ margin: '8px 0 0 18px', fontSize: 'var(--fs-caption)', color: 'var(--sub)', display: 'grid', gap: 2 }}>
        {(editing ? text.split('\n') : result.explanation).map((e, i) => (
          <li key={i}>{e}</li>
        ))}
      </ol>
      {result.notes && <div style={{ marginTop: 8, color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>※ {result.notes}</div>}

      <div style={{ display: 'flex', gap: 8, marginTop: 12, flexWrap: 'wrap' }}>
        <Btn size="sm" danger={result.risk === 'high'} loading={running} onClick={doExecute}>执行</Btn>
        <Btn size="sm" variant="ghost" onClick={() => setEditing(!editing)}>{editing ? '预览' : '编辑'}</Btn>
        <Btn size="sm" variant="ghost" onClick={() => { void navigator.clipboard.writeText(text).then(() => toast('ok', '已复制')); }}>
          <IcCopy width={12} height={12} /> 复制
        </Btn>
        <span style={{ flex: 1 }} />
        <Btn size="sm" variant="ghost" onClick={() => toast('info', '已取消，未执行')}>取消</Btn>
      </div>
      {confirmModal}
    </div>
  );
}

// ===== 高危/中危确认弹窗（F6.1：high 输入主机名确认） =====
function ConfirmDangerModal({ level, commands, hostId, hostConfirm, setHostConfirm, onCancel, onConfirm }: {
  level: 'medium' | 'high';
  commands: string;
  hostId?: number;
  hostConfirm: string;
  setHostConfirm: (v: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const hosts = useApp((s) => s.hosts);
  const hostName = hosts.find((h) => h.id === hostId)?.name ?? '';
  const ok = level === 'medium' || hostConfirm.trim() === hostName;
  return (
    <div className="modal-mask" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className={`modal ${level === 'high' ? 'danger' : ''}`} role="alertdialog" aria-modal="true">
        <h3>{level === 'high' ? '高风险操作确认' : '确认执行该操作？'}</h3>
        <div className="danger-reason">{commands}</div>
        {level === 'high' && (
          <div style={{ marginBottom: 'var(--space-3)', fontSize: 'var(--fs-caption)', color: 'var(--tag-warn-fg)' }}>
            此操作风险等级为 <b>high</b>，可能造成不可逆影响。请输入主机名 <b>{hostName || '(未连接主机)'}</b> 以确认。
            <div style={{ marginTop: 8 }}>
              <input value={hostConfirm} onChange={(e) => setHostConfirm(e.target.value)} placeholder="输入主机名" style={{ width: '100%' }} aria-label="输入主机名确认" />
            </div>
          </div>
        )}
        <div className="modal-actions">
          <Btn variant="ghost" onClick={onCancel}>取消</Btn>
          <Btn danger disabled={!ok} onClick={onConfirm}>确认执行</Btn>
        </div>
      </div>
    </div>
  );
}

// ===== 诊断结果卡片（F4.3：诊断→根因→修复步骤→验证→回滚 + 历史引用标注） =====
function DiagnosisCard({ d, sessionId, hostId, snippet }: { d: DiagnosisResult; sessionId: string; hostId?: number; snippet?: string }) {
  const toast = useApp((s) => s.toast);
  const [runningStep, setRunningStep] = useState<number | null>(null);
  const [saved, setSaved] = useState(false);

  async function runStep(i: number, cmd: string) {
    const b = getBackend();
    setRunningStep(i);
    try {
      await b.executeCommand(sessionId, cmd, { audit: { user_input: '', risk: 'medium' } });
      toast('ok', `已执行: ${cmd.slice(0, 40)}`);
    } catch (e) {
      toast('err', (e as Error).message);
    } finally {
      setRunningStep(null);
    }
  }

  async function verify() {
    if (!d.verify_cmd) return toast('warn', '没有验证命令');
    await runStep(-1, d.verify_cmd);
  }

  async function rollback() {
    if (!d.rollback_cmd) return toast('warn', '该场景无法回滚');
    await runStep(-2, d.rollback_cmd);
  }

  async function saveCase() {
    const b = getBackend();
    if (!hostId) return toast('err', '未连接到主机');
    try {
      await b.memorySave({
        host_id: hostId,
        description: snippet?.split('\n')[0]?.slice(0, 120) ?? d.diagnosis.slice(0, 120),
        error_snippet: snippet,
        root_cause: d.root_cause,
        solution_cmd: d.fix_steps.map((f) => f.cmd).join('\n'),
        solution_text: d.fix_steps.map((f) => f.desc).join('\n'),
        verify_cmd: d.verify_cmd,
        rollback_cmd: d.rollback_cmd,
        hit_count: 0,
        verified: false,
        source: 'user_manual',
      } satisfies MemoryCase);
      setSaved(true);
      toast('ok', '已保存到历史记录');
    } catch (e) {
      toast('err', (e as Error).message);
    }
  }

  return (
    <div className="card pad" style={{ alignSelf: 'flex-start', width: '100%' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <Tag kind="warn">诊断</Tag>
        {d.ref_memory.length > 0 && <Tag kind="info"><IcMemory width={11} height={11} /> 参考了历史记录</Tag>}
        {d.ref_memory.length > 0 && d.ref_memory.map((id) => <Tag key={id} kind="neutral">#{id}</Tag>)}
      </div>

      <Section label="发生了什么">
        <p>{d.diagnosis}</p>
      </Section>
      <Section label="根因">
        <p style={{ color: 'var(--tag-warn-fg)' }}>{d.root_cause}</p>
      </Section>

      {d.fix_steps.length > 0 && (
        <Section label="修复步骤">
          <div style={{ display: 'grid', gap: 6 }}>
            {d.fix_steps.map((f, i) => (
              <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                <span className="tag neutral" style={{ marginTop: 2 }}>{i + 1}</span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="cmd-block" style={{ padding: '6px 10px', fontSize: '0.8rem' }}>
                    <span className="ps">$ </span>{f.cmd}
                  </div>
                  <div style={{ fontSize: 'var(--fs-caption)', color: 'var(--sub)', marginTop: 2 }}>{f.desc}</div>
                </div>
                <Btn size="sm" variant="ghost" loading={runningStep === i} onClick={() => runStep(i, f.cmd)}>执行</Btn>
              </div>
            ))}
          </div>
        </Section>
      )}

      <div style={{ display: 'flex', gap: 8, marginTop: 12, flexWrap: 'wrap' }}>
        <Btn size="sm" loading={runningStep === -1} onClick={verify}>验证</Btn>
        <Btn size="sm" variant="ghost" onClick={rollback}><IcRollback width={12} height={12} /> 回滚</Btn>
        <Btn size="sm" variant="ghost" onClick={saveCase} disabled={saved}>
          <IcSave width={12} height={12} /> {saved ? '已保存' : '保存本次排查'}
        </Btn>
        <span style={{ flex: 1 }} />
        <FeedbackButtons id={d.ref_memory[0]} />
      </div>
    </div>
  );
}

function FeedbackButtons({ id }: { id?: string }) {
  const toast = useApp((s) => s.toast);
  const b = getBackend();
  if (!id) return null;
  return (
    <>
      <Btn size="sm" variant="ghost" title="有用 (+2)" onClick={() => { void b.memoryFeedback(Number(id), true); toast('ok', '已记录 👍'); }}>
        <IcThumbsUp width={12} height={12} />
      </Btn>
      <Btn size="sm" variant="ghost" title="无用 (-1)" onClick={() => { void b.memoryFeedback(Number(id), false); toast('warn', '已记录 👎'); }}>
        <IcThumbsDown width={12} height={12} />
      </Btn>
    </>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ marginTop: 10 }}>
      <div style={{ fontSize: 'var(--fs-caption)', color: 'var(--faint)', letterSpacing: '0.06em', marginBottom: 4 }}>{label}</div>
      <div style={{ fontSize: 'var(--fs-body)', lineHeight: 1.65 }}>{children}</div>
    </div>
  );
}
