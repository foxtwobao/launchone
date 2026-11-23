import { buildEnvironment, selectedEnvironment, applicationConfig } from "./build-environment.mjs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const windowsTarget = "x86_64-pc-windows-msvc";

function main() {
  if (process.platform !== "win32") {
    console.error("Windows 安装包必须在 Windows 上构建。");
    process.exit(1);
  }
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

  run("rustup", ["target", "add", windowsTarget]);
  run(process.execPath, [join(root, "node_modules/@tauri-apps/cli/tauri.js"), "build", "--target", windowsTarget, "--bundles", "nsis", "--config", JSON.stringify({
    ...applicationConfig(profile, env.LAUNCHONE_UPDATE_PUBLIC_KEY),
    // Windows packages are currently unsigned; updater signatures remain required.
    bundle: { createUpdaterArtifacts: signedUpdater },
  })]);

  const release = join(target, windowsTarget, "release");
  const bundle = join(release, "bundle", "nsis");
  exportWindowsInstaller(bundle, output, version);
  console.log(`Windows 安装包已生成：${output}`);
}

export function exportWindowsInstaller(bundle, output, version) {
  const suffix = `_${version}_x64-setup.exe`;
  const files = readdirSync(bundle);
  const installers = files.filter(name => name.endsWith(suffix));
  if (installers.length !== 1) throw new Error("未找到唯一的当前版本 Windows 安装包。");
  const [installer] = installers;
  const signature = `${installer}.sig`;
  if (!files.includes(signature)) throw new Error("签名 NSIS 安装包缺失");

  const filename = `${installer.slice(0, -suffix.length)}_${version}.exe`;
  const names = [filename, `${filename}.sig`];
  mkdirSync(output, { recursive: true });
  copyFileSync(join(bundle, installer), join(output, names[0]));
  copyFileSync(join(bundle, signature), join(output, names[1]));
  writeFileSync(join(output, "SHA256SUMS.txt"), names.map(name =>
    `${createHash("sha256").update(readFileSync(join(output, name))).digest("hex")}  ${name}\n`).join(""));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
