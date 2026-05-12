"use client";

import { useConnection } from "@solana/wallet-adapter-react";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import {
  deserializeIssuerRegistry,
  IssuerRegistryAccount,
  explorerUrl,
} from "@/lib/program";

export default function IssuerPage() {
  const { connection } = useConnection();
  const { pda } = useParams<{ pda: string }>();
  const [issuer, setIssuer] = useState<IssuerRegistryAccount | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!pda) return;
    let pdaPubkey: PublicKey;
    try {
      pdaPubkey = new PublicKey(pda);
    } catch {
      setError("Invalid PDA address");
      setLoading(false);
      return;
    }
    connection
      .getAccountInfo(pdaPubkey)
      .then((info) => {
        if (!info) { setError("Issuer not found on-chain"); return; }
        setIssuer(deserializeIssuerRegistry(Buffer.from(info.data)));
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [connection, pda]);

  if (loading) return <p className="text-sm text-gray-500">Loading…</p>;
  if (error) return <p className="text-sm text-red-400">{error}</p>;
  if (!issuer) return null;

  const pdaPubkey = new PublicKey(pda);

  return (
    <div className="flex flex-col gap-6 max-w-lg">
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          Issuer Profile
        </div>
        <h1 className="text-2xl font-bold">{issuer.name}</h1>
        <a
          href={issuer.website}
          target="_blank"
          rel="noopener noreferrer"
          className="text-sm text-purple-400 hover:text-purple-300"
        >
          {issuer.website} ↗
        </a>
      </div>

      <div className="rounded-xl border border-gray-800 bg-gray-900 p-5 flex flex-col gap-4">
        {issuer.deactivatedAt !== null ? (
          <div className="flex items-center gap-2 text-red-400">
            <span>🚫</span>
            <span className="font-medium">Deactivated</span>
            <span className="text-xs text-gray-500">
              on{" "}
              {new Date(Number(issuer.deactivatedAt) * 1000).toLocaleDateString()}
            </span>
          </div>
        ) : issuer.isVerified ? (
          <div className="flex items-center gap-2 text-green-400">
            <span>✅</span>
            <span className="font-medium">Verified by platform</span>
            {issuer.verifiedAt && (
              <span className="text-xs text-gray-500">
                on{" "}
                {new Date(Number(issuer.verifiedAt) * 1000).toLocaleDateString()}
              </span>
            )}
          </div>
        ) : (
          <div className="flex items-center gap-2 text-yellow-500">
            <span>⏳</span>
            <span className="font-medium">Pending platform verification</span>
          </div>
        )}

        {issuer.collection && (
          <div className="flex flex-col gap-2 pt-3 border-t border-gray-800">
            <span className="text-xs text-gray-500">MPL-Core Collection</span>
            <code className="text-xs text-gray-300 bg-gray-800 rounded px-2 py-1 break-all">
              {issuer.collection.toBase58()}
            </code>
            <a
              href={explorerUrl(issuer.collection)}
              target="_blank"
              rel="noopener noreferrer"
              className="self-start text-sm text-purple-400 hover:text-purple-300"
            >
              Open Collection in Phantom →
            </a>
          </div>
        )}

        <div className="flex flex-col gap-1">
          <span className="text-xs text-gray-500">Credentials issued</span>
          <span className="font-mono text-sm">
            {issuer.credentialsIssued.toString()}
          </span>
        </div>
      </div>

      <a
        href={explorerUrl(pdaPubkey)}
        target="_blank"
        rel="noopener noreferrer"
        className="text-sm text-purple-400 hover:text-purple-300"
      >
        View on Solana Explorer →
      </a>
    </div>
  );
}
