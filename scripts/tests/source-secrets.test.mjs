import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

test('publication scan rejects credential-shaped data without echoing it and accepts empty PEM fixtures', () => {
  const directory=mkdtempSync(join(tmpdir(),'cosigna-secret-scan-'));
  try {
    execFileSync('git',['init','--quiet',directory]);
    writeFileSync(join(directory,'fixture.txt'),'-----BEGIN PRIVATE KEY-----');
    execFileSync('git',['-C',directory,'add','fixture.txt']);
    const scanner=fileURLToPath(new URL('../check-source-secrets.mjs',import.meta.url));
    const run=()=>spawnSync(process.execPath,[scanner],{cwd:directory,encoding:'utf8'});
    assert.equal(run().status,0);
    const credential='gsk_'+'A'.repeat(40);
    writeFileSync(join(directory,'fixture.txt'),credential);
    const found=run();assert.equal(found.status,1);assert.match(found.stderr,/groq-key/);
    assert(!found.stderr.includes(credential));assert(!found.stdout.includes(credential));
    writeFileSync(join(directory,'fixture.txt'),'-----BEGIN PRIVATE KEY-----\n'+'A'.repeat(64));
    assert.equal(run().status,1);
  } finally {
    const target=resolve(directory);
    assert.equal(dirname(target),resolve(tmpdir()));
    assert(basename(target).startsWith('cosigna-secret-scan-'));
    rmSync(target,{recursive:true,force:true});
  }
});
