/// PDA seeds — байтовые префиксы для всех аккаунтов программы.
///
/// Solana вычисляет адрес аккаунта из seeds + program_id,
/// поэтому один и тот же набор seeds всегда даёт один и тот же адрес.
/// Держим их в одном месте, чтобы не расходились между программой и тестами.
pub mod seeds {
    /// Глобальный конфиг платформы. Один на всю программу — singleton PDA.
    pub const PLATFORM_CONFIG: &[u8] = b"platform_config";

    /// Специальный PDA, который будет владеть MPL-Core коллекциями и ассетами.
    /// Именно он подписывает CPI к mpl-core — ни issuer, ни admin напрямую это сделать не могут.
    pub const PROGRAM_SIGNER: &[u8] = b"program_signer";

    /// Аккаунт выпускателя (университет, работодатель и т.д.)
    pub const ISSUER_REGISTRY: &[u8] = b"issuer_registry";

    /// Запись о конкретной выданной квалификации. Привязана к MPL-Core ассету двумя ссылками:
    /// Credential.core_asset → PublicKey ассета, и атрибут в ассете → Credential PDA.
    pub const CREDENTIAL: &[u8] = b"credential";

    /// Запись об эндорсменте с 30-дневной блокировкой SOL — создаёт реальную стоимость Sybil-атаки.
    pub const ENDORSEMENT: &[u8] = b"endorsement";
}

/// Разрешённые префиксы для URI метаданных коллекций и credentials.
/// Только Arweave — метаданные должны жить вечно, HTTP-серверы умирают.
pub const ALLOWED_URI_PREFIXES: &[&str] = &[
    "ar://",
    "https://arweave.net/",
    "https://gateway.irys.xyz/",
];
