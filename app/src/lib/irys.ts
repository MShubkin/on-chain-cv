// Утилита загрузки метаданных на Arweave через Irys.
// Загруженные данные постоянны и адресуются по содержимому — URI вида ar://<txId>
// никогда не протухает, что требует программа при verify_issuer.
// На devnet Irys принимает devnet SOL без реальной оплаты.

// Метаданные коллекции в формате Metaplex JSON standard.
// Загружаются на Arweave и передаются как collection_uri при вызове verify_issuer.
export interface CollectionMetadata {
  name: string;
  description: string;
  image: string;          // ar://<txId> загруженного логотипа
  external_url?: string;  // сайт организации
}

export interface IrysUploader {
  // Загружает JSON-объект на Arweave и возвращает ar://<txId>.
  uploadJson(data: object): Promise<string>;
  // Загружает файл (обычно изображение) на Arweave и возвращает ar://<txId>.
  uploadFile(file: File): Promise<string>;
}

// Создаёт Irys-аплоадер через WebSolana-провайдер подключённого кошелька.
//
// Dynamic import обязателен: пакет @irys/web-upload использует browser API
// (window, TextEncoder и т.д.) и не совместим с SSR Next.js. Вызывать только
// внутри browser event handler после подключения кошелька.
//
// walletAdapter: any — Irys ожидает duck-typed объект с signTransaction/sendTransaction,
// точный тип зависит от версии @solana/wallet-adapter и в SDK не экспортируется.
export async function createIrysUploader(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  walletAdapter: any,
  cluster: "devnet" | "mainnet-beta"
): Promise<IrysUploader> {
  const { WebUploader } = await import("@irys/web-upload");
  const { WebSolana } = await import("@irys/web-upload-solana");

  const rpcUrl =
    cluster === "devnet"
      ? "https://api.devnet.solana.com"
      : "https://api.mainnet-beta.solana.com";

  const uploader = await WebUploader(WebSolana)
    .withProvider(walletAdapter)
    .withRpc(rpcUrl);

  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  // Пополняет баланс на ноде Irys если его не хватает для загрузки byteCount байт.
  // fund() подписывает SOL-транзакцию через кошелёк — пользователь увидит approve.
  //
  // На devnet бандлер Irys иногда не находит транзакцию сразу после отправки —
  // он возвращает 400 "Confirmed tx not found". SOL при этом уже списан, поэтому
  // повторно отправлять транзакцию нельзя. Вместо этого ждём подтверждения и
  // вызываем submitFundTransaction с тем же tx ID из сообщения об ошибке.
  async function ensureFunded(byteCount: number): Promise<void> {
    const price = await uploader.getPrice(byteCount);
    const balance = await uploader.getBalance();
    if (!balance.lt(price)) return;

    const amount = price.multipliedBy(2);
    try {
      // Пополняем с запасом x2 чтобы не делать несколько fund-транзакций подряд
      await uploader.fund(amount);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      // Irys включает tx ID прямо в сообщение: "failed to post funding tx - <base58txId> - ..."
      const match = msg.match(/failed to post funding tx - ([A-Za-z0-9]{80,})/);
      if (!match) throw e;

      const txId = match[1];
      // Повторяем регистрацию без повторной отправки SOL: ждём пока devnet подтвердит
      for (let attempt = 1; attempt <= 5; attempt++) {
        await sleep(6000 * attempt); // 6s, 12s, 18s, 24s, 30s
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          await (uploader as any).submitFundTransaction(txId);
          return;
        } catch {
          if (attempt === 5) throw e;
        }
      }
    }
  }

  return {
    async uploadJson(data: object): Promise<string> {
      const serialized = JSON.stringify(data);
      await ensureFunded(Buffer.byteLength(serialized, "utf8"));
      const receipt = await uploader.upload(serialized, {
        tags: [{ name: "Content-Type", value: "application/json" }],
      });
      return `ar://${receipt.id}`;
    },
    async uploadFile(file: File): Promise<string> {
      await ensureFunded(file.size);
      const receipt = await uploader.uploadFile(file);
      return `ar://${receipt.id}`;
    },
  };
}

// Собирает JSON метаданных коллекции по стандарту Metaplex.
// Суффикс " Credentials" в name повторяет логику on-chain (format!("{} Credentials", name)),
// чтобы название коллекции в кошельке совпадало с тем, что отображает Explorer.
export function buildCollectionMetadataJson(params: {
  issuerName: string;
  description: string;
  imageUri: string;
  externalUrl?: string;
}): CollectionMetadata {
  return {
    name: `${params.issuerName} Credentials`,
    description: params.description,
    image: params.imageUri,
    external_url: params.externalUrl,
  };
}

// JSON schema for a credential's Arweave metadata.
// The `on_chain_ref` field creates the bidirectional link:
//   - Credential PDA → core_asset (in Credential.core_asset)
//   - core_asset.uri → Arweave → on_chain_ref.credential_pda (this field)
export interface CredentialMetadata {
  version: "1.0";
  name: string;
  issuer: {
    name: string;
    pubkey: string;
    collection: string | null;
  };
  recipient_pubkey: string;
  period: { from: string; to: string } | null;
  skills: string[];
  level: number;
  issued_at: number;
  expires_at: number | null;
  on_chain_ref: {
    program: string;
    credential_pda: string;
    core_asset: string;
  };
  image: string;
}

// Builds the metadata JSON object for Arweave upload.
// Call BEFORE issuing the credential — you need the URI to pass to issue_credential.
// Both credentialPda and coreAsset are known before the TX (deterministic/ephemeral keypair).
export function buildCredentialMetadataJson(params: {
  credentialName: string;
  issuerName: string;
  issuerPda: string;
  issuerCollection: string | null;
  recipientPubkey: string;
  periodFrom: string | null;
  periodTo: string | null;
  skills: string[];
  level: number;
  expiresAt: number | null;
  credentialPda: string;
  coreAsset: string;
  imageUri: string;
  programId: string;
}): CredentialMetadata {
  return {
    version: "1.0",
    name: params.credentialName,
    issuer: {
      name: params.issuerName,
      pubkey: params.issuerPda,
      collection: params.issuerCollection,
    },
    recipient_pubkey: params.recipientPubkey,
    period:
      params.periodFrom
        ? { from: params.periodFrom, to: params.periodTo ?? "" }
        : null,
    skills: params.skills,
    level: params.level,
    issued_at: Math.floor(Date.now() / 1000),
    expires_at: params.expiresAt,
    on_chain_ref: {
      program: params.programId,
      credential_pda: params.credentialPda,
      core_asset: params.coreAsset,
    },
    image: params.imageUri,
  };
}
