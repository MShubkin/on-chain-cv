"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import dynamic from "next/dynamic";
import { Keypair, Transaction } from "@solana/web3.js";
import { useCallback, useEffect, useState } from "react";
import { PublicKey } from "@solana/web3.js";
import {
  CredentialAccount,
  IssuerRegistryAccount,
  SkillCategory,
  buildCloseCredentialIx,
  buildIssueCredentialIx,
  buildRevokeCredentialIx,
  deserializeIssuerRegistry,
  fetchCredentialsByIssuer,
  fetchCollectionUri,
  getCredentialPda,
  getIssuerRegistryPda,
  isExpired,
  explorerUrl,
  arToHttp,
  PROGRAM_ID,
} from "@/lib/program";
import { createIrysUploader, buildCredentialMetadataJson, SkillEntry } from "@/lib/irys";

const WalletMultiButton = dynamic(
  () =>
    import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

const SKILL_OPTIONS: SkillCategory[] = [
  "Work",
  "Education",
  "Certificate",
  "Achievement",
];

interface CredentialRow {
  pda: PublicKey;
  credential: CredentialAccount;
}

export default function DashboardPage() {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { publicKey, sendTransaction } = wallet;

  const [issuer, setIssuer] = useState<IssuerRegistryAccount | null>(null);
  const [issuerPda, setIssuerPda] = useState<PublicKey | null>(null);
  const [issuerLoading, setIssuerLoading] = useState(false);
  const [issuerError, setIssuerError] = useState<string | null>(null);

  // Form state
  const [recipient, setRecipient] = useState("");
  const [skill, setSkill] = useState<SkillCategory>("Work");
  const [level, setLevel] = useState("4");
  const [credentialName, setCredentialName] = useState("");
  const [skillEntries, setSkillEntries] = useState<{ name: string; url: string }[]>([{ name: "", url: "" }]);
  const [periodFrom, setPeriodFrom] = useState("");
  const [periodTo, setPeriodTo] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [imageFile, setImageFile] = useState<File | null>(null);
  const [collectionImageUri, setCollectionImageUri] = useState<string | null>(null);
  const [submitLoading, setSubmitLoading] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [lastCredentialPda, setLastCredentialPda] = useState<string | null>(null);

  // Issued credentials list
  const [issuedRows, setIssuedRows] = useState<CredentialRow[]>([]);
  const [issuedLoading, setIssuedLoading] = useState(false);
  const [revokingPda, setRevokingPda] = useState<string | null>(null);
  const [revokeError, setRevokeError] = useState<string | null>(null);
  const [closingPda, setClosingPda] = useState<string | null>(null);
  const [closeError, setCloseError] = useState<string | null>(null);

  const loadIssuer = useCallback(() => {
    if (!publicKey) return;
    const [pda] = getIssuerRegistryPda(publicKey);
    setIssuerPda(pda);
    setIssuerLoading(true);
    setIssuerError(null);
    connection
      .getAccountInfo(pda)
      .then(async (info) => {
        if (!info) {
          setIssuerError("not_registered");
          return;
        }
        const reg = deserializeIssuerRegistry(Buffer.from(info.data));
        setIssuer(reg);
        if (reg.collection) {
          const uri = await fetchCollectionUri(connection, reg.collection);
          if (uri) {
            fetch(arToHttp(uri))
              .then((r) => r.ok ? r.json() : null)
              .then((json) => {
                const imageUri = json?.image as string | undefined;
                if (imageUri) setCollectionImageUri(imageUri);
              })
              .catch(() => {});
          }
        }
      })
      .catch((e) => setIssuerError(e.message))
      .finally(() => setIssuerLoading(false));
  }, [publicKey, connection]);

  const loadIssuedCredentials = useCallback(() => {
    if (!issuerPda) return;
    setIssuedLoading(true);
    fetchCredentialsByIssuer(connection, issuerPda)
      .then((results) => {
        const sorted = results.sort((a, b) =>
          Number(b.credential.issuedAt - a.credential.issuedAt)
        );
        setIssuedRows(sorted);
      })
      .catch((e) => setRevokeError(e instanceof Error ? e.message : String(e)))
      .finally(() => setIssuedLoading(false));
  }, [connection, issuerPda]);

  useEffect(() => {
    loadIssuer();
  }, [loadIssuer]);

  useEffect(() => {
    loadIssuedCredentials();
  }, [loadIssuedCredentials]);

  // Выдаёт credential держателю. Два варианта метаданных:
  // A) manualUri заполнен → URI используется напрямую, Irys не нужен (localnet/тесты).
  // B) поле пустое → загружает JSON на Irys devnet и получает ar:// URI.
  // В обоих случаях создаётся Credential PDA и soulbound MPL-Core Asset в одной транзакции.
  const handleIssue = async () => {
    if (!publicKey || !issuer || !issuerPda) return;
    setSubmitLoading(true);
    setSubmitError(null);
    setLastCredentialPda(null);

    try {
      let recipientPubkey: PublicKey;
      try {
        recipientPubkey = new PublicKey(recipient.trim());
      } catch {
        throw new Error("Invalid recipient address");
      }

      const lvl = parseInt(level, 10);
      if (lvl < 1 || lvl > 5) throw new Error("Level must be between 1 and 5");

      // Перечитываем счётчик из сети — локальный issuer мог устареть при параллельных выдачах.
      const freshInfo = await connection.getAccountInfo(issuerPda);
      if (!freshInfo) throw new Error("Issuer registry not found");
      const freshIssuer = deserializeIssuerRegistry(Buffer.from(freshInfo.data));
      const currentIndex = freshIssuer.credentialsIssued;

      // Generate ephemeral asset keypair — signed once, then discarded
      const assetKeypair = Keypair.generate();

      const [credPda] = getCredentialPda(issuerPda, recipientPubkey, currentIndex);
      const expiresAtSec = expiresAt
        ? BigInt(Math.floor(new Date(expiresAt).getTime() / 1000))
        : null;

      if (!wallet.wallet) throw new Error("No wallet connected");
      const irys = await createIrysUploader(wallet.wallet.adapter, "devnet");

      // If issuer chose a custom badge — upload it; otherwise reuse the collection logo.
      let resolvedImageUri = collectionImageUri ?? "ar://placeholder";
      if (imageFile) resolvedImageUri = await irys.uploadFile(imageFile);

      const metadata = buildCredentialMetadataJson({
        credentialName: credentialName || `${skill} Level ${level}`,
        issuerName: freshIssuer.name,
        issuerPda: issuerPda.toBase58(),
        issuerCollection: freshIssuer.collection?.toBase58() ?? null,
        recipientPubkey: recipientPubkey.toBase58(),
        periodFrom: periodFrom || null,
        periodTo: periodTo || null,
        skills: skillEntries
          .filter((e) => e.name.trim())
          .map((e): SkillEntry =>
            e.url.trim() ? { name: e.name.trim(), url: e.url.trim() } : { name: e.name.trim() }
          ),
        level: lvl,
        expiresAt: expiresAtSec !== null ? Number(expiresAtSec) : null,
        credentialPda: credPda.toBase58(),
        coreAsset: assetKeypair.publicKey.toBase58(),
        imageUri: resolvedImageUri,
        programId: PROGRAM_ID.toBase58(),
      });

      const metadataUri = await irys.uploadJson(metadata);

      if (!freshIssuer.collection) throw new Error("Issuer has no collection — run verify first");

      const ix = buildIssueCredentialIx({
        issuerAuthority: publicKey,
        payer: publicKey,
        recipient: recipientPubkey,
        assetPubkey: assetKeypair.publicKey,
        issuerRegistryPda: issuerPda,
        issuerCollectionPubkey: freshIssuer.collection,
        credentialPda: credPda,
        skill,
        level: lvl,
        name: credentialName || `${skill} Level ${lvl}`,
        expiresAt: expiresAtSec,
        metadataUri,
      });

      const tx = new Transaction().add(ix);
      const sig = await sendTransaction(tx, connection, {
        signers: [assetKeypair],
      });
      await connection.confirmTransaction(sig, "confirmed");

      setLastCredentialPda(credPda.toBase58());
      loadIssuer(); // refresh counter
      loadIssuedCredentials();
    } catch (e) {
      setSubmitError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitLoading(false);
    }
  };

  // Отзывает credential: сжигает MPL-Core Asset и помечает Credential.revoked = true.
  // Asset исчезает из кошелька держателя сразу после подтверждения.
  const handleRevoke = async (row: CredentialRow) => {
    if (!publicKey || !issuerPda || !issuer?.collection) return;
    setRevokingPda(row.pda.toBase58());
    setRevokeError(null);
    try {
      const ix = buildRevokeCredentialIx({
        issuerAuthority: publicKey,
        payer: publicKey,
        issuerRegistryPda: issuerPda,
        credentialPda: row.pda,
        assetPubkey: row.credential.coreAsset,
        issuerCollectionPubkey: issuer.collection,
      });
      const tx = new Transaction().add(ix);
      const sig = await sendTransaction(tx, connection);
      await connection.confirmTransaction(sig, "confirmed");
      loadIssuedCredentials();
    } catch (e) {
      setRevokeError(e instanceof Error ? e.message : String(e));
    } finally {
      setRevokingPda(null);
    }
  };

  // Закрывает PDA отозванного Credential и возвращает ренту эмитенту.
  // Вызывается только когда isRevoked && endorsementCount === 0 — кнопка заблокирована иначе.
  const handleClose = async (pda: PublicKey) => {
    if (!publicKey || !issuerPda) return;
    setClosingPda(pda.toBase58());
    setCloseError(null);
    try {
      const ix = buildCloseCredentialIx({ issuerRegistryPda: issuerPda, issuerAuthority: publicKey, credentialPda: pda });
      const tx = new Transaction().add(ix);
      const sig = await sendTransaction(tx, connection);
      await connection.confirmTransaction(sig, "confirmed");
      loadIssuedCredentials();
    } catch (e) {
      setCloseError(e instanceof Error ? e.message : String(e));
    } finally {
      setClosingPda(null);
    }
  };

  const notVerified = issuer && (!issuer.isVerified || issuer.deactivatedAt !== null);

  if (!publicKey) {
    return (
      <div className="flex flex-col items-center justify-center gap-4 py-24">
        <p className="text-gray-400">Connect your wallet to access the issuer dashboard.</p>
        <WalletMultiButton className="!bg-purple-700 hover:!bg-purple-600 !rounded-lg !text-sm !h-10" />
      </div>
    );
  }

  if (issuerLoading) {
    return <p className="text-sm text-gray-500 py-24 text-center">Loading…</p>;
  }

  if (issuerError === "not_registered") {
    return (
      <div className="flex flex-col items-center justify-center gap-3 py-24">
        <p className="text-xl font-bold text-gray-300">Not registered as an issuer</p>
        <p className="text-sm text-gray-500">
          Register your organisation first to issue credentials.
        </p>
        <a
          href="/issuer/register"
          className="mt-2 rounded-lg bg-purple-700 hover:bg-purple-600 px-5 py-2 text-sm font-medium text-white"
        >
          Register as Issuer
        </a>
      </div>
    );
  }

  if (issuerError) {
    return <p className="text-sm text-red-400 py-24 text-center">{issuerError}</p>;
  }

  if (notVerified) {
    return (
      <div className="flex flex-col items-center justify-center gap-3 py-24">
        <p className="text-xl font-bold text-gray-300">
          {issuer!.deactivatedAt !== null ? "Account deactivated" : "Pending verification"}
        </p>
        <p className="text-sm text-gray-500">
          {issuer!.deactivatedAt !== null
            ? "Your issuer account has been deactivated. Contact the platform admin."
            : "Your issuer account is awaiting admin approval before you can issue credentials."}
        </p>
        <p className="text-xs font-mono text-gray-600">{issuer!.name}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-8">
      <div>
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
          Issuer
        </div>
        <h1 className="text-2xl font-bold">Issue Credential</h1>
      </div>

      <div className="rounded-xl border border-gray-800 bg-gray-900 p-4 flex items-center gap-4">
        <div className="flex flex-col gap-0.5 flex-1">
          <p className="font-medium">{issuer!.name}</p>
          <p className="text-xs text-gray-500">
            {issuer!.credentialsIssued.toString()} credentials issued
          </p>
        </div>
        <span className="text-xs text-green-400 bg-green-950 border border-green-800 rounded-full px-2 py-0.5">
          ✅ Verified
        </span>
      </div>

      {issuer && !notVerified && (
        <div className="rounded-xl border border-gray-800 bg-gray-900 p-6 flex flex-col gap-5">
          <h2 className="text-lg font-semibold">New Credential</h2>

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-1 sm:col-span-2">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Recipient wallet address *
              </label>
              <input
                value={recipient}
                onChange={(e) => setRecipient(e.target.value)}
                placeholder="Solana public key"
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm font-mono text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">Skill category</label>
              <select
                value={skill}
                onChange={(e) => setSkill(e.target.value as SkillCategory)}
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white focus:outline-none focus:border-purple-500"
              >
                {SKILL_OPTIONS.map((s) => (
                  <option key={s} value={s}>{s}</option>
                ))}
              </select>
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">Level (1–5)</label>
              <select
                value={level}
                onChange={(e) => setLevel(e.target.value)}
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white focus:outline-none focus:border-purple-500"
              >
                {[1, 2, 3, 4, 5].map((n) => (
                  <option key={n} value={n}>{n}</option>
                ))}
              </select>
            </div>

            <div className="flex flex-col gap-1 sm:col-span-2">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Job title / credential name (for metadata)
              </label>
              <input
                value={credentialName}
                onChange={(e) => setCredentialName(e.target.value)}
                placeholder="e.g. Senior Rust Developer"
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
              />
            </div>

            <div className="flex flex-col gap-2 sm:col-span-2">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Skills
              </label>
              {skillEntries.map((entry, i) => (
                <div key={i} className="flex gap-2 items-center">
                  <input
                    value={entry.name}
                    onChange={(e) =>
                      setSkillEntries((prev) =>
                        prev.map((s, idx) => idx === i ? { ...s, name: e.target.value } : s)
                      )
                    }
                    placeholder="e.g. Rust"
                    className="flex-1 rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
                  />
                  <input
                    value={entry.url}
                    onChange={(e) =>
                      setSkillEntries((prev) =>
                        prev.map((s, idx) => idx === i ? { ...s, url: e.target.value } : s)
                      )
                    }
                    placeholder="Certificate URL (optional)"
                    className="flex-1 rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
                  />
                  {skillEntries.length > 1 && (
                    <button
                      type="button"
                      onClick={() =>
                        setSkillEntries((prev) => prev.filter((_, idx) => idx !== i))
                      }
                      className="text-gray-500 hover:text-red-400 text-lg leading-none px-1"
                    >
                      ✕
                    </button>
                  )}
                </div>
              ))}
              <button
                type="button"
                onClick={() =>
                  setSkillEntries((prev) => [...prev, { name: "", url: "" }])
                }
                className="self-start text-xs text-purple-400 hover:text-purple-300"
              >
                + Add skill
              </button>
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Period from (YYYY-MM)
              </label>
              <input
                value={periodFrom}
                onChange={(e) => setPeriodFrom(e.target.value)}
                placeholder="2022-01"
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Period to (YYYY-MM, blank = present)
              </label>
              <input
                value={periodTo}
                onChange={(e) => setPeriodTo(e.target.value)}
                placeholder="2024-06"
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-purple-500"
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Expires at (optional)
              </label>
              <input
                type="date"
                value={expiresAt}
                onChange={(e) => setExpiresAt(e.target.value)}
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white focus:outline-none focus:border-purple-500"
              />
            </div>

            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                Badge image (optional)
              </label>
              <input
                type="file"
                accept="image/*"
                onChange={(e) => setImageFile(e.target.files?.[0] ?? null)}
                className="text-sm text-gray-400"
              />
              <p className="text-xs text-gray-600">
                {imageFile
                  ? `Will upload: ${imageFile.name}`
                  : collectionImageUri
                  ? "No file — issuer logo will be used"
                  : "No file — placeholder will be used"}
              </p>
            </div>

          </div>

          <button
            onClick={handleIssue}
            disabled={submitLoading || !recipient}
            className="self-start rounded-lg bg-purple-600 hover:bg-purple-500 disabled:bg-gray-700 disabled:text-gray-500 disabled:cursor-not-allowed px-5 py-2.5 text-sm font-medium"
          >
            {submitLoading ? "Uploading + signing…" : "Issue Credential"}
          </button>

          {submitError && (
            <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
              {submitError}
            </div>
          )}

          {lastCredentialPda && (
            <div className="flex flex-col gap-2 rounded-xl border border-green-900 bg-gray-900 p-4">
              <span className="text-green-400 text-sm font-medium">
                ✅ Credential issued
              </span>
              <a
                href={`/credential/${lastCredentialPda}`}
                className="text-sm text-purple-400 hover:text-purple-300"
              >
                View credential →
              </a>
              <a
                href={explorerUrl(new PublicKey(lastCredentialPda))}
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-purple-400 hover:text-purple-300"
              >
                View on Explorer →
              </a>
            </div>
          )}
        </div>
      )}

      {/* ── Issued Credentials list ─────────────────────────────────────── */}
      {issuer && issuerPda && (
        <div className="flex flex-col gap-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold">Issued Credentials</h2>
            <button
              onClick={loadIssuedCredentials}
              className="text-xs text-gray-500 hover:text-gray-300 transition-colors"
            >
              Refresh
            </button>
          </div>

          {revokeError && (
            <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
              {revokeError}
            </div>
          )}

          {closeError && (
            <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
              {closeError}
            </div>
          )}

          {issuedLoading && (
            <p className="text-sm text-gray-500">Loading…</p>
          )}

          {!issuedLoading && issuedRows.length === 0 && (
            <p className="text-sm text-gray-500">No credentials issued yet.</p>
          )}

          {!issuedLoading && issuedRows.length > 0 && (
            <div className="flex flex-col gap-2">
              {issuedRows.map(({ pda, credential }) => {
                const pdaStr = pda.toBase58();
                const isRevoked = credential.revoked;
                const isExp = isExpired(credential.expiresAt);
                const isRevoking = revokingPda === pdaStr;
                // Expired-but-not-revoked credentials can still be revoked on-chain;
                // allow the issuer to set an explicit audit trail even after expiry.
                const canRevoke = !isRevoked;

                return (
                  <div
                    key={pdaStr}
                    className="rounded-xl border border-gray-800 bg-gray-900 p-4 flex items-center gap-4"
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <p className="text-sm font-medium">
                          {credential.skill}{" "}
                          <span className="text-gray-500">Level {credential.level}</span>
                        </p>
                        {isRevoked ? (
                          <span className="text-xs text-red-400 bg-red-950 border border-red-800 rounded-full px-2 py-0.5">
                            Revoked{credential.revokedAt
                              ? ` ${new Date(Number(credential.revokedAt) * 1000).toLocaleDateString()}`
                              : ""}
                          </span>
                        ) : isExp ? (
                          <span className="text-xs text-yellow-500 bg-yellow-950 border border-yellow-800 rounded-full px-2 py-0.5">
                            Expired
                          </span>
                        ) : (
                          <span className="text-xs text-green-400 bg-green-950 border border-green-800 rounded-full px-2 py-0.5">
                            Active
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-gray-500 font-mono mt-0.5 truncate">
                        → {credential.recipient.toBase58().slice(0, 16)}…
                      </p>
                      <p className="text-xs text-gray-600 mt-0.5">
                        Issued {new Date(Number(credential.issuedAt) * 1000).toLocaleDateString()}
                      </p>
                    </div>

                    <div className="flex items-center gap-3 flex-shrink-0">
                      <a
                        href={`/credential/${pdaStr}`}
                        className="text-xs text-purple-400 hover:text-purple-300 transition-colors"
                      >
                        View
                      </a>
                      {canRevoke && (
                        <button
                          onClick={() => handleRevoke({ pda, credential })}
                          disabled={isRevoking}
                          className="text-xs text-red-400 hover:text-red-300 disabled:text-gray-600 disabled:cursor-not-allowed transition-colors border border-red-800 hover:border-red-600 disabled:border-gray-700 rounded-full px-3 py-1"
                        >
                          {isRevoking ? "Revoking…" : "Revoke"}
                        </button>
                      )}
                      {isRevoked && (
                        <button
                          onClick={() => handleClose(pda)}
                          disabled={!!closingPda || credential.endorsementCount > 0}
                          title={
                            credential.endorsementCount > 0
                              ? `${credential.endorsementCount} endorsement(s) — close them first`
                              : undefined
                          }
                          className="text-xs text-gray-400 hover:text-gray-200 disabled:text-gray-600 disabled:cursor-not-allowed transition-colors border border-gray-700 hover:border-gray-500 disabled:border-gray-700 rounded-full px-3 py-1"
                        >
                          {closingPda === pdaStr
                            ? "Closing…"
                            : credential.endorsementCount > 0
                            ? `Blocked (${credential.endorsementCount})`
                            : "Close"}
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
