import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { parseEnv } from "node:util";

export const root = resolve(import.meta.dirname, "..");
const keys = ["LAUNCHONE_IDONE_ISSUER", "LAUNCHONE_CLIENT_ID", "LAUNCHONE_SERVICE_URL",
  "LAUNCHONE_CODEX_BASE_URL", "LAUNCHONE_CODEX_MODEL", "LAUNCHONE_CLAUDE_BASE_URL",
  "LAUNCHONE_CLAUDE_MODEL", "LAUNCHONE_UPDATE_PUBLIC_KEY"];

export function buildEnvironment(profile, input = process.env, directory = root) {
  if (!["lab", "production"].includes(profile)) throw new Error("请明确选择 --environment lab 或 production");
  const file = resolve(directory, profile === "lab" ? "config/lab.json" : ".env.production");
  const configured = profile === "lab" ? JSON.parse(readFileSync(file, "utf8"))
    : { ...(existsSync(file) ? parseEnv(readFileSync(file, "utf8")) : {}), ...input };
  const missing = keys.filter(key => !configured[key]?.trim());
  if (missing.length) throw new Error(`环境 ${profile} 配置缺失：${missing.join(", ")}`);
  for (const key of keys.filter(key => key.endsWith("_URL") || key.endsWith("_ISSUER"))) {
    const url = new URL(configured[key]);
    if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash) {
      throw new Error(`环境地址必须为无凭据的 HTTPS：${key}`);
    }
    if (profile === "production" && (url.hostname.endsWith(".home.arpa") || url.hostname === "localhost")) {
      throw new Error(`正式包不能连接 lab：${key}`);
    }
  }
  return { ...input, ...Object.fromEntries(keys.map(key => [key, configured[key]])),
    LAUNCHONE_BUILD_ENV: profile, VITE_BUILD_ENV: profile };
}

export function selectedEnvironment(args = process.argv.slice(2)) {
  const index = args.indexOf("--environment");
  return index >= 0 ? args[index + 1] : undefined;
}

export function applicationConfig(profile, publicKey) {
  const identity = profile === "lab" ? { productName: "LaunchOne Lab", identifier: "com.tokenone.launchone.lab",
    app: { windows: [{ label: "main", title: "LaunchOne Lab", width: 980, height: 740,
      minWidth: 680, minHeight: 600, center: true }] } } : {};
  return { ...identity, ...(publicKey ? { plugins: { updater: { pubkey: publicKey, requireSignedVersion: true } } } : {}) };
}
