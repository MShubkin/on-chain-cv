"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import dynamic from "next/dynamic";
import { Transaction } from "@solana/web3.js";
import { useState } from "react";
import {
  buildRegisterIssuerIx,
  getIssuerRegistryPda,
} from "@/lib/program";
import { useRouter } from "next/navigation";

const WalletMultiButton = dynamic(
  () =>
    import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

export default function RegisterIssuerPage() {
  const { connection } = useConnection();
  const { publicKey, sendTransaction } = useWallet();
  const router = useRouter();

  const [name, setName] = useState("");
  const [website, setWebsite] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleRegister = async () => {
    if (!publicKey) return;
    setLoading(true);
    setError(null);
    try {
      const ix = buildRegisterIssuerIx(publicKey, name, website);
      const tx = new Transaction().add(ix);
      const signature = await sendTransaction(tx, connection);
      await connection.confirmTransaction(signature, "confirmed");
      const [issuerPda] = getIssuerRegistryPda(publicKey);
      router.push(`/issuer/${issuerPda.toBase58()}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const canSubmit =
    !!publicKey && name.trim().length > 0 && website.trim().length > 0 && !loading;

  return (
    <div className="flex flex-col gap-8 max-w-lg">
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          Issuer Onboarding
        </div>
        <h1 className="text-2xl font-bold">Register as Credential Issuer</h1>
        <p className="mt-2 text-sm text-gray-400">
          Creates an{" "}
          <code className="text-purple-300 bg-gray-800 px-1 rounded">
            IssuerRegistry
          </code>{" "}
          PDA. A platform admin must verify you before you can issue credentials.
        </p>
      </div>

      <WalletMultiButton className="!bg-purple-700 hover:!bg-purple-600 !rounded-lg !text-sm !h-10 self-start" />

      <div className="rounded-xl border border-gray-800 bg-gray-900 p-6 flex flex-col gap-5">
        <div className="flex flex-col gap-1.5">
          <label className="text-xs text-gray-400 uppercase tracking-wide">
            Company / Organisation name
          </label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="EPAM Systems"
            maxLength={64}
            className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
          />
          <span className="text-xs text-gray-600">{name.length}/64</span>
        </div>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs text-gray-400 uppercase tracking-wide">
            Website URL
          </label>
          <input
            value={website}
            onChange={(e) => setWebsite(e.target.value)}
            placeholder="https://epam.com"
            maxLength={128}
            className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
          />
        </div>

        <button
          onClick={handleRegister}
          disabled={!canSubmit}
          className="self-start rounded-lg bg-purple-600 hover:bg-purple-500 disabled:bg-gray-700 disabled:text-gray-500 disabled:cursor-not-allowed transition-colors px-5 py-2.5 text-sm font-medium"
        >
          {loading ? "Sending…" : "Register Issuer"}
        </button>

        {error && (
          <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
