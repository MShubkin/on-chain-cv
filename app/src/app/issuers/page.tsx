"use client";

import { useConnection } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import { fetchAllIssuers, IssuerRegistryAccount } from "@/lib/program";

interface IssuerRow {
  pda: PublicKey;
  issuer: IssuerRegistryAccount;
}

export default function IssuersPage() {
  const { connection } = useConnection();
  const [rows, setRows] = useState<IssuerRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchAllIssuers(connection)
      .then(setRows)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [connection]);

  return (
    <div className="flex flex-col gap-6">
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          Registry
        </div>
        <h1 className="text-2xl font-bold">Credential Issuers</h1>
        <p className="mt-1 text-sm text-gray-400">
          All registered organisations on this platform.
        </p>
      </div>

      {loading && <p className="text-sm text-gray-500">Loading…</p>}
      {error && <p className="text-sm text-red-400">{error}</p>}
      {!loading && !error && rows.length === 0 && (
        <p className="text-sm text-gray-500">No issuers registered yet.</p>
      )}

      <div className="flex flex-col gap-3">
        {rows.map(({ pda, issuer }) => (
          <Link
            key={pda.toBase58()}
            href={`/issuer/${pda.toBase58()}`}
            className="rounded-xl border border-gray-800 bg-gray-900 p-4 flex items-center justify-between hover:border-purple-700 transition-colors"
          >
            <div className="flex flex-col gap-0.5">
              <span className="font-medium">{issuer.name}</span>
              <span className="text-xs text-gray-500">{issuer.website}</span>
            </div>
            <div className="flex items-center gap-3">
              {issuer.deactivatedAt !== null ? (
                <span className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-full px-2 py-0.5">
                  Deactivated
                </span>
              ) : issuer.isVerified ? (
                <span className="text-xs text-green-400 bg-green-950 border border-green-800 rounded-full px-2 py-0.5">
                  ✅ Verified
                </span>
              ) : (
                <span className="text-xs text-yellow-500 bg-yellow-950 border border-yellow-800 rounded-full px-2 py-0.5">
                  ⏳ Pending
                </span>
              )}
              <span className="text-gray-600">→</span>
            </div>
          </Link>
        ))}
      </div>

      <Link
        href="/issuer/register"
        className="self-start text-sm text-purple-400 hover:text-purple-300 transition-colors"
      >
        + Register your organisation →
      </Link>
    </div>
  );
}
