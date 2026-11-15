import { readFile, writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { getAdapter } from "./installer-sources.mjs";
const execute = promisify(execFile);
const maxReport = 1024 * 1024;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

export function githubConfig(env = process.env) {
  const repo = env.LAUNCHONE_INSTALLER_GITHUB_REPOSITORY;
  const ref = env.LAUNCHONE_INSTALLER_GITHUB_REF;
  const token = env.LAUNCHONE_INSTALLER_GITHUB_TOKEN;
  const workflow = env.LAUNCHONE_INSTALLER_GITHUB_WORKFLOW || "verify-installers.yml";
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo || "") || !/^[A-Za-z0-9][A-Za-z0-9_./-]*$/.test(ref || "")
    || ref.includes("..") || !/^[A-Za-z0-9_-]+\.ya?ml$/.test(workflow) || !token) {
    throw new Error("Configure installer GitHub repository, protected branch, workflow and token");
  }
  return { repo, ref, token, workflow };
}

export function validateReport(candidate, run, report) {
  if (!report || typeof report !== "object") throw new Error("Missing native verification report");
  for (const key of ["requestId", "environment", "app", "platform", "arch", "sha256", "size", "format"]) {
    if (report[key] !== candidate[key]) throw new Error(`Native report ${key} mismatch`);
  }
  if (String(report.runId) !== String(run.id) || report.runAttempt !== 1 || report.workflowSha !== candidate.workflowSha
    || !report.checks || ["signature", "publisher", "identity", "architecture", "system", "dependencies"].some(key => report.checks[key] !== true)) {
    throw new Error("Native verification evidence did not pass");
  }
  const adapter = getAdapter(candidate.app, candidate.platform, candidate.arch);
  const identity = report.bundleId ?? report.identity;
  if (!adapter.identities.includes(identity) || (adapter.teamId && report.teamId !== adapter.teamId)
    || (adapter.publisher && report.publisher !== adapter.publisher) || !adapter.dependenciesVerified) {
    throw new Error("Native package publisher, identity or dependency verification mismatch");
  }
  if (!Array.isArray(report.supportedArchs) || !report.supportedArchs.includes(candidate.arch)
    || report.supportedArchs.some(a => !["aarch64", "x86_64"].includes(a))
    || !/^\d+(?:\.\d+){1,3}$/.test(report.minOsVersion || "")
    || !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,95}$/.test(report.version || "")
    || !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,95}$/.test(report.build || "")) throw new Error("Native package metadata is invalid");
  return report;
}

export async function verifyOnGithub(candidate, directory, config, { request = fetch, sleep = pause, now = Date.now, save = async () => {} } = {}) {
  const base = `https://api.github.com/repos/${config.repo}`;
  const headers = { Authorization: `Bearer ${config.token}`, Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" };
  async function api(path, body) {
    const response = await request(`${base}${path}`, { method: body ? "POST" : "GET", headers: { ...headers, ...(body ? { "Content-Type": "application/json" } : {}) },
      ...(body ? { body: JSON.stringify(body) } : {}), redirect: "error", signal: AbortSignal.timeout(30_000) });
    if (!response.ok) throw new Error(`GitHub verification API failed (${response.status})`);
    return response.status === 204 ? null : response.json();
  }
  if (now() >= candidate.deadline) throw new Error("Native verification request expired");
  if (!candidate.workflowSha) {
    const branch = await api(`/branches/${encodeURIComponent(config.ref)}`);
    if (!branch.protected || !/^[a-f0-9]{40}$/.test(branch.commit?.sha || "")) throw new Error("Installer verifier must run on a protected branch");
    const workflow = await api(`/actions/workflows/${encodeURIComponent(config.workflow)}`);
    if (workflow.path !== `.github/workflows/${config.workflow}` || workflow.state !== "active") throw new Error("Unexpected verifier workflow");
    candidate.workflowSha = branch.commit.sha;
    candidate.workflowId = workflow.id;
    candidate.repository = config.repo;
    candidate.ref = config.ref;
    candidate.dispatchedAt = now();
    await save(candidate);
    await api(`/actions/workflows/${candidate.workflowId}/dispatches`, {
      ref: config.ref, inputs: { request_id: candidate.requestId, candidate: JSON.stringify(candidate) },
    });
  }
  if (candidate.repository !== config.repo || candidate.ref !== config.ref) throw new Error("Verification environment changed while pending");
  let run;
  while (now() < candidate.deadline) {
    if (!candidate.runId) {
      const listed = await api(`/actions/workflows/${candidate.workflowId}/runs?event=workflow_dispatch&branch=${encodeURIComponent(config.ref)}&per_page=100`);
      const matches = (listed.workflow_runs || []).filter(item => item.display_title === `installer-verify-${candidate.requestId}`);
      if (matches.length > 1) throw new Error("Duplicate verifier runs for request");
      if (matches.length === 1) { candidate.runId = matches[0].id; await save(candidate); }
    }
    if (candidate.runId) {
      run = await api(`/actions/runs/${candidate.runId}`);
      if (run.id !== candidate.runId || run.workflow_id !== candidate.workflowId || run.head_sha !== candidate.workflowSha
        || run.head_branch !== config.ref || run.event !== "workflow_dispatch" || run.run_attempt !== 1
        || run.head_repository?.full_name?.toLowerCase() !== config.repo.toLowerCase()
        || run.display_title !== `installer-verify-${candidate.requestId}`
        || run.path?.split("@")[0] !== `.github/workflows/${config.workflow}`) throw new Error("Untrusted verifier run identity");
      if (run.status === "completed") {
        if (run.conclusion !== "success") throw new Error("Native verifier failed");
        break;
      }
    }
    await sleep(Math.min(15_000, candidate.deadline - now()));
  }
  if (!run || run.status !== "completed" || now() >= candidate.deadline) throw new Error("Native verification timed out");
  const listed = await api(`/actions/runs/${candidate.runId}/artifacts?per_page=100`);
  const artifacts = (listed.artifacts || []).filter(a => a.name === `installer-report-${candidate.requestId}`);
  if (artifacts.length !== 1 || artifacts[0].expired || artifacts[0].size_in_bytes > maxReport
    || (artifacts[0].workflow_run && artifacts[0].workflow_run.id !== candidate.runId)) throw new Error("Missing or duplicate verifier artifact");
  const result = await request(`${base}/actions/artifacts/${artifacts[0].id}/zip`, { headers, redirect: "manual", signal: AbortSignal.timeout(30_000) });
  let archiveResponse = result;
  if ([302, 307].includes(result.status)) {
    const target = new URL(result.headers.get("location"));
    if (target.protocol !== "https:" || target.username || target.password
      || !(target.hostname.endsWith(".blob.core.windows.net") || target.hostname.endsWith(".githubusercontent.com"))) throw new Error("Untrusted GitHub artifact redirect");
    archiveResponse = await request(target, { redirect: "error", signal: AbortSignal.timeout(30_000) });
  }
  if (!archiveResponse.ok || !archiveResponse.body) throw new Error("Unable to download verifier artifact");
  const chunks = [];
  let bytes = 0;
  for await (const chunk of archiveResponse.body) {
    bytes += chunk.length;
    if (bytes > maxReport) throw new Error("Verifier artifact too large");
    chunks.push(chunk);
  }
  const zip = join(directory, "verification.zip");
  await writeFile(zip, Buffer.concat(chunks), { mode: 0o600 });
  try {
    const files = await execute("unzip", ["-Z1", zip], { timeout: 10_000, maxBuffer: 4096 });
    if (files.stdout.trim() !== "report.json") throw new Error("Unexpected files in verifier artifact");
    const extracted = await execute("unzip", ["-p", zip, "report.json"], { timeout: 10_000, maxBuffer: maxReport });
    return validateReport(candidate, run, JSON.parse(extracted.stdout));
  } finally { await rm(zip, { force: true }); }
}
