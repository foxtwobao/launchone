import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { buildEnvironment, selectedEnvironment, applicationConfig, root } from "./build-environment.mjs";

const args = process.argv.slice(2);
const profile = selectedEnvironment(args);
const env = buildEnvironment(profile);
const index = args.indexOf("--environment");
args.splice(index, 2);
const result = spawnSync(process.execPath,
  [resolve(root, "node_modules/@tauri-apps/cli/tauri.js"), "build", "--config", JSON.stringify(applicationConfig(profile, env.LAUNCHONE_UPDATE_PUBLIC_KEY)), ...args],
  { cwd: root, env, stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
