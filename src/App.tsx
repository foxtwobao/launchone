import { useEffect, useRef, useState } from "react";
import { ArrowUpRight, Check, ChevronDown, CircleHelp, LoaderCircle, LogIn, LogOut, ShieldCheck, Sparkles, X } from "lucide-react";
import { AppCard } from "./components/AppCard";
import { bridge as defaultBridge } from "./lib/bridge";
import type { Bridge, Progress, Snapshot } from "./lib/types";

export default function App({ api = defaultBridge }: { api?: Bridge }) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [loadingError, setLoadingError] = useState<string>();
  const [loginBusy, setLoginBusy] = useState(false);
  const [busyApp, setBusyApp] = useState<string>();
  const [accountBusy, setAccountBusy] = useState(false);
  const [progress, setProgress] = useState<Progress>();
  const [errors, setErrors] = useState<Record<string, string | undefined>>({});
  const [notices, setNotices] = useState<Record<string, string | undefined>>({});
  const [accountError, setAccountError] = useState<string>();
  const [helpOpen, setHelpOpen] = useState(false);
  const mounted = useRef(false);
  const busy = loginBusy || Boolean(busyApp) || accountBusy;

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
      if (next.account && mounted.current) await prepareDefaults().catch(error => setAccountError(String(error)));
    })().catch(() => { if (mounted.current) setLoadingError("暂时无法检测应用，请重试。"); });
    return () => { mounted.current = false; unsubscribe?.(); };
  }, [api]);

  useEffect(() => {
    // Return from a Windows installer or from the original app: read actual state.
    const focused = () => {
      if (!busy) void refresh().then(next => {
        const newlyInstalled = next.apps.some(a => a.installed && !snapshot?.apps.find(old => old.manifest.id === a.manifest.id)?.installed);
        if (newlyInstalled && next.account) return prepareDefaults().catch(error => setAccountError(String(error)));
      }).catch(() => {});
    };
    window.addEventListener("focus", focused);
    return () => window.removeEventListener("focus", focused);
  }, [api, busy, snapshot]);

  async function prepareDefaults() {
    setBusyApp("defaults");
    try {
      const failures = await api.defaults();
      if (mounted.current) setErrors(prev => ({ ...prev, ...Object.fromEntries(failures.map(f => [f.appId, f.message])) }));
      await refresh();
    } finally { if (mounted.current) { setBusyApp(undefined); setProgress(undefined); } }
  }

  async function login() {
    setLoginBusy(true); setAccountError(undefined); setProgress(undefined);
    try { await api.login(); await refresh(); await prepareDefaults(); }
    catch (error) { if (String(error) !== "已取消操作") setAccountError(String(error)); }
    finally { setLoginBusy(false); setProgress(undefined); }
  }

  async function logout() {
    setAccountBusy(true); setAccountError(undefined);
    try { await api.logout(); await refresh(); }
    catch (error) { setAccountError(String(error)); }
    finally { setAccountBusy(false); }
  }

  async function operate(appId: string, kind: "enable" | "official" | "restore" | "open" | "install") {
    setBusyApp(appId);
    setErrors(prev => ({ ...prev, [appId]: undefined }));
    setNotices(prev => ({ ...prev, [appId]: undefined }));
    setProgress({ appId, stage: "starting", message: kind === "open" ? "正在打开" : kind === "install" ? "正在安装" : "正在切换" });
    try {
      if (kind === "install") {
        const installed = await api.install(appId);
        if (installed) {
          const next = await refresh();
          if (next.account) await prepareDefaults();
        }
        setNotices(prev => ({ ...prev, [appId]: installed ? "安装完成，可打开应用。" : "已打开官方下载页，完成安装后返回此处重新检测。" }));
      } else {
        await api[kind](appId);
        if (kind !== "open") setNotices(prev => ({ ...prev, [appId]: kind === "restore" ? "已恢复原配置，请先退出原应用再打开。" : kind === "official" ? "已切回官方订阅，请先退出原应用再打开；登录和订阅状态由官方客户端确认。" : "已切换到 TokenOne，请先退出原应用，再点击打开。" }));
      }
    } catch (error) { if (String(error) !== "已取消操作") setErrors(prev => ({ ...prev, [appId]: String(error) })); }
    finally {
      await refresh().catch(() => setLoadingError("操作已结束，暂时无法刷新状态，请重试。"));
      setBusyApp(undefined); setProgress(undefined);
    }
  }

  return <div className="shell">
    <header className="app-header">
      <a className="wordmark" href="#main" aria-label="LaunchOne 首页"><span className="brand-mark" aria-hidden="true"><ArrowUpRight size={20} strokeWidth={2.5} /></span>Launch<span>One</span></a>
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
      {accountError && <p className="account-error" role="alert">{accountError}</p>}
      {snapshot && !snapshot.desktop && <p className="environment-note"><CircleHelp size={15} />当前为界面预览。登录、安装和配置需在桌面应用中完成。</p>}
      {snapshot?.desktop && (!snapshot.loginReady || !snapshot.tokenoneReady) && <p className="environment-note"><CircleHelp size={15} />此开发版本尚未开放 TokenOne 接入，服务配置完成后即可登录启用。</p>}

      <section className="applications" aria-labelledby="applications-title">
        <div className="section-heading"><div><h2 id="applications-title">选择你的应用</h2><p>已安装直接打开，使用方式随时切换。</p></div><button className="text-button" disabled={busy} onClick={() => void refresh().catch(() => setLoadingError("暂时无法检测应用，请重试。"))}>重新检测</button></div>
        {loadingError ? <div className="load-error" role="alert">{loadingError}<button className="text-button" onClick={() => void refresh().catch(() => {})}>重新检测</button></div> : !snapshot ? <div className="loading-state" role="status"><LoaderCircle className="spin" size={20} />正在检测工具…</div> : <div className="app-grid">{snapshot.apps.map(app => <AppCard key={app.manifest.id} app={app} signedIn={Boolean(snapshot.account)} desktop={snapshot.desktop} ready={snapshot.tokenoneReady} busy={busy} progress={progress?.appId === app.manifest.id ? progress : undefined} error={errors[app.manifest.id]} notice={notices[app.manifest.id]} onCancel={() => void api.cancelLogin()} onOpen={() => void operate(app.manifest.id, "open")} onInstall={() => void operate(app.manifest.id, "install")} onEnable={() => void operate(app.manifest.id, "enable")} onOfficial={() => void operate(app.manifest.id, "official")} onRestore={() => void operate(app.manifest.id, "restore")} />)}</div>}
      </section>

      <div className="assurance"><ShieldCheck size={17} /><span>保留原有设置</span><i /><span>随时切回订阅</span><i /><span>配置完成即可关闭</span></div>
    </main>

    <footer><span className="footer-brand"><Sparkles size={13} />Made for your next idea.</span><button className="help-button" onClick={() => setHelpOpen(!helpOpen)} aria-expanded={helpOpen}><CircleHelp size={14} />使用帮助</button></footer>
    {helpOpen && <aside className="help-panel" aria-label="使用帮助"><button className="icon-button" aria-label="关闭帮助" onClick={() => setHelpOpen(false)}><X size={16} /></button><h2>配置好，就可以继续工作。</h2><p>已安装应用直接点击“打开”。首次登录 IDONE 后默认准备 TokenOne，之后保留你选择的使用方式；未安装的应用点击“安装”。</p><p>在卡片上切换“TokenOne / 官方订阅”，切换后重新打开原应用。官方登录与订阅资格由原应用确认。“更多”中的“恢复原配置”用于撤销 LaunchOne 的修改，并保留无关设置。</p></aside>}
  </div>;
}
