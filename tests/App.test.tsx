import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, test, expect } from "vitest";
import App from "../src/App";
import type { AppStatus, Bridge, Snapshot } from "../src/lib/types";

function tool(id = "future-tool", name = "Future Tool"): AppStatus {
  return { manifest:{id,name,description:"可扩展桌面应用",command:"",icon:"generic",supportsOfficial:true,platforms:["macos"]},
    installed:true,tokenoneSupported:true,restartRequired:false,version:"1.0",mode:"official",canRestore:false,supported:true,error:null };
}
function setup(overrides:Partial<Snapshot>={}, changes:Partial<Bridge>={}) {
  const snapshot:Snapshot={apps:[tool()],account:{id:"user",name:"开发者"},desktop:true,loginReady:true,tokenoneReady:true,...overrides};
  const api:Bridge={
    snapshot:vi.fn(async()=>structuredClone(snapshot)),
    login:vi.fn(async()=>{ snapshot.account={id:"user",name:"开发者"};return snapshot.account; }),
    cancelLogin:vi.fn(async()=>{}),logout:vi.fn(async()=>{snapshot.account=null;}),
    enable:vi.fn(async(id)=>{snapshot.apps.find(a=>a.manifest.id===id)!.mode="tokenone";}),
    official:vi.fn(async(id)=>{snapshot.apps.find(a=>a.manifest.id===id)!.mode="official";}),
    restore:vi.fn(async()=>{}),open:vi.fn(async(id)=>snapshot.apps.find(a=>a.manifest.id===id)!.restartRequired ? "restart_required" as const : "opened" as const),reopen:vi.fn(async(id)=>{snapshot.apps.find(a=>a.manifest.id===id)!.restartRequired=false;}),install:vi.fn(async()=>true),defaults:vi.fn(async()=>[]),
    checkUpdate:vi.fn(async()=>null),installUpdate:vi.fn(async()=>{}),subscribe:vi.fn(async()=>()=>{}),...changes,
  };
  const view=render(<App api={api}/>);
  return {api,snapshot,...view};
}
async function idle() { await waitFor(()=>expect(screen.getByRole("button",{name:"Future Tool：打开"})).toBeEnabled()); }

test("已安装应用未登录也能打开，不修改配置、不启动授权",async()=>{
  const {api}=setup({account:null});
  await userEvent.click(await screen.findByRole("button",{name:"Future Tool：打开"}));
  expect(api.open).toHaveBeenCalledWith("future-tool");
  expect(api.login).not.toHaveBeenCalled();expect(api.enable).not.toHaveBeenCalled();
  expect(screen.getByRole("button",{name:"Future Tool：TokenOne"})).toBeDisabled();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("应用已运行且没有配置变更也先提示，稍后不退出，确认才重开",async()=>{
  const {api}=setup({account:null}, {open:vi.fn(async()=>"already_running" as const)});
  const open=await screen.findByRole("button",{name:"Future Tool：打开"});
  await userEvent.click(open);
  const dialog=await screen.findByRole("dialog",{name:"应用已在运行"});
  expect(dialog).toHaveTextContent("Future Tool 已在运行");
  expect(dialog).not.toHaveTextContent("配置已更新");
  expect(api.reopen).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button",{name:"稍后"}));await idle();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();expect(api.reopen).not.toHaveBeenCalled();
  await userEvent.click(open);
  await userEvent.click(await screen.findByRole("button",{name:"重新打开"}));
  await waitFor(()=>expect(api.reopen).toHaveBeenCalledOnce());await idle();
  expect(api.open).toHaveBeenCalledTimes(2);expect(api.enable).not.toHaveBeenCalled();
});

test("运行状态检测失败时显示错误，不打开或重开，允许重试",async()=>{
  const {api}=setup({account:null}, {open:vi.fn(async()=>{throw "无法确认应用是否正在运行";})});
  await userEvent.click(await screen.findByRole("button",{name:"Future Tool：打开"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("无法确认应用是否正在运行");await idle();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();expect(api.reopen).not.toHaveBeenCalled();
  vi.mocked(api.open).mockResolvedValueOnce("opened");
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：打开"}));await idle();
  expect(api.open).toHaveBeenCalledTimes(2);expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
test("卡片直接切换两种模式，选中项由成功后的真实配置决定",async()=>{
  const {api}=setup();await idle();
  expect(screen.getByRole("button",{name:"Future Tool：官方订阅"})).toHaveAttribute("aria-pressed","true");
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：TokenOne"}));
  await waitFor(()=>expect(screen.getByRole("button",{name:"Future Tool：TokenOne"})).toHaveAttribute("aria-pressed","true"));
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：官方订阅"}));
  await waitFor(()=>expect(api.official).toHaveBeenCalledWith("future-tool"));
  expect(await screen.findByText(/已切回官方订阅/)).toBeVisible();
});
test("切回官方不依赖 IDONE 登录",async()=>{
  const app=tool();app.mode="tokenone";const {api}=setup({account:null,apps:[app]});
  await userEvent.click(await screen.findByRole("button",{name:"Future Tool：官方订阅"}));
  expect(api.official).toHaveBeenCalledWith("future-tool");expect(api.login).not.toHaveBeenCalled();
});
test("切换失败保留原模式，允许重试，打开仍可用",async()=>{
  const {api}=setup();await idle();vi.mocked(api.enable).mockRejectedValueOnce("连接失败，已撤销本次修改");
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：TokenOne"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("连接失败");await idle();
  expect(screen.getByRole("button",{name:"Future Tool：官方订阅"})).toHaveAttribute("aria-pressed","true");
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：TokenOne"}));
  await waitFor(()=>expect(api.enable).toHaveBeenCalledTimes(2));
});
test("首次登录触发默认准备，后端负责保留已记住的订阅选择",async()=>{
  const {api}=setup({account:null});
  await userEvent.click(await screen.findByRole("button",{name:"登录 IDONE"}));
  await waitFor(()=>expect(api.defaults).toHaveBeenCalledOnce());
  expect(api.install).not.toHaveBeenCalled();
});
test("恢复登录会话时检查默认准备，直接显示后端实际模式",async()=>{
  const {api}=setup();await idle();expect(api.defaults).toHaveBeenCalledOnce();
  expect(screen.getByRole("button",{name:"Future Tool：官方订阅"})).toHaveAttribute("aria-pressed","true");
  expect(api.enable).not.toHaveBeenCalled();
});
test("默认准备失败呈现在对应卡片，不伪报成功",async()=>{
  setup({}, {defaults:vi.fn(async()=>[{appId:"future-tool",message:"默认接入失败"}])});
  expect(await screen.findByRole("alert")).toHaveTextContent("默认接入失败");await idle();
  expect(screen.getByRole("button",{name:"Future Tool：官方订阅"})).toHaveAttribute("aria-pressed","true");
});
test("未安装提供安装按钮，下载页面不算安装成功",async()=>{
  const app=tool();app.installed=false;app.mode=null;
  const {api}=setup({account:null,apps:[app]}, {install:vi.fn(async()=>false)});
  await userEvent.click(await screen.findByRole("button",{name:"Future Tool：安装"}));
  expect(await screen.findByText(/已打开官方下载页/)).toBeVisible();
  expect(screen.queryByRole("button",{name:"Future Tool：打开"})).not.toBeInTheDocument();
  expect(api.enable).not.toHaveBeenCalled();
});
test("浏览器预览不伪造安装和登录",async()=>{
  setup({account:null,desktop:false,loginReady:false});
  expect(await screen.findByRole("button",{name:"Future Tool：打开"})).toBeDisabled();
  expect(screen.getByRole("button",{name:"登录 IDONE"})).toBeDisabled();
});
test("Codex 会话提示保留历史与会话原 Provider",async()=>{
  const {api}=setup({apps:[tool("codex","ChatGPT / Codex")]});
  await screen.findByRole("button",{name:"ChatGPT / Codex：打开"});
  expect(screen.getByText(/切换不会删除会话；续接沿用会话原 Provider/)).toBeVisible();
  await userEvent.click(screen.getByRole("button",{name:"ChatGPT / Codex：TokenOne"}));
  expect(await screen.findByText(/已切换到 TokenOne/)).toBeVisible();
  expect(screen.getByText(/切换不会删除会话；续接沿用会话原 Provider/)).toBeVisible();
  expect(api.official).not.toHaveBeenCalled();
});
test("发现更新后可安装，启动时检查由桌面桥接执行",async()=>{
  const {api}=setup({}, {checkUpdate:vi.fn(async()=>({version:"0.2.4",notes:"修复问题",date:null}))});
  const update=await screen.findByRole("button",{name:"更新到 v0.2.4"});
  expect(api.checkUpdate).toHaveBeenCalledOnce();
  await userEvent.click(update);
  await waitFor(()=>expect(api.installUpdate).toHaveBeenCalledOnce());
});
test("更新检查失败后应用重新获得焦点时会重试",async()=>{
  const checkUpdate=vi.fn().mockRejectedValueOnce("network error").mockResolvedValue(null);
  const {api}=setup({}, {checkUpdate});
  await waitFor(()=>expect(api.checkUpdate).toHaveBeenCalledOnce());
  await act(async()=>window.dispatchEvent(new Event("focus")));
  await waitFor(()=>expect(api.checkUpdate).toHaveBeenCalledTimes(2));
});
test("不支持官方方式的新增应用不展示订阅选项",async()=>{
  const app=tool();app.manifest.supportsOfficial=false;setup({apps:[app]});await idle();
  expect(screen.queryByRole("button",{name:"Future Tool：官方订阅"})).not.toBeInTheDocument();
});
test("未知模式不高亮订阅，配置完成也保留打开按钮",async()=>{
  const app=tool();app.mode=null;setup({apps:[app]});await idle();
  expect(screen.getByRole("button",{name:"Future Tool：官方订阅"})).toHaveAttribute("aria-pressed","false");
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：TokenOne"}));await idle();
  expect(screen.getByRole("button",{name:"Future Tool：TokenOne"})).toHaveAttribute("aria-pressed","true");
});
test("真实进度替代固定准备文案",async()=>{
  const {api}=setup();await idle();let finish!:()=>void;
  vi.mocked(api.enable).mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：TokenOne"}));
  act(()=>vi.mocked(api.subscribe).mock.calls[0][0]({appId:"future-tool",stage:"verifying",message:"正在验证连接"}));
  expect(screen.queryByText("正在准备")).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"Future Tool：正在验证连接"})).toBeDisabled();
  await act(async()=>finish());
});
test("设备登录和模型授权可取消，Key 不进入页面",async()=>{
  const {api}=setup({account:null});let finish!:()=>void;
  vi.mocked(api.login).mockImplementationOnce(()=>new Promise((_,reject)=>{finish=()=>reject("已取消操作");}));
  await userEvent.click(await screen.findByRole("button",{name:"登录 IDONE"}));
  act(()=>vi.mocked(api.subscribe).mock.calls[0][0]({appId:"",stage:"login",message:"请核对确认码：ABCD"}));
  expect(screen.getByRole("status")).toHaveTextContent("ABCD");
  await userEvent.click(screen.getByRole("button",{name:"取消登录"}));expect(api.cancelLogin).toHaveBeenCalledOnce();
  await act(async()=>finish());expect(screen.queryByRole("alert")).not.toBeInTheDocument();expect(screen.queryByText(/sk-/)).not.toBeInTheDocument();
});

test("切换后先确认，选择稍后不退出应用，再次打开仍可确认重开",async()=>{
  const {api,snapshot}=setup();await idle();
  vi.mocked(api.enable).mockImplementationOnce(async()=>{snapshot.apps[0].mode="tokenone";snapshot.apps[0].restartRequired=true;});
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：TokenOne"}));
  expect(await screen.findByRole("dialog",{name:"重新打开，使配置生效"})).toHaveTextContent("Future Tool");
  expect(api.reopen).not.toHaveBeenCalled();expect(api.open).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button",{name:"稍后"}));await idle();
  expect(snapshot.apps[0].restartRequired).toBe(true);
  await userEvent.click(screen.getByRole("button",{name:"Future Tool：打开"}));
  await userEvent.click(await screen.findByRole("button",{name:"重新打开"}));
  await waitFor(()=>expect(api.reopen).toHaveBeenCalledWith("future-tool"));await idle();
  expect(snapshot.apps[0].restartRequired).toBe(false);
  expect(api.enable).toHaveBeenCalledOnce();
});
test("重开失败保留待重开状态和已选模式，允许稍后再试",async()=>{
  const app=tool();app.restartRequired=true;
  const {api,snapshot}=setup({account:null,apps:[app]}, {reopen:vi.fn(async()=>{throw "请保存工作并完全退出应用后重试";})});
  await userEvent.click(await screen.findByRole("button",{name:"Future Tool：打开"}));
  await userEvent.click(await screen.findByRole("button",{name:"重新打开"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("请保存工作");await idle();
  expect(snapshot.apps[0].restartRequired).toBe(true);expect(snapshot.apps[0].mode).toBe("official");
  expect(api.open).toHaveBeenCalledWith("future-tool");
});
test("首次准备多个应用只显示一次确认，未确认不重开任何应用",async()=>{
  const {api,snapshot}=setup({apps:[tool(),tool("second-tool","Second Tool")]});
  vi.mocked(api.defaults).mockImplementation(async()=>{snapshot.apps.forEach(app=>{app.restartRequired=true;});return [];});
  const dialog=await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("Future Tool、Second Tool");expect(api.reopen).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button",{name:"重新打开"}));
  await waitFor(()=>expect(api.reopen).toHaveBeenCalledTimes(2));
  expect(api.reopen).toHaveBeenNthCalledWith(1,"future-tool");expect(api.reopen).toHaveBeenNthCalledWith(2,"second-tool");
});
test("高级入口明确显示恢复原配置及无备份说明",async()=>{
  setup();await idle();
  await userEvent.click(screen.getByLabelText("Future Tool 高级操作"));
  expect(screen.getByRole("button",{name:"恢复原配置"})).toBeDisabled();
  expect(screen.getByText("暂无可恢复的备份")).toBeVisible();
});
test("高级恢复原配置可用且恢复后需要确认重开",async()=>{
  const app=tool();app.canRestore=true;
  const {api,snapshot}=setup({apps:[app]});await idle();
  vi.mocked(api.restore).mockImplementationOnce(async()=>{snapshot.apps[0].canRestore=false;snapshot.apps[0].restartRequired=true;});
  await userEvent.click(screen.getByLabelText("Future Tool 高级操作"));
  await userEvent.click(screen.getByRole("button",{name:"恢复原配置"}));
  expect(api.restore).toHaveBeenCalledWith("future-tool");
  expect(await screen.findByRole("dialog")).toBeVisible();expect(api.reopen).not.toHaveBeenCalled();
});
