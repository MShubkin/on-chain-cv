use anchor_lang::prelude::*;

/// Глобальный синглтон платформы. Создаётся один раз при деплое программы.
///
/// PDA seeds: `[b"platform_config"]`. Нет флага "активна/не активна" —
/// сам факт существования аккаунта означает, что платформа инициализирована.
#[account]
#[derive(InitSpace)]
pub struct PlatformConfig {
    /// Публичный ключ текущего администратора платформы.
    /// Только он может верифицировать и деактивировать эмитентов.
    pub authority: Pubkey,

    /// Bump PDA. Хранится, чтобы не вызывать дорогой `find_program_address` при каждой инструкции.
    pub bump: u8,
}

/// Реестр одного эмитента — университета, работодателя или любой другой организации,
/// которая имеет право выдавать квалификационные credentials.
///
/// PDA seeds: `[b"issuer_registry", authority.key()]`. Один кошелёк — один реестр:
/// зарегистрироваться дважды с одного адреса не выйдет.
///
/// Жизненный цикл: `register_issuer` → `verify_issuer` → (опционально) `deactivate_issuer`.
#[account]
#[derive(InitSpace)]
pub struct IssuerRegistry {
    /// Кошелёк, от имени которого зарегистрирован эмитент.
    /// Только он может обновлять `name` и `website`.
    pub authority: Pubkey,

    /// Отображаемое название организации. Максимум 64 символа.
    /// Используется как префикс в названии MPL-Core коллекции: `"{name} Credentials"`.
    /// Смена имени сбрасывает `is_verified` — потребуется повторная верификация,
    /// чтобы платформа убедилась, что новое название принадлежит реальной организации.
    #[max_len(64)]
    pub name: String,

    /// URL сайта организации. Максимум 128 символов. Хранится только информационно.
    #[max_len(128)]
    pub website: String,

    /// Верифицирован ли эмитент платформой. Без верификации выдавать credentials нельзя.
    pub is_verified: bool,

    /// Кошелёк администратора, который провёл верификацию. `None` до первой верификации.
    pub verified_by: Option<Pubkey>,

    /// Unix timestamp верификации (секунды, UTC). `None` до первой верификации.
    pub verified_at: Option<i64>,

    /// Unix timestamp деактивации. `None`, пока эмитент активен.
    /// Ненулевое значение означает: новые credentials выдавать нельзя,
    /// но уже выданные остаются в кошельках держателей.
    pub deactivated_at: Option<i64>,

    /// Адрес MPL-Core коллекции, созданной при первой верификации.
    /// `None` до верификации. Все credential-assets этого эмитента принадлежат именно этой коллекции.
    pub collection: Option<Pubkey>,

    /// Счётчик выданных credentials. Используется как часть seed при создании каждого нового
    /// Credential PDA — гарантирует уникальность адреса внутри одного эмитента.
    pub credentials_issued: u64,

    /// Bump PDA. Хранится, чтобы не вызывать `find_program_address` при каждом вызове.
    pub bump: u8,
}

/// Событие, эмитируемое программой при успешной верификации эмитента.
///
/// Клиенты подписываются через `program.addEventListener("issuerVerified", callback)`.
/// Индексаторы используют это событие для построения реестра верифицированных организаций без
/// полного скана аккаунтов программы.
#[event]
pub struct IssuerVerified {
    /// PDA аккаунта `IssuerRegistry`, который был верифицирован.
    pub issuer: Pubkey,

    /// Адрес MPL-Core коллекции, созданной в этой же транзакции.
    pub collection: Pubkey,

    /// Unix timestamp момента верификации.
    pub timestamp: i64,
}

/// Категория квалификации. Anchor сериализует как однобайтовый индекс (Work=0, Education=1, …).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, InitSpace)]
pub enum SkillCategory {
    Work,
    Education,
    Certificate,
    Achievement,
}

/// On-chain запись о выданном credential.
///
/// PDA seeds: `[b"credential", issuer_registry.key(), recipient.key(), index.to_le_bytes()]`.
/// `index` равен `issuer_registry.credentials_issued` в момент выдачи. Транзакция атомарна,
/// поэтому счётчик не коллидирует — каждый PDA уникален.
#[account]
#[derive(InitSpace)]
pub struct Credential {
    /// PDA аккаунта `IssuerRegistry` эмитента, выдавшего этот credential.
    pub issuer: Pubkey,
    /// Кошелёк получателя.
    pub recipient: Pubkey,
    /// Адрес MPL-Core Asset, созданного вместе с этим credential (soulbound NFT).
    pub core_asset: Pubkey,
    pub skill: SkillCategory,
    /// Уровень профессионализма от 1 до 5.
    pub level: u8,
    pub issued_at: i64,
    /// Если задан, credential считается истёкшим после этой Unix-метки.
    pub expires_at: Option<i64>,
    pub revoked: bool,
    pub revoked_at: Option<i64>,
    /// Число активных `Endorsement`-аккаунтов, ссылающихся на этот credential.
    /// Должно быть 0 перед `close_credential` — иначе закрытый PDA оставит висячие ссылки.
    pub endorsement_count: u32,
    /// Arweave URI (`ar://…`, `https://arweave.net/…` или Irys gateway).
    #[max_len(200)]
    pub metadata_uri: String,
    /// Значение `issuer_registry.credentials_issued` на момент выдачи.
    /// Хранится, чтобы PDA можно было вывести заново без текущего счётчика реестра.
    pub index: u64,
    pub bump: u8,
}

/// Генерируется при успешной выдаче credential.
#[event]
pub struct CredentialIssued {
    /// Адрес созданного Credential PDA.
    pub credential: Pubkey,
    /// Адрес MPL-Core Asset, выпущенного в той же транзакции.
    pub core_asset: Pubkey,
    pub issuer: Pubkey,
    pub recipient: Pubkey,
    pub skill: SkillCategory,
    pub level: u8,
    pub timestamp: i64,
}

/// Генерируется при отзыве credential. MPL-Core Asset сожжён в той же транзакции —
/// он исчезает из кошелька держателя немедленно.
#[event]
pub struct CredentialRevoked {
    /// Адрес Credential PDA. Сам аккаунт остаётся жить до `close_credential`.
    pub credential: Pubkey,
    /// Адрес MPL-Core Asset, который уже сожжён к этому моменту.
    pub core_asset: Pubkey,
    pub issuer: Pubkey,
    pub recipient: Pubkey,
    pub timestamp: i64,
}

/// On-chain запись об одном эндорсменте.
///
/// PDA seeds: `[b"endorsement", credential.key(), endorser.key()]`.
/// Один эндорсер — один эндорсмент на credential: `init` не даст создать дубль.
/// Рента (~0.002 SOL) заблокирована на 30 дней, потом возвращается через `close_endorsement`.
/// Блокировка создаёт реальную стоимость Sybil-атаки на систему доверия.
#[account]
#[derive(InitSpace)]
pub struct Endorsement {
    /// PDA аккаунта `Credential`, к которому относится эндорсмент.
    pub credential: Pubkey,
    /// Кошелёк, создавший эндорсмент. Именно на него вернётся рента.
    pub endorser: Pubkey,
    /// Unix-метка создания эндорсмента — отсчёт 30-дневной блокировки идёт отсюда.
    pub endorsed_at: i64,
    pub bump: u8,
}

/// Генерируется при создании нового эндорсмента.
#[event]
pub struct EndorsementAdded {
    pub endorsement: Pubkey,
    pub credential: Pubkey,
    pub endorser: Pubkey,
    pub timestamp: i64,
}

/// Генерируется при закрытии эндорсмента и возврате ренты эндорсеру.
#[event]
pub struct EndorsementClosed {
    pub endorsement: Pubkey,
    pub credential: Pubkey,
    pub endorser: Pubkey,
    pub timestamp: i64,
}

/// Генерируется при закрытии PDA отозванного Credential и возврате ренты эмитенту.
///
/// Финальный шаг жизненного цикла. После этого события PDA перестаёт
/// существовать. Инструкция не выполнится, если endorsement_count > 0:
/// сначала эндорсеры закрывают свои PDAs, потом эмитент закрывает Credential.
#[event]
pub struct CredentialClosed {
    /// Адрес закрытого Credential PDA.
    pub credential: Pubkey,
    /// IssuerRegistry эмитента, через который прошла авторизация.
    pub issuer: Pubkey,
    /// Кошелёк получателя — нужен фронту для обновления профиля.
    pub recipient: Pubkey,
    /// Unix-метка времени закрытия.
    pub timestamp: i64,
}
