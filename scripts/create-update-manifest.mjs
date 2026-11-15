import { mkdirSync, readFileSync, writeFileSync, renameSync } from "node:fs";
import { dirname, isAbsolute } from "node:path";
import packageJson from "../package.json" with { type: "json" };

const macUrl = process.env.LAUNCHONE_UPDATE_MACOS_URL?.trim();
const macSignatureFile = process.env.LAUNCHONE_UPDATE_MACOS_SIGNATURE_FILE?.trim();
const windowsUrl = process.env.LAUNCHONE_UPDATE_WINDOWS_URL?.trim();
const windowsSignatureFile = process.env.LAUNCHONE_UPDATE_WINDOWS_SIGNATURE_FILE?.trim();
const output = process.env.LAUNCHONE_UPDATE_MANIFEST_OUTPUT?.trim();
const version = packageJson.version;
if (!output || (!macUrl && !windowsUrl)) throw new Error("至少需要一个平台的更新包和清单输出路径");
if (Boolean(macUrl) !== Boolean(macSignatureFile) || Boolean(windowsUrl) !== Boolean(windowsSignatureFile)) {
  throw new Error("每个平台必须同时提供更新包 URL 和签名文件");
}
if (!isAbsolute(output)) throw new Error("LAUNCHONE_UPDATE_MANIFEST_OUTPUT 必须是绝对路径");

function platform(urlValue, signatureFile) {
  const url = new URL(urlValue);
  if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash) {
    throw new Error("更新包 URL 必须是无凭据、无查询参数的 HTTPS 地址");
  }
  const signature = readFileSync(signatureFile, "utf8").trim();
  if (!signature) throw new Error(`更新包签名文件为空：${signatureFile}`);
  return { url: url.href, signature };
}

const manifest = {
  version,
  pub_date: new Date().toISOString(),
  notes: process.env.LAUNCHONE_UPDATE_NOTES?.trim() || "",
  platforms: {
    ...(macUrl ? { "darwin-aarch64": platform(macUrl, macSignatureFile),
      "darwin-x86_64": platform(macUrl, macSignatureFile) } : {}),
    ...(windowsUrl ? { "windows-x86_64-nsis": platform(windowsUrl, windowsSignatureFile) } : {}),
  },
};
mkdirSync(dirname(output), { recursive: true });
const temporary = `${output}.${process.pid}.tmp`;
writeFileSync(temporary, `${JSON.stringify(manifest, null, 2)}\n`, { mode: 0o644 });
renameSync(temporary, output);
console.log(`已生成 ${output}`);
