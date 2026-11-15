import { open, rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import { allowedSource } from "./installer-sources.mjs";

export async function downloadOfficial(adapter, sourceUrl, file, { request = fetch, timeout = 20 * 60_000, previous } = {}) {
  const signal = AbortSignal.timeout(timeout);
  let current = allowedSource(sourceUrl, adapter);
  let response;
  for (let redirects = 0; redirects <= 5; redirects++) {
    response = await request(current, { redirect: "manual", signal, headers: { "Accept-Encoding": "identity", ...(previous?.etag ? { "If-None-Match": previous.etag } : previous?.lastModified ? { "If-Modified-Since": previous.lastModified } : {}) } });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location || redirects === 5) throw new Error("Official download redirect limit exceeded");
      current = allowedSource(new URL(location, current).href, adapter);
      continue;
    }
    break;
  }
  if (response.status === 304 && previous?.sha256) { await response.body?.cancel(); return { unchanged: true }; }
  if (response.status !== 200 || !response.body || /(?:text\/|html|json)/i.test(response.headers.get("content-type") || "")) {
    await response.body?.cancel();
    throw new Error("Official installer download unavailable or returned a document");
  }
  const declared = response.headers.get("content-length");
  if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) <= 0 || Number(declared) > adapter.maxSize)) {
    await response.body.cancel();
    throw new Error("Official installer size is invalid");
  }
  const output = await open(file, "wx", 0o600);
  const digest = createHash("sha256");
  let size = 0;
  let prefix = Buffer.alloc(0);
  try {
    for await (const chunk of response.body) {
      const bytes = Buffer.from(chunk);
      size += bytes.length;
      if (size > adapter.maxSize) throw new Error("Official installer exceeds size limit");
      if (prefix.length < 512) prefix = Buffer.concat([prefix, bytes.subarray(0, 512 - prefix.length)]);
      digest.update(bytes);
      await output.writeFile(bytes);
    }
    if (size === 0 || (declared !== null && size !== Number(declared)) || /^\s*(?:<!doctype|<html|\{|\[)/i.test(prefix.toString("utf8"))) {
      throw new Error("Official installer is incomplete or not a package");
    }
    if (adapter.format === "msix" && prefix.subarray(0, 4).toString("hex") !== "504b0304") throw new Error("MSIX is not a ZIP package");
    await output.sync();
    return { size, sha256: digest.digest("hex"), sourceUrl: current.href, etag: response.headers.get("etag"), lastModified: response.headers.get("last-modified") };
  } catch (error) {
    await rm(file, { force: true });
    throw error;
  } finally { await output.close(); }
}
