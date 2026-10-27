// Runs the same checks as GitHub Actions (ci.yml and product-checks.yml) on this machine, for
// when hosted CI is unavailable. Writes a summary to paste into a pull request.
//
// Usage, from the repository root:
//   node scripts/check-all.mjs                 every suite whose tools are installed
//   node scripts/check-all.mjs --only rust,gateway
//   node scripts/check-all.mjs --skip mobile
//   node scripts/check-all.mjs --list
//
// A step whose tool is missing is reported as SKIPPED, never as passed; the run is then
// incomplete. Local runs cover one operating system; CI also runs the Rust suite on Linux,
// macOS and Windows. Logs and the summary go to target/check-all/ (git-ignored).
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const windows = process.platform === 'win32';
const exe = windows ? '.exe' : '';
const out = join(root, 'target', 'check-all');
const logs = join(out, 'logs');
const binary = join(root, 'target', 'debug', `cosigna-agent${exe}`);
const venv = join(out, 'venv');
const venvPython = join(venv, windows ? 'Scripts' : 'bin', `python${exe}`);

// Docker counts only when its engine answers, not just when the command is installed.
function has(tool) {
  const probe = { go: ['version'], docker: ['info'] }[tool] ?? ['--version'];
  return spawnSync(tool, probe, { stdio: 'ignore', shell: windows }).status === 0;
}
const why = (tool) =>
  tool === 'docker' && spawnSync('docker', ['--version'], { stdio: 'ignore', shell: windows }).status === 0
    ? 'docker is installed but its engine is not running'
    : `needs ${tool}`;
const python = ['python3', 'python'].find(has);
const exists = (path) => () => existsSync(join(root, path));

// Each step: a name, the command, where it runs, the tools it needs and an optional condition.
const step = (name, cmd, opts = {}) => ({ name, cmd, cwd: opts.cwd ?? '.', needs: opts.needs ?? [], when: opts.when, env: opts.env });
const suites = {
  rust: [
    step('cargo fmt --check', ['cargo', 'fmt', '--all', '--check'], { needs: ['cargo'] }),
    step('cargo clippy -D warnings', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'], { needs: ['cargo'] }),
    step('cargo test --workspace', ['cargo', 'test', '--workspace', '--locked'], { needs: ['cargo'] }),
  ],
  gateway: [
    step('source secret scan', ['node', 'scripts/check-source-secrets.mjs'], { needs: ['node'] }),
    // CI's shell expands scripts/tests/*.test.mjs; list the files the same way on every OS.
    step('script tests', ['node', '--test', ...readdirSync(join(root, 'scripts', 'tests')).filter((f) => f.endsWith('.test.mjs')).map((f) => `scripts/tests/${f}`)], { needs: ['node'] }),
    step('pipeline lab unit tests', [python, '-m', 'unittest', 'discover', '-s', 'testbeds/pipeline-lab/tests'], { needs: ['python'] }),
    step('build gateway', ['cargo', 'build', '-p', 'cosigna-agent', '--locked'], { needs: ['cargo'] }),
    step('pipeline demo', ['cargo', 'run', '-q', '-p', 'cosigna-agent', '--locked', '--', 'pipeline-demo'], { needs: ['cargo'] }),
    step('gateway service check', ['node', 'scripts/check-gateway-service.mjs'], { needs: ['node'] }),
    step('verifier environment', [python, '-m', 'venv', venv], { needs: ['python'], when: () => !existsSync(venvPython) }),
    step('verifier dependency', [venvPython, '-m', 'pip', 'install', '--quiet', 'blake3==1.0.11'], { needs: ['python'] }),
    step('independent evidence verifier', [venvPython, '-I', 'tools/evidence-verifier/test_verify_evidence.py'], { needs: ['python'], env: { COSIGNA_AGENT_BIN: binary } }),
    step('attack, stress and crash lab', [venvPython, '-I', 'testbeds/gateway-attack-lab/attack_lab.py', '--binary', binary, '--out', join(out, 'attack-lab.json')], { needs: ['python'] }),
    step('systemd templates', ['systemd-analyze', 'verify', 'deploy/systemd/cosigna-api.service', 'deploy/systemd/cosigna-worker.service'], { needs: ['systemd-analyze'] }),
    step('agent refund demo tests', [python, '-I', '-W', 'error::ResourceWarning', '-m', 'unittest', 'discover', '-s', 'experiments/agent-refund-demo', '-p', 'test_*.py'], { needs: ['python'], when: exists('experiments/agent-refund-demo') }),
    step('voice merchant install', ['npm', 'ci', '--no-audit', '--no-fund'], { cwd: 'testbeds/voice-merchant', needs: ['npm'] }),
    step('voice merchant tests', ['npm', 'test'], { cwd: 'testbeds/voice-merchant', needs: ['npm'] }),
  ],
  console: [
    step('console install', ['npm', 'ci', '--no-audit', '--no-fund'], { cwd: 'console', needs: ['npm'] }),
    ...['test', 'typecheck', 'lint', 'build', 'test:integration', 'test:dev', 'test:gateway'].map((script) =>
      step(`console ${script}`, ['npm', 'run', script], { cwd: 'console', needs: ['npm'] })),
    step('site check', ['npm', 'run', 'check'], { cwd: 'site', needs: ['npm'] }),
    step('site tests', ['npm', 'test'], { cwd: 'site', needs: ['npm'] }),
  ],
  mobile: [
    step('mobile install', ['npm', 'ci', '--no-audit', '--no-fund'], { cwd: 'mobile', needs: ['npm'] }),
    step('mobile lint', ['npm', 'run', 'lint'], { cwd: 'mobile', needs: ['npm'] }),
    step('mobile typecheck', ['npm', 'run', 'typecheck'], { cwd: 'mobile', needs: ['npm'] }),
    step('mobile tests', ['npm', 'test', '--', '--watch=false'], { cwd: 'mobile', needs: ['npm'] }),
    step('mobile iOS tests', ['npm', 'run', 'test:ios', '--', '--watch=false'], { cwd: 'mobile', needs: ['npm'] }),
    step('mobile native config', ['npm', 'run', 'test:native-config'], { cwd: 'mobile', needs: ['npm'] }),
    step('mobile Android bundle', ['npm', 'run', 'bundle:android'], { cwd: 'mobile', needs: ['npm'] }),
  ],
  cli: [
    step('go vet', ['go', 'vet', './...'], { cwd: 'cli', needs: ['go'] }),
    step('go test', ['go', 'test', './...'], { cwd: 'cli', needs: ['go'] }),
  ],
  // CI uses Ruby directly; without Ruby, the same tests run in the backend's own container.
  rails: has('ruby') ? [
    step('rails install', ['bundle', 'install', '--quiet'], { cwd: 'backend', needs: ['ruby'] }),
    step('rails zeitwerk:check', ['bundle', 'exec', 'rails', 'zeitwerk:check'], { cwd: 'backend', needs: ['ruby'], env: { RAILS_ENV: 'test' } }),
    step('rails test', ['bundle', 'exec', 'rails', 'test'], { cwd: 'backend', needs: ['ruby'], env: { RAILS_ENV: 'test' } }),
  ] : [
    step('rails zeitwerk:check (container)', ['docker', 'compose', '-f', 'backend/compose.yaml', 'run', '--build', '--rm', 'test', 'bundle', 'exec', 'rails', 'zeitwerk:check'], { needs: ['docker'] }),
    step('rails test (container)', ['docker', 'compose', '-f', 'backend/compose.yaml', 'run', '--rm', 'test'], { needs: ['docker'] }),
  ],
  // Not in CI yet: the self-hosted image must still build.
  docker: [
    step('gateway image build', ['docker', 'build', '-q', '-f', 'deploy/docker/Dockerfile', '-t', 'cosigna-gateway:check', '.'], { needs: ['docker'], when: exists('deploy/docker/Dockerfile') }),
  ],
};

const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(name);
  return at === -1 ? null : (args[at + 1] ?? '').split(',').filter(Boolean);
};
if (args.includes('--list')) {
  for (const [suite, steps] of Object.entries(suites)) console.log(`${suite}: ${steps.map((s) => s.name).join('; ')}`);
  process.exit(0);
}
const only = option('--only');
const skip = option('--skip') ?? [];
const unknown = [...(only ?? []), ...skip].filter((name) => !suites[name]);
if (unknown.length) {
  console.error(`Unknown suite: ${unknown.join(', ')}. Suites: ${Object.keys(suites).join(', ')}`);
  process.exit(2);
}
const chosen = Object.keys(suites).filter((name) => (only ? only.includes(name) : true) && !skip.includes(name));

rmSync(logs, { recursive: true, force: true });
mkdirSync(logs, { recursive: true });
const available = new Map();
const tool = (name) => {
  if (name === 'python') return Boolean(python);
  if (!available.has(name)) available.set(name, has(name));
  return available.get(name);
};
const git = (...a) => spawnSync('git', a, { cwd: root, encoding: 'utf8' }).stdout.trim();
const results = [];
let index = 0;
for (const suite of chosen) {
  let suiteBroken = false;
  for (const s of suites[suite]) {
    index += 1;
    const label = `${suite} / ${s.name}`;
    if (s.when && !s.when()) {
      results.push({ suite, name: s.name, status: 'not applicable' });
      continue;
    }
    const missing = s.needs.filter((n) => !tool(n));
    if (missing.length || suiteBroken) {
      const reason = missing.length ? missing.map(why).join('; ') : 'an earlier step in this suite failed';
      results.push({ suite, name: s.name, status: 'SKIPPED', why: reason });
      console.log(`SKIP  ${label} (${reason})`);
      continue;
    }
    if (process.stdout.isTTY) process.stdout.write(`....  ${label}\r`);
    const started = Date.now();
    const run = spawnSync(s.cmd[0], s.cmd.slice(1), {
      cwd: join(root, s.cwd), env: { ...process.env, ...s.env }, encoding: 'utf8',
      shell: windows, maxBuffer: 256 * 1024 * 1024, timeout: 30 * 60 * 1000,
    });
    const seconds = Math.round((Date.now() - started) / 1000);
    const log = join(logs, `${String(index).padStart(2, '0')}-${suite}-${s.name.replace(/[^a-z0-9]+/gi, '-')}.log`);
    writeFileSync(log, `$ ${s.cmd.join(' ')}\n\n${run.stdout ?? ''}${run.stderr ?? ''}${run.error ? `\n${run.error.message}` : ''}`);
    const ok = run.status === 0;
    results.push({ suite, name: s.name, status: ok ? 'passed' : 'FAILED', seconds, log });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${label} (${seconds}s)${ok ? '' : `\n      log: ${log}`}`);
    if (!ok) {
      const tail = `${run.stdout ?? ''}${run.stderr ?? ''}`.trimEnd().split('\n').slice(-15);
      for (const line of tail) console.log(`      | ${line}`);
      // Later steps build on earlier ones (installs, builds); skip them rather than pile up
      // failures that only repeat this one.
      suiteBroken = true;
    }
  }
}

const count = (status) => results.filter((r) => r.status === status).length;
const failed = count('FAILED');
const skipped = count('SKIPPED');
const dirty = git('status', '--porcelain') !== '';
const verdict = failed ? 'FAILED' : skipped ? 'PASSED, INCOMPLETE (some steps skipped)' : 'PASSED';
const summary = [
  `### Local checks: ${verdict}`,
  '',
  `- Commit \`${git('rev-parse', '--short', 'HEAD')}\` on \`${git('branch', '--show-current') || 'detached'}\`${dirty ? ' **with uncommitted changes**' : ''}`,
  `- ${process.platform} ${process.arch}; node ${process.versions.node}; ${spawnSync('cargo', ['--version'], { encoding: 'utf8', shell: windows }).stdout?.trim() ?? 'cargo missing'}`,
  `- Suites: ${chosen.join(', ')}. ${count('passed')} passed, ${failed} failed, ${skipped} skipped.`,
  '- Run with `node scripts/check-all.mjs`, the same steps as ci.yml and product-checks.yml, on one operating system only (CI also runs Rust on Linux, macOS and Windows).',
  '',
  '| Suite | Step | Result |',
  '|---|---|---|',
  ...results.filter((r) => r.status !== 'not applicable').map((r) =>
    `| ${r.suite} | ${r.name} | ${r.status}${r.seconds !== undefined ? ` (${r.seconds}s)` : ''}${r.why ? `: ${r.why}` : ''} |`),
  '',
];
writeFileSync(join(out, 'summary.md'), summary.join('\n'));
console.log(`\n${verdict}: ${count('passed')} passed, ${failed} failed, ${skipped} skipped.`);
console.log(`Summary for the pull request: ${join(out, 'summary.md')}`);
process.exit(failed ? 1 : 0);
