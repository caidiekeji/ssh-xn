// 基础 UI 组件：Button / Tag / Field / Modal / Toast / EmptyState —— 全部从 token 推导
import { useEffect, type ReactNode } from 'react';
import { useApp } from '../store';
import { IcX } from './icons';
import type { RiskLevel } from '../types';

// ===== Button =====
export function Btn({
  variant = 'primary',
  size,
  danger,
  disabled,
  loading,
  className,
  children,
  onClick,
  type,
  title,
  style,
}: {
  variant?: 'primary' | 'ghost';
  size?: 'sm';
  danger?: boolean;
  disabled?: boolean;
  loading?: boolean;
  className?: string;
  children: ReactNode;
  onClick?: (e?: React.MouseEvent) => void;
  type?: 'button' | 'submit';
  title?: string;
  style?: React.CSSProperties;
}) {
  const cls = ['btn', variant === 'ghost' ? 'ghost' : '', danger ? 'danger' : '', size ? size : '', className ?? '']
    .filter(Boolean)
    .join(' ');
  return (
    <button
      type={type ?? 'button'}
      className={cls}
      disabled={disabled || loading}
      onClick={onClick}
      title={title}
      style={style}
      aria-busy={loading || undefined}
    >
      {loading && <span className="spin" aria-hidden />}
      {children}
    </button>
  );
}

// ===== Tag =====
const riskKind: Record<RiskLevel, 'ok' | 'warn' | 'err'> = { low: 'ok', medium: 'warn', high: 'err' };

export function RiskTag({ level }: { level: RiskLevel }) {
  const label = { low: '低风险', medium: '中风险', high: '高风险' }[level];
  return (
    <span className={`tag ${riskKind[level]}`}>
      <i className="dot" aria-hidden />
      {label}
    </span>
  );
}

export function Tag({
  kind = 'neutral',
  children,
  onClick,
  title,
}: {
  kind?: 'ok' | 'warn' | 'err' | 'info' | 'neutral';
  children: ReactNode;
  onClick?: () => void;
  title?: string;
}) {
  return (
    <span className={`tag ${kind} ${onClick ? 'clickable' : ''}`} onClick={onClick} title={title} role={onClick ? 'button' : undefined} tabIndex={onClick ? 0 : undefined}>
      {children}
    </span>
  );
}

// ===== Field =====
export function Field({
  label,
  error,
  children,
  hint,
}: {
  label: string;
  error?: string;
  children: ReactNode;
  hint?: string;
}) {
  return (
    <div className="field">
      <label>{label}</label>
      {children}
      {error ? (
        <span className="err-text" role="alert">
          {error}
        </span>
      ) : hint ? (
        <span style={{ color: 'var(--faint)', fontSize: 'var(--fs-caption)' }}>{hint}</span>
      ) : null}
    </div>
  );
}

// ===== Modal =====
export function Modal({
  title,
  danger,
  onClose,
  children,
  wide,
}: {
  title: ReactNode;
  danger?: boolean;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="modal-mask" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal ${danger ? 'danger' : ''}`} role="dialog" aria-modal="true" style={wide ? { width: 'min(720px, 94vw)' } : undefined}>
        <h3>
          {title}
          <span style={{ flex: 1 }} />
          <button className="btn ghost sm" onClick={onClose} aria-label="关闭">
            <IcX width={14} height={14} />
          </button>
        </h3>
        {children}
      </div>
    </div>
  );
}

// ===== Toast 容器 =====
export function ToastHost() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  const icon = { ok: '✓', warn: '!', err: '✕', info: 'i' } as const;
  return (
    <div className="toast-wrap" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`} onClick={() => dismiss(t.id)} role="status">
          <span style={{ fontWeight: 700 }}>{icon[t.kind]}</span>
          <span>{t.text}</span>
        </div>
      ))}
    </div>
  );
}

// ===== EmptyState =====
export function EmptyState({
  icon,
  title,
  desc,
  action,
}: {
  icon?: ReactNode;
  title: string;
  desc?: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      {icon}
      <div className="empty-title">{title}</div>
      {desc && <div>{desc}</div>}
      {action}
    </div>
  );
}

// ===== 终端风格加载条 =====
export function LineLoader({ text }: { text?: string }) {
  return (
    <div style={{ color: 'var(--acc2)', fontSize: 'var(--fs-caption)', fontFamily: 'var(--font-body)' }}>
      <span style={{ animation: 'cursor-blink 1s steps(1) infinite' }}>█</span> {text ?? 'working…'}
    </div>
  );
}
