"use client";

import { useConnection } from "@solana/wallet-adapter-react";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import {
  CredentialAccount,
  IssuerRegistryAccount,
  SkillCategory,
  deserializeIssuerRegistry,
  fetchCollectionUri,
  fetchCredentialsByRecipient,
  isExpired,
  arToHttp,
} from "@/lib/program";
import { CredentialMetadata } from "@/lib/irys";

const SKILL_COLOR: Record<SkillCategory, string> = {
  Work: "bg-blue-950 border-blue-800 text-blue-300",
  Education: "bg-purple-950 border-purple-800 text-purple-300",
  Certificate: "bg-green-950 border-green-800 text-green-300",
  Achievement: "bg-yellow-950 border-yellow-800 text-yellow-300",
};

interface CredentialRow {
  pda: PublicKey;
  credential: CredentialAccount;
  issuer: IssuerRegistryAccount | null;
}

export default function ProfilePage() {
  const { connection } = useConnection();
  const { pubkey } = useParams<{ pubkey: string }>();

  const [rows, setRows] = useState<CredentialRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showRevoked, setShowRevoked] = useState(false);

  // Progressive: issuer logos keyed by issuer PDA string
  const [logoUrls, setLogoUrls] = useState<Map<string, string | null>>(new Map());
  // Progressive: aggregated skills from Arweave metadata
  const [skills, setSkills] = useState<string[]>([]);
  // Progressive: per-credential Arweave metadata keyed by credential PDA string
  const [credMeta, setCredMeta] = useState<Map<string, CredentialMetadata | null>>(new Map());

  // Fetch on-chain data
  useEffect(() => {
    if (!pubkey) return;
    let walletPubkey: PublicKey;
    try {
      walletPubkey = new PublicKey(pubkey);
    } catch {
      setError("Invalid wallet address");
      setLoading(false);
      return;
    }

    fetchCredentialsByRecipient(connection, walletPubkey)
      .then(async (results) => {
        const enriched = await Promise.all(
          results.map(async ({ pda, credential }) => {
            let issuer: IssuerRegistryAccount | null = null;
            try {
              const info = await connection.getAccountInfo(credential.issuer);
              if (info) issuer = deserializeIssuerRegistry(Buffer.from(info.data));
            } catch {}
            return { pda, credential, issuer };
          })
        );
        enriched.sort((a, b) =>
          Number(b.credential.issuedAt - a.credential.issuedAt)
        );
        setRows(enriched);
        return enriched;
      })
      .then((enriched) => {
        // Progressive: load issuer logos
        const seen = new Set<string>();
        enriched.forEach(({ credential, issuer }) => {
          if (!issuer?.collection) return;
          const key = credential.issuer.toBase58();
          if (seen.has(key)) return;
          seen.add(key);
          fetchCollectionUri(connection, issuer.collection)
            .then(async (uri) => {
              if (!uri) return;
              const res = await fetch(arToHttp(uri));
              if (!res.ok) return;
              const json = await res.json();
              const imageUri = (json.image as string | undefined);
              setLogoUrls((prev) =>
                new Map(prev).set(key, imageUri ? arToHttp(imageUri) : null)
              );
            })
            .catch(() =>
              setLogoUrls((prev) => new Map(prev).set(key, null))
            );
        });

        // Progressive: fetch Arweave metadata per credential for job title, period, and skills
        enriched.forEach(({ pda, credential }) => {
          const key = pda.toBase58();
          fetch(arToHttp(credential.metadataUri))
            .then((r) => (r.ok ? r.json() : null))
            .then((json) => {
              if (!json) return;
              const meta = json as CredentialMetadata;
              setCredMeta((prev) => new Map(prev).set(key, meta));
              if (!credential.revoked && !isExpired(credential.expiresAt)) {
                setSkills((prev) => {
                  const updated = new Set(prev);
                  (meta.skills ?? []).forEach((s) => updated.add(s));
                  return [...updated].sort();
                });
              }
            })
            .catch(() => {});
        });
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [connection, pubkey]);

  const visibleRows = rows.filter((r) => {
    if (showRevoked) return true;
    return !r.credential.revoked && !isExpired(r.credential.expiresAt);
  });

  const hiddenCount = rows.filter(
    (r) => r.credential.revoked || isExpired(r.credential.expiresAt)
  ).length;

  return (
    <div className="flex flex-col gap-8">
      {/* Header */}
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          Portfolio
        </div>
        <h1 className="text-2xl font-bold">
          {pubkey.slice(0, 8)}…{pubkey.slice(-4)}
        </h1>
        <div className="flex items-center gap-2 mt-1">
          <p className="text-sm text-gray-500 font-mono truncate max-w-xs">{pubkey}</p>
          <button
            onClick={() => navigator.clipboard.writeText(pubkey)}
            className="text-xs text-gray-600 hover:text-gray-400 transition-colors flex-shrink-0"
          >
            Copy
          </button>
        </div>
      </div>

      {/* Skill cloud — appears when Arweave metadata loads */}
      {skills.length > 0 && (
        <div>
          <p className="text-xs text-gray-500 uppercase tracking-widest mb-3">Skills</p>
          <div className="flex flex-wrap gap-2">
            {skills.map((skill) => (
              <span
                key={skill}
                className="rounded-full bg-gray-800 border border-gray-700 px-3 py-1 text-sm text-gray-300"
              >
                {skill}
              </span>
            ))}
          </div>
        </div>
      )}

      {loading && <p className="text-sm text-gray-500">Loading credentials…</p>}
      {error && <p className="text-sm text-red-400">{error}</p>}

      {!loading && !error && (
        <>
          <div className="flex items-center justify-between">
            <p className="text-sm text-gray-400">
              {visibleRows.length} credential{visibleRows.length !== 1 ? "s" : ""}
              {!showRevoked && hiddenCount > 0
                ? ` (${hiddenCount} hidden)`
                : ""}
            </p>
            {hiddenCount > 0 && (
              <button
                onClick={() => setShowRevoked(!showRevoked)}
                className="text-xs text-gray-500 hover:text-gray-300 transition-colors"
              >
                {showRevoked ? "Hide revoked" : "Show revoked"}
              </button>
            )}
          </div>

          {visibleRows.length === 0 && (
            <p className="text-sm text-gray-500">No credentials found.</p>
          )}

          {/* Timeline */}
          <div className="relative flex flex-col gap-0">
            {/* Vertical line */}
            <div className="absolute left-3.5 top-4 bottom-4 w-px bg-gray-800" />

            {visibleRows.map(({ pda, credential, issuer }, idx) => {
              const issuerKey = credential.issuer.toBase58();
              const logoUrl = logoUrls.get(issuerKey);
              const meta = credMeta.get(pda.toBase58());
              const cardTitle = meta?.name ?? `${credential.skill} Level ${credential.level}`;
              const cardPeriod = meta?.period
                ? `${meta.period.from}${meta.period.to ? ` → ${meta.period.to}` : " → present"}`
                : new Date(Number(credential.issuedAt) * 1000).toLocaleDateString("en", { year: "numeric", month: "short" }) +
                  (credential.expiresAt
                    ? ` → ${new Date(Number(credential.expiresAt) * 1000).toLocaleDateString("en", { year: "numeric", month: "short" })}`
                    : "");
              const isLast = idx === visibleRows.length - 1;
              const isRevoked = credential.revoked;
              const isExp = isExpired(credential.expiresAt);
              const isValid = issuer?.isVerified && issuer.deactivatedAt === null;

              return (
                <div key={pda.toBase58()} className={`flex gap-4 ${isLast ? "pb-0" : "pb-6"}`}>
                  {/* Timeline dot */}
                  <div className="flex-shrink-0 w-7 flex flex-col items-center">
                    <div
                      className={`w-3.5 h-3.5 rounded-full border-2 mt-1 z-10 ${
                        isRevoked
                          ? "border-red-700 bg-gray-950"
                          : isExp
                          ? "border-yellow-700 bg-gray-950"
                          : isValid
                          ? "border-green-600 bg-green-900"
                          : "border-gray-600 bg-gray-900"
                      }`}
                    />
                  </div>

                  {/* Card */}
                  <a
                    href={`/credential/${pda.toBase58()}`}
                    className="flex-1 rounded-xl border border-gray-800 bg-gray-900 p-4 hover:border-purple-700 transition-colors mb-1"
                  >
                    <div className="flex items-start gap-3">
                      {/* Issuer logo */}
                      <div className="flex-shrink-0 mt-0.5">
                        {logoUrl ? (
                          <img
                            src={logoUrl}
                            alt={issuer?.name ?? "Issuer"}
                            className="w-9 h-9 rounded-full object-cover border border-gray-700"
                          />
                        ) : (
                          <div className="w-9 h-9 rounded-full bg-purple-900 border border-purple-700 flex items-center justify-center text-sm font-bold text-purple-300">
                            {(issuer?.name ?? "?")[0].toUpperCase()}
                          </div>
                        )}
                      </div>

                      <div className="flex-1 min-w-0">
                        <div className="flex items-start justify-between gap-2">
                          <div>
                            <p className="font-medium text-sm leading-snug">
                              {cardTitle}
                            </p>
                            {issuer && (
                              <p className="text-xs text-gray-400 mt-0.5">{issuer.name}</p>
                            )}
                          </div>
                          <div className="flex-shrink-0">
                            {isRevoked ? (
                              <span className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-full px-2 py-0.5">
                                Revoked
                              </span>
                            ) : isExp ? (
                              <span className="text-xs text-yellow-500 bg-yellow-950 border border-yellow-800 rounded-full px-2 py-0.5">
                                Expired
                              </span>
                            ) : isValid ? (
                              <span className="text-xs text-green-400 bg-green-950 border border-green-800 rounded-full px-2 py-0.5">
                                ✅ Verified
                              </span>
                            ) : (
                              <span className="text-xs text-yellow-500 bg-yellow-950 border border-yellow-800 rounded-full px-2 py-0.5">
                                ⚠️ Issuer
                              </span>
                            )}
                          </div>
                        </div>

                        <div className="flex items-center gap-3 mt-2 text-xs text-gray-600">
                          <span
                            className={`rounded-full border px-2 py-0.5 ${
                              SKILL_COLOR[credential.skill]
                            }`}
                          >
                            {credential.skill}
                          </span>
                          <span className="rounded-full border border-gray-700 bg-gray-800 px-2 py-0.5 text-gray-400">
                            Level {credential.level}
                          </span>
                          <span>{cardPeriod}</span>
                          {credential.endorsementCount > 0 && (
                            <span>
                              {credential.endorsementCount} endorsement
                              {credential.endorsementCount !== 1 ? "s" : ""}
                            </span>
                          )}
                        </div>
                      </div>
                    </div>
                  </a>
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
