// Fixed first-party adapters, shared by the synchronizer and native verifier.
// Source overrides may update an official path, never add a new download host.
export function getAdapter(app, platform, arch) {
  if (!["aarch64", "x86_64"].includes(arch)) throw new Error("Unsupported installer architecture");
  const base = { app, platform, arch, maxSize: 4 * 1024 ** 3 };
  if (app === "codex" && platform === "macos") return {
    ...base, format: "dmg", sourceUrl: "https://persistent.oaistatic.com/codex-app-prod/ChatGPT.dmg",
    allowedHosts: ["persistent.oaistatic.com"], identities: ["com.openai.codex", "com.openai.chat"],
    teamId: "2DC432GLL2", publisher: null, dependenciesVerified: true,
  };
  if (app === "claude-desktop" && platform === "macos") return {
    ...base, format: "dmg", sourceUrl: "https://downloads.claude.ai/releases/darwin/universal/latest/Claude.dmg",
    allowedHosts: ["claude.ai", "downloads.claude.ai"], identities: ["com.anthropic.claudefordesktop"],
    teamId: "Q6L2SF6YDW", publisher: null, dependenciesVerified: true,
  };
  if (app === "claude-desktop" && platform === "windows") return {
    ...base, format: "msix", sourceUrl: `https://claude.ai/api/desktop/win32/${arch === "aarch64" ? "arm64" : "x64"}/msix/latest/redirect`,
    allowedHosts: ["claude.ai", "downloads.claude.ai"], identities: ["Claude"],
    teamId: null, publisher: null,
    // Enable only after an exact publisher pin and full-package dependency test
    // are committed here. A bootstrapper is never accepted as a complete package.
    dependenciesVerified: false,
  };
  throw new Error("Unsupported installer combination");
}
export function allowedSource(value, adapter) {
  const url = new URL(value);
  if (url.protocol !== "https:" || (url.port && url.port !== "443") || url.username || url.password || url.hash
    || !adapter.allowedHosts.includes(url.hostname)) throw new Error("Installer source is not an allowed official HTTPS host");
  return url;
}
export function sourceOverride(adapter, env = process.env) {
  const name = adapter.app === "codex" ? "CODEX" : "CLAUDE";
  const key = `LAUNCHONE_${name}_${adapter.platform.toUpperCase()}_SOURCE_URL`;
  return allowedSource(env[key] || adapter.sourceUrl, adapter).href;
}
export function allAdapters() {
  return ["codex", "claude-desktop"].flatMap(app => ["macos", "windows"].flatMap(platform =>
    ["aarch64", "x86_64"].flatMap(arch => { try { return [getAdapter(app, platform, arch)]; } catch { return []; } })));
}
