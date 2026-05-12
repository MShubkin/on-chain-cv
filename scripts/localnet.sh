#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> Building Anchor program..."
cd "$ROOT"
NO_DNA=1 anchor build

echo "==> Starting solana-test-validator (reset)..."
# Запускаем валидатор в фоне; убиваем предыдущий если был
pkill -f solana-test-validator 2>/dev/null || true
# --clone-upgradeable-program клонирует и аккаунт программы, и её program data (байткод).
# Обычный --clone для upgradeable BPF программ копирует только указатель — без байткода.
# verify_issuer делает CPI к MPL-Core, поэтому программа должна быть исполняемой.
solana-test-validator --reset --quiet \
  --clone-upgradeable-program CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d \
  --url https://api.mainnet-beta.solana.com &
VALIDATOR_PID=$!
echo "    validator PID: $VALIDATOR_PID"

# Ждём пока валидатор поднимется
echo "==> Waiting for validator..."
until solana cluster-version --url http://127.0.0.1:8899 &>/dev/null; do
  sleep 1
done
echo "    validator ready"

echo "==> Deploying program..."
anchor deploy --provider.cluster localnet

echo "==> airdrop 10 Sol everybody..."
for addr in E8LGVoNz2oL3MssCMde9vztwS1LWQPiUxHo3ZYAbPgEK Bjig4Ti7R92jjpSDrNHK6KysEyzvqkUKhUz6kdgRSqSo BGy6C7Hz9JFUNTzzFukiHQpeaetcKvWifucGH72Ui3y3 Fkqo4vWFpdzKM8qmStW8FPMhCCACnW9Zve7LpcLKyr8N; do solana airdrop 10 $addr --url localhost; done

echo ""
echo "✅ Done! Program deployed at: $(solana address -k target/deploy/on_chain_cv-keypair.json --url http://127.0.0.1:8899)"
echo ""
echo "   Start the frontend:"
echo "   cd app && npm install && npm run dev"
echo ""
echo "   Stop validator when done:"
echo "   kill $VALIDATOR_PID"
