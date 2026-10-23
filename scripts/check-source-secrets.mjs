import { execFileSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';

// A publication guard for common credential formats, not a complete secret detector.
// Print location and category only; a matched credential must never reach CI logs.
const files=execFileSync('git',['ls-files','-z'],{encoding:'utf8'}).split('\0').filter(Boolean);
const rules=[
  ['payment-key',/\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9_]{16,}\b/g],
  ['openai-key',/\bsk-(?:proj-|svcacct-)[A-Za-z0-9_-]{20,}\b/g],
  ['groq-key',/\bgsk_[A-Za-z0-9]{20,}\b/g],
  ['github-token',/\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})\b/g],
  // A header-only test fixture contains no key material.
  ['private-key',/-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----[\t ]*(?:\r?\n|\\n)[A-Za-z0-9+/=]{16,}/g],
];
let checked=0,findings=0;
for(const file of files){
  if(!statSync(file).isFile())continue;
  const bytes=readFileSync(file);if(bytes.includes(0))continue;
  const content=bytes.toString('utf8');checked++;
  for(const[kind,pattern]of rules)for(const match of content.matchAll(pattern)){
    if(/synthetic|fixture|example|placeholder|redacted/i.test(match[0]))continue;
    findings++;console.error(`${file}:${content.slice(0,match.index).split('\n').length}: ${kind} pattern; value omitted`);
  }
}
if(findings){console.error('Remove or replace credentials before publishing.');process.exitCode=1;}
else console.log(`Source credential-pattern check passed: ${checked} tracked text files.`);
