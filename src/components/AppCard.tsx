import { useEffect, useRef, useState } from "react";
import { ArrowRight, Asterisk, Check, ChevronRight, Ellipsis, LoaderCircle, RotateCcw, Terminal, TriangleAlert } from "lucide-react";
import type { AppStatus, Progress } from "../lib/types";

interface Props {
  app: AppStatus;
  signedIn: boolean;
  desktop: boolean;
  ready: boolean;
  busy: boolean;
  progress?: Progress;
  error?: string;
  notice?: string;
  onEnable(): void;
  onOfficial(): void;
  onRestore(): void;
  onCancel(): void;
}

export function AppCard({ app, signedIn, desktop, ready, busy, progress, error, notice, onEnable, onOfficial, onRestore, onCancel }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDetailsElement>(null);
  const active = Boolean(progress);
  const configured = app.installed && app.mode === "tokenone" && !error;
  const problem = error ?? app.error;
  const name = app.manifest.name;
  const blocked = !desktop || !app.supported || !signedIn || !ready;

  useEffect(() => {
    if (!menuOpen) return;
    const outside = (event: MouseEvent) => {
      if (!menu.current?.contains(event.target as Node)) menu.current?.removeAttribute("open");
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        menu.current?.removeAttribute("open");
        menu.current?.querySelector("summary")?.focus();
      }
    };
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("mousedown", outside); document.removeEventListener("keydown", escape); };
  }, [menuOpen]);

  const action = (callback: () => void) => {
    menu.current?.removeAttribute("open");
    callback();
  };
  let label = configured ? "已启用" : !signedIn ? "登录后启用" : problem ? "重试" : "启用 TokenOne";
  if (active) label = progress!.message;
  const status = active ? "正在准备" : problem ? "需要处理" : configured ? "已连接 TokenOne" : !desktop ? "等待桌面检测" : !app.supported ? "当前平台暂不支持" : !app.installed ? "尚未安装" : app.mode === "official" ? "使用官方方式" : "待配置";

  return <article className={`app-card ${configured ? "is-connected" : ""} ${problem ? "has-error" : ""}`} aria-labelledby={`name-${app.manifest.id}`}>
    <div className="card-heading">
      <div className={`tool-icon tool-${app.manifest.icon}`} aria-hidden="true">
        {app.manifest.icon === "claude" ? <Asterisk size={34} strokeWidth={1.5} /> : <Terminal size={27} strokeWidth={1.8} />}
      </div>
      <details ref={menu} className="tool-menu" onToggle={event => setMenuOpen(event.currentTarget.open)}>
        <summary aria-label={`${name} 更多操作`} aria-expanded={menuOpen}><Ellipsis size={20} /></summary>
        <div className="menu-panel">
          {app.manifest.supportsOfficial && <button disabled={!desktop || !app.supported || busy} onClick={() => action(onOfficial)}><ArrowRight size={15} />使用官方登录<ChevronRight size={14} /></button>}
          <button disabled={!app.canRestore || busy} onClick={() => action(onRestore)}><RotateCcw size={15} />撤销配置修改</button>
        </div>
      </details>
    </div>
    <h3 id={`name-${app.manifest.id}`}>{name}</h3>
    <p className="tool-description">{app.manifest.description}</p>
    {configured && <p className="command-hint">在终端运行 <code>{app.manifest.command}</code></p>}
    <div className={`tool-status ${problem ? "status-error" : ""}`}>
      {active ? <LoaderCircle size={14} className="spin" /> : configured ? <Check size={14} /> : problem ? <TriangleAlert size={14} /> : <span className="status-dot" />}
      <span>{status}</span>
      {app.version && <span className="tool-version" title={app.version}>{app.version}</span>}
    </div>
    <button className={`setup-button ${configured ? "connected-button" : ""}`} disabled={busy || blocked || configured} onClick={onEnable} aria-label={`${name}：${label}`}>
      {active ? <LoaderCircle size={16} className="spin" /> : configured ? <Check size={17} /> : null}
      {label}
      {!active && !configured && <ArrowRight size={16} />}
    </button>
    {progress?.stage === "authorization" && <button className="text-button" onClick={onCancel}>取消授权等待</button>}
    {problem && <p className="inline-error" role="alert">{problem}</p>}
    {notice && !problem && <p className="inline-notice" role="status">{notice}</p>}
  </article>;
}
