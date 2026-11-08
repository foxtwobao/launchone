import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { buildEnvironment, applicationConfig, root } from "./build-environment.mjs";

const result = spawnSync(process.execPath,
  [resolve(root, "node_modules/@tauri-apps/cli/tauri.js"), "dev", "--config", JSON.stringify(applicationConfig("lab"))],
  { cwd: root, env: buildEnvironment("lab"), stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
