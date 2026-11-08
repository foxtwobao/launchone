import { buildEnvironment, selectedEnvironment, applicationConfig } from "./build-environment.mjs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.error("Windows 安装包必须在 Windows 上构建。");
  process.exit(1);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const profile = selectedEnvironment();
const target = join(root, "src-tauri", "target", profile || "unknown");
const output = join(root, "output", profile || "unknown", "windows");
const { version } = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const env = { ...buildEnvironment(profile), CARGO_TARGET_DIR: target };
const signedUpdater = Boolean(env.TAURI_SIGNING_PRIVATE_KEY?.trim());
if (!signedUpdater) throw new Error('打包需要当前环境的 TAURI_SIGNING_PRIVATE_KEY');

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(process.execPath, [join(root, "node_modules/@tauri-apps/cli/tauri.js"), "build", "--bundles", "nsis", "--config", JSON.stringify({
  ...applicationConfig(profile, env.LAUNCHONE_UPDATE_PUBLIC_KEY),
  bundle: { createUpdaterArtifacts: signedUpdater },
})]);

const bundle = join(target, "release", "bundle", "nsis");
const installer = readdirSync(bundle).filter(name => name.endsWith(".exe") && name.includes(version));
if (installer.length !== 1) throw new Error("未找到唯一的当前版本 Windows 安装包。");

mkdirSync(output, { recursive: true });
const names = [installer[0]];
copyFileSync(join(bundle, installer[0]), join(output, installer[0]));
if (signedUpdater) {
  // Tauri v2 signs the NSIS .exe directly (the .nsis.zip is v1 format).
  const signature = `${installer[0]}.sig`;
  if (!readdirSync(bundle).includes(signature)) throw new Error('签名 NSIS 安装包缺失');
  copyFileSync(join(bundle, signature), join(output, signature));
  names.push(signature);
}
writeFileSync(join(output, "SHA256SUMS.txt"), names.map(name =>
  `${createHash("sha256").update(readFileSync(join(output, name))).digest("hex")}  ${name}\n`).join(""));
console.log(`Windows 安装包已生成：${output}`);
