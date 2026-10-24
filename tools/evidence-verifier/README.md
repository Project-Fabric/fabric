# Independent evidence verifier

`verify_evidence.py` checks a Cosigna evidence chain (`evidence.jsonl`) without using any
Cosigna code. It is Python's standard library plus the `blake3` hash package, and is short enough
to read in one sitting.

```powershell
cosigna-agent evidence export --data-dir <gateway profile> --out evidence.jsonl
python -m pip install blake3
python -I tools/evidence-verifier/verify_evidence.py evidence.jsonl
```

It checks two things:
1. **The chain:** each record's BLAKE3 hash, its sequence number, and its link to the previous record.
2. **The rules, replayed from the records alone:**
   - no budget is overspent, and no operation exceeds its limit;
   - automatic approvals stay within the agent's grant at that moment;
   - only the owner approves or rejects;
   - holds match the requested amounts;
   - every paid operation has a matching receipt and spent hold;
   - every approval names the digest it approved.

Exit code 0 means everything passed; 5 means a problem was found.

The Rust verifier (`cosigna-agent evidence verify`) checks that the chain and the live database
agree. This one checks that the recorded decisions obeyed the rules, so a forger who rewrites the
whole chain consistently, with every hash recomputed, is still caught if a rule was broken. Both run in
CI and in the attack lab, and they must agree. Without `blake3` installed, hashes are reported as
"not checked" rather than assumed. Tests: `python -I tools/evidence-verifier/test_verify_evidence.py`.
