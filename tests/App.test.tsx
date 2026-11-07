import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi, test, expect } from "vitest";
import App from "../src/App";
import type { AppStatus, Bridge, Snapshot } from "../src/lib/types";

function tool(id = "future-tool", name = "Future Tool"): AppStatus {
  return {
    manifest: { id, name, description: "可扩展工具", command: id, icon: "generic", supportsOfficial: false, platforms: ["macos"] },
    installed: true, version: "1.0", mode: null, canRestore: false, supported: true, error: null,
  };
}

function setup(overrides: Partial<Snapshot> = {}) {
  const snapshot: Snapshot = { apps: [tool()], account: { id: "user", name: "开发者" }, desktop: true, loginReady: true, tokenoneReady: true, ...overrides };
  const api: Bridge = {
    snapshot: vi.fn(async () => structuredClone(snapshot)),
    login: vi.fn(async () => ({ id: "user", name: "开发者" })),
    cancelLogin: vi.fn(async () => {}),
    logout: vi.fn(async () => {}),
    enable: vi.fn(async () => {}),
    official: vi.fn(async () => {}),
    restore: vi.fn(async () => {}),
    subscribe: vi.fn(async () => () => {}),
  };
  render(<App api={api} />);
  return { api, snapshot };
}

test("新增应用使用相同卡片与流程，不要求页面知道应用 ID", async () => {
  const { api } = setup();
  await userEvent.click(await screen.findByRole("button", { name: "Future Tool：启用 TokenOne" }));
  await waitFor(() => expect(api.enable).toHaveBeenCalledWith("future-tool"));
  expect(await screen.findByText("配置完成，在原有终端或编辑器中使用即可。")).toBeVisible();
});

test("未登录不能配置，浏览器预览不伪造检测和登录", async () => {
  const { api } = setup({ account: null, desktop: false, loginReady: false });
  expect(await screen.findByRole("button", { name: "Future Tool：登录后启用" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "登录 IDONE" })).toBeDisabled();
  expect(screen.getByText("等待桌面检测")).toBeVisible();
  expect(api.enable).not.toHaveBeenCalled();
});

test("接口失败在对应卡片呈现，允许重试", async () => {
  const { api } = setup();
  vi.mocked(api.enable).mockRejectedValueOnce("连接失败，已撤销本次修改");
  await userEvent.click(await screen.findByRole("button", { name: "Future Tool：启用 TokenOne" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("连接失败，已撤销本次修改");
  await userEvent.click(await screen.findByRole("button", { name: "Future Tool：重试" }));
  await waitFor(() => expect(api.enable).toHaveBeenCalledTimes(2));
});

test("不支持官方登录的应用不显示官方入口", async () => {
  setup();
  await userEvent.click(await screen.findByText("Future Tool", { selector: "h3" }));
  expect(screen.queryByRole("button", { name: /使用官方登录/ })).not.toBeInTheDocument();
});

test("配置已完成后不重复触发准备", async () => {
  const ready = tool(); ready.mode = "tokenone";
  const { api } = setup({ apps: [ready] });
  expect(await screen.findByRole("button", { name: "Future Tool：已启用" })).toBeDisabled();
  expect(api.enable).not.toHaveBeenCalled();
});

test("设备登录展示确认码，允许取消等待", async () => {
  const { api } = setup({ account: null });
  let finish!: () => void;
  vi.mocked(api.login).mockImplementationOnce(() => new Promise((_, reject) => { finish = () => reject("已取消操作"); }));
  await userEvent.click(await screen.findByRole("button", { name: "登录 IDONE" }));
  act(() => vi.mocked(api.subscribe).mock.calls[0][0]({ appId: "", stage: "login", message: "请在浏览器核对确认码：ABCD" }));
  expect(screen.getByRole("status")).toHaveTextContent("ABCD");
  await userEvent.click(screen.getByRole("button", { name: "取消登录" }));
  expect(api.cancelLogin).toHaveBeenCalledOnce();
  await act(async () => finish());
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("首次模型授权允许取消等待，Key 不进入页面", async () => {
  const { api } = setup();
  let finish!: () => void;
  vi.mocked(api.enable).mockImplementationOnce(() => new Promise((_, reject) => { finish = () => reject("已取消操作"); }));
  await userEvent.click(await screen.findByRole("button", { name: "Future Tool：启用 TokenOne" }));
  act(() => vi.mocked(api.subscribe).mock.calls[0][0]({ appId: "future-tool", stage: "authorization", message: "请在浏览器中完成模型服务授权" }));
  await userEvent.click(screen.getByRole("button", { name: "取消授权等待" }));
  expect(api.cancelLogin).toHaveBeenCalledOnce();
  await act(async () => finish());
  expect(screen.queryByText(/sk-/)).not.toBeInTheDocument();
});
