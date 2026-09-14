// 项目专属 SVG 图标集（design-plan 第 4 节）
// 意象来自画面句：终端窗口 / 示波波形 / SSH 管道 / 盾 / 记忆卡 / 信号灯 / 回滚
// 统一 24×24、stroke-width 1.5、round cap/join、主笔 currentColor
import type { SVGProps } from 'react';

const base = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.5,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true as const,
  focusable: false as const,
};

export const IcTerminal = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="2.5" y="4" width="19" height="16" rx="1.5" /><path d="M6.5 9l3 3-3 3M12 15h5" /></svg>
);

// 签名图标：示波器波形（monitor 用）
export const IcWave = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M2.5 12h3l2-6 3.5 12 2.5-8 1.5 2h7" /></svg>
);

export const IcPipe = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><circle cx="6" cy="6" r="2.4" /><circle cx="18" cy="18" r="2.4" /><path d="M8.4 6H14a3 3 0 0 1 3 3v6.6" /><path d="M6 8.4V14a3 3 0 0 0 3 3h6.6" /></svg>
);

export const IcShield = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 2.5l7 2.6v5.6c0 4.6-3 8.3-7 10.2-4-1.9-7-5.6-7-10.2V5.1l7-2.6z" /><path d="M8.8 11.8l2.2 2.2 4.2-4.4" /></svg>
);

export const IcMemory = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="3" y="5.5" width="18" height="13" rx="1.5" /><path d="M8 5.5v-2M12 5.5v-2M16 5.5v-2M8 20.5v-2M12 20.5v-2M16 20.5v-2M6 10h2M6 14h2M16 10h2M16 14h2" /></svg>
);

export const IcSignal = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><circle cx="12" cy="12" r="8.5" /><path d="M12 8v4l2.8 2.2" /></svg>
);

export const IcRollback = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M3.5 8.5h11a5 5 0 0 1 0 10H9" /><path d="M8 4.5L3.5 8.5 8 12.5" /></svg>
);

export const IcGear = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><circle cx="12" cy="12" r="3" /><path d="M12 2.8v2.4M12 18.8v2.4M4.9 4.9l1.7 1.7M17.4 17.4l1.7 1.7M2.8 12h2.4M18.8 12h2.4M4.9 19.1l1.7-1.7M17.4 6.6l1.7-1.7" /></svg>
);

export const IcSearch = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><circle cx="10.5" cy="10.5" r="6.5" /><path d="M15.5 15.5L21 21" /></svg>
);

export const IcEdit = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M14.5 4.5l5 5L9 20H4v-5L14.5 4.5z" /><path d="M12 7l5 5" /></svg>
);

export const IcTrash = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M4 6.5h16M9.5 6.5v-2h5v2M6.5 6.5l1 13h9l1-13M10 10.5v5.5M14 10.5v5.5" /></svg>
);

export const IcPlus = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 5v14M5 12h14" /></svg>
);

export const IcX = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M6 6l12 12M18 6L6 18" /></svg>
);

export const IcCheck = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M4.5 12.5l5 5L19.5 6.5" /></svg>
);

export const IcChevron = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M6 9l6 6 6-6" /></svg>
);

export const IcFolder = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M3 6.5h6l2 2.5h10v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18V6.5z" /></svg>
);

export const IcUpload = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 15V4M7.5 8.5L12 4l4.5 4.5M4 15.5v3.5h16v-3.5" /></svg>
);

export const IcDownload = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M4 15.5v3.5h16v-3.5" /></svg>
);

export const IcAlert = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 3.5L2.5 20h19L12 3.5z" /><path d="M12 9.5v4.5M12 17.2v.1" /></svg>
);

export const IcAi = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 2.5l1.6 4.9L18.5 9l-4.9 1.6L12 15.5l-1.6-4.9L5.5 9l4.9-1.6L12 2.5z" /><path d="M19 14l.7 2.3L22 17l-2.3.7L19 20l-.7-2.3L16 17l2.3-.7L19 14z" /></svg>
);

export const IcCopy = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="8.5" y="8.5" width="12" height="12" rx="1.5" /><path d="M15.5 8.5v-3a1.5 1.5 0 0 0-1.5-1.5H5A1.5 1.5 0 0 0 3.5 5.5v9A1.5 1.5 0 0 0 5 16h3.5" /></svg>
);

export const IcRefresh = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M20 12a8 8 0 1 1-2.3-5.7M20 3.5V8h-4.5" /></svg>
);

export const IcCpu = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="6" y="6" width="12" height="12" rx="1.5" /><rect x="9.5" y="9.5" width="5" height="5" /><path d="M9 2.5v3M15 2.5v3M9 18.5v3M15 18.5v3M2.5 9h3M2.5 15h3M18.5 9h3M18.5 15h3" /></svg>
);

export const IcMem = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="2.5" y="7" width="19" height="10" rx="1.5" /><path d="M6 7v10M10 7v10M14 7v10M18 7v10" /></svg>
);

export const IcDisk = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><circle cx="12" cy="12" r="8.5" /><circle cx="12" cy="12" r="2.5" /><path d="M12 3.5a8.5 8.5 0 0 1 8.5 8.5h-5" /></svg>
);

export const IcNet = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M3.5 8.5h17M3.5 15.5h17" /><rect x="7" y="5" width="10" height="14" rx="1.5" /></svg>
);

export const IcSave = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M4.5 3.5h13L21 7v13.5H4.5z" /><path d="M8 3.5v5h8v-5M8 20.5v-6h8v6" /></svg>
);

export const IcImport = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M12 16V4M7.5 8.5L12 4l4.5 4.5" /><path d="M4 15.5v4h16v-4" /></svg>
);

export const IcFilter = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M3.5 5.5h17l-6.5 7.5v5.5l-4 2v-7.5L3.5 5.5z" /></svg>
);

export const IcThumbsUp = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M7 10.5v10H4v-10h3zM7 10.5L11 3a2.2 2.2 0 0 1 2.2 2.4L12.5 9H19a2 2 0 0 1 2 2.4l-1.4 7A2 2 0 0 1 17.6 20.5H7" /></svg>
);

export const IcThumbsDown = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M17 13.5v-10h3v10h-3zM17 13.5L13 21a2.2 2.2 0 0 1-2.2-2.4l.7-3.6H5a2 2 0 0 1-2-2.4l1.4-7A2 2 0 0 1 6.4 3.5H17" /></svg>
);

export const IcEye = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" /><circle cx="12" cy="12" r="3" /></svg>
);

export const IcEyeOff = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M4 4l16 16M9.5 6.2A9.8 9.8 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17.6 17.6 0 0 1-3 3.7M6.2 7.5A17 17 0 0 0 2.5 12S6 18.5 12 18.5a9.4 9.4 0 0 0 4-.9" /></svg>
);

export const IcLink = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><path d="M9.5 14.5l5-5M8 12l-2.5 2.5a3.5 3.5 0 0 0 5 5L13 17M16 12l2.5-2.5a3.5 3.5 0 0 0-5-5L11 7" /></svg>
);

export const IcLock = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="5" y="10.5" width="14" height="10" rx="1.5" /><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3" /></svg>
);

// 签名图标：块状终端光标
export const IcCursor = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base} {...p}><rect x="6" y="4" width="8" height="16" rx="1" /><path d="M16 8l4 4-4 4" /></svg>
);
