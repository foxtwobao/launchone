import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDownToLine, ArrowUpRight, Check, ChevronDown, CircleHelp, LoaderCircle, LogIn, LogOut, ShieldCheck, Sparkles, X } from "lucide-react";
import { AppCard } from "./components/AppCard";
import { ReopenPrompt } from "./components/ReopenPrompt";
import { RecoveryActions } from "./components/RecoveryActions";
import { failureFrom, type Failure } from "./lib/failure";
import { bridge as defaultBridge } from "./lib/bridge";
import type { Bridge, Progress, Snapshot, UpdateInfo } from "./lib/types";
import { version } from "../package.json";

const updateCheckInterval = 24 * 60 * 60 * 1000;
type Operation = "enable" | "official" | "restore" | "open" | "install" | "reopen";
interface Intent { appId: string; kind: Operation }

export default function App({ api = defaultBridge }: { api?: Bridge }) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [loadingError, setLoadingError] = useState<string>();
  const [loginBusy, setLoginBusy] = useState(false);
  const [busyApp, setBusyApp] = useState<string>();
  const [accountBusy, setAccountBusy] = useState(false);
  const [progress, setProgress] = useState<Progress>();
  const [errors, setErrors] = useState<Record<string, Failure | undefined>>({});
  const [notices, setNotices] = useState<Record<string, string | undefined>>({});
  const [accountError, setAccountError] = useState<Failure>();
  const [helpOpen, setHelpOpen] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string>();
  const [updateError, setUpdateError] = useState<string>();
  const [reopenPrompt, setReopenPrompt] = useState<{ appIds: string[]; alreadyRunning: boolean } | null>(null);
  const mounted = useRef(false);
  const updateCheckInFlight = useRef(false);
  const lastUpdateCheckAt = useRef(0);
  const retryActions = useRef<Record<string, Operation>>({});
  const loginIntent = useRef<Intent>();
  const busy = loginBusy || Boolean(busyApp) || accountBusy || Boolean(reopenPrompt) || updateInstalling;

  const checkForUpdates = useCallback(async (manual = false) => {
    if (!snapshot?.desktop || updateCheckInFlight.current) return;
    updateCheckInFlight.current = true;
    setUpdateChecking(true);
    if (manual) { setUpdateError(undefined); setUpdateMessage(undefined); }
    try {
      const found = await api.checkUpdate();
      if (mounted.current) {
        setUpdateInfo(found);
        lastUpdateCheckAt.current = Date.now();
        if (manual) setUpdateMessage(found ? undefined : "已是最新版本");
      }
    } catch (error) {
      if (manual && mounted.current) setUpdateError(String(error) || "检查更新失败，请稍后重试。");
    } finally {
      updateCheckInFlight.current = false;
      if (mounted.current) setUpdateChecking(false);
    }
  }, [api, snapshot?.desktop]);

  async function installUpdate() {
    setUpdateInstalling(true);
    setUpdateError(undefined);
    try { await api.installUpdate(); }
    catch (error) {
      if (mounted.current) {
        setUpdateError(String(error));
        setUpdateInstalling(false);
        await checkForUpdates();
      }
    }
  }

  async function refresh() {
    const next = await api.snapshot();
    if (mounted.current) { setSnapshot(next); setLoadingError(undefined); }
    return next;
  }

  useEffect(() => {
    mounted.current = true;
    let unsubscribe: (() => void) | undefined;
    void (async () => {
      const stop = await api.subscribe(p => { if (mounted.current) setProgress(p); });
      if (!mounted.current) { stop(); return; }
      unsubscribe = stop;
      const next = await refresh();
      if (next.account && mounted.current) await prepareDefaults().catch(error => setAccountError(failureFrom(error)));
    })().catch(() => { if (mounted.current) setLoadingError("暂时无法检测应用，请重试。"); });
    return () => { mounted.current = false; unsubscribe?.(); };
  }, [api]);

  useEffect(() => {
    // Return from a Windows installer or from the original app: read actual state.
    const focused = () => {
      if (!busy) void refresh().then(next => {
        const newlyInstalled = next.apps.some(a => a.installed && !snapshot?.apps.find(old => old.manifest.id === a.manifest.id)?.installed);
        if (newlyInstalled && next.account) return prepareDefaults().catch(error => setAccountError(failureFrom(error)));
      }).catch(() => {});
      if (!busy && snapshot?.desktop && Date.now() - lastUpdateCheckAt.current >= updateCheckInterval) {
        void checkForUpdates();
      }
    };
    window.addEventListener("focus", focused);
    return () => window.removeEventListener("focus", focused);
  }, [api, busy, snapshot, checkForUpdates]);

  useEffect(() => {
    if (!snapshot?.desktop) return;
    void checkForUpdates();
    const timer = window.setInterval(() => void checkForUpdates(), updateCheckInterval);
    return () => window.clearInterval(timer);
  }, [snapshot?.desktop, checkForUpdates]);

  async function prepareDefaults() {
    setBusyApp("defaults");
    try {
      const failures = await api.defaults();
      for (const f of failures) retryActions.current[f.appId] = "enable";
      if (mounted.current) setErrors(prev => ({ ...prev, ...Object.fromEntries(failures.map(f => [f.appId, { message: f.message, action: f.action ?? undefined }])) }));
      const next = await refresh();
      const appIds = next.apps.filter(a => a.installed && a.restartRequired).map(a => a.manifest.id);
      if (mounted.current && appIds.length) setReopenPrompt({ appIds, alreadyRunning: false });
    } finally { if (mounted.current) { setBusyApp(undefined); setProgress(undefined); } }
  }

  async function login(intent?: Intent) {
    if (intent) loginIntent.current = intent;
    setLoginBusy(true); setAccountError(undefined); setProgress(undefined);
    try {
      await api.login(); await refresh();
      const next = loginIntent.current;
      loginIntent.current = undefined;
      if (next) await operate(next.appId, next.kind);
      else await prepareDefaults();
    }
    catch (error) { const failure = failureFrom(error); if (failure.message !== "已取消操作") setAccountError(failure); else loginIntent.current = undefined; }
    finally { setLoginBusy(false); setProgress(undefined); }
  }

  async function logout() {
    setAccountBusy(true); setAccountError(undefined);
    try { await api.logout(); loginIntent.current = undefined; setErrors({}); setNotices({}); await refresh(); }
    catch (error) { setAccountError(failureFrom(error)); }
    finally { setAccountBusy(false); }
  }

  async function openPortal(appId?: string) {
    try { await api.openTokenonePortal(); }
    catch (error) {
      if (appId) setErrors(prev => ({ ...prev, [appId]: failureFrom(error) }));
      else setAccountError(failureFrom(error));
    }
  }

  async function operate(appId: string, kind: Operation) {
    let changed = false;
    retryActions.current[appId] = kind;
    setBusyApp(appId);
    setErrors(prev => ({ ...prev, [appId]: undefined }));
    setNotices(prev => ({ ...prev, [appId]: undefined }));
    setProgress({ appId, stage: "starting", message: kind === "open" ? "正在打开" : kind === "reopen" ? "正在重新打开应用" : kind === "install" ? "正在安装" : "正在切换" });
    try {
      if (kind === "install") {
        const installed = await api.install(appId);
        if (installed) {
          const next = await refresh();
          if (next.account) await prepareDefaults();
        }
        setNotices(prev => ({ ...prev, [appId]: installed ? "安装完成。登录 IDONE 后可连接 TokenOne；已登录时会自动准备。" : "已打开官方安装入口，完成安装后返回这里，继续为你配置。" }));
      } else if (kind === "open") {
        const outcome = await api.open(appId);
        if (outcome !== "opened") setReopenPrompt({ appIds: [appId], alreadyRunning: outcome === "already_running" });
      } else {
        await api[kind](appId);
        changed = kind !== "reopen";
        const message = kind === "reopen" ? "已重新打开应用。"
          : kind === "restore" ? "已恢复原配置，重新打开应用后生效。"
          : kind === "official" ? appId === "codex" ? "已切回官方订阅，重新打开 Codex 后请新建对话。" : "已切回官方订阅，重新打开应用后生效。"
          : appId === "codex" ? "已切换到 TokenOne，重新打开 Codex 后请新建对话。" : "已切换到 TokenOne，重新打开应用后生效。";
        setNotices(prev => ({ ...prev, [appId]: message }));
      }
    } catch (error) { const failure = failureFrom(error); if (failure.message !== "已取消操作") setErrors(prev => ({ ...prev, [appId]: failure })); }
    finally {
      await refresh().then(next => {
        if (changed && next.apps.find(a => a.manifest.id === appId)?.restartRequired) setReopenPrompt({ appIds: [appId], alreadyRunning: false });
      }).catch(() => setLoadingError("操作已结束，暂时无法刷新状态，请重试。"));
      setBusyApp(undefined); setProgress(undefined);
    }
  }

  return <div className="shell">
    <header className="app-header">
      <a className="wordmark" href="#main" aria-label={`LaunchOne v${version} 首页`}><span className="brand-mark" aria-hidden="true"><ArrowUpRight size={20} strokeWidth={2.5} /></span>Launch<span>One</span><span className="wordmark-version">v{version}{import.meta.env.VITE_BUILD_ENV === "lab" ? " · Lab" : ""}</span></a>
      <div className="header-right"><span className="product-tag">FOR TOKENONE</span>
        {snapshot?.account && <details className="account-menu"><summary><span className="avatar">{snapshot.account.name.slice(0, 1)}</span>{snapshot.account.name}<ChevronDown size={14} /></summary><div className="menu-panel"><p>退出不会清除工具配置</p><button disabled={busy} onClick={() => void logout()}><LogOut size={15} />退出 IDONE</button></div></details>}
      </div>
    </header>

    <main id="main">
      <section className="intro" aria-labelledby="page-title">
        <div className="eyebrow"><span />少一点配置，多一点创造</div>
        <h1 id="page-title">你的工具，<span>即刻就绪。</span></h1>
        <p>登录 IDONE，把熟悉的 AI 工具接入 TokenOne。<br className="desktop-break" />打开、切换，简单完成。</p>
      </section>
      {loginBusy && progress?.stage === "login" && <p className="environment-note" role="status">{progress.message}</p>}

      <section className={`login-strip ${snapshot?.account ? "is-signed-in" : ""}`} aria-label="IDONE 账号">
        <div className="login-symbol" aria-hidden="true">{snapshot?.account ? <Check size={22} /> : <LogIn size={23} />}</div>
        <div className="login-copy"><h2>{snapshot?.account ? `你好，${snapshot.account.name}` : "连接你的 IDONE"}</h2><p>{snapshot?.account ? "首次登录默认准备 TokenOne，之后保留你的使用方式。" : "使用同一个账号，无需手动填写地址和密钥。"}</p></div>
        {snapshot?.account ? <span className="account-connected"><span />已登录</span> : <button className="primary-button" onClick={() => void login()} disabled={busy || !snapshot?.desktop || !snapshot.loginReady}>{loginBusy ? <><LoaderCircle className="spin" size={16} />等待浏览器授权</> : <>登录 IDONE<ArrowUpRight size={16} /></>}</button>}
        {loginBusy && <button className="icon-button" aria-label="取消登录" onClick={() => void api.cancelLogin()}><X size={17} /></button>}
      </section>
      {accountError && <><p className="account-error" role="alert">{accountError.message}</p><RecoveryActions failure={accountError} disabled={busy || !snapshot?.desktop} portalReady={Boolean(snapshot?.tokenonePortalReady)} onRetry={() => void login()} onLogin={() => void login()} onPortal={() => void openPortal()} /></>}
      {snapshot && !snapshot.desktop && <p className="environment-note"><CircleHelp size={15} />当前为界面预览。登录、安装和配置需在桌面应用中完成。</p>}
      {snapshot?.desktop && (!snapshot.loginReady || !snapshot.tokenoneReady) && <p className="environment-note"><CircleHelp size={15} />暂时无法读取接入配置，请检查网络后重新打开 LaunchOne。已安装的应用仍可打开。</p>}

      <section className="applications" aria-labelledby="applications-title">
        <div className="section-heading"><div><h2 id="applications-title">选择你的应用</h2><p>已安装直接打开，使用方式随时切换。</p></div><button className="text-button" disabled={busy} onClick={() => void refresh().catch(() => setLoadingError("暂时无法检测应用，请重试。"))}>重新检测</button></div>
        {loadingError ? <div className="load-error" role="alert">{loadingError}<button className="text-button" onClick={() => void refresh().catch(() => {})}>重新检测</button></div> : !snapshot ? <div className="loading-state" role="status"><LoaderCircle className="spin" size={20} />正在检测工具…</div> : <div className="app-grid">{snapshot.apps.map(app => <AppCard key={app.manifest.id} app={app} signedIn={Boolean(snapshot.account)} desktop={snapshot.desktop} ready={snapshot.tokenoneReady} busy={busy} progress={progress?.appId === app.manifest.id ? progress : undefined} error={errors[app.manifest.id]} notice={notices[app.manifest.id]} onCancel={() => void api.cancelLogin()} onOpen={() => void operate(app.manifest.id, "open")} onInstall={() => void operate(app.manifest.id, "install")} onEnable={() => void operate(app.manifest.id, "enable")} onOfficial={() => void operate(app.manifest.id, "official")} onRestore={() => void operate(app.manifest.id, "restore")} portalReady={snapshot.tokenonePortalReady} onRetry={() => void operate(app.manifest.id, retryActions.current[app.manifest.id] ?? "enable")} onLogin={() => void login({appId: app.manifest.id, kind: retryActions.current[app.manifest.id] ?? "enable"})} onPortal={() => void openPortal(app.manifest.id)} />)}</div>}
      </section>

      <div className="assurance"><ShieldCheck size={17} /><span>保留原有设置</span><i /><span>随时切回订阅</span><i /><span>配置完成即可关闭</span></div>
    </main>

    <footer><span className="footer-brand"><Sparkles size={13} />LaunchOne v{version}
      <button className="update-button" onClick={() => updateInfo ? void installUpdate() : void checkForUpdates(true)} disabled={!snapshot?.desktop || updateChecking || busy} title={updateInfo?.notes || "检查 LaunchOne 更新"}>
        {updateChecking || updateInstalling ? <LoaderCircle className="spin" size={13} /> : updateInfo ? <ArrowDownToLine size={13} /> : null}
        {updateInstalling ? "正在安装" : updateChecking ? "检查中" : updateInfo ? `更新到 v${updateInfo.version}` : "检查更新"}
      </button>
      {updateMessage && <span role="status">{updateMessage}</span>}
      {updateError && <span className="update-error" role="alert">{updateError}</span>}
    </span><button className="help-button" onClick={() => setHelpOpen(!helpOpen)} aria-expanded={helpOpen}><CircleHelp size={14} />使用帮助</button></footer>
    {helpOpen && <aside className="help-panel" aria-label="使用帮助"><button className="icon-button" aria-label="关闭帮助" onClick={() => setHelpOpen(false)}><X size={16} /></button><h2>安装、连接、打开</h2><p>未安装的应用点“安装”；登录 IDONE 后自动连接 TokenOne。已安装的应用可以直接点“打开”。</p><p>选择“TokenOne”或“官方订阅”后，按提示重新打开应用。Codex 切换后请新建对话，旧对话可能仍使用原来的 TokenOne 或官方订阅。</p><p>TokenOne 无法使用时，在“高级”中点“重新连接 TokenOne”。“恢复原配置”可恢复接入前的连接设置，保留聊天记录。</p><p>官方账号的登录和订阅请在原应用里管理。</p></aside>}
    {reopenPrompt && <ReopenPrompt alreadyRunning={reopenPrompt.alreadyRunning} names={reopenPrompt.appIds.map(id => snapshot?.apps.find(a => a.manifest.id === id)?.manifest.name ?? id)} onLater={() => setReopenPrompt(null)} onConfirm={() => {
      const ids = reopenPrompt.appIds;
      setReopenPrompt(null);
      void (async () => { for (const id of ids) await operate(id, "reopen"); })();
    }} />}
  </div>;
}
