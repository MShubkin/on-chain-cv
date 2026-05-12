"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import dynamic from "next/dynamic";
import { Keypair, Transaction } from "@solana/web3.js";
import { useCallback, useEffect, useState } from "react";
import {
  buildInitializePlatformIx,
  buildTransferPlatformAuthorityIx,
  buildVerifyIssuerIx,
  buildDeactivateIssuerIx,
  fetchAllIssuers,
  IssuerRegistryAccount,
  getPlatformConfigPda,
  explorerUrl,
} from "@/lib/program";
import { PublicKey } from "@solana/web3.js";
import { createIrysUploader, buildCollectionMetadataJson } from "@/lib/irys";

const WalletMultiButton = dynamic(
  () =>
    import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

// "loading" нужен, чтобы не показывать ошибку пока идёт первый запрос к ноде.
type ChainStatus = "loading" | "not_initialized" | "initialized";

interface IssuerRow {
  pda: PublicKey;
  issuer: IssuerRegistryAccount;
}

export default function AdminPage() {
  const { connection } = useConnection();
  const wallet = useWallet();
  const { publicKey, sendTransaction } = wallet;

  // ── initialize section state ──────────────────────────────────────────────
  const [chainStatus, setChainStatus] = useState<ChainStatus>("loading");
  const [justInitialized, setJustInitialized] = useState(false);
  const [initLoading, setInitLoading] = useState(false);
  const [initError, setInitError] = useState<string | null>(null);

  // ── issuer section state ──────────────────────────────────────────────────
  const [issuers, setIssuers] = useState<IssuerRow[]>([]);
  const [issuersLoading, setIssuersLoading] = useState(false);
  const [verifyingPda, setVerifyingPda] = useState<string | null>(null);
  const [logoFile, setLogoFile] = useState<File | null>(null);
  const [collectionDescription, setCollectionDescription] = useState("");
  // Если заполнено — Irys не используется. Удобно на localnet: вписать ar://test
  const [manualCollectionUri, setManualCollectionUri] = useState("");

  // ── transfer authority section state ─────────────────────────────────────
  const [newAuthorityInput, setNewAuthorityInput] = useState("");
  const [transferLoading, setTransferLoading] = useState(false);
  const [transferError, setTransferError] = useState<string | null>(null);
  const [transferDone, setTransferDone] = useState(false);
  const [verifyLoading, setVerifyLoading] = useState(false);
  const [verifyError, setVerifyError] = useState<string | null>(null);
  const [deactivatingPda, setDeactivatingPda] = useState<string | null>(null);
  const [deactivateError, setDeactivateError] = useState<string | null>(null);

  const [pdaPubkey] = getPlatformConfigPda();

  useEffect(() => {
    connection.getAccountInfo(pdaPubkey).then((info) => {
      setChainStatus(info ? "initialized" : "not_initialized");
    });
  }, [connection, pdaPubkey]);

  const loadIssuers = useCallback(() => {
    setIssuersLoading(true);
    fetchAllIssuers(connection)
      .then(setIssuers)
      .finally(() => setIssuersLoading(false));
  }, [connection]);

  useEffect(() => {
    if (chainStatus === "initialized") loadIssuers();
  }, [chainStatus, loadIssuers]);

  const handleInitialize = async () => {
    if (!publicKey) return;
    setInitLoading(true);
    setInitError(null);
    try {
      const ix = buildInitializePlatformIx(publicKey);
      const tx = new Transaction().add(ix);
      const signature = await sendTransaction(tx, connection);
      await connection.confirmTransaction(signature, "confirmed");
      setJustInitialized(true);
      setChainStatus("initialized");
    } catch (e) {
      setInitError(e instanceof Error ? e.message : String(e));
    } finally {
      setInitLoading(false);
    }
  };

  // handleVerify — два варианта получения collectionUri:
  // A) manualCollectionUri заполнен → использует его напрямую, Irys не нужен (localnet/тесты)
  // B) поле пустое → загружает логотип + JSON на Irys devnet и получает ar:// URI
  // После этого отправляет транзакцию verify_issuer с collectionKeypair как доп. подписантом.
  const handleVerify = async (row: IssuerRow) => {
    if (!publicKey || !wallet.wallet) return;
    setVerifyLoading(true);
    setVerifyError(null);
    try {
      let collectionUri: string;

      if (manualCollectionUri.trim()) {
        // Прямой ввод URI — Irys пропускается полностью
        collectionUri = manualCollectionUri.trim();
      } else {
        const irys = await createIrysUploader(wallet.wallet.adapter, "devnet");

        let imageUri = "ar://placeholder";
        if (logoFile) imageUri = await irys.uploadFile(logoFile);

        const metadata = buildCollectionMetadataJson({
          issuerName: row.issuer.name,
          description:
            collectionDescription || `${row.issuer.name} verified credentials`,
          imageUri,
          externalUrl: row.issuer.website,
        });
        collectionUri = await irys.uploadJson(metadata);
      }

      // Keypair генерируется на лету — он нужен только чтобы подписать эту одну транзакцию.
      // После создания коллекции MPL-Core владеет аккаунтом по этому pubkey.
      const collectionKeypair = Keypair.generate();

      const ix = buildVerifyIssuerIx(
        publicKey,
        row.issuer.authority,
        collectionKeypair.publicKey,
        collectionUri
      );
      const tx = new Transaction().add(ix);
      const signature = await sendTransaction(tx, connection, {
        signers: [collectionKeypair],
      });
      await connection.confirmTransaction(signature, "confirmed");

      setVerifyingPda(null);
      setLogoFile(null);
      setCollectionDescription("");
      setManualCollectionUri("");
      loadIssuers();
    } catch (e) {
      setVerifyError(e instanceof Error ? e.message : String(e));
    } finally {
      setVerifyLoading(false);
    }
  };

  const handleDeactivate = async (row: IssuerRow) => {
    if (!publicKey) return;
    const key = row.pda.toBase58();
    setDeactivatingPda(key);
    setDeactivateError(null);
    try {
      const ix = buildDeactivateIssuerIx(publicKey, row.issuer.authority);
      const tx = new Transaction().add(ix);
      const signature = await sendTransaction(tx, connection);
      await connection.confirmTransaction(signature, "confirmed");
      loadIssuers();
    } catch (e) {
      setDeactivateError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeactivatingPda(null);
    }
  };

  // Передаёт права администратора на новый кошелёк.
  // После подтверждения текущий кошелёк теряет доступ к admin-инструкциям.
  // PublicKey.fromString бросает исключение при невалидном адресе — ловится в catch.
  const handleTransferAuthority = async () => {
    if (!publicKey) return;
    setTransferLoading(true);
    setTransferError(null);
    setTransferDone(false);
    try {
      const newAuthority = new PublicKey(newAuthorityInput.trim());
      const ix = buildTransferPlatformAuthorityIx({ authority: publicKey, newAuthority });
      const tx = new Transaction().add(ix);
      const sig = await sendTransaction(tx, connection);
      await connection.confirmTransaction(sig, "confirmed");
      setTransferDone(true);
      setNewAuthorityInput("");
    } catch (e) {
      setTransferError(e instanceof Error ? e.message : String(e));
    } finally {
      setTransferLoading(false);
    }
  };

  const initButtonLabel = initLoading
    ? "Sending…"
    : chainStatus === "loading"
    ? "Checking…"
    : chainStatus === "initialized"
    ? "Already initialized"
    : "Initialize Platform";

  const pendingIssuers = issuers.filter(
    (r) => !r.issuer.isVerified && r.issuer.deactivatedAt === null
  );
  const verifiedIssuers = issuers.filter(
    (r) => r.issuer.isVerified && r.issuer.deactivatedAt === null
  );

  return (
    <div className="flex flex-col gap-10">
      {/* ── Initialize section ─────────────────────────────────────────────── */}
      <section className="flex flex-col gap-4">
        <div>
          <div className="text-xs font-mono text-purple-400 uppercase tracking-widest mb-2">
            Platform Admin
          </div>
          <h1 className="text-2xl font-bold">OnChainCV Admin</h1>
        </div>
        <WalletMultiButton className="!bg-purple-700 hover:!bg-purple-600 !rounded-lg !text-sm !h-10 self-start" />
        <div className="rounded-xl border border-gray-800 bg-gray-900 p-6 flex flex-col gap-4">
          {justInitialized && (
            <div className="text-green-400 flex flex-col gap-1">
              <span>✅ Platform initialized</span>
              <a
                href={explorerUrl(pdaPubkey)}
                target="_blank"
                rel="noopener noreferrer"
                className="text-sm text-purple-400"
              >
                View on Explorer →
              </a>
            </div>
          )}
          {!justInitialized && chainStatus === "initialized" && (
            <span className="text-sm text-gray-400">
              ℹ️ PlatformConfig exists on-chain.
            </span>
          )}
          {!justInitialized && chainStatus === "not_initialized" && (
            <span className="text-sm text-yellow-500">
              ⚠️ Platform not yet initialized.
            </span>
          )}
          <button
            onClick={handleInitialize}
            disabled={
              !publicKey || initLoading || chainStatus !== "not_initialized"
            }
            className="self-start rounded-lg bg-purple-600 hover:bg-purple-500 disabled:bg-gray-700 disabled:text-gray-500 disabled:cursor-not-allowed px-5 py-2.5 text-sm font-medium"
          >
            {initButtonLabel}
          </button>
          {initError && (
            <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
              {initError}
            </div>
          )}
        </div>
      </section>

      {/* ── Pending issuers ─────────────────────────────────────────────────── */}
      {chainStatus === "initialized" && (
        <section className="flex flex-col gap-4">
          <h2 className="text-xl font-bold">Pending Verification</h2>
          {issuersLoading && (
            <p className="text-sm text-gray-500">Loading issuers…</p>
          )}
          {!issuersLoading && pendingIssuers.length === 0 && (
            <p className="text-sm text-gray-500">
              No issuers pending verification.
            </p>
          )}
          {pendingIssuers.map((row) => (
            <div
              key={row.pda.toBase58()}
              className="rounded-xl border border-gray-800 bg-gray-900 p-5 flex flex-col gap-4"
            >
              <div className="flex items-start justify-between">
                <div className="flex flex-col gap-0.5">
                  <p className="font-medium">{row.issuer.name}</p>
                  <p className="text-xs text-gray-500">{row.issuer.website}</p>
                  <code className="text-xs text-gray-700 font-mono mt-1">
                    {row.pda.toBase58()}
                  </code>
                </div>
                {verifyingPda !== row.pda.toBase58() && (
                  <button
                    onClick={() => setVerifyingPda(row.pda.toBase58())}
                    className="rounded-lg bg-green-700 hover:bg-green-600 px-4 py-2 text-sm font-medium"
                  >
                    Verify
                  </button>
                )}
              </div>

              {verifyingPda === row.pda.toBase58() && (
                <div className="flex flex-col gap-3 pt-3 border-t border-gray-700">
                  {/* Ручной ввод URI — обходит Irys, нужен на localnet */}
                  <div className="flex flex-col gap-1">
                    <label className="text-xs text-gray-500 uppercase tracking-wide">
                      Collection URI — localnet / testing
                    </label>
                    <input
                      value={manualCollectionUri}
                      onChange={(e) => setManualCollectionUri(e.target.value)}
                      placeholder="ar://... — вписать чтобы пропустить Irys"
                      className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-green-600 font-mono"
                    />
                  </div>

                  {/* Irys upload — только если URI не введён вручную */}
                  {!manualCollectionUri.trim() && (
                    <>
                      <p className="text-xs text-gray-400">
                        Или загрузить логотип и описание на Irys (devnet SOL):
                      </p>
                      <input
                        type="file"
                        accept="image/*"
                        onChange={(e) => setLogoFile(e.target.files?.[0] ?? null)}
                        className="text-sm text-gray-400"
                      />
                      <input
                        value={collectionDescription}
                        onChange={(e) => setCollectionDescription(e.target.value)}
                        placeholder={`Verified credentials issued by ${row.issuer.name}`}
                        className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm text-white placeholder-gray-600 focus:outline-none focus:border-green-600"
                      />
                    </>
                  )}

                  <div className="flex gap-3">
                    <button
                      onClick={() => handleVerify(row)}
                      disabled={verifyLoading || !publicKey}
                      className="rounded-lg bg-green-700 hover:bg-green-600 disabled:bg-gray-700 disabled:text-gray-500 disabled:cursor-not-allowed px-4 py-2 text-sm font-medium"
                    >
                      {verifyLoading
                        ? manualCollectionUri.trim() ? "Signing…" : "Uploading + signing…"
                        : "Confirm Verify"}
                    </button>
                    <button
                      onClick={() => {
                        setVerifyingPda(null);
                        setVerifyError(null);
                      }}
                      className="rounded-lg bg-gray-700 hover:bg-gray-600 px-4 py-2 text-sm"
                    >
                      Cancel
                    </button>
                  </div>
                  {verifyError && (
                    <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
                      {verifyError}
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}
        </section>
      )}

      {/* ── Verified issuers ─────────────────────────────────────────────────── */}
      {chainStatus === "initialized" && verifiedIssuers.length > 0 && (
        <section className="flex flex-col gap-4">
          <h2 className="text-xl font-bold">Verified Issuers</h2>
          {verifiedIssuers.map((row) => (
            <div
              key={row.pda.toBase58()}
              className="rounded-xl border border-green-900 bg-gray-900 p-5 flex items-center justify-between"
            >
              <div className="flex flex-col gap-0.5">
                <p className="font-medium text-green-400">
                  ✅ {row.issuer.name}
                </p>
                <p className="text-xs text-gray-500">{row.issuer.website}</p>
                <p className="text-xs text-gray-600">
                  {row.issuer.credentialsIssued.toString()} credentials issued
                </p>
              </div>
              <button
                onClick={() => handleDeactivate(row)}
                disabled={deactivatingPda === row.pda.toBase58()}
                className="rounded-lg bg-red-900 hover:bg-red-800 disabled:opacity-50 px-4 py-2 text-sm font-medium text-red-300"
              >
                {deactivatingPda === row.pda.toBase58() ? "Deactivating…" : "Deactivate"}
              </button>
            </div>
          ))}
          {deactivateError && (
            <p className="text-sm text-red-400 mt-2">{deactivateError}</p>
          )}
        </section>
      )}

      {/* ── Transfer Platform Authority ───────────────────────────────────────── */}
      {chainStatus === "initialized" && (
        <section className="flex flex-col gap-4">
          <h2 className="text-xl font-bold">Transfer Platform Authority</h2>
          <div className="rounded-xl border border-gray-800 bg-gray-900 p-6 flex flex-col gap-4">
            <p className="text-sm text-gray-400">
              Transfer admin rights to a new wallet. The current wallet loses all admin access.
            </p>
            {transferDone && (
              <div className="text-green-400 text-sm">Authority transferred.</div>
            )}
            <div className="flex flex-col gap-1">
              <label className="text-xs text-gray-500 uppercase tracking-wide">
                New authority pubkey
              </label>
              <input
                value={newAuthorityInput}
                onChange={(e) => setNewAuthorityInput(e.target.value)}
                placeholder="Solana public key"
                className="rounded-lg bg-gray-800 border border-gray-700 px-3 py-2 text-sm font-mono text-white placeholder-gray-600 focus:outline-none focus:border-red-600"
              />
            </div>
            <button
              onClick={handleTransferAuthority}
              disabled={!publicKey || transferLoading || !newAuthorityInput.trim()}
              className="self-start rounded-lg bg-red-700 hover:bg-red-600 disabled:bg-gray-700 disabled:text-gray-500 disabled:cursor-not-allowed px-5 py-2.5 text-sm font-medium"
            >
              {transferLoading ? "Sending…" : "Transfer Authority"}
            </button>
            {transferError && (
              <div className="rounded-lg border border-red-800 bg-red-950 px-4 py-3 text-sm text-red-300">
                {transferError}
              </div>
            )}
          </div>
        </section>
      )}
    </div>
  );
}
