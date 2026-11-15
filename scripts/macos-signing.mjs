import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createPrivateKey } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";

const requiredSecrets = [
  "MACOS_DEVELOPER_ID_P12_BASE64",
  "MACOS_DEVELOPER_ID_P12_PASSWORD",
  "APP_STORE_CONNECT_KEY_ID",
  "APP_STORE_CONNECT_ISSUER_ID",
  "APP_STORE_CONNECT_API_KEY_BASE64",
];

function decodeApiKey(value) {
  // Buffer.from silently ignores invalid Base64 characters, including the
  // punctuation in a raw PEM. Accept only canonical, optionally wrapped Base64.
  const encoded = value.replace(/[ \t\r\n]/g, "");
  const validBase64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
  if (!encoded || !validBase64.test(encoded)) {
    throw new Error("APP_STORE_CONNECT_API_KEY_BASE64 必须是 P8 私钥的 Base64 编码，不能直接填写 PEM 内容");
  }
  const key = Buffer.from(encoded, "base64");
  if (key.toString("base64") !== encoded) {
    throw new Error("APP_STORE_CONNECT_API_KEY_BASE64 不是有效的标准 Base64 编码");
  }
  try {
    createPrivateKey({ key, format: "pem" });
  } catch {
    throw new Error("APP_STORE_CONNECT_API_KEY_BASE64 解码后不是有效的 PEM 私钥");
  }
  return key;
}

// Tauri prefers Apple ID credentials over an API key. Drop inherited Apple
// settings so production uses the supplied API key and Lab stays ad-hoc.
export async function withMacSigning(profile, input, build, temporaryRoot = tmpdir()) {
  if (!["lab", "production"].includes(profile)) throw new Error("请明确选择 lab 或 production 环境");
  const env = Object.fromEntries(Object.entries(input).filter(([key]) =>
    !key.startsWith("APPLE_") && !key.startsWith("DESKTOP_MAC_CSC_") &&
    !key.startsWith("MACOS_DEVELOPER_ID_") && !key.startsWith("APP_STORE_CONNECT_")));
  if (profile === "lab") return await build(env);

  const missing = requiredSecrets.filter(key => !input[key]?.trim());
  if (missing.length) throw new Error(`正式 macOS 打包缺少签名与公证凭据：${missing.join(", ")}`);
  const apiKey = decodeApiKey(input.APP_STORE_CONNECT_API_KEY_BASE64);

  let directory;
  try {
    directory = mkdtempSync(join(temporaryRoot, "launchone-macos-signing-"));
    chmodSync(directory, 0o700);
    const keyPath = join(directory, "AuthKey.p8");
    writeFileSync(keyPath, apiKey, { mode: 0o600, flag: "wx" });
    return await build({
      ...env,
      APPLE_CERTIFICATE: input.MACOS_DEVELOPER_ID_P12_BASE64.trim(),
      APPLE_CERTIFICATE_PASSWORD: input.MACOS_DEVELOPER_ID_P12_PASSWORD,
      APPLE_API_KEY: input.APP_STORE_CONNECT_KEY_ID.trim(),
      APPLE_API_ISSUER: input.APP_STORE_CONNECT_ISSUER_ID.trim(),
      APPLE_API_KEY_PATH: keyPath,
    });
  } finally {
    if (directory) rmSync(directory, { recursive: true, force: true });
  }
}
