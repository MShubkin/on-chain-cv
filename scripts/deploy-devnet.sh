#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROGRAM_KEYPAIR="$ROOT/target/deploy/on_chain_cv-keypair.json"
DEPLOY_WALLET="${SOLANA_WALLET:-$HOME/.config/solana/id.json}"

echo "==> Building Anchor program..."
cd "$ROOT"
NO_DNA=1 anchor build

echo "==> Checking deploy wallet balance..."
BALANCE=$(solana balance "$DEPLOY_WALLET" --url devnet | awk '{print $1}')
echo "    Balance: $BALANCE SOL"

# Deploying a ~300 KB BPF program costs ~3 SOL on devnet.
# If balance is below 3, request airdrops.
if (( $(echo "$BALANCE < 3" | bc -l) )); then
  echo "==> Funding deploy wallet via airdrop..."
  solana airdrop 2 --keypair "$DEPLOY_WALLET" --url devnet
  solana airdrop 2 --keypair "$DEPLOY_WALLET" --url devnet
fi

echo "==> Deploying to devnet..."
anchor deploy --provider.cluster devnet --provider.wallet "$DEPLOY_WALLET"

PROGRAM_ID=$(solana address -k "$PROGRAM_KEYPAIR")
echo ""
echo "✅ Program deployed at: $PROGRAM_ID"
echo "   Explorer: https://explorer.solana.com/address/$PROGRAM_ID?cluster=devnet"
