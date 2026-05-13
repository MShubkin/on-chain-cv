"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { PublicKey, Transaction } from "@solana/web3.js";
import QRCode from "react-qr-code";
import {
  CredentialAccount,
  EndorsementAccount,
  IssuerRegistryAccount,
  buildEndorseCredentialIx,
  deserializeCredential,
  deserializeIssuerRegistry,
  fetchEndorsementsByCredential,
  isExpired,
  explorerUrl,
  checkAssetFrozen,
  arToHttp,
} from "@/lib/program";
import { CredentialMetadata, SkillEntry } from "@/lib/irys";

type VerifyStatus = "loading" | "valid" | "revoked" | "expired" | "issuer_unverified" | "tampered";

export default function CredentialPage() {
  const { connection } = useConnection();
  const { publicKey, sendTransaction } = useWallet();
  const { pda } = useParams<{ pda: string }>();

  const [credential, setCredential] = useState<CredentialAccount | null>(null);
  const [issuer, setIssuer] = useState<IssuerRegistryAccount | null>(null);
  const [metadata, setMetadata] = useState<CredentialMetadata | null>(null);
  const [status, setStatus] = useState<VerifyStatus>("loading");
  const [error, setError] = useState<string | null>(null);
  const [credentialUrl, setCredentialUrl] = useState("");
  const [endorsements, setEndorsements] = useState<Array<{ pda: PublicKey; endorsement: EndorsementAccount }>>([]);
  const [endorseLoading, setEndorseLoading] = useState(false);
  const [endorseError, setEndorseError] = useState<string | null>(null);

  useEffect(() => {
    setCredentialUrl(`${window.location.origin}/credential/${pda}`);
  }, [pda]);

  useEffect(() => {
    if (!pda) return;
    let credPubkey: PublicKey;
    try {
      credPubkey = new PublicKey(pda);
    } catch {
      setError("Invalid PDA address");
      setStatus("loading");
      return;
    }

    connection.getAccountInfo(credPubkey).then(async (info) => {
      if (!info) { setError("Credential not found on-chain"); return; }

      const cred = deserializeCredential(Buffer.from(info.data));
      setCredential(cred);

      const issuerInfo = await connection.getAccountInfo(cred.issuer);
      const iss = issuerInfo
        ? deserializeIssuerRegistry(Buffer.from(issuerInfo.data))
        : null;
      setIssuer(iss);

      if (cred.revoked) { setStatus("revoked"); return; }
      if (isExpired(cred.expiresAt)) { setStatus("expired"); return; }
      if (!iss?.isVerified || iss.deactivatedAt !== null) { setStatus("issuer_unverified"); return; }

      // Parallel: check asset.frozen on-chain + fetch Arweave metadata for JSON↔PDA link
      const arweaveUri = arToHttp(cred.metadataUri);

      const [frozen, metaJson] = await Promise.all([
        checkAssetFrozen(connection, cred.coreAsset),
        fetch(arweaveUri)
          .then((r) => (r.ok ? r.json() : null))
          .catch(() => null),
      ]);

      if (metaJson) setMetadata(metaJson as CredentialMetadata);

      // Integrity checks: asset must still be frozen + metadata must link back to this PDA
      const pdaLinked =
        !metaJson ||
        (metaJson as CredentialMetadata)?.on_chain_ref?.credential_pda === pda;

      if (!frozen || !pdaLinked) {
        setStatus("tampered");
        return;
      }

      setStatus("valid");
    }).catch((e) => setError(e.message));
  }, [connection, pda]);

  useEffect(() => {
    if (!pda) return;
    let credPubkey: PublicKey;
    try { credPubkey = new PublicKey(pda); } catch { return; }
    fetchEndorsementsByCredential(connection, credPubkey)
      .then(setEndorsements)
      .catch(() => {});
  }, [connection, pda]);

  const handleEndorse = async () => {
    if (!publicKey || !pda) return;
    setEndorseLoading(true);
    setEndorseError(null);
    try {
      const credPubkey = new PublicKey(pda);
      const ix = buildEndorseCredentialIx({ endorser: publicKey, credentialPda: credPubkey });
      const tx = new Transaction().add(ix);
      const sig = await sendTransaction(tx, connection);
      await connection.confirmTransaction(sig, "confirmed");
      const updated = await fetchEndorsementsByCredential(connection, credPubkey);
      setEndorsements(updated);
    } catch (e) {
      setEndorseError(e instanceof Error ? e.message : String(e));
    } finally {
      setEndorseLoading(false);
    }
  };

  if (error) return <p className="text-sm text-red-400">{error}</p>;
  if (!credential && status === "loading") return <p className="text-sm text-gray-500">Loading…</p>;
  if (!credential) return null;

  const statusBadge = {
    loading: null,
    valid: (
      <div className="flex items-center gap-2 text-green-400">
        <span>✅</span>
        <span className="font-medium">Verified credential</span>
      </div>
    ),
    revoked: (
      <div className="flex items-center gap-2 text-red-400">
        <span>❌</span>
        <span className="font-medium">
          Revoked{" "}
          {credential.revokedAt
            ? `on ${new Date(Number(credential.revokedAt) * 1000).toLocaleDateString()}`
            : ""}
        </span>
      </div>
    ),
    expired: (
      <div className="flex items-center gap-2 text-yellow-500">
        <span>⏰</span>
        <span className="font-medium">
          Expired on{" "}
          {credential.expiresAt
            ? new Date(Number(credential.expiresAt) * 1000).toLocaleDateString()
            : "—"}
        </span>
      </div>
    ),
    issuer_unverified: (
      <div className="flex items-center gap-2 text-yellow-500">
        <span>⚠️</span>
        <span className="font-medium">Issuer not currently verified</span>
      </div>
    ),
    tampered: (
      <div className="flex items-center gap-2 text-red-400">
        <span>🚨</span>
        <span className="font-medium">Credential integrity compromised</span>
      </div>
    ),
  }[status];

  const displayName =
    metadata?.name ??
    `${credential.skill} Credential (Level ${credential.level})`;

  return (
    <div className="flex flex-col gap-6 max-w-lg">
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          Credential
        </div>
        <h1 className="text-2xl font-bold">{displayName}</h1>
        {issuer && (
          <p className="mt-1 text-sm text-gray-400">
            Issued by{" "}
            <a
              href={`/issuer/${credential.issuer.toBase58()}`}
              className="text-purple-400 hover:text-purple-300"
            >
              {issuer.name}
            </a>
          </p>
        )}
      </div>

      <div className="rounded-xl border border-gray-800 bg-gray-900 p-5 flex flex-col gap-4">
        {statusBadge}

        {metadata?.image && (
          <div className="flex justify-center pt-1">
            <img
              src={arToHttp(metadata.image)}
              alt={displayName}
              className="w-24 h-24 rounded-2xl object-cover border border-gray-700"
            />
          </div>
        )}

        <div className="grid grid-cols-2 gap-3 text-sm pt-2">
          <div>
            <p className="text-xs text-gray-500 mb-0.5">Skill category</p>
            <p>{credential.skill}</p>
          </div>
          <div>
            <p className="text-xs text-gray-500 mb-0.5">Level</p>
            <p>{credential.level} / 5</p>
          </div>
          <div>
            <p className="text-xs text-gray-500 mb-0.5">Issued</p>
            <p>{new Date(Number(credential.issuedAt) * 1000).toLocaleDateString()}</p>
          </div>
          {credential.expiresAt && (
            <div>
              <p className="text-xs text-gray-500 mb-0.5">Expires</p>
              <p>{new Date(Number(credential.expiresAt) * 1000).toLocaleDateString()}</p>
            </div>
          )}
          {metadata?.period && (
            <div className="col-span-2">
              <p className="text-xs text-gray-500 mb-0.5">Period</p>
              <p>
                {metadata.period.from}
                {metadata.period.to ? ` → ${metadata.period.to}` : " → present"}
              </p>
            </div>
          )}
          {metadata?.skills && metadata.skills.length > 0 && (
            <div className="col-span-2">
              <p className="text-xs text-gray-500 mb-1">Skills</p>
              <div className="flex flex-wrap gap-1.5">
                {(metadata.skills as (SkillEntry | string)[]).map((raw) => {
                  const s = typeof raw === "string" ? { name: raw } : raw;
                  return s.url ? (
                    <a
                      key={s.name}
                      href={s.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="rounded-full bg-gray-800 border border-purple-700 px-2 py-0.5 text-xs text-purple-400 hover:text-purple-300 hover:border-purple-500 transition-colors"
                    >
                      {s.name} ↗
                    </a>
                  ) : (
                    <span
                      key={s.name}
                      className="rounded-full bg-gray-800 border border-gray-700 px-2 py-0.5 text-xs text-gray-300"
                    >
                      {s.name}
                    </span>
                  );
                })}
              </div>
            </div>
          )}
          <div className="col-span-2">
            <p className="text-xs text-gray-500 mb-0.5">Endorsements</p>
            <p>{endorsements.length}</p>
          </div>
        </div>

        <div className="pt-3 border-t border-gray-800 flex flex-col gap-2">
          <p className="text-xs text-gray-500">MPL-Core Asset</p>
          <code className="text-xs text-gray-300 bg-gray-800 rounded px-2 py-1 break-all">
            {credential.coreAsset.toBase58()}
          </code>
          <p className="text-xs text-gray-500 mt-2">Recipient</p>
          <a
            href={`/profile/${credential.recipient.toBase58()}`}
            className="text-sm text-purple-400 hover:text-purple-300"
          >
            View profile →
          </a>
        </div>
      </div>

      <div className="rounded-xl border border-gray-800 bg-gray-900 p-6 flex flex-col items-center gap-4">
        <p className="text-xs text-gray-500 uppercase tracking-widest">Scan to verify</p>
        {credentialUrl && (
          <div className="bg-white p-3 rounded-lg">
            <QRCode value={credentialUrl} size={160} />
          </div>
        )}
        <p className="text-xs text-gray-600 text-center break-all max-w-xs">
          {credentialUrl}
        </p>
        <button
          onClick={() => navigator.clipboard.writeText(credentialUrl)}
          className="text-xs text-purple-400 hover:text-purple-300 transition-colors"
        >
          Copy link
        </button>
      </div>

      {/* Endorsements */}
      <div className="rounded-xl border border-gray-800 bg-gray-900 p-5 flex flex-col gap-4">
        <p className="text-xs text-gray-500 uppercase tracking-widest">Endorsements</p>
        {endorsements.length === 0 ? (
          <p className="text-sm text-gray-500">No endorsements yet.</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {endorsements.map(({ endorsement }) => {
              const addr = endorsement.endorser.toBase58();
              return (
                <span
                  key={addr}
                  className="rounded-full bg-gray-800 border border-gray-700 px-2 py-0.5 text-xs text-gray-300 font-mono"
                  title={addr}
                >
                  {addr.slice(0, 6)}…{addr.slice(-4)}
                </span>
              );
            })}
          </div>
        )}

        {/* Endorse button — shown to any connected wallet that is not the recipient */}
        {(() => {
          if (!publicKey) return null;
          const isRecipient = credential && publicKey.toBase58() === credential.recipient.toBase58();
          const alreadyEndorsed = endorsements.some(
            (e) => e.endorsement.endorser.toBase58() === publicKey.toBase58()
          );
          if (isRecipient || alreadyEndorsed) return null;
          return (
            <div className="pt-2 border-t border-gray-800 flex flex-col gap-2">
              <button
                onClick={handleEndorse}
                disabled={endorseLoading}
                className="text-sm text-purple-400 hover:text-purple-300 transition-colors disabled:opacity-50 text-left"
              >
                {endorseLoading ? "Endorsing…" : "Endorse this skill →"}
              </button>
              {endorseError && (
                <p className="text-xs text-red-400">{endorseError}</p>
              )}
            </div>
          );
        })()}
      </div>

      <a
        href={explorerUrl(new PublicKey(pda))}
        target="_blank"
        rel="noopener noreferrer"
        className="text-sm text-purple-400 hover:text-purple-300"
      >
        View on Solana Explorer →
      </a>
    </div>
  );
}
