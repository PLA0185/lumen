import { readFileSync, writeFileSync } from 'node:fs';

const args = Object.fromEntries(
  process.argv.slice(2).map((arg) => {
    const match = /^--([^=]+)=(.*)$/.exec(arg);
    if (!match) throw new Error(`参数格式错误：${arg}`);
    return [match[1], match[2]];
  }),
);

const path = args.path ?? 'latest.json';
const repo = args.repo;
const tag = args.tag;
const asset = args.asset;

if (!repo || !/^[^/]+\/[^/]+$/.test(repo)) throw new Error('必须提供 --repo=owner/repo');
if (!tag) throw new Error('必须提供 --tag=<release tag>');
if (!asset || asset.includes('/') || asset.includes('\\')) {
  throw new Error('必须提供不含路径分隔符的 --asset=<installer filename>');
}

const manifest = JSON.parse(readFileSync(path, 'utf8'));
if (!manifest.platforms || typeof manifest.platforms !== 'object') {
  throw new Error('latest.json 缺少 platforms');
}

const directUrl = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset)}`;
let updated = 0;

for (const [platform, entry] of Object.entries(manifest.platforms)) {
  if (platform === 'windows-x86_64' || platform === 'windows-x86_64-nsis') {
    if (!entry || typeof entry !== 'object' || !entry.signature) {
      throw new Error(`${platform} 缺少 updater signature`);
    }
    entry.url = directUrl;
    updated += 1;
  }
}

if (updated === 0) throw new Error('latest.json 不含 Windows x86_64 updater 条目');

writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
console.log(`已把 ${updated} 个 Windows updater URL 规范为匿名 Release 直链。`);
