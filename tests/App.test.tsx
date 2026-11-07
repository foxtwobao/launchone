import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, test, expect } from "vitest";
import App from "../src/App";
import type { AppStatus, Bridge, Snapshot } from "../src/lib/types";

function tool(id = "future-tool", name = "Future Tool"): AppStatus {
  return { manifest:{id,name,description:"可扩展桌面应用",command:"",icon:"generic",supportsOfficial:true,platforms:["macos"]},
    installed:true,tokenoneSupported:true,version:"1.0",mode:"official",canRestore:false,supported:true,error:null };
}
function setup(overrides:Partial<Snapshot>={}, changes:Partial<Bridge>={}) {
  const snapshot:Snapshot={apps:[tool()],account:{id:"user",name:"开发者"},desktop:true,loginReady:true,tokenoneReady:true,...overrides};
  const api:Bridge={
    snapshot:vi.fn(async()=>structuredClone(snapshot)),
    login:vi.fn(async()=>{ snapshot.account={id:"user",name:"开发者"};return snapshot.account; }),
    cancelLogin:vi.fn(async()=>{}),logout:vi.fn(async()=>{snapshot.account=null;}),
    enable:vi.fn(async(id)=>{snapshot.apps.find(a=>a.manifest.id===id)!.mode="tokenone";}),
    official:vi.fn(async(id)=>{snapshot.apps.find(a=>a.manifest.id===id)!.mode="official";}),
    restore:vi.fn(async()=>{}),open:vi.fn(async()=>{}),install:vi.fn(async()=>true),defaults:vi.fn(async()=>[]),
    subscribe:vi.fn(async()=>()=>{}),...changes,
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
