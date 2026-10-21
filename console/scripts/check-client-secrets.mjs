import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
const root = path.join(process.cwd(), '.next', 'static');
const patterns = [/(?:sk|rk)_(?:test|live)_[A-Za-z0-9_]{8,}/, /fat_[A-Za-z0-9_-]{8,}/,
  /aa_live_[A-Za-z0-9_-]{8,}/, /eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/];
let checked = 0;
async function scan(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const target = path.join(dir, entry.name);
    if (entry.isDirectory()) await scan(target);
    else if (entry.name.endsWith('.js')) {
      const content = await readFile(target, 'utf8'); checked++;
      if (patterns.some(pattern => pattern.test(content))) throw new Error(`A credential pattern was found in client bundle ${path.relative(root, target)}. Value omitted.`);
    }
  }
}
await scan(root);
if (!checked) throw new Error('No production client bundles found. Run npm run build first.');
console.log(`Client secret check passed: ${checked} production JavaScript files.`);
