import express from "express";
import { createHash, randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { isAbsolute } from "node:path";
import { z } from "zod";
import { registerInstallers } from "./installers.mjs";

const random = () => randomBytes(32).toString("base64url");
const hash = value => createHash("sha256").update(value).digest("base64url");
const text = z.string().min(1);
const opaqueId = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
const emptyBody = z.object({}).strict();
const upstreamErrors = new Set([
  "UNAUTHORIZED", "APP_DISABLED", "SCOPE_FORBIDDEN", "ISSUER_FORBIDDEN", "REDIRECT_URI_FORBIDDEN",
  "APP_USER_AUTHORIZATION_REQUIRED", "TOKENONE_USER_NOT_FOUND", "TOKENONE_USER_DISABLED",
  "TOKENONE_GROUP_FORBIDDEN", "IDENTITY_BINDING_CONFLICT", "APP_KEY_UNAVAILABLE", "TOKENONE_KEY_QUOTA_EXHAUSTED",
  "APP_DATABASE_UNAVAILABLE", "APP_SERVICE_UNAVAILABLE", "APP_KEY_RECOVERY_UNAVAILABLE", "AUTHORIZATION_FLOW_INVALID",
]);

class ServiceError extends Error {
  constructor(status, code, retryable = false) { super(code); this.status = status; this.code = code; this.retryable = retryable; }
}
const invalidFlow = () => new ServiceError(400, "AUTHORIZATION_FLOW_INVALID");

export function serviceUrl(value, allowLoopback = false) {
  const url = new URL(value);
  const local = allowLoopback && ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
  if ((url.protocol !== "https:" && !(local && url.protocol === "http:")) || url.username || url.password || url.search || url.hash) {
    throw new Error("Service URLs must use HTTPS and contain no credentials, query or fragment");
  }
  return url;
}

const optionalSetting = schema => z.preprocess(value => typeof value === "string" ? value.trim() || undefined : value, schema.optional());
const publicText = max => z.string().min(1).max(max).refine(value => !/[\u0000-\u001f\u007f-\u009f]/.test(value));
const publicUrl = rootOnly => z.string().max(2048).refine(value => {
  try {
    const url = serviceUrl(value);
    return url.protocol === "https:" && !value.includes("?") && !value.includes("#") && (!rootOnly || url.pathname === "/");
  } catch { return false; }
});

export function readConfig(env = process.env) {
  const values = z.object({
    LAUNCHONE_PUBLIC_ORIGIN: text,
    IDONE_ISSUER: text,
    RESELLER_ORIGIN: text,
    RESELLER_APP_CREDENTIAL: z.string().min(16),
    LAUNCHONE_CLIENT_ID: optionalSetting(publicText(1024)),
    LAUNCHONE_TOKENONE_URL: optionalSetting(publicUrl(true)),
    LAUNCHONE_CODEX_MODEL: optionalSetting(publicText(256)),
    LAUNCHONE_CLAUDE_MODEL: optionalSetting(publicText(256)),
    LAUNCHONE_TOKENONE_PORTAL_URL: optionalSetting(publicUrl(false)),
    LAUNCHONE_UPDATE_MANIFEST_FILE: z.preprocess(value => value === "" ? undefined : value, z.string().trim().min(1).optional()),
    LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY: z.preprocess(value => value === "" ? undefined : value, z.string().trim().min(1).optional()),
    LAUNCHONE_DOWNLOAD_BASE_URL: z.string().trim().min(1).optional().or(z.literal("")),
    LAUNCHONE_INSTALLER_DIRECTORY: z.string().trim().min(1).optional().or(z.literal("")),
    HOST: z.string().default("127.0.0.1"),
    PORT: z.coerce.number().int().min(1).max(65535).default(55400),
  }).safeParse(env);
  if (!values.success) throw new Error("LaunchOne service configuration is incomplete; see server/.env.example");
  const v = values.data;
  const clientSettings = [v.LAUNCHONE_CLIENT_ID, v.LAUNCHONE_TOKENONE_URL, v.LAUNCHONE_CODEX_MODEL, v.LAUNCHONE_CLAUDE_MODEL];
  const clientConfigured = clientSettings.some(Boolean) || Boolean(v.LAUNCHONE_TOKENONE_PORTAL_URL);
  if (clientConfigured && clientSettings.some(value => !value)) {
    throw new Error("Configure all LaunchOne public client settings together; see server/.env.example");
  }
  if (v.LAUNCHONE_UPDATE_MANIFEST_FILE && !isAbsolute(v.LAUNCHONE_UPDATE_MANIFEST_FILE)) {
    throw new Error("Configure LAUNCHONE_UPDATE_MANIFEST_FILE as an absolute path");
  }
  if (v.LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY && !isAbsolute(v.LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY)) {
    throw new Error("Configure LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY as an absolute path");
  }
  if (v.LAUNCHONE_INSTALLER_DIRECTORY && !isAbsolute(v.LAUNCHONE_INSTALLER_DIRECTORY)) {
    throw new Error("Configure LAUNCHONE_INSTALLER_DIRECTORY as an absolute path");
  }
  const downloadUrl = v.LAUNCHONE_DOWNLOAD_BASE_URL ? serviceUrl(v.LAUNCHONE_DOWNLOAD_BASE_URL) : undefined;
  if (downloadUrl && downloadUrl.pathname !== "/") throw new Error("Configure download origin without paths");
  if (v.LAUNCHONE_INSTALLER_DIRECTORY && !downloadUrl) {
    throw new Error("Installer mirror requires both download origin and storage directory");
  }
  const urls = [v.LAUNCHONE_PUBLIC_ORIGIN, v.IDONE_ISSUER, v.RESELLER_ORIGIN]
    .map(value => serviceUrl(value, env.NODE_ENV !== "production"));
  if (urls.some(url => url.pathname !== "/")) throw new Error("Configure service origins without paths");
  return Object.freeze({
    origin: urls[0].origin, issuer: v.IDONE_ISSUER, resellerOrigin: urls[2].origin,
    credential: v.RESELLER_APP_CREDENTIAL, host: v.HOST, port: v.PORT,
    downloadBaseUrl: downloadUrl?.origin, installerDirectory: v.LAUNCHONE_INSTALLER_DIRECTORY || undefined,
    updateManifestFile: v.LAUNCHONE_UPDATE_MANIFEST_FILE,
    updateArtifactDirectory: v.LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY,
    clientConfig: clientConfigured ? Object.freeze({
      LAUNCHONE_IDONE_ISSUER: v.IDONE_ISSUER,
      LAUNCHONE_CLIENT_ID: v.LAUNCHONE_CLIENT_ID,
      LAUNCHONE_TOKENONE_URL: v.LAUNCHONE_TOKENONE_URL,
      LAUNCHONE_CODEX_MODEL: v.LAUNCHONE_CODEX_MODEL,
      LAUNCHONE_CLAUDE_MODEL: v.LAUNCHONE_CLAUDE_MODEL,
      LAUNCHONE_TOKENONE_PORTAL_URL: v.LAUNCHONE_TOKENONE_PORTAL_URL || v.LAUNCHONE_TOKENONE_URL,
    }) : undefined,
  });
}

// One process, bounded five-minute transactions. No user database or model proxy.
export function createApp(config, { fetch: request = globalThis.fetch, now = Date.now } = {}) {
  const app = express();
  const flows = new Map();
  const starts = new Map();
  let discovery;
  app.disable("x-powered-by");
  app.use((_req, res, next) => {
    res.set({ "Cache-Control": "no-store", "Referrer-Policy": "no-referrer", "X-Content-Type-Options": "nosniff",
      "Content-Security-Policy": "default-src 'none'; frame-ancestors 'none'; base-uri 'none'" });
    next();
  });
  app.use(express.json({ limit: "2kb" }));

  async function upstream(url, options, timeout = 30_000) {
    let response;
    try { response = await request(url, { ...options, redirect: "error", cache: "no-store", signal: AbortSignal.timeout(timeout) }); }
    catch { throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true); }
    let value;
    try { value = await response.json(); }
    catch { throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true); }
    return { response, value };
  }

  async function identity(token) {
    if (!discovery) {
      const { response, value } = await upstream(`${config.issuer.replace(/\/$/, "")}/.well-known/openid-configuration`, {}, 10_000);
      const parsed = z.object({ issuer: z.literal(config.issuer), userinfo_endpoint: text }).safeParse(value);
      if (!response.ok || !parsed.success) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
      let endpoint;
      try { endpoint = serviceUrl(parsed.data.userinfo_endpoint, config.issuer.startsWith("http://127.0.0.1:")); }
      catch { throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true); }
      if (endpoint.origin !== new URL(config.issuer).origin) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
      discovery = parsed.data;
    }
    const { response, value } = await upstream(discovery.userinfo_endpoint, { headers: { Authorization: `Bearer ${token}` } }, 10_000);
    if (response.status === 401 || response.status === 403) throw new ServiceError(401, "UNAUTHORIZED");
    if (!response.ok) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    const parsed = z.object({ sub: text.max(1024), iss: z.literal(config.issuer) }).safeParse(value);
    if (!parsed.success) throw new ServiceError(401, "UNAUTHORIZED");
    return { issuer: config.issuer, subject: parsed.data.sub };
  }

  async function reseller(path, body) {
    const { response, value } = await upstream(`${config.resellerOrigin}/api/apps${path}`, {
      method: "POST", headers: { Authorization: `Bearer ${config.credential}`, "Content-Type": "application/json" }, body: JSON.stringify(body),
    });
    if (!response.ok) {
      const code = response.status !== 401 && upstreamErrors.has(value?.error) ? value.error : "APP_SERVICE_UNAVAILABLE";
      throw new ServiceError(response.status === 401 ? 502 : response.status, code, value?.retryable === true);
    }
    return value;
  }

  async function ensure(user) {
    const value = await reseller("/keys/ensure", { identity: user });
    const parsed = z.object({ status: z.literal("ready"), tokenone_user_id: text,
      api_key: z.object({ id: text, key: text.regex(/^\S+$/), group_id: text, status: z.literal("active") }),
    }).safeParse(value);
    if (!parsed.success) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    return { status: "ready", key: parsed.data.api_key.key };
  }

  function prune() {
    for (const [id, flow] of flows) if (flow.expiresAt <= now()) flows.delete(id);
  }
  const cleanup = setInterval(prune, 30_000);
  cleanup.unref();
  app.dispose = () => { clearInterval(cleanup); flows.clear(); starts.clear(); };
  const ownerOf = (token, user) => hash(JSON.stringify([token, user.issuer, user.subject]));
  const publicFlow = flow => ({ status: "authorization_required", id: flow.id,
    authorization_url: `${config.origin}/authorize/${flow.id}`, expires_in: Math.max(1, Math.ceil((flow.expiresAt - now()) / 1000)) });

  async function credentials(token, user, owner) {
    try { return await ensure(user); }
    catch (error) { if (!(error instanceof ServiceError) || error.status !== 403 || error.code !== "APP_USER_AUTHORIZATION_REQUIRED") throw error; }
    prune();
    const existing = [...flows.values()].find(flow => flow.owner === owner && ["pending", "exchanging", "uncertain", "authorized"].includes(flow.status));
    if (existing) return publicFlow(existing);
    if (flows.size + starts.size >= 256) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    const flow = { id: random(), state: random(), verifier: random(), owner, user, token, expiresAt: now() + 300_000, status: "starting" };
    const result = await reseller("/authorizations", { identity: user, redirect_uri: `${config.origin}/auth/reseller/callback`,
      state: flow.state, code_challenge: hash(flow.verifier) });
    const parsed = z.object({ authorization_url: text, expires_in: z.number().int().min(1).max(300) }).safeParse(result);
    if (!parsed.success) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    let url;
    try { url = new URL(parsed.data.authorization_url); } catch { throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true); }
    // Authorization entry points are owned by the configured Reseller.
    if (url.protocol !== "https:" || url.username || url.password || url.hash) {
      throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    }
    flow.authorizationUrl = url.href;
    flow.expiresAt = now() + parsed.data.expires_in * 1000;
    flow.status = "pending";
    flows.set(flow.id, flow);
    return publicFlow(flow);
  }

  registerInstallers(app, config);
  app.get("/health", (_req, res) => res.json({ status: "ok" }));
  app.get("/v1/client-config", (_req, res) => {
    if (!config.clientConfig) throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    res.json(config.clientConfig);
  });
  if (config.updateArtifactDirectory) {
    app.use("/updates/files", express.static(config.updateArtifactDirectory, {
      dotfiles: "deny", index: false, redirect: false,
      setHeaders: res => res.set("Cache-Control", "public, max-age=3600, immutable"),
    }));
  }
  app.get("/v1/updates/:target/:arch/:currentVersion", async (req, res) => {
    if (!/^[A-Za-z0-9._-]{1,64}$/.test(req.params.target)
      || !/^[A-Za-z0-9._-]{1,64}$/.test(req.params.arch)
      || !/^[A-Za-z0-9.+-]{1,64}$/.test(req.params.currentVersion)) {
      throw new ServiceError(400, "INVALID_REQUEST");
    }
    if (!config.updateManifestFile) return res.status(204).end();
    let manifest;
    try { manifest = await readFile(config.updateManifestFile, "utf8"); }
    catch (error) {
      if (error?.code === "ENOENT") return res.status(204).end();
      throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    }
    let value;
    try { value = JSON.parse(manifest); } catch { throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true); }
    if (typeof value?.version !== "string" || !value.platforms || typeof value.platforms !== "object") {
      throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    }
    const platform = value.platforms[`${req.params.target}-${req.params.arch}`]
      ?? (req.params.target === "windows" ? value.platforms[`${req.params.target}-${req.params.arch}-nsis`] : undefined);
    if (!platform) return res.status(204).end();
    if (typeof platform.url !== "string" || typeof platform.signature !== "string") {
      throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    }
    let artifactUrl;
    try { artifactUrl = new URL(platform.url); } catch { throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true); }
    if (artifactUrl.protocol !== "https:" || artifactUrl.username || artifactUrl.password) {
      throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    }
    // Only rewrite local update paths; preserve separately hosted/signed artifacts.
    if (config.downloadBaseUrl && artifactUrl.origin === config.origin && artifactUrl.pathname.startsWith("/updates/files/")) {
      artifactUrl = new URL(artifactUrl.pathname, config.downloadBaseUrl);
    }
    const update = { version: value.version, url: artifactUrl.href, signature: platform.signature };
    if (typeof value.notes === "string") update.notes = value.notes;
    if (typeof value.pub_date === "string") update.pub_date = value.pub_date;
    res.set("Cache-Control", "public, max-age=60").json(update);
  });
  app.use("/v1", async (req, _res, next) => {
    // Keys are for the native Rust client. No browser CORS or Cookie authentication.
    if (req.get("Origin")) throw new ServiceError(403, "BROWSER_ACCESS_FORBIDDEN");
    const match = /^Bearer ([^\s]{1,16384})$/.exec(req.get("Authorization") || "");
    if (!match) throw new ServiceError(401, "UNAUTHORIZED");
    req.userToken = match[1];
    req.verifiedIdentity = await identity(req.userToken);
    req.flowOwner = ownerOf(req.userToken, req.verifiedIdentity);
    next();
  });
  app.post("/v1/credentials", async (req, res) => {
    if (!emptyBody.safeParse(req.body).success || Object.keys(req.query).length) throw new ServiceError(400, "INVALID_REQUEST");
    // Coalesce duplicate starts from this login session without creating extra authorizations.
    if (!starts.has(req.flowOwner)) {
      const pending = credentials(req.userToken, req.verifiedIdentity, req.flowOwner).finally(() => starts.delete(req.flowOwner));
      starts.set(req.flowOwner, pending);
    }
    res.json(await starts.get(req.flowOwner));
  });
  app.get("/v1/authorizations/:id", async (req, res) => {
    prune();
    const flow = flows.get(req.params.id);
    if (!opaqueId.safeParse(req.params.id).success || !flow || flow.owner !== req.flowOwner) throw invalidFlow();
    if (flow.status === "failed") throw flow.error;
    if (flow.status === "authorized" || flow.status === "uncertain") {
      try {
        const result = await ensure(req.verifiedIdentity);
        flows.delete(flow.id);
        return res.json(result);
      } catch (error) {
        if (error instanceof ServiceError && error.code === "APP_USER_AUTHORIZATION_REQUIRED") {
          flows.delete(flow.id);
          throw new ServiceError(409, "AUTHORIZATION_RETRY_REQUIRED");
        }
        throw error;
      }
    }
    res.json({ status: "pending" });
  });
  app.get("/authorize/:id", (req, res) => {
    prune();
    const flow = flows.get(req.params.id);
    if (!flow || flow.status !== "pending") throw invalidFlow();
    res.redirect(302, flow.authorizationUrl);
  });
  app.get("/auth/reseller/callback", async (req, res) => {
    prune();
    const parsed = z.object({ state: opaqueId, code: text.max(2048) }).strict().safeParse(req.query);
    if (!parsed.success) throw invalidFlow();
    const flow = [...flows.values()].find(item => item.state === parsed.data.state);
    if (!flow || flow.status !== "pending") throw invalidFlow();
    // Claim once before any await. A duplicate callback can never exchange this code again.
    flow.status = "exchanging";
    try {
      const verified = await identity(flow.token);
      if (verified.issuer !== flow.user.issuer || verified.subject !== flow.user.subject) throw new ServiceError(401, "UNAUTHORIZED");
      const result = await reseller("/authorizations/complete", { code: parsed.data.code, code_verifier: flow.verifier });
      if (result?.status !== "authorized") throw new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
      flow.status = "authorized";
    } catch (error) {
      // A lost exchange response must be recovered with ensure, never a replayed code.
      if (error instanceof ServiceError && error.status >= 500) flow.status = "uncertain";
      else { flow.status = "failed"; flow.error = error instanceof ServiceError ? error : invalidFlow(); }
    }
    flow.token = undefined;
    flow.verifier = undefined;
    res.redirect(303, "/authorization-finished");
  });
  app.get("/authorization-finished", (_req, res) => res.type("html").send(
    '<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>LaunchOne</title><body><h1>请返回 LaunchOne</h1><p>授权处理已结束，请返回应用查看结果。</p></body></html>',
  ));
  app.use((_req, _res, next) => next(new ServiceError(404, "NOT_FOUND")));
  app.use((error, _req, res, _next) => {
    // Never log or return upstream bodies, identity claims, tokens, query strings or Key values.
    const safe = error instanceof ServiceError ? error
      : error?.type === "entity.too.large" || error instanceof SyntaxError ? new ServiceError(400, "INVALID_REQUEST")
      : new ServiceError(503, "APP_SERVICE_UNAVAILABLE", true);
    res.status(safe.status).json({ error: safe.code, retryable: safe.retryable });
  });
  return app;
}
