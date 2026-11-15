import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Account, AppManifest, Bridge, Progress, Snapshot, UpdateInfo } from "./types";

// Browser preview is read-only. It never simulates installation or authentication.
const previewManifests: AppManifest[] = [
  { id: "codex", name: "ChatGPT / Codex", description: "OpenAI 桌面版 · TokenOne 用于 Codex 任务", command: "codex", icon: "codex", supportsOfficial: true, platforms: ["macos", "windows"] },
  { id: "claude-desktop", name: "Claude", description: "Anthropic 桌面版", command: "", icon: "claude", supportsOfficial: true, platforms: ["macos", "windows"] },
];

export const bridge: Bridge = {
  async snapshot() {
    if (isTauri()) return invoke<Snapshot>("get_snapshot");
    return {
      apps: previewManifests.map(manifest => ({ manifest, installed: false, tokenoneSupported: true, restartRequired: false, version: null, mode: null, canRestore: false, supported: false, error: null, credentialAccount: null })),
      account: null, loginReady: false, tokenoneReady: false, tokenonePortalReady: false, desktop: false,
    };
  },
  login: () => invoke<Account>("login"),
  cancelLogin: () => invoke("cancel_login"),
  logout: () => invoke("logout"),
  openTokenonePortal: () => invoke("open_tokenone_portal"),
  enable: appId => invoke("enable_app", { appId }),
  official: appId => invoke("use_official", { appId }),
  restore: appId => invoke("restore_app", { appId }),
  open: appId => invoke("open_app", { appId }),
  reopen: appId => invoke("reopen_app", { appId }),
  install: appId => invoke("install_app", { appId }),
  defaults: () => invoke("prepare_defaults"),
  checkUpdate: () => isTauri() ? invoke<UpdateInfo | null>("check_update") : Promise.resolve(null),
  installUpdate: () => invoke("install_update"),
  async subscribe(callback) {
    if (!isTauri()) return () => {};
    return listen<Progress>("setup-progress", event => callback(event.payload));
  },
};
