import { buildEnvironment, selectedEnvironment, applicationConfig } from "./build-environment.mjs";
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
const profile = selectedEnvironment();
const target = join(root, "src-tauri", "target", profile || "unknown");
const output = join(root, 'output', profile || 'unknown', 'macos');
const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const env = { ...buildEnvironment(profile), CARGO_TARGET_DIR: target };
const signedUpdater = Boolean(env.TAURI_SIGNING_PRIVATE_KEY?.trim());
if (!signedUpdater) throw new Error('打包需要当前环境的 TAURI_SIGNING_PRIVATE_KEY');
const appName = profile === 'lab' ? 'LaunchOne Lab' : 'LaunchOne';

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

// A local test signature needs no shared Apple credentials. Production signing
// and notarization are a separate release operation.
run('xcrun', ['--find', 'clang']);
run('rustup', ['target', 'add', 'aarch64-apple-darwin', 'x86_64-apple-darwin']);
run(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'build', '--target', 'universal-apple-darwin',
  '--bundles', 'app,dmg', '--config', JSON.stringify({
    ...applicationConfig(profile, env.LAUNCHONE_UPDATE_PUBLIC_KEY),
    bundle: { macOS: { signingIdentity: profile === 'lab' ? '-' : undefined }, createUpdaterArtifacts: signedUpdater },
  })]);

const bundle = join(target, 'universal-apple-darwin', 'release', 'bundle');
const app = join(bundle, 'macos', `${appName}.app`);
run('lipo', [join(app, 'Contents', 'MacOS', 'launchone'), '-verify_arch', 'arm64', 'x86_64']);
run('codesign', ['--verify', '--deep', '--strict', app]);

const dmgs = readdirSync(join(bundle, 'dmg')).filter(name => name.endsWith('.dmg') && name.includes(version));
if (dmgs.length !== 1) throw new Error('未找到唯一的当前版本 DMG，请清理旧的 bundle 后重试。');
const dmg = join(bundle, 'dmg', dmgs[0]);
run('hdiutil', ['verify', dmg]);

mkdirSync(output, { recursive: true });
const dmgName = `LaunchOne_${version}_macOS_universal_${profile}.dmg`;
const zipName = `LaunchOne_${version}_macOS_universal_${profile}.app.zip`;
copyFileSync(dmg, join(output, dmgName));
run('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, join(output, zipName)]);
copyFileSync(join(root, 'docs', 'mac-test.md'), join(output, '测试说明.md'));
const names = [dmgName, zipName];
if (signedUpdater) {
  const macBundle = join(bundle, 'macos');
  const updaterArchives = readdirSync(macBundle).filter(name => name.endsWith('.app.tar.gz'));
  if (updaterArchives.length !== 1) throw new Error('签名更新包缺失或不唯一。');
  for (const name of [updaterArchives[0], `${updaterArchives[0]}.sig`]) {
    const source = join(macBundle, name);
    if (!readdirSync(macBundle).includes(name)) throw new Error(`更新包签名文件缺失：${name}`);
    copyFileSync(source, join(output, name));
    names.push(name);
  }
}
writeFileSync(join(output, 'SHA256SUMS.txt'), names.map(name =>
  `${createHash('sha256').update(readFileSync(join(output, name))).digest('hex')}  ${name}\n`).join(''));
console.log(`macOS 通用测试包已生成：${output}`);
