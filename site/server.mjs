import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = path.dirname(fileURLToPath(import.meta.url));
const port = Number(process.env.SITE_PORT || 3091);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Invalid SITE_PORT.');
const routes = new Map([
  ['/concept', ['pixel.html', 'text/html; charset=utf-8']],
  ['/pixel.css', ['pixel.css', 'text/css; charset=utf-8']],
  ['/pixel.js', ['pixel.js', 'text/javascript; charset=utf-8']],
  ['/pixel-renderer.js', ['pixel-renderer.js', 'text/javascript; charset=utf-8']],
  ['/assets/pixel-poster.png', ['assets/pixel-poster.png', 'image/png']],
  ['/', ['index.html', 'text/html; charset=utf-8']], ['/styles.css', ['styles.css', 'text/css; charset=utf-8']],
  ['/app.js', ['app.js', 'text/javascript; charset=utf-8']], ['/favicon.svg', ['assets/favicon.svg', 'image/svg+xml']],
  ['/assets/inter-regular.woff2', ['assets/inter-regular.woff2', 'font/woff2']],
  ['/assets/inter-medium.woff2', ['assets/inter-medium.woff2', 'font/woff2']],
  ['/assets/FONT-LICENSE.txt', ['assets/FONT-LICENSE.txt', 'text/plain; charset=utf-8']],
  ['/assets/fonts/clash-grotesk-400.woff2', ['assets/fonts/clash-grotesk-400.woff2', 'font/woff2']],
  ['/assets/fonts/clash-grotesk-500.woff2', ['assets/fonts/clash-grotesk-500.woff2', 'font/woff2']],
  ['/assets/vendor/gsap.min.js', ['assets/vendor/gsap.min.js', 'text/javascript; charset=utf-8']],
  ['/assets/vendor/ScrollTrigger.min.js', ['assets/vendor/ScrollTrigger.min.js', 'text/javascript; charset=utf-8']],
]);
const server = createServer(async (req, res) => {
  const deny = (status, message) => { res.writeHead(status, { 'Content-Type': 'text/plain; charset=utf-8' }); res.end(message); };
  res.setHeader('X-Content-Type-Options', 'nosniff'); res.setHeader('Referrer-Policy', 'no-referrer');
  res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'");
  if (req.headers.host !== `127.0.0.1:${port}`) return deny(403, 'Invalid local host.');
  if (!['GET', 'HEAD'].includes(req.method || '')) return deny(405, 'Read-only preview.');
  const pathname = new URL(req.url || '/', `http://127.0.0.1:${port}`).pathname;
  const entry = routes.get(pathname); if (!entry) return deny(404, 'Not found.');
  try { const bytes = await readFile(path.join(root, entry[0])); res.writeHead(200, { 'Content-Type': entry[1], 'Cache-Control': 'no-store' }); res.end(req.method === 'HEAD' ? undefined : bytes); }
  catch { deny(500, 'The preview could not load this asset.'); }
});
server.listen(port, '127.0.0.1', () => console.log(`Cosigna landing preview: http://127.0.0.1:${port}`));
for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => server.close(() => process.exit(0)));
