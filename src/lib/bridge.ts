import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Account, AppManifest, Bridge, Progress, Snapshot } from "./types";

// Browser preview is read-only. It never simulates installation or authentication.
const previewManifests: AppManifest[] = [
  { id: "codex", name: "Codex", description: "OpenAI 的编程助手", command: "codex", icon: "codex", supportsOfficial: true, platforms: ["macos", "windows"] },
  { id: "claude-code", name: "Claude Code", description: "Anthropic 的编程助手", command: "claude", icon: "claude", supportsOfficial: true, platforms: ["macos", "windows"] },
];

export const bridge: Bridge = {
  async snapshot() {
    if (isTauri()) return invoke<Snapshot>("get_snapshot");
    return {
      apps: previewManifests.map(manifest => ({ manifest, installed: false, version: null, mode: null, canRestore: false, supported: false, error: null })),
      account: null, loginReady: false, tokenoneReady: false, desktop: false,
    };
  },
  login: () => invoke<Account>("login"),
  cancelLogin: () => invoke("cancel_login"),
  logout: () => invoke("logout"),
  enable: appId => invoke("enable_app", { appId }),
  official: appId => invoke("use_official", { appId }),
  restore: appId => invoke("restore_app", { appId }),
  async subscribe(callback) {
    if (!isTauri()) return () => {};
    return listen<Progress>("setup-progress", event => callback(event.payload));
  },
};
