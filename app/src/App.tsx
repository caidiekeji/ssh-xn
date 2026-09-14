// 应用外壳：顶栏 + 主机面板 + 终端标签区 + AI 面板 + 监控条带（终端台架布局）
import { useEffect, useState } from 'react';
import { useApp } from './store';
import { getBackend } from './api/backend';
import { HostPane } from './components/HostPane';
import { TerminalView } from './components/TerminalView';
import { AiPanel } from './components/AiPanel';
import { MonitorPanel } from './components/MonitorPanel';
import { ToastHost } from './components/ui';
import { HistoryPage } from './pages/HistoryPage';
import { SettingsPage } from './pages/SettingsPage';
import { IcCursor, IcGear, IcMemory, IcTerminal, IcX } from './components/icons';

export function App() {
  const view = useApp((s) => s.view);
  const setView = useApp((s) => s.setView);
  const tabs = useApp((s) => s.tabs);
  const activeSessionId = useApp((s) => s.activeSessionId);
  const sessionMeta = useApp((s) => s.sessionMeta);
  const hosts = useApp((s) => s.hosts);
  const init = useApp((s) => s.init);
  const closeTab = useApp((s) => s.closeTab);
  const activateTab = useApp((s) => s.activateTab);
  const setHosts = useApp((s) => s.setHosts);
  const [booting, setBooting] = useState(true);

  useEffect(() => {
    void (async () => {
      try {
        await init();
      } catch (e) {
        console.error('初始化失败', e);
      } finally {
        setBooting(false);
      }
    })();
    // 主机增删后刷新列表
    const onHosts = () => {
      void getBackend().listHosts().then((hs) => setHosts(hs));
    };
    window.addEventListener('ai-ssh:hosts-changed', onHosts);
    return () => window.removeEventListener('ai-ssh:hosts-changed', onHosts);
  }, [init, setHosts]);

  if (booting) {
    return (
      <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 10, color: 'var(--sub)', fontFamily: 'var(--font-body)' }}>
        <span style={{ color: 'var(--acc2)', animation: 'cursor-blink 1s steps(1) infinite' }}>█</span>
        正在启动 AI-SSH…
      </div>
    );
  }

  const activeHost = activeSessionId ? hosts.find((h) => h.id === sessionMeta[activeSessionId]?.hostId) : undefined;

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <IcCursor width={16} height={16} className="brand-mark" />
          AI-SSH
          <span style={{ color: 'var(--faint)', fontWeight: 400, fontSize: 'var(--fs-caption)' }}>v1.2.0 · 纯本地</span>
        </div>
        <nav aria-label="主导航">
          <button className={view === 'workspace' ? 'on' : ''} onClick={() => setView('workspace')}>
            <IcTerminal width={13} height={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />工作台
          </button>
          <button className={view === 'history' ? 'on' : ''} onClick={() => setView('history')}>
            <IcMemory width={13} height={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />历史记录
          </button>
          <button className={view === 'settings' ? 'on' : ''} onClick={() => setView('settings')}>
            <IcGear width={13} height={13} style={{ verticalAlign: '-2px', marginRight: 4 }} />设置
          </button>
        </nav>
        <span className="spacer" />
        {activeHost && <span style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>{activeHost.username}@{activeHost.host}:{activeHost.port}</span>}
      </header>

      {view === 'workspace' ? (
        <main className="workspace">
          <HostPane />
          <div className="term-pane">
            {tabs.length === 0 ? (
              <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                <div className="empty">
                  <IcTerminal width={44} height={44} />
                  <div className="empty-title">没有打开的会话</div>
                  <div style={{ color: 'var(--faint)' }}>从左侧选择主机连接，或新建主机</div>
                </div>
              </div>
            ) : (
              <>
                {/* 标签页条 */}
                <div style={{ display: 'flex', gap: 2, padding: '6px 8px 0', background: 'var(--bg2)', borderBottom: '1px solid var(--line)', overflowX: 'auto', flex: 'none' }}>
                  {tabs.map((sid) => {
                    const meta = sessionMeta[sid];
                    const name = hosts.find((h) => h.id === meta?.hostId)?.name ?? sid.slice(0, 8);
                    return (
                      <div
                        key={sid}
                        className={`tab ${sid === activeSessionId ? 'on' : ''}`}
                        onClick={() => activateTab(sid)}
                        onAuxClick={(e) => e.button === 1 && void closeTab(sid)}
                        role="tab"
                        aria-selected={sid === activeSessionId}
                        tabIndex={0}
                        onKeyDown={(e) => e.key === 'Enter' && activateTab(sid)}
                      >
                        <IcTerminal width={12} height={12} />
                        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 120 }}>{name}</span>
                        <button className="tab-x" aria-label={`关闭 ${name}`} onClick={(e) => { e.stopPropagation(); void closeTab(sid); }}>
                          <IcX width={10} height={10} />
                        </button>
                      </div>
                    );
                  })}
                </div>
                {tabs.map((sid) => (
                  <div key={sid} style={{ display: sid === activeSessionId ? 'flex' : 'none', flex: 1, minHeight: 0 }}>
                    <TerminalView sessionId={sid} />
                  </div>
                ))}
              </>
            )}
          </div>
          <div className="ai-pane">
            <AiPanel />
          </div>
          <div className="monitor-pane">
            <div className="scanline" aria-hidden />
            <MonitorPanel />
          </div>
        </main>
      ) : view === 'history' ? (
        <HistoryPage />
      ) : (
        <SettingsPage />
      )}

      <ToastHost />
    </div>
  );
}
