import { createServer } from 'node:http';
import { randomBytes } from 'node:crypto';
import next from 'next';
import nextEnv from '@next/env';
import { COOKIE, equal, issueSession, readSession } from './server/security.mjs';

const dev = process.argv.includes('--dev');
nextEnv.loadEnvConfig(process.cwd(), dev);
if (process.argv.includes('--fixtures')) process.env.CONSOLE_FIXTURES = '1';
const fixtures = process.env.CONSOLE_FIXTURES === '1';
if (fixtures && !process.env.CONSOLE_USER) process.env.CONSOLE_USER = 'priya@northwind.test';
if (!process.env.CONSOLE_USER?.trim()) throw new Error('Configure CONSOLE_USER before starting live mode.');
if (!fixtures && !process.env.CONSOLE_PROJECT) throw new Error('Configure CONSOLE_PROJECT before starting live mode.');
const port = Number(process.env.CONSOLE_PORT || 3090);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Invalid CONSOLE_PORT.');
process.env.CONSOLE_INTERNAL_KEY = randomBytes(64).toString('hex');
process.env.CONSOLE_ORIGIN = `http://127.0.0.1:${port}`;
// Next dev restores its initial environment while preparing/reloading the app.
// Preserve our server-only runtime configuration in that snapshot as well, so
// the HTTP wrapper and compiled route handlers use the same session key/mode.
nextEnv.updateInitialEnv({
  CONSOLE_FIXTURES: process.env.CONSOLE_FIXTURES,
  CONSOLE_USER: process.env.CONSOLE_USER,
  CONSOLE_INTERNAL_KEY: process.env.CONSOLE_INTERNAL_KEY,
  CONSOLE_ORIGIN: process.env.CONSOLE_ORIGIN,
});
const token = randomBytes(32).toString('hex');
const bootstrapExpires = Date.now() + 10 * 60 * 1000;
let used = false;
const app = next({ dev, hostname: '127.0.0.1', port });
await app.prepare();
const handle = app.getRequestHandler();
const server = createServer(async (req, res) => {
  res.setHeader('Cache-Control', 'no-store');
  res.setHeader('Referrer-Policy', 'no-referrer');
  res.setHeader('X-Content-Type-Options', 'nosniff');
  res.setHeader('X-Frame-Options', 'DENY');
  res.setHeader('Content-Security-Policy', "frame-ancestors 'none'; object-src 'none'; base-uri 'self'");
  const deny = (code, message) => { res.writeHead(code, { 'Content-Type': 'text/plain; charset=utf-8' }); res.end(message); };
  if (req.headers.host !== `127.0.0.1:${port}`) return deny(403, 'Invalid local host.');
  const url = new URL(req.url || '/', process.env.CONSOLE_ORIGIN);
  if (url.searchParams.has('token')) {
    if (req.method !== 'GET' || url.pathname !== '/' || used || Date.now() > bootstrapExpires || !equal(url.searchParams.get('token'), token))
      return deny(403, 'This access link is invalid, expired or already used. Restart the console for a new link.');
    used = true;
    const issued = issueSession(process.env.CONSOLE_USER);
    res.setHeader('Set-Cookie', `${COOKIE}=${issued.cookie}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200`);
    res.writeHead(303, { Location: '/approvals' }); res.end(); return;
  }
  try {
    const session = readSession(req.headers.cookie);
    if (!session) return deny(401, 'Open the one-time access URL printed in the console terminal.');
    if (!['GET', 'HEAD'].includes(req.method || '')) {
      if (req.headers.origin !== process.env.CONSOLE_ORIGIN || !equal(req.headers['x-csrf-token'], session.csrf))
        return deny(403, 'Request rejected: session or CSRF check failed.');
      if (Number(req.headers['content-length'] || 0) > 65536) return deny(413, 'Request is too large.');
    }
    await handle(req, res);
  } catch { if (!res.headersSent) deny(500, 'The local console could not serve this request.'); else res.end(); }
});
server.on('upgrade', (req, socket, head) => {
  try {
    if (req.headers.host !== `127.0.0.1:${port}` || req.headers.origin !== process.env.CONSOLE_ORIGIN || !readSession(req.headers.cookie)) {
      socket.destroy(); return;
    }
    void app.getUpgradeHandler()(req, socket, head);
  } catch { socket.destroy(); }
});
server.listen(port, '127.0.0.1', () => {
  console.log(`Cosigna Console · ${fixtures ? 'FIXTURES — actions are simulated' : 'LIVE — decisions are recorded'} · ${process.env.CONSOLE_USER}`);
  console.log(`Open once (expires in 10 minutes): ${process.env.CONSOLE_ORIGIN}/?token=${token}`);
});
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => server.close(() => process.exit(0)));
