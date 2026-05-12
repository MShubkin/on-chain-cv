"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import dynamic from "next/dynamic";
import { Transaction } from "@solana/web3.js";
import { useCallback, useEffect, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import {
  CredentialAccount,
  EndorsementAccount,
  buildCloseEndorsementIx,
  deserializeCredential,
  fetchEndorsementsByEndorser,
} from "@/lib/program";

const WalletMultiButton = dynamic(
  () =>
    import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

const LOCKUP_SECONDS = 30 * 24 * 60 * 60; // 30 days

interface EndorsementRow {
  pda: PublicKey;
  endorsement: EndorsementAccount;
  credential: CredentialAccount | null;
}

function formatCountdown(secsLeft: number): string {
  if (secsLeft <= 0) return "Ready to reclaim";
  const days = Math.floor(secsLeft / 86400);
  const hours = Math.floor((secsLeft % 86400) / 3600);
  return `${days}d ${hours}h until reclaim`;
}

export default function MyEndorsementsPage() {
  const { connection } = useConnection();
  const { publicKey, sendTransaction } = useWallet();

  const [rows, setRows] = useState<EndorsementRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reclaimingPda, setReclaimingPda] = useState<string | null>(null);
  const [reclaimError, setReclaimError] = useState<string | null>(null);
  // Rerender every minute to update countdown displays
  const [, setTick] = useState(0);

  const loadEndorsements = useCallback(() => {
    if (!publicKey) return;
    setLoading(true);
    setError(null);
    fetchEndorsementsByEndorser(connection, publicKey)
      .then(async (results) => {
        const enriched = await Promise.all(
          results.map(async ({ pda, endorsement }) => {
            let credential: CredentialAccount | null = null;
            try {
              const info = await connection.getAccountInfo(endorsement.credential);
              if (info) credential = deserializeCredential(Buffer.from(info.data));
            } catch {}
            return { pda, endorsement, credential };
          })
        );
        // Sort: reclaim-ready first, then by endorsed_at descending
        enriched.sort((a, b) => {
          const nowSec = Math.floor(Date.now() / 1000);
          const aEnd = Number(a.endorsement.endorsedAt) + LOCKUP_SECONDS;
          const bEnd = Number(b.endorsement.endorsedAt) + LOCKUP_SECONDS;
          const aReady = aEnd <= nowSec;
          const bReady = bEnd <= nowSec;
          if (aReady !== bReady) return aReady ? -1 : 1;
          if (b.endorsement.endorsedAt > a.endorsement.endorsedAt) return 1;
          if (b.endorsement.endorsedAt < a.endorsement.endorsedAt) return -1;
          return 0;
        });
        setRows(enriched);
      })
      .catch((e) => {
        setError(e instanceof Error ? e.message : String(e));
        setRows([]);
      })
      .finally(() => setLoading(false));
  }, [connection, publicKey]);

  useEffect(() => {
    if (!publicKey) { setRows([]); return; }
    loadEndorsements();
  }, [loadEndorsements, publicKey]);

  // Countdown tick every 60 seconds
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 60_000);
    return () => clearInterval(id);
  }, []);

  const handleReclaim = async (credentialPda: PublicKey, endPda: PublicKey) => {
    if (!publicKey) return;
    const pdaStr = endPda.toBase58();
    setReclaimingPda(pdaStr);
    setReclaimError(null);
    try {
      const ix = buildCloseEndorsementIx({ endorser: publicKey, credentialPda });
      const tx = new Transaction().add(ix);
      const sig = await sendTransaction(tx, connection);
      await connection.confirmTransaction(sig, "confirmed");
      setRows((prev) => prev.filter((r) => r.pda.toBase58() !== pdaStr));
    } catch (e) {
      setReclaimError(e instanceof Error ? e.message : String(e));
    } finally {
      setReclaimingPda(null);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          My Endorsements
        </div>
        <h1 className="text-2xl font-bold">Endorsement Deposits</h1>
        <p className="text-sm text-gray-500 mt-1">
          SOL is locked for 30 days per endorsement. Reclaim after lockup expires.
        </p>
      </div>

      <WalletMultiButton />

      {!publicKey && (
        <p className="text-sm text-gray-500">Connect your wallet to view your endorsement deposits.</p>
      )}
      {loading && <p className="text-sm text-gray-500">Loading endorsements…</p>}
      {error && <p className="text-sm text-red-400">{error}</p>}
      {reclaimError && <p className="text-sm text-red-400">{reclaimError}</p>}

      {!loading && !error && publicKey && rows.length === 0 && (
        <p className="text-sm text-gray-500">No active endorsements.</p>
      )}

      {!loading && rows.length > 0 && (
        <div className="flex flex-col gap-3">
          {rows.map(({ pda, endorsement, credential }) => {
            const pdaStr = pda.toBase58();
            const nowSec = Math.floor(Date.now() / 1000);
            const lockupEnd = Number(endorsement.endorsedAt) + LOCKUP_SECONDS;
            const secsLeft = Math.max(0, lockupEnd - nowSec);
            const canReclaim = secsLeft <= 0;
            const isReclaiming = reclaimingPda === pdaStr;

            const credAddr = endorsement.credential.toBase58();
            const displayName = credential
              ? `${credential.skill} Level ${credential.level}`
              : `${credAddr.slice(0, 8)}…${credAddr.slice(-4)}`;

            return (
              <div
                key={pdaStr}
                className="rounded-xl border border-gray-800 bg-gray-900 p-4 flex items-center gap-4"
              >
                <div className="flex-1 min-w-0">
                  <a
                    href={`/credential/${credAddr}`}
                    className="text-sm font-medium hover:text-purple-400 transition-colors"
                  >
                    {displayName}
                  </a>
                  <p className="text-xs text-gray-500 mt-0.5">
                    Endorsed{" "}
                    {new Date(Number(endorsement.endorsedAt) * 1000).toLocaleDateString()}
                  </p>
                  <p
                    className={`text-xs mt-1 ${
                      canReclaim ? "text-green-400" : "text-gray-500"
                    }`}
                  >
                    {formatCountdown(secsLeft)}
                  </p>
                </div>

                <button
                  onClick={() => handleReclaim(endorsement.credential, pda)}
                  disabled={!canReclaim || isReclaiming}
                  className="flex-shrink-0 text-xs px-3 py-1.5 rounded-lg border transition-colors disabled:opacity-40
                    border-purple-700 text-purple-400 hover:border-purple-500 hover:text-purple-300
                    disabled:border-gray-700 disabled:text-gray-500"
                >
                  {isReclaiming ? "Reclaiming…" : "Reclaim deposit"}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
