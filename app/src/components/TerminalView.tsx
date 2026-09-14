// 终端组件（xterm.js）：PRD F1.3 终端 + F4.2 报错提示条
import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { getBackend } from '../api/backend';
import { useApp } from '../store';
import { Btn } from './ui';
import { IcAlert, IcAi } from './icons';

const THEME = {
  background: '#0D0B09',
  foreground: '#E8E4DA',
  cursor: '#22D48C',
  cursorAccent: '#0D0B09',
  selectionBackground: 'rgba(77,141,255,0.35)',
  black: '#0D0B09',
  red: '#E5484D',
  green: '#2FC66E',
  yellow: '#F0A64A',
  blue: '#4D8DFF',
  magenta: '#A855F7',
  cyan: '#22D48C',
  white: '#E8E4DA',
  brightBlack: '#7D7262',
  brightRed: '#F0716F',
  brightGreen: '#5FD98F',
  brightYellow: '#F5B83D',
  brightBlue: '#7FACFF',
  brightMagenta: '#C792FF',
  brightCyan: '#6FE8B8',
  brightWhite: '#F2F5FA',
};

export function TerminalView({ sessionId }: { sessionId: string }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const errorSnippet = useApp((s) => s.sessionMeta[sessionId]?.errorSnippet);
  const clearError = useApp((s) => s.clearSessionError);

  useEffect(() => {
    const b = getBackend();
    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "var(--font-body)",
      theme: THEME,
      scrollback: 5000, // F1.3：会话缓冲区保留最近 5000 行
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    termRef.current = term;
    fitRef.current = fit;

    if (hostRef.current) term.open(hostRef.current);
    requestAnimationFrame(() => fit.fit());

    const ro = new ResizeObserver(() => {
      try { fit.fit(); } catch { /* 终端未就绪 */ }
    });
    if (hostRef.current) ro.observe(hostRef.current);

    // 终端输出事件 → 渲染
    const unOut = b.onTerminalOutput((sid, data) => {
      if (sid === sessionId) term.write(data);
    });
    const unExit = b.onTerminalExit((sid, code) => {
      if (sid === sessionId) {
        term.write(`\r\n\x1b[31m[连接已断开 (exit ${code ?? '?'})]\x1b[0m\r\n`);
      }
    });

    // 用户输入 → SSH channel
    const onData = (data: string) => {
      void b.sshWrite(sessionId, data);
    };
    term.onData(onData);

    // Ctrl+K 唤起 AI 浮层（F3.1）
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('ai-ssh:focus-input'));
      }
    };
    window.addEventListener('keydown', onKey);

    // 初始提示
    term.write('\x1b[90m—— 终端就绪，Ctrl+K 唤起 AI 命令生成 ——\x1b[0m\r\n');

    return () => {
      ro.disconnect();
      window.removeEventListener('keydown', onKey);
      unOut();
      unExit();
      term.dispose();
    };
  }, [sessionId]);

  // 新 tab 激活时重新 fit
  useEffect(() => {
    const t = setTimeout(() => fitRef.current?.fit(), 60);
    return () => clearTimeout(t);
  }, [sessionId]);

  return (
    <div className="term-wrap" style={{ position: 'relative', flex: 1, minHeight: 0, background: 'var(--terminal-bg)', display: 'flex', flexDirection: 'column' }}>
      {errorSnippet && (
        <div
          className="error-bar"
          role="alert"
          style={{
            position: 'absolute',
            top: 8,
            left: '50%',
            transform: 'translateX(-50%)',
            zIndex: 20,
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            background: 'var(--paper)',
            border: '1px solid var(--warn)',
            borderLeft: `3px solid var(--warn)`,
            borderRadius: 'var(--radius-md)',
            padding: '8px 12px',
            fontSize: 'var(--fs-caption)',
            boxShadow: 'var(--shadow-md)',
            maxWidth: '86%',
          }}
        >
          <IcAlert width={16} height={16} style={{ color: 'var(--warn)', flex: 'none' }} />
          <span style={{ color: 'var(--ink)', whiteSpace: 'nowrap' }}>检测到错误，是否分析？</span>
          <Btn
            size="sm"
            onClick={() => {
              window.dispatchEvent(new CustomEvent('ai-ssh:analyze-snippet', { detail: { sessionId, snippet: errorSnippet } }));
              clearError(sessionId);
            }}
          >
            <IcAi width={13} height={13} /> AI 分析
          </Btn>
          <Btn size="sm" variant="ghost" onClick={() => clearError(sessionId)}>
            忽略
          </Btn>
        </div>
      )}
      <div ref={hostRef} style={{ flex: 1, minHeight: 0, padding: 4 }} />
    </div>
  );
}
