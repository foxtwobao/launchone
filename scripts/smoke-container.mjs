import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";

const image = process.argv[2] || "launchone-service:verify";
if (process.argv.length > 3 || image.startsWith("-")) {
  throw new Error("Usage: node scripts/smoke-container.mjs [image]");
}

const name = `launchone-smoke-${randomUUID()}`;
const port = "55777";
const fixture = {
  NODE_ENV: "production",
  HOST: "0.0.0.0",
  PORT: port,
  LAUNCHONE_PUBLIC_ORIGIN: "https://launchone.example.test",
  IDONE_ISSUER: "https://idone.example.test",
  RESELLER_ORIGIN: "https://reseller.example.test",
  RESELLER_APP_CREDENTIAL: "fixture-only-credential-not-a-secret",
  LAUNCHONE_CLIENT_ID: "fixture-device-client",
  LAUNCHONE_TOKENONE_URL: "https://tokenone.example.test",
  LAUNCHONE_CODEX_MODEL: "fixture-codex-model",
  LAUNCHONE_CLAUDE_MODEL: "fixture-claude-model",
};

function docker(args) {
  const result = spawnSync("docker", args, {
    encoding: "utf8",
    timeout: 20_000,
    maxBuffer: 64 * 1024,
  });
  if (result.error) throw new Error(`Docker ${args[0]} failed: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`Docker ${args[0]} failed: ${result.stderr.trim()}`);
  return result.stdout.trim();
}

const pause = () => new Promise(resolve => setTimeout(resolve, 200));
let created = false;
try {
  // All application settings are fixtures; none are inherited from the host.
  created = true;
  docker([
    "run", "--detach", "--rm", "--name", name,
    "--publish", `127.0.0.1::${port}`,
    "--health-interval", "1s", "--health-start-period", "1s",
    ...Object.entries(fixture).flatMap(([key, value]) => ["--env", `${key}=${value}`]),
    image,
  ]);
  const base = `http://${docker(["port", name, `${port}/tcp`])}`;
  const deadline = Date.now() + 20_000;
  let health;
  while (Date.now() < deadline) {
    try {
      health = await fetch(`${base}/health`, { signal: AbortSignal.timeout(1000) });
      if (health.status === 200) break;
    } catch {}
    await pause();
  }
  assert.equal(health?.status, 200, "Container health endpoint did not become ready");

  const get = path => fetch(`${base}${path}`, { signal: AbortSignal.timeout(5000) });
  const response = await get("/v1/client-config");
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), {
    LAUNCHONE_IDONE_ISSUER: fixture.IDONE_ISSUER,
    LAUNCHONE_CLIENT_ID: fixture.LAUNCHONE_CLIENT_ID,
    LAUNCHONE_TOKENONE_URL: fixture.LAUNCHONE_TOKENONE_URL,
    LAUNCHONE_CODEX_MODEL: fixture.LAUNCHONE_CODEX_MODEL,
    LAUNCHONE_CLAUDE_MODEL: fixture.LAUNCHONE_CLAUDE_MODEL,
    LAUNCHONE_TOKENONE_PORTAL_URL: fixture.LAUNCHONE_TOKENONE_URL,
  });
  const credentials = await fetch(`${base}/v1/credentials`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: "{}",
    signal: AbortSignal.timeout(5000),
  });
  assert.equal(credentials.status, 401, "Credentials must still require login");

  const files = JSON.parse(docker([
    "exec", name, "node", "--input-type=module", "-e",
    "import fs from 'node:fs'; console.log(JSON.stringify({uid:process.getuid(),app:fs.readdirSync('/app').sort(),server:fs.readdirSync('/app/server').sort()}))",
  ]));
  assert.notEqual(files.uid, 0, "Service must run as a non-root user");
  assert.deepEqual(files.app, ["node_modules", "package-lock.json", "package.json", "server"]);
  assert.deepEqual(files.server, ["app.mjs", "installers.mjs", "main.mjs"]);

  let status;
  while (Date.now() < deadline) {
    status = docker(["inspect", "--format", "{{.State.Health.Status}}", name]);
    if (status === "healthy") break;
    await pause();
  }
  assert.equal(status, "healthy", "Docker health check did not pass with the runtime port");
  console.log("Container smoke passed: health, public config, authentication, non-root user and image contents.");
} finally {
  if (created) {
    // Only the unique container created by this invocation may be removed.
    spawnSync("docker", ["rm", "--force", name], { stdio: "ignore", timeout: 20_000 });
  }
}
