import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { setBackend, type Backend } from './api/backend';
import { TauriBackend, isTauriRuntime } from './api/tauri';
import { MockBackend } from './api/mock';
import './styles/tokens.css';
import './styles/base.css';
import './styles/components.css';

// 运行环境判定：Tauri 桌面端 → 真实 IPC；浏览器 → Mock 演示数据
let backend: Backend;
if (isTauriRuntime()) {
  backend = new TauriBackend();
} else {
  backend = new MockBackend();
  console.info('[AI-SSH] 浏览器演示模式：使用 Mock 数据，SSH/LLM 为模拟');
}
setBackend(backend);

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
