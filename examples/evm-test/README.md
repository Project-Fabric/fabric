# Stablecoin refunds on a test network

`--evm-test <file>` connects the `evm_test` rail: an agent refunds a customer who paid in a
stablecoin, back to the address that paid, under the same grants, holds, approvals and evidence
as every other rail. Test networks only.

```sh
cosigna-agent serve  --evm-test examples/evm-test/evm-rail.json
cosigna-agent worker --evm-test examples/evm-test/evm-rail.json
```

- **Chains:** a local chain (31337), Sepolia, Base Sepolia, Arbitrum Sepolia or Optimism Sepolia.
  Any other chain id is refused, and so is a node that reports a different chain from the file.
- **Token:** the example is Circle's USDC on Base Sepolia. Check the address against Circle's
  documentation before use. Amounts are in US cents; the rail converts them to token units.
- **Node:** put the RPC address in `COSIGNA_EVM_RPC_URL` (or a file named by
  `COSIGNA_EVM_RPC_URL_FILE`). It often contains an API key, so it is never written down or
  printed. It must be `https://`.
- **Key:** create a fresh wallet used for nothing else, and put its private key in
  `COSIGNA_EVM_PRIVATE_KEY` (or `..._FILE`). Fund it with test USDC from Circle's faucet and a
  little test ETH for fees. The gateway prints the wallet's address at startup.
- **Refunds:** `source` is the customer's payment transaction hash. The rail checks that it
  transferred the token to the gateway's wallet from one sender, with `confirmations` blocks
  on top. It refunds at most what that payment brought in, less what was already refunded.

A refund is signed with its own nonce and saved before it is sent, and afterwards only those
same bytes are ever sent again. A lost reply or a crash can delay a refund but cannot pay it
twice. If another transaction took its nonce, it can never be included, and resolving it voids
it with no money moved. The details and limits are in `docs/STORAGE_FORMAT.md` (Stablecoin rail).
