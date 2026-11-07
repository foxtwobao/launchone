import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'darwin') {
  console.error('macOS 安装包必须在 Mac 上构建。请使用 GitHub Actions 的 macOS test build 工作流。');
  process.exit(1);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const target = join(root, 'src-tauri', 'target');
const output = join(root, 'output', 'macos');
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const env = { ...process.env, CARGO_TARGET_DIR: target };

// A distributable package must have usable service settings. Fail the build
// instead of shipping an app with disabled login and TokenOne buttons.
const requiredSettings = [
  'LAUNCHONE_IDONE_ISSUER', 'LAUNCHONE_CLIENT_ID', 'LAUNCHONE_SERVICE_URL',
  'LAUNCHONE_CODEX_BASE_URL', 'LAUNCHONE_CODEX_MODEL',
  'LAUNCHONE_CLAUDE_BASE_URL', 'LAUNCHONE_CLAUDE_MODEL',
];
const missing = requiredSettings.filter(key => !env[key]?.trim());
if (missing.length) throw new Error(`打包配置缺失：${missing.join(', ')}`);
for (const key of requiredSettings.filter(key => key.endsWith('_URL') || key.endsWith('_ISSUER'))) {
  const url = new URL(env[key]);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) {
    throw new Error(`打包配置必须为不含凭据的 HTTPS 地址：${key}`);
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

// A local test signature needs no shared Apple credentials. Production signing
// and notarization are a separate release operation.
run('xcrun', ['--find', 'clang']);
run('rustup', ['target', 'add', 'aarch64-apple-darwin', 'x86_64-apple-darwin']);
run('npm', ['run', 'desktop:build', '--', '--target', 'universal-apple-darwin',
  '--bundles', 'app,dmg', '--config', JSON.stringify({ bundle: { macOS: { signingIdentity: '-' } } })]);

const bundle = join(target, 'universal-apple-darwin', 'release', 'bundle');
const app = join(bundle, 'macos', 'LaunchOne.app');
run('lipo', [join(app, 'Contents', 'MacOS', 'launchone'), '-verify_arch', 'arm64', 'x86_64']);
run('codesign', ['--verify', '--deep', '--strict', app]);

const dmgs = readdirSync(join(bundle, 'dmg')).filter(name => name.endsWith('.dmg') && name.includes(version));
if (dmgs.length !== 1) throw new Error('未找到唯一的当前版本 DMG，请清理旧的 bundle 后重试。');
const dmg = join(bundle, 'dmg', dmgs[0]);
run('hdiutil', ['verify', dmg]);

mkdirSync(output, { recursive: true });
const dmgName = `LaunchOne_${version}_macOS_universal_test.dmg`;
const zipName = `LaunchOne_${version}_macOS_universal_test.app.zip`;
copyFileSync(dmg, join(output, dmgName));
run('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, join(output, zipName)]);
copyFileSync(join(root, 'docs', 'mac-test.md'), join(output, '测试说明.md'));
const names = [dmgName, zipName];
writeFileSync(join(output, 'SHA256SUMS.txt'), names.map(name =>
  `${createHash('sha256').update(readFileSync(join(output, name))).digest('hex')}  ${name}\n`).join(''));
console.log(`macOS 通用测试包已生成：${output}`);
