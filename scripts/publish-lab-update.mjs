import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { root, buildEnvironment } from "./build-environment.mjs";

const env = buildEnvironment("lab");
const { version } = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const releaseRoot = join(root, "output/updates/lab");
const destination = join(releaseRoot, "files", version);
const publicUrl = `${env.LAUNCHONE_SERVICE_URL}/updates/files/${version}`;
const manifestEnv = { ...process.env, LAUNCHONE_UPDATE_MANIFEST_OUTPUT: join(releaseRoot, "latest.json"),
  LAUNCHONE_UPDATE_MACOS_URL: "", LAUNCHONE_UPDATE_MACOS_SIGNATURE_FILE: "",
  LAUNCHONE_UPDATE_WINDOWS_URL: "", LAUNCHONE_UPDATE_WINDOWS_SIGNATURE_FILE: "" };
let platforms = 0;
function run(command, args, environment = process.env) {
  const result = spawnSync(command, args, { cwd: root, env: environment, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error("发布校验失败，更新清单未切换");
}
const sha256 = path => createHash("sha256").update(readFileSync(path)).digest("hex");
for (const [platform, suffix, key] of [["macos", ".app.tar.gz", "MACOS"], ["windows", ".exe", "WINDOWS"]]) {
  const directory = join(root, "output/lab", platform);
  if (!existsSync(directory)) continue;
  const names = readdirSync(directory).filter(name => name.endsWith(suffix));
  if (names.length !== 1) throw new Error(`${platform} 更新包缺失或不唯一`);
  const name = names[0];
  const artifact = join(directory, name);
  run("cargo", ["run", "--quiet", "--manifest-path", "src-tauri/Cargo.toml", "--no-default-features",
    "--example", "verify_update", "--", "config/lab.json", artifact, `${artifact}.sig`, version]);
  // Every copied file must also match the checksums produced by the packaging job.
  const checksums = readFileSync(join(directory, "SHA256SUMS.txt"), "utf8").trim().split("\n");
  const verified = [];
  for (const line of checksums) {
    const match = /^([a-f0-9]{64})  ([^/\\]+)$/.exec(line);
    if (!match || [".", ".."].includes(match[2])) throw new Error("校验清单格式无效");
    if (sha256(join(directory, match[2])) !== match[1]) throw new Error(`文件校验失败：${match[2]}`);
    verified.push(match[2]);
  }
  if (!verified.includes(name) || !verified.includes(`${name}.sig`)) throw new Error("更新包或签名未列入校验清单");
  mkdirSync(destination, { recursive: true });
  for (const file of verified) {
    const target = join(destination, file);
    if (existsSync(target) && sha256(target) !== sha256(join(directory, file))) {
      throw new Error(`已发布的版本不可覆盖，请增加版本号：${file}`);
    }
    copyFileSync(join(directory, file), target);
  }
  manifestEnv[`LAUNCHONE_UPDATE_${key}_URL`] = `${publicUrl}/${encodeURIComponent(name)}`;
  manifestEnv[`LAUNCHONE_UPDATE_${key}_SIGNATURE_FILE`] = `${artifact}.sig`;
  platforms++;
}
if (!platforms) throw new Error("output/lab 下没有构建产物；请先从 Desktop packages 下载产物");
run(process.execPath, ["scripts/create-update-manifest.mjs"], manifestEnv);
console.log(`Lab 更新已发布：${version}，${platforms} 个平台；无需重启服务`);
