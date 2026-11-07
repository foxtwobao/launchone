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
  }

  useEffect(() => {
    mounted.current = true;
    void refresh().catch(() => setLoadingError("暂时无法检测工具，请重试。"));
    let unsubscribe: (() => void) | undefined;
    void api.subscribe(p => { if (mounted.current) setProgress(p); })
      .then(stop => { if (mounted.current) unsubscribe = stop; else stop(); })
      .catch(() => {});
    return () => { mounted.current = false; unsubscribe?.(); };
  }, [api]);

  async function login() {
    setLoginBusy(true); setAccountError(undefined); setProgress(undefined);
    try { await api.login(); await refresh(); }
    catch (error) { if (String(error) !== "已取消操作") setAccountError(String(error)); }
    finally { setLoginBusy(false); setProgress(undefined); }
  }

  async function logout() {
    setAccountBusy(true); setAccountError(undefined);
    try { await api.logout(); await refresh(); }
    catch (error) { setAccountError(String(error)); }
    finally { setAccountBusy(false); }
  }

  async function operate(appId: string, kind: "enable" | "official" | "restore") {
    setBusyApp(appId);
    setErrors(prev => ({ ...prev, [appId]: undefined }));
    setNotices(prev => ({ ...prev, [appId]: undefined }));
    setProgress({ appId, stage: "starting", message: "正在准备" });
    try {
      await api[kind](appId);
      setNotices(prev => ({ ...prev, [appId]: kind === "restore" ? "已撤销配置修改，请重新打开工具。" : kind === "official" ? "已切换，请重新打开工具。" : "配置完成，在原有终端或编辑器中使用即可。" }));
    } catch (error) { setErrors(prev => ({ ...prev, [appId]: String(error) })); }
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
        <p>登录 IDONE，把熟悉的 AI 工具接入 TokenOne。<br className="desktop-break" />安装、配置，一次完成。</p>
      </section>
      {loginBusy && progress?.stage === "login" && <p className="environment-note" role="status">{progress.message}</p>}

      <section className={`login-strip ${snapshot?.account ? "is-signed-in" : ""}`} aria-label="IDONE 账号">
        <div className="login-symbol" aria-hidden="true">{snapshot?.account ? <Check size={22} /> : <LogIn size={23} />}</div>
        <div className="login-copy"><h2>{snapshot?.account ? `你好，${snapshot.account.name}` : "连接你的 IDONE"}</h2><p>{snapshot?.account ? "账户已连接，选择下方工具即可启用。" : "使用同一个账号，无需手动填写地址和密钥。"}</p></div>
        {snapshot?.account ? <span className="account-connected"><span />已登录</span> : <button className="primary-button" onClick={() => void login()} disabled={busy || !snapshot?.desktop || !snapshot.loginReady}>{loginBusy ? <><LoaderCircle className="spin" size={16} />等待浏览器授权</> : <>登录 IDONE<ArrowUpRight size={16} /></>}</button>}
        {loginBusy && <button className="icon-button" aria-label="取消登录" onClick={() => void api.cancelLogin()}><X size={17} /></button>}
      </section>
      {accountError && <p className="account-error" role="alert">{accountError}</p>}
      {snapshot && !snapshot.desktop && <p className="environment-note"><CircleHelp size={15} />当前为界面预览。登录、安装和配置需在桌面应用中完成。</p>}
      {snapshot?.desktop && (!snapshot.loginReady || !snapshot.tokenoneReady) && <p className="environment-note"><CircleHelp size={15} />此开发版本尚未开放 TokenOne 接入，服务配置完成后即可登录启用。</p>}

      <section className="applications" aria-labelledby="applications-title">
        <div className="section-heading"><div><h2 id="applications-title">选择你的工具</h2><p>已有安装直接复用，缺少时自动安装。</p></div><span className="section-caption">只需一键启用</span></div>
        {loadingError ? <div className="load-error" role="alert">{loadingError}<button className="text-button" onClick={() => void refresh().catch(() => {})}>重新检测</button></div> : !snapshot ? <div className="loading-state" role="status"><LoaderCircle className="spin" size={20} />正在检测工具…</div> : <div className="app-grid">{snapshot.apps.map(app => <AppCard key={app.manifest.id} app={app} signedIn={Boolean(snapshot.account)} desktop={snapshot.desktop} ready={snapshot.tokenoneReady} busy={busy} progress={busyApp === app.manifest.id ? progress : undefined} error={errors[app.manifest.id]} notice={notices[app.manifest.id]} onCancel={() => void api.cancelLogin()} onEnable={() => void operate(app.manifest.id, "enable")} onOfficial={() => void operate(app.manifest.id, "official")} onRestore={() => void operate(app.manifest.id, "restore")} />)}</div>}
      </section>

      <div className="assurance"><ShieldCheck size={17} /><span>保留原有设置</span><i /><span>随时切回官方</span><i /><span>配置完成即可关闭</span></div>
    </main>

    <footer><span className="footer-brand"><Sparkles size={13} />Made for your next idea.</span><button className="help-button" onClick={() => setHelpOpen(!helpOpen)} aria-expanded={helpOpen}><CircleHelp size={14} />使用帮助</button></footer>
    {helpOpen && <aside className="help-panel" aria-label="使用帮助"><button className="icon-button" aria-label="关闭帮助" onClick={() => setHelpOpen(false)}><X size={16} /></button><h2>配置好，就可以继续工作。</h2><p>登录 IDONE 后，点击工具的「启用 TokenOne」。准备完成后，在原有终端或编辑器中使用工具；首次使用可在终端运行卡片对应的命令。</p><p>需要官方账号时，打开卡片右上角菜单，选择「使用官方登录」。撤销配置修改会保留你原有的无关设置。</p></aside>}
  </div>;
}
