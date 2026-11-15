import { useEffect, useRef, useState } from "react";
import { ArrowRight, Asterisk, Check, Ellipsis, LoaderCircle, RotateCcw, Terminal, TriangleAlert } from "lucide-react";
import type { AppStatus, Progress } from "../lib/types";
import type { Failure } from "../lib/failure";
import { RecoveryActions } from "./RecoveryActions";

interface Props {
  app: AppStatus;
  signedIn: boolean;
  desktop: boolean;
  ready: boolean;
  busy: boolean;
  progress?: Progress;
  error?: Failure;
  notice?: string;
  onOpen(): void;
  onInstall(): void;
  onEnable(): void;
  onOfficial(): void;
  onRestore(): void;
  onCancel(): void;
  onRetry(): void;
  onLogin(): void;
  onPortal(): void;
  portalReady: boolean;
}

export function AppCard({ app, signedIn, desktop, ready, busy, progress, error, notice, onOpen, onInstall, onEnable, onOfficial, onRestore, onCancel, onRetry, onLogin, onPortal, portalReady }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDetailsElement>(null);
  const active = Boolean(progress);
  const problem = error ?? (app.error ? { message: app.error } : undefined);
  const accountNeedsUpdate = signedIn && app.mode === "tokenone" && (app.credentialAccount === "other" || app.credentialAccount === "unknown");
  const configured = app.installed && app.mode === "tokenone" && !problem && !accountNeedsUpdate;
  const name = app.manifest.name;
  const unavailable = !desktop || !app.supported;
  const canConnect = !busy && !unavailable && app.installed && signedIn && ready && app.tokenoneSupported;

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
  const label = active ? progress!.message : app.installed ? "打开" : "安装";
  const status = active ? progress!.message : problem ? "需要处理" : !desktop ? "等待桌面检测" : !app.supported ? "当前平台暂不支持" : !app.installed ? "尚未安装" : accountNeedsUpdate ? app.credentialAccount === "other" ? "TokenOne 待切换账号" : "TokenOne 待重新连接" : configured ? "已配置 TokenOne" : app.mode === "official" ? "使用官方订阅" : "已安装 · 待选择使用方式";

  return <article className={`app-card ${configured ? "is-connected" : ""} ${problem ? "has-error" : ""}`} aria-labelledby={`name-${app.manifest.id}`}>
    <div className="card-heading">
      <div className={`tool-icon tool-${app.manifest.icon}`} aria-hidden="true">
        {app.manifest.icon === "claude" ? <Asterisk size={34} strokeWidth={1.5} /> : <Terminal size={27} strokeWidth={1.8} />}
      </div>
      <details ref={menu} className="tool-menu" onToggle={event => setMenuOpen(event.currentTarget.open)}>
        <summary aria-label={`${name} 高级操作`} aria-expanded={menuOpen}>高级<Ellipsis size={15} /></summary>
        <div className="menu-panel">
          <button disabled={!canConnect} onClick={() => action(onEnable)}><RotateCcw size={15} />重新连接 TokenOne</button>
          <button disabled={!app.canRestore || busy} onClick={() => action(onRestore)}><RotateCcw size={15} />恢复原配置</button>
          <p>{app.canRestore ? "恢复使用 LaunchOne 前的连接设置，保留聊天记录。" : "暂无可恢复的备份"}</p>
        </div>
      </details>
    </div>
    <h3 id={`name-${app.manifest.id}`}>{name}</h3>
    <p className="tool-description">{app.manifest.description}</p>
    <div className={`tool-status ${problem ? "status-error" : ""}`}>
      {active ? <LoaderCircle size={14} className="spin" /> : configured ? <Check size={14} /> : problem ? <TriangleAlert size={14} /> : <span className="status-dot" />}
      <span>{status}</span>
      {app.version && <span className="tool-version" title={app.version}>{app.version}</span>}
    </div>
    <div className="mode-switch" role="group" aria-label={`${name} 使用方式`}>
      <span>使用方式</span>
      <button aria-label={`${name}：TokenOne`} aria-pressed={app.installed && app.mode === "tokenone"} disabled={!canConnect || (app.mode === "tokenone" && !problem && !accountNeedsUpdate)} onClick={onEnable}>TokenOne</button>
      {app.manifest.supportsOfficial && <button aria-label={`${name}：官方订阅`} aria-pressed={app.installed && app.mode === "official"} disabled={busy || unavailable || !app.installed || app.mode === "official"} onClick={onOfficial}>官方订阅</button>}
    </div>
    {app.installed && app.restartRequired && <p className="mode-hint">设置已更新，点击“打开”后按提示重新打开应用。</p>}
    {accountNeedsUpdate && <div className="account-warning"><p>{app.credentialAccount === "other" ? "TokenOne 还在使用你之前登录的账号。" : "请重新连接 TokenOne，使用当前登录的账号。"}</p><button className="text-button" disabled={!canConnect} onClick={onEnable}>{app.credentialAccount === "other" ? "改用当前账号" : "重新连接 TokenOne"}</button></div>}
    {app.installed && !app.tokenoneSupported && <p className="mode-hint">当前版本未检测到 Codex 网关支持，请先升级官方桌面应用。仍可直接打开使用。</p>}
    {app.installed && !signedIn && <p className="mode-hint">登录 IDONE 后可使用 TokenOne；打开应用和官方订阅无需登录。</p>}
    {!app.installed && !problem && <p className="mode-hint">先安装应用，再登录 IDONE，自动完成 TokenOne 配置。</p>}
    <button className="setup-button" disabled={busy || unavailable || (!app.installed && Boolean(app.error))} onClick={app.installed ? onOpen : onInstall} aria-label={`${name}：${label}`}>
      {active ? <LoaderCircle size={16} className="spin" /> : null}{label}{!active && <ArrowRight size={16} />}
    </button>
    {progress?.stage === "authorization" && <button className="text-button" onClick={onCancel}>取消授权等待</button>}
    {problem && <><p className="inline-error" role="alert">{problem.message}</p><RecoveryActions failure={problem} disabled={busy || !desktop} portalReady={portalReady} onRetry={onRetry} onLogin={onLogin} onPortal={onPortal} /></>}
    {notice && !problem && <p className="inline-notice" role="status">{notice}</p>}
  </article>;
}
