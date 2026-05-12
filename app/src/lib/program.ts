// Константы и хелперы для работы с программой on-chain-cv со стороны фронтенда.
// Намеренно не используем @coral-xyz/anchor на клиенте — он тянет тяжёлые зависимости.
// Вместо этого собираем инструкции вручную по данным из IDL.
import {
  PublicKey,
  TransactionInstruction,
  SystemProgram,
  Connection,
} from "@solana/web3.js";
import bs58 from "bs58";

// Адрес задеплоенной программы. Берём из target/deploy/on_chain_cv-keypair.json
// (после anchor build) или из Anchor.toml — поле programs.localnet.on_chain_cv.
export const PROGRAM_ID = new PublicKey(
  "4YDMjUyfuj4efEbyceNknSjwuFzxR1yZJSi6fzKsCH52"
);

// Anchor идентифицирует инструкции по первым 8 байтам instruction data.
// Формула: sha256("global:<имя_инструкции>")[0..8]
// Эти байты взяты из target/idl/on_chain_cv.json → instructions[N].discriminator
const INITIALIZE_PLATFORM_DISCRIMINATOR = Buffer.from([
  119, 201, 101, 45, 75, 122, 89, 3,
]);
const REGISTER_ISSUER_DISCRIMINATOR = Buffer.from([145, 117, 52, 59, 189, 27, 127, 18]);
const VERIFY_ISSUER_DISCRIMINATOR = Buffer.from([175, 158, 45, 6, 185, 85, 67, 61]);
const DEACTIVATE_ISSUER_DISCRIMINATOR = Buffer.from([52, 10, 163, 187, 247, 22, 150, 37]);
const UPDATE_ISSUER_METADATA_DISCRIMINATOR = Buffer.from([167, 77, 168, 94, 175, 115, 147, 33]);

// Discriminator аккаунта IssuerRegistry — используется как memcmp-фильтр при
// getProgramAccounts, чтобы нода отдавала только этот тип аккаунтов.
const ISSUER_REGISTRY_DISCRIMINATOR = Buffer.from([252, 217, 20, 87, 39, 96, 228, 46]);

const MPL_CORE_PROGRAM_ID = new PublicKey(
  "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
);

// ── PDA helpers ───────────────────────────────────────────────────────────────

// Вычисляет PDA аккаунта PlatformConfig.
// findProgramAddressSync перебирает bump от 255 вниз, пока не найдёт точку вне кривой ed25519.
// Результат детерминирован — одни и те же seeds + programId всегда дают один адрес.
export function getPlatformConfigPda(): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("platform_config")],
    PROGRAM_ID
  );
}

// Вычисляет PDA реестра эмитента по кошельку его владельца.
// Seeds включают authority, поэтому один адрес → один реестр: зарегистрироваться дважды нельзя.
export function getIssuerRegistryPda(authority: PublicKey): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("issuer_registry"), authority.toBuffer()],
    PROGRAM_ID
  );
}

// PDA-подписант программы. Передаётся как update_authority MPL-Core коллекций —
// только программа может подписывать CPI с этими seeds, ни issuer, ни admin напрямую.
export function getProgramSignerPda(): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("program_signer")],
    PROGRAM_ID
  );
}

// ── Borsh encoding ────────────────────────────────────────────────────────────

// Кодирует строку в формат Borsh: 4 байта LE-длины + UTF-8 содержимое.
// Anchor использует этот формат для всех String-полей в instruction data.
function borshString(s: string): Buffer {
  const bytes = Buffer.from(s, "utf8");
  const len = Buffer.alloc(4);
  len.writeUInt32LE(bytes.length, 0);
  return Buffer.concat([len, bytes]);
}

// ── initialize_platform ───────────────────────────────────────────────────────

// Собирает инструкцию initialize_platform вручную, без Anchor SDK.
// Порядок аккаунтов строго совпадает с #[derive(Accounts)] InitializePlatform в программе.
export function buildInitializePlatformIx(
  authority: PublicKey
): TransactionInstruction {
  const [platformConfig] = getPlatformConfigPda();

  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      // platform_config: новый PDA — writable, сам не подписывает
      { pubkey: platformConfig, isSigner: false, isWritable: true },
      // authority: платит за ренту аккаунта, подписывает транзакцию
      { pubkey: authority, isSigner: true, isWritable: true },
      // system_program: нужен Anchor'у для создания аккаунта через CPI
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    // Для инструкций без аргументов data = только discriminator (8 байт)
    data: INITIALIZE_PLATFORM_DISCRIMINATOR,
  });
}

// ── transfer_platform_authority ───────────────────────────────────────────────

// sha256("global:transfer_platform_authority")[0..8]
const TRANSFER_PLATFORM_AUTHORITY_DISCRIMINATOR = Buffer.from([110, 215, 33, 194, 127, 233, 129, 146]);

// Передаёт права администратора платформы на новый кошелёк.
// После подтверждения транзакции текущий кошелёк теряет все admin-права.
// newAuthority не подписывает — принятие прав одностороннее.
export function buildTransferPlatformAuthorityIx(params: {
  authority: PublicKey;
  newAuthority: PublicKey;
}): TransactionInstruction {
  const [platformConfigPda] = getPlatformConfigPda();
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: platformConfigPda, isSigner: false, isWritable: true },
      { pubkey: params.authority, isSigner: true, isWritable: false },
      // newAuthority читается только как pubkey — подпись не требуется
      { pubkey: params.newAuthority, isSigner: false, isWritable: false },
    ],
    data: TRANSFER_PLATFORM_AUTHORITY_DISCRIMINATOR,
  });
}

// ── register_issuer ───────────────────────────────────────────────────────────

// Порядок аккаунтов соответствует RegisterIssuer #[derive(Accounts)] в программе.
export function buildRegisterIssuerIx(
  authority: PublicKey,
  name: string,
  website: string
): TransactionInstruction {
  const [issuerRegistry] = getIssuerRegistryPda(authority);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: issuerRegistry, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      REGISTER_ISSUER_DISCRIMINATOR,
      borshString(name),
      borshString(website),
    ]),
  });
}

// ── verify_issuer ─────────────────────────────────────────────────────────────

// Порядок аккаунтов соответствует VerifyIssuer #[derive(Accounts)] в программе.
// collectionPubkey — keypair нового аккаунта коллекции; вызывающий должен передать
// его в sendTransaction({ signers: [collectionKeypair] }), иначе MPL-Core откажет.
// platformAuthority фигурирует дважды: как authority (индекс 4) и как payer ренты (индекс 5).
export function buildVerifyIssuerIx(
  platformAuthority: PublicKey,
  issuerAuthority: PublicKey,
  collectionPubkey: PublicKey,
  collectionUri: string
): TransactionInstruction {
  const [platformConfig] = getPlatformConfigPda();
  const [issuerRegistry] = getIssuerRegistryPda(issuerAuthority);
  const [programSigner] = getProgramSignerPda();
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: platformConfig, isSigner: false, isWritable: false },
      { pubkey: issuerRegistry, isSigner: false, isWritable: true },
      { pubkey: programSigner, isSigner: false, isWritable: false },
      { pubkey: collectionPubkey, isSigner: true, isWritable: true },
      { pubkey: platformAuthority, isSigner: true, isWritable: true },  // authority
      { pubkey: platformAuthority, isSigner: true, isWritable: true },  // payer
      { pubkey: MPL_CORE_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([VERIFY_ISSUER_DISCRIMINATOR, borshString(collectionUri)]),
  });
}

// ── deactivate_issuer ─────────────────────────────────────────────────────────

// Порядок аккаунтов соответствует DeactivateIssuer #[derive(Accounts)] в программе.
export function buildDeactivateIssuerIx(
  platformAuthority: PublicKey,
  issuerAuthority: PublicKey
): TransactionInstruction {
  const [platformConfig] = getPlatformConfigPda();
  const [issuerRegistry] = getIssuerRegistryPda(issuerAuthority);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: platformConfig, isSigner: false, isWritable: false },
      { pubkey: issuerRegistry, isSigner: false, isWritable: true },
      { pubkey: platformAuthority, isSigner: true, isWritable: false },
    ],
    data: DEACTIVATE_ISSUER_DISCRIMINATOR,
  });
}

// ── update_issuer_metadata ────────────────────────────────────────────────────

// Порядок аккаунтов соответствует UpdateIssuerMetadata #[derive(Accounts)] в программе.
// Если у эмитента ещё нет коллекции (до верификации), передавай SystemProgram.programId
// как collectionPubkey — on-chain constraint пропустит проверку через unwrap_or(true).
export function buildUpdateIssuerMetadataIx(
  authority: PublicKey,
  collectionPubkey: PublicKey,
  newName: string,
  newWebsite: string
): TransactionInstruction {
  const [issuerRegistry] = getIssuerRegistryPda(authority);
  const [programSigner] = getProgramSignerPda();
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: issuerRegistry, isSigner: false, isWritable: true },
      { pubkey: collectionPubkey, isSigner: false, isWritable: false },
      { pubkey: programSigner, isSigner: false, isWritable: false },
      { pubkey: authority, isSigner: true, isWritable: false },
      { pubkey: MPL_CORE_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      UPDATE_ISSUER_METADATA_DISCRIMINATOR,
      borshString(newName),
      borshString(newWebsite),
    ]),
  });
}

// ── IssuerRegistry deserializer ───────────────────────────────────────────────

// Типизированное отображение on-chain аккаунта IssuerRegistry.
// bigint для i64/u64 — JS Number не может точно представить 64-битные целые.
export interface IssuerRegistryAccount {
  authority: PublicKey;
  name: string;
  website: string;
  isVerified: boolean;
  verifiedBy: PublicKey | null;
  verifiedAt: bigint | null;       // Unix timestamp (секунды), null до верификации
  deactivatedAt: bigint | null;    // null пока эмитент активен
  collection: PublicKey | null;    // адрес MPL-Core коллекции, null до верификации
  credentialsIssued: bigint;
  bump: number;
}

// Вспомогательные ридеры — каждый возвращает значение и новый offset.
function readPubkey(buf: Buffer, offset: number): [PublicKey, number] {
  return [new PublicKey(buf.slice(offset, offset + 32)), offset + 32];
}

function readString(buf: Buffer, offset: number): [string, number] {
  const len = buf.readUInt32LE(offset);
  return [buf.slice(offset + 4, offset + 4 + len).toString("utf8"), offset + 4 + len];
}

function readBool(buf: Buffer, offset: number): [boolean, number] {
  return [buf[offset] === 1, offset + 1];
}

function readI64(buf: Buffer, offset: number): [bigint, number] {
  return [buf.readBigInt64LE(offset), offset + 8];
}

function readU64(buf: Buffer, offset: number): [bigint, number] {
  return [buf.readBigUInt64LE(offset), offset + 8];
}

// Option<T> в Borsh: 0x00 = None, 0x01 + bytes = Some(T).
function readOption<T>(
  buf: Buffer,
  offset: number,
  read: (b: Buffer, o: number) => [T, number]
): [T | null, number] {
  if (buf[offset] === 0) return [null, offset + 1];
  return read(buf, offset + 1);
}

// Ручной Borsh-декодер — точно повторяет on-chain layout struct IssuerRegistry (state.rs).
// Порядок чтения должен совпадать с порядком полей в Rust-структуре.
// Если добавить поле в state.rs, этот декодер нужно обновить вручную.
export function deserializeIssuerRegistry(data: Buffer): IssuerRegistryAccount {
  let o = 8; // пропускаем 8-байтовый discriminator
  const [authority, o1] = readPubkey(data, o);
  const [name, o2] = readString(data, o1);
  const [website, o3] = readString(data, o2);
  const [isVerified, o4] = readBool(data, o3);
  const [verifiedBy, o5] = readOption(data, o4, readPubkey);
  const [verifiedAt, o6] = readOption(data, o5, readI64);
  const [deactivatedAt, o7] = readOption(data, o6, readI64);
  const [collection, o8] = readOption(data, o7, readPubkey);
  const [credentialsIssued, o9] = readU64(data, o8);
  return {
    authority, name, website, isVerified, verifiedBy, verifiedAt,
    deactivatedAt, collection, credentialsIssued, bump: data[o9],
  };
}

// ── fetchAllIssuers ───────────────────────────────────────────────────────────

// Запрашивает все аккаунты программы с discriminator IssuerRegistry.
// memcmp-фильтр работает на стороне ноды: RPC отдаёт только подходящие аккаунты,
// без него пришлось бы гонять весь набор аккаунтов программы и фильтровать на клиенте.
export async function fetchAllIssuers(
  connection: import("@solana/web3.js").Connection
): Promise<Array<{ pda: PublicKey; issuer: IssuerRegistryAccount }>> {
  const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
    filters: [
      {
        memcmp: {
          offset: 0,
          // memcmp.bytes требует base58, а не base64 — стандарт Solana RPC API
          bytes: bs58.encode(ISSUER_REGISTRY_DISCRIMINATOR),
        },
      },
    ],
  });
  return accounts.map(({ pubkey, account }) => ({
    pda: pubkey,
    issuer: deserializeIssuerRegistry(Buffer.from(account.data)),
  }));
}

function explorerClusterParam(): string {
  const rpc = process.env.NEXT_PUBLIC_RPC_ENDPOINT ?? "http://127.0.0.1:8899";
  if (rpc.includes("devnet")) return "?cluster=devnet";
  if (rpc.includes("mainnet")) return "?cluster=mainnet-beta";
  return `?cluster=custom&customUrl=${encodeURIComponent(rpc)}`;
}

export function explorerUrl(address: PublicKey): string {
  return `https://explorer.solana.com/address/${address.toBase58()}${explorerClusterParam()}`;
}

// ── Credential: discriminators ────────────────────────────────────────────────

// sha256("global:issue_credential")[0..8]
const ISSUE_CREDENTIAL_DISCRIMINATOR = Buffer.from([255, 193, 171, 224, 68, 171, 194, 87]);
// Anchor account discriminator for the Credential account type
const CREDENTIAL_DISCRIMINATOR = Buffer.from([145, 44, 68, 220, 67, 46, 100, 135]);

// ── SkillCategory ─────────────────────────────────────────────────────────────

// Зеркало Rust-enum SkillCategory из state.rs.
// Порядок вариантов должен совпадать — Borsh кодирует enum как u8-индекс.
export type SkillCategory = "Work" | "Education" | "Certificate" | "Achievement";

const SKILL_INDEX: Record<SkillCategory, number> = {
  Work: 0,
  Education: 1,
  Certificate: 2,
  Achievement: 3,
};

const SKILL_NAMES: SkillCategory[] = ["Work", "Education", "Certificate", "Achievement"];

// ── Borsh: Option<i64> ────────────────────────────────────────────────────────

// Кодирует Option<i64> в Borsh: 0x00 = None, 0x01 + 8 байт LE = Some(value).
function borshOptionI64(value: bigint | null): Buffer {
  if (value === null) return Buffer.from([0]);
  const buf = Buffer.alloc(9);
  buf.writeUInt8(1, 0);
  buf.writeBigInt64LE(value, 1);
  return buf;
}

// ── getCredentialPda ──────────────────────────────────────────────────────────

// Вычисляет PDA аккаунта Credential.
// Seeds: [b"credential", issuer_registry, recipient, index_le_bytes]
// index — порядковый номер выданного credential'а из IssuerRegistry.credentials_issued.
export function getCredentialPda(
  issuerRegistryPda: PublicKey,
  recipientPubkey: PublicKey,
  index: bigint
): [PublicKey, number] {
  const indexBuf = Buffer.alloc(8);
  indexBuf.writeBigUInt64LE(index, 0);
  return PublicKey.findProgramAddressSync(
    [
      Buffer.from("credential"),
      issuerRegistryPda.toBuffer(),
      recipientPubkey.toBuffer(),
      indexBuf,
    ],
    PROGRAM_ID
  );
}

// ── buildIssueCredentialIx ────────────────────────────────────────────────────

// Собирает инструкцию issue_credential вручную, без Anchor SDK.
// assetPubkey — keypair нового MPL-Core Asset; вызывающий должен передать его
// в sendTransaction({ signers: [assetKeypair] }), иначе MPL-Core откажет.
// Порядок аккаунтов строго совпадает с IssueCredential #[derive(Accounts)] в программе.
export function buildIssueCredentialIx(params: {
  issuerAuthority: PublicKey;
  payer: PublicKey;
  recipient: PublicKey;
  assetPubkey: PublicKey;
  issuerRegistryPda: PublicKey;
  issuerCollectionPubkey: PublicKey;
  credentialPda: PublicKey;
  skill: SkillCategory;
  level: number;
  name: string;
  expiresAt: bigint | null;
  metadataUri: string;
}): TransactionInstruction {
  const [programSigner] = getProgramSignerPda();
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: params.issuerRegistryPda, isSigner: false, isWritable: true },
      { pubkey: params.credentialPda, isSigner: false, isWritable: true },
      { pubkey: params.recipient, isSigner: false, isWritable: false },
      { pubkey: params.assetPubkey, isSigner: true, isWritable: true },
      { pubkey: params.issuerCollectionPubkey, isSigner: false, isWritable: true },
      { pubkey: programSigner, isSigner: false, isWritable: false },
      { pubkey: params.issuerAuthority, isSigner: true, isWritable: false },
      { pubkey: params.payer, isSigner: true, isWritable: true },
      { pubkey: MPL_CORE_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.concat([
      ISSUE_CREDENTIAL_DISCRIMINATOR,
      Buffer.from([SKILL_INDEX[params.skill]]),
      Buffer.from([params.level]),
      borshString(params.name),
      borshOptionI64(params.expiresAt),
      borshString(params.metadataUri),
    ]),
  });
}

// ── CredentialAccount interface ───────────────────────────────────────────────

// Типизированное отображение on-chain аккаунта Credential.
// Порядок полей зеркалит Rust-структуру Credential из state.rs.
export interface CredentialAccount {
  issuer: PublicKey;
  recipient: PublicKey;
  coreAsset: PublicKey;
  skill: SkillCategory;
  level: number;
  issuedAt: bigint;
  expiresAt: bigint | null;
  revoked: boolean;
  revokedAt: bigint | null;
  endorsementCount: number;
  metadataUri: string;
  index: bigint;
  bump: number;
}

// ── deserializeCredential ─────────────────────────────────────────────────────

// Ручной Borsh-декодер — точно повторяет on-chain layout struct Credential (state.rs).
// Порядок чтения: issuer, recipient, core_asset, skill, level, issued_at,
// expires_at, revoked, revoked_at, endorsement_count, metadata_uri, index, bump.
export function deserializeCredential(data: Buffer): CredentialAccount {
  let o = 8; // пропускаем 8-байтовый discriminator
  const [issuer, o1] = readPubkey(data, o);
  const [recipient, o2] = readPubkey(data, o1);
  const [coreAsset, o3] = readPubkey(data, o2);
  const skillVariant = data[o3];
  let o4 = o3 + 1;
  const skill: SkillCategory = SKILL_NAMES[skillVariant] ?? "Work";
  const level = data[o4++];
  const [issuedAt, o5] = readI64(data, o4);
  const [expiresAt, o6] = readOption(data, o5, readI64);
  const revoked = data[o6] === 1;
  let o7 = o6 + 1;
  const [revokedAt, o8] = readOption(data, o7, readI64);
  const endorsementCount = data.readUInt32LE(o8);
  let o9 = o8 + 4;
  const [metadataUri, o10] = readString(data, o9);
  const [index, o11] = readU64(data, o10);
  const bump = data[o11];
  return {
    issuer, recipient, coreAsset, skill, level,
    issuedAt, expiresAt, revoked, revokedAt,
    endorsementCount, metadataUri, index, bump,
  };
}

// ── fetchCredentialsByRecipient + isExpired ───────────────────────────────────

// Запрашивает все Credential-аккаунты для конкретного получателя.
// Двойной memcmp-фильтр: discriminator (offset 0) + recipient pubkey (offset 8+32).
// recipient находится на offset 8 (discriminator) + 32 (issuer pubkey) = 40.
export async function fetchCredentialsByRecipient(
  connection: import("@solana/web3.js").Connection,
  recipientPubkey: PublicKey
): Promise<Array<{ pda: PublicKey; credential: CredentialAccount }>> {
  const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
    filters: [
      {
        memcmp: {
          offset: 0,
          bytes: bs58.encode(CREDENTIAL_DISCRIMINATOR),
        },
      },
      {
        memcmp: {
          offset: 8 + 32, // пропускаем discriminator + issuer pubkey
          bytes: recipientPubkey.toBase58(),
        },
      },
    ],
  });
  return accounts.map(({ pubkey, account }) => ({
    pda: pubkey,
    credential: deserializeCredential(Buffer.from(account.data)),
  }));
}

// Возвращает true, если credential истёк (expiresAt в прошлом).
// null означает «без срока действия» — такой credential не истекает никогда.
export function isExpired(expiresAt: bigint | null): boolean {
  return expiresAt !== null && expiresAt < BigInt(Math.floor(Date.now() / 1000));
}

// ── MPL-Core helpers ──────────────────────────────────────────────────────────

// Parses a BaseCollectionV1 account to extract its `uri` field.
// MPL-Core collection accounts have no Anchor discriminator — first byte is the Key.
// Key::CollectionV1 = 5. UpdateAuthority for collections is a plain Pubkey (32 bytes),
// not an UpdateAuthority enum like assets.
export async function fetchCollectionUri(
  connection: Connection,
  collectionPubkey: PublicKey
): Promise<string | null> {
  const info = await connection.getAccountInfo(collectionPubkey);
  if (!info) return null;
  const data = Buffer.from(info.data);
  try {
    if (data[0] !== 5) return null; // Key::CollectionV1 = 5
    // [0] Key (1 byte) + [1-32] UpdateAuthority (32 bytes) = offset 33
    const [, afterName] = readString(data, 33);
    const [uri] = readString(data, afterName);
    return uri;
  } catch {
    return null;
  }
}

// Checks whether an MPL-Core asset account has FreezeDelegate.frozen === true.
// Returns false if the account no longer exists (asset burned = revoked outside normal flow).
// Falls back to true if binary parsing fails so the verify badge stays functional.
export async function checkAssetFrozen(
  connection: Connection,
  assetPubkey: PublicKey
): Promise<boolean> {
  const info = await connection.getAccountInfo(assetPubkey);
  if (!info) return false;

  const data = Buffer.from(info.data);
  try {
    if (data[0] !== 1) return false; // Key::AssetV1 = 1
    // Key(1) + Owner(32)
    let offset = 33;

    // UpdateAuthority: variant byte + optional 32 bytes
    const uaVariant = data[offset++];
    if (uaVariant === 1 || uaVariant === 2) offset += 32;

    // Name: Borsh string
    const nameLen = data.readUInt32LE(offset);
    offset += 4 + nameLen;

    // Uri: Borsh string
    const uriLen = data.readUInt32LE(offset);
    offset += 4 + uriLen;

    // Seq: Option<u64> — 1 byte tag + optional 8 bytes
    if (data[offset++] === 1) offset += 8;

    // PluginHeaderV1: Key must be 3
    if (data[offset] !== 3) return false;
    offset++;

    // plugin_registry_offset: u64 LE (absolute offset to registry in account)
    const regLo = data.readUInt32LE(offset);
    const regHi = data.readUInt32LE(offset + 4);
    const registryOffset = regLo + regHi * 4294967296;

    // Jump to PluginRegistryV1
    let ro = registryOffset;
    if (data[ro++] !== 4) return true; // Key must be PluginRegistryV1=4

    // registry: Vec<RegistryRecord> — 4 byte count + records
    const count = data.readUInt32LE(ro);
    ro += 4;

    for (let i = 0; i < count; i++) {
      const pluginType = data[ro++];

      // Authority variant: 0=None,1=Owner,2=UpdateAuthority,3=Address(+32 bytes)
      const authVariant = data[ro++];
      if (authVariant === 3) ro += 32;

      // Absolute offset of plugin content in the account
      const pdLo = data.readUInt32LE(ro);
      const pdHi = data.readUInt32LE(ro + 4);
      const pluginDataOffset = pdLo + pdHi * 4294967296;
      ro += 8;

      if (pluginType === 1) {
        // FreezeDelegate: Plugin variant byte at offset, frozen bool at offset+1
        return data[pluginDataOffset + 1] === 1;
      }
    }

    return false;
  } catch {
    return true;
  }
}

// ── revoke_credential ─────────────────────────────────────────────────────────

// sha256("global:revoke_credential")[0..8] — precomputed, matches Anchor IDL
const REVOKE_CREDENTIAL_DISCRIMINATOR = Buffer.from([38, 123, 95, 95, 223, 158, 169, 87]);

// Builds the revoke_credential instruction.
// Account order matches RevokeCredential #[derive(Accounts)] exactly:
// issuer_registry, credential, asset, issuer_collection, program_signer,
// authority, payer, mpl_core_program, system_program.
export function buildRevokeCredentialIx(params: {
  issuerAuthority: PublicKey;
  payer: PublicKey;
  issuerRegistryPda: PublicKey;
  credentialPda: PublicKey;
  assetPubkey: PublicKey;
  issuerCollectionPubkey: PublicKey;
}): TransactionInstruction {
  const [programSigner] = getProgramSignerPda();
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: params.issuerRegistryPda, isSigner: false, isWritable: true },
      { pubkey: params.credentialPda, isSigner: false, isWritable: true },
      { pubkey: params.assetPubkey, isSigner: false, isWritable: true },
      { pubkey: params.issuerCollectionPubkey, isSigner: false, isWritable: true },
      { pubkey: programSigner, isSigner: false, isWritable: false },
      { pubkey: params.issuerAuthority, isSigner: true, isWritable: false },
      { pubkey: params.payer, isSigner: true, isWritable: true },
      { pubkey: MPL_CORE_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: REVOKE_CREDENTIAL_DISCRIMINATOR,
  });
}

// ── close_credential ─────────────────────────────────────────────────────────

// sha256("global:close_credential")[0..8]
const CLOSE_CREDENTIAL_DISCRIMINATOR = Buffer.from([213, 210, 242, 210, 169, 79, 220, 112]);

// Закрывает PDA отозванного Credential и возвращает ренту эмитенту.
// Вызывать только после revoke_credential и только когда credential.endorsementCount === 0.
// Порядок аккаунтов совпадает с CloseCredential #[derive(Accounts)] в программе.
export function buildCloseCredentialIx(params: {
  issuerRegistryPda: PublicKey;
  issuerAuthority: PublicKey;
  credentialPda: PublicKey;
}): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: params.issuerRegistryPda, isSigner: false, isWritable: true },
      { pubkey: params.credentialPda, isSigner: false, isWritable: true },
      { pubkey: params.issuerAuthority, isSigner: true, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: CLOSE_CREDENTIAL_DISCRIMINATOR,
  });
}

// Fetches all Credential accounts issued by a specific issuer.
// Credential layout: [8 discriminator][32 issuer][32 recipient]…
// The issuer PDA field is at byte offset 8 (right after the 8-byte discriminator).
export async function fetchCredentialsByIssuer(
  connection: Connection,
  issuerRegistryPda: PublicKey
): Promise<Array<{ pda: PublicKey; credential: CredentialAccount }>> {
  const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
    filters: [
      {
        memcmp: {
          offset: 0,
          bytes: bs58.encode(CREDENTIAL_DISCRIMINATOR),
        },
      },
      {
        memcmp: {
          offset: 8,
          bytes: issuerRegistryPda.toBase58(),
        },
      },
    ],
  });
  return accounts.map(({ pubkey, account }) => ({
    pda: pubkey,
    credential: deserializeCredential(Buffer.from(account.data)),
  }));
}

// ── endorsement ───────────────────────────────────────────────────────────────

// Endorsement account layout (Borsh, after 8-byte discriminator):
//   credential  : 32 bytes (Pubkey)
//   endorser    : 32 bytes (Pubkey)
//   endorsed_at : 8 bytes  (i64 little-endian)
//   bump        : 1 byte
// Total with discriminator: 81 bytes

export interface EndorsementAccount {
  credential: PublicKey;
  endorser: PublicKey;
  endorsedAt: bigint;
  bump: number;
}

// Account discriminator: sha256("account:Endorsement")[0..8]
const ENDORSEMENT_DISCRIMINATOR = Buffer.from([167, 137, 37, 17, 220, 102, 104, 52]);

// Instruction discriminators
const ENDORSE_CREDENTIAL_DISCRIMINATOR = Buffer.from([89, 7, 229, 22, 151, 137, 59, 148]);
const CLOSE_ENDORSEMENT_DISCRIMINATOR = Buffer.from([24, 65, 233, 17, 236, 13, 78, 246]);

// Computes the Endorsement PDA for a given credential+endorser pair.
// Seeds: [b"endorsement", credentialPda, endorser]
export function getEndorsementPda(
  credentialPda: PublicKey,
  endorser: PublicKey
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("endorsement"), credentialPda.toBuffer(), endorser.toBuffer()],
    PROGRAM_ID
  );
}

export function deserializeEndorsement(data: Buffer): EndorsementAccount {
  if (data.length < 81) {
    throw new Error(`Endorsement account too short: expected 81 bytes, got ${data.length}`);
  }
  let offset = 8; // skip 8-byte Anchor discriminator
  const credential = new PublicKey(data.subarray(offset, offset + 32)); offset += 32;
  const endorser = new PublicKey(data.subarray(offset, offset + 32)); offset += 32;
  const endorsedAt = data.readBigInt64LE(offset); offset += 8;
  const bump = data[offset];
  return { credential, endorser, endorsedAt, bump };
}

// Builds the endorse_credential instruction.
// Account order matches EndorseCredential #[derive(Accounts)] exactly.
export function buildEndorseCredentialIx(params: {
  endorser: PublicKey;
  credentialPda: PublicKey;
}): TransactionInstruction {
  const [endorsementPda] = getEndorsementPda(params.credentialPda, params.endorser);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: params.credentialPda, isSigner: false, isWritable: true },
      { pubkey: endorsementPda, isSigner: false, isWritable: true },
      { pubkey: params.endorser, isSigner: true, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: ENDORSE_CREDENTIAL_DISCRIMINATOR,
  });
}

// Builds the close_endorsement instruction.
// Account order matches CloseEndorsement #[derive(Accounts)] exactly.
export function buildCloseEndorsementIx(params: {
  endorser: PublicKey;
  credentialPda: PublicKey;
}): TransactionInstruction {
  const [endorsementPda] = getEndorsementPda(params.credentialPda, params.endorser);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: params.credentialPda, isSigner: false, isWritable: true },
      { pubkey: endorsementPda, isSigner: false, isWritable: true },
      { pubkey: params.endorser, isSigner: true, isWritable: true },
    ],
    data: CLOSE_ENDORSEMENT_DISCRIMINATOR,
  });
}

// Returns all Endorsement accounts for a given credential PDA.
// Endorsement layout: [8 disc][32 credential][32 endorser][8 endorsed_at][1 bump]
// memcmp offset 8: credential pubkey (32 bytes)
export async function fetchEndorsementsByCredential(
  connection: Connection,
  credentialPda: PublicKey
): Promise<Array<{ pda: PublicKey; endorsement: EndorsementAccount }>> {
  const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
    filters: [
      { memcmp: { offset: 0, bytes: bs58.encode(ENDORSEMENT_DISCRIMINATOR) } },
      { memcmp: { offset: 8, bytes: credentialPda.toBase58() } },
    ],
  });
  return accounts.map(({ pubkey, account }) => ({
    pda: pubkey,
    endorsement: deserializeEndorsement(Buffer.from(account.data)),
  }));
}

// Returns all Endorsement accounts where endorser == the given wallet.
// memcmp offset 40: endorser pubkey (after 8-disc + 32-credential)
export async function fetchEndorsementsByEndorser(
  connection: Connection,
  endorser: PublicKey
): Promise<Array<{ pda: PublicKey; endorsement: EndorsementAccount }>> {
  const accounts = await connection.getProgramAccounts(PROGRAM_ID, {
    filters: [
      { memcmp: { offset: 0, bytes: bs58.encode(ENDORSEMENT_DISCRIMINATOR) } },
      { memcmp: { offset: 40, bytes: endorser.toBase58() } },
    ],
  });
  return accounts.map(({ pubkey, account }) => ({
    pda: pubkey,
    endorsement: deserializeEndorsement(Buffer.from(account.data)),
  }));
}
