import { createServer } from 'node:http';
import { randomBytes } from 'node:crypto';
import next from 'next';
import nextEnv from '@next/env';
import { COOKIE, equal, issueSession, readSession } from './server/security.mjs';
import { forget, identify, remember, reserveSignIn, signInAllowed, wellFormed } from './server/approvers.mjs';

const dev = process.argv.includes('--dev');
nextEnv.loadEnvConfig(process.cwd(), dev);
if (process.argv.includes('--fixtures')) process.env.CONSOLE_FIXTURES = '1';
const fixtures = process.env.CONSOLE_FIXTURES === '1';
if (fixtures && !process.env.CONSOLE_USER) process.env.CONSOLE_USER = 'priya@northwind.test';
if (!process.env.CONSOLE_USER?.trim()) throw new Error('Configure CONSOLE_USER before starting live mode.');
// Live mode reads a Fabric project, a Cosigna gateway, or both (a server deployment has only the gateway).
if (!fixtures && !process.env.CONSOLE_PROJECT && !process.env.CONSOLE_GATEWAY_URL)
  throw new Error('Configure CONSOLE_PROJECT or CONSOLE_GATEWAY_URL before starting live mode.');
const port = Number(process.env.CONSOLE_PORT || 3090);
if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Invalid CONSOLE_PORT.');
// Behind a TLS proxy (deploy/docker), the address people open, such as https://cosigna.example.com.
// Requests must then name that host and arrive with X-Forwarded-Proto: https; cookies are Secure.
let publicOrigin = null;
if (process.env.CONSOLE_PUBLIC_ORIGIN) {
  let url;
  try { url = new URL(process.env.CONSOLE_PUBLIC_ORIGIN); } catch { url = null; }
  if (!url || url.protocol !== 'https:' || url.username || url.password || url.pathname !== '/' || url.search || url.hash)
    throw new Error('CONSOLE_PUBLIC_ORIGIN must be https://<host>[:port], with no path.');
  publicOrigin = url;
}
const expectedHost = publicOrigin ? publicOrigin.host : `127.0.0.1:${port}`;
const listenHost = publicOrigin ? '0.0.0.0' : '127.0.0.1';
const cookieFlags = `Path=/; HttpOnly; SameSite=Strict${publicOrigin ? '; Secure' : ''}`;
// Who is signing in, for the guessing limit: behind the proxy, the address it appended last to
// X-Forwarded-For (earlier entries come from the client); otherwise the connection's peer.
const clientOf = req => {
  const forwarded = publicOrigin && String(req.headers['x-forwarded-for'] || '').split(',').pop()?.trim();
  return forwarded || req.socket.remoteAddress || 'unknown';
};
const admitted = req => req.headers.host === expectedHost && (!publicOrigin || req.headers['x-forwarded-proto'] === 'https');
process.env.CONSOLE_INTERNAL_KEY = randomBytes(64).toString('hex');
process.env.CONSOLE_ORIGIN = publicOrigin ? publicOrigin.origin : `http://127.0.0.1:${port}`;
// Next dev restores its initial environment while preparing/reloading the app.
// Preserve our server-only runtime configuration in that snapshot as well, so
// the HTTP wrapper and compiled route handlers use the same session key/mode.
nextEnv.updateInitialEnv({
  CONSOLE_FIXTURES: process.env.CONSOLE_FIXTURES,
  CONSOLE_USER: process.env.CONSOLE_USER,
  CONSOLE_INTERNAL_KEY: process.env.CONSOLE_INTERNAL_KEY,
  CONSOLE_ORIGIN: process.env.CONSOLE_ORIGIN,
});
// Approver sign-in page: plain HTML, no scripts. The key field is a password field.
const signInPage = (message = '', status = 200) => [status, `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Approver sign-in · Cosigna</title>
<style>body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0b0c0e;color:#e8e9ec;font:15px/1.6 system-ui,sans-serif}
main{width:min(420px,calc(100% - 32px))}h1{font-size:22px;margin:0 0 8px}p{color:#a7aab3;margin:0 0 20px}
label{display:block;font-size:13px;color:#a7aab3;margin-bottom:6px}input{width:100%;box-sizing:border-box;padding:11px 12px;border-radius:8px;
border:1px solid #2c2f36;background:#131418;color:#e8e9ec;font:13px ui-monospace,monospace}button{margin-top:14px;width:100%;padding:11px;
border:0;border-radius:8px;background:#e8e9ec;color:#0b0c0e;font-weight:600;font-size:14px;cursor:pointer}
.error{color:#ff8a80;margin:14px 0 0}</style></head><body><main><h1>Approver sign-in</h1>
<p>Sign in with the approver key the owner gave you. You will see the payments waiting for a person, and can approve them up to your limit.</p>
<form method="post" action="/signin"><label for="key">Approver key</label>
<input id="key" name="key" type="password" autocomplete="off" spellcheck="false" required>
<button type="submit">Sign in</button></form>${message ? `<p class="error" role="alert">${message}</p>` : ''}</main></body></html>`];
async function readForm(req) {
  let body = '';
  for await (const chunk of req) { body += chunk; if (body.length > 2048) return null; }
  return new URLSearchParams(body);
}
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
  if (!admitted(req)) return deny(403, publicOrigin ? 'Invalid host, or not through the HTTPS proxy.' : 'Invalid local host.');
  // Everything from here runs inside the try: an error answers this request instead of escaping
  // as an unhandled rejection, whose outcome would depend on what Next has installed.
  try {
    let url;
    try { url = new URL(req.url || '/', process.env.CONSOLE_ORIGIN); }
    catch { return deny(400, 'Invalid request address.'); }
    if (url.searchParams.has('token')) {
      if (req.method !== 'GET' || url.pathname !== '/' || used || Date.now() > bootstrapExpires || !equal(url.searchParams.get('token'), token))
        return deny(403, 'This access link is invalid, expired or already used. Restart the console for a new link.');
      used = true;
      const issued = issueSession(process.env.CONSOLE_USER);
      res.setHeader('Set-Cookie', `${COOKIE}=${issued.cookie}; ${cookieFlags}; Max-Age=43200`);
      res.writeHead(303, { Location: '/approvals' }); res.end(); return;
    }
    const html = ([status, page]) => { res.writeHead(status, { 'Content-Type': 'text/html; charset=utf-8' }); res.end(page); };
    if (url.pathname === '/signin' && req.method === 'GET') return html(signInPage());
    if (url.pathname === '/signin' && req.method === 'POST') {
      if (req.headers.origin !== process.env.CONSOLE_ORIGIN) return deny(403, 'Request rejected: origin check failed.');
      if (fixtures) return html(signInPage('Approver sign-in needs a live Cosigna gateway; fixtures mode has none.', 503));
      const client = clientOf(req);
      const tooMany = () => html(signInPage('Too many failed sign-ins. Wait a minute and try again.', 429));
      if (!signInAllowed(Date.now(), client)) return tooMany();
      const form = await readForm(req);
      const key = form?.get('key');
      const refused = () => html(signInPage('That key was not accepted. Check it with the owner who issued it.', 401));
      // A key of the wrong shape cannot be a guess at a real key, so it is refused without counting.
      if (!wellFormed(key)) return refused();
      const release = reserveSignIn(client);
      if (!release) return tooMany();
      const approver = await identify(key);
      if (!approver) return refused();
      release();
      const issued = issueSession(approver.name, Date.now(), 'approver');
      remember(issued.session.id, approver);
      res.setHeader('Set-Cookie', `${COOKIE}=${issued.cookie}; ${cookieFlags}; Max-Age=43200`);
      res.writeHead(303, { Location: '/approvals' }); res.end(); return;
    }
    if (url.pathname === '/signout' && req.method === 'POST') {
      if (req.headers.origin !== process.env.CONSOLE_ORIGIN) return deny(403, 'Request rejected: origin check failed.');
      const session = readSession(req.headers.cookie);
      if (session) forget(session.id);
      res.setHeader('Set-Cookie', `${COOKIE}=; ${cookieFlags}; Max-Age=0`);
      res.writeHead(303, { Location: '/signin' }); res.end(); return;
    }
    const session = readSession(req.headers.cookie);
    if (!session) return deny(401, 'Open the one-time access URL printed in the console terminal, or sign in as an approver at /signin.');
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
    if (!admitted(req) || req.headers.origin !== process.env.CONSOLE_ORIGIN || !readSession(req.headers.cookie)) {
      socket.destroy(); return;
    }
    void app.getUpgradeHandler()(req, socket, head);
  } catch { socket.destroy(); }
});
server.listen(port, listenHost, () => {
  console.log(`Cosigna Console · ${fixtures ? 'FIXTURES — actions are simulated' : 'LIVE — decisions are recorded'} · ${process.env.CONSOLE_USER}`);
  console.log(`Open once (expires in 10 minutes): ${process.env.CONSOLE_ORIGIN}/?token=${token}`);
});
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => server.close(() => process.exit(0)));
