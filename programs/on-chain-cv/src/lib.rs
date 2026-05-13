// Модули программы
pub mod constants; // PDA seeds — байтовые префиксы для всех аккаунтов
pub mod error; // Кастомные ошибки (OnChainCVError)
pub mod instructions; // Все инструкции
pub mod state; // Структуры on-chain аккаунтов

use anchor_lang::prelude::*;

// Реэкспортируем всё наружу, чтобы тесты могли использовать
// on_chain_cv::accounts::*, on_chain_cv::instruction::* и т.д.
pub use constants::*;
pub use error::*;
// Anchor генерирует символы с теми же именами, что и `pub use instructions::*`.
// Предупреждение безвредно — нужный экспорт берётся из макроса #[program].
#[allow(ambiguous_glob_reexports)]
pub use instructions::*;
pub use state::*;

declare_id!("4YDMjUyfuj4efEbyceNknSjwuFzxR1yZJSi6fzKsCH52");

/// Точка входа для всех инструкций программы
///
/// Anchor роутит вызовы по 8-байтовому дискриминатору в начале instruction data
/// Дискриминатор = sha256("global:<имя_инструкции>")[0..8].
#[program]
pub mod on_chain_cv {
    use super::*;

    /// Создаёт `PlatformConfig` PDA. Разовая операция при первом деплое
    pub fn initialize_platform(ctx: Context<InitializePlatform>) -> Result<()> {
        instructions::initialize_platform(ctx)
    }

    /// Передаёт права администратора платформы другому кошельку
    pub fn transfer_platform_authority(ctx: Context<TransferPlatformAuthority>) -> Result<()> {
        instructions::transfer_platform_authority(ctx)
    }

    /// Регистрирует нового эмитента. После этого `is_verified = false` —
    /// выдавать credentials нельзя до верификации администратором.
    pub fn register_issuer(ctx: Context<RegisterIssuer>, name: String, website: String) -> Result<()> {
        instructions::register_issuer(ctx, name, website)
    }

    /// Верифицирует эмитента и (при первом вызове) создаёт MPL-Core коллекцию
    /// `collection_uri` должен указывать на Arweave — другие хранилища отклоняются
    pub fn verify_issuer(ctx: Context<VerifyIssuer>, collection_uri: String) -> Result<()> {
        instructions::verify_issuer(ctx, collection_uri)
    }

    /// Деактивирует эмитента: новые credentials выдавать нельзя, старые не затрагиваются
    pub fn deactivate_issuer(ctx: Context<DeactivateIssuer>) -> Result<()> {
        instructions::deactivate_issuer(ctx)
    }

    /// Обновляет `name` и `website` эмитента. При смене имени сбрасывает верификацию
    /// и синхронизирует название MPL-Core коллекции через CPI
    pub fn update_issuer_metadata(
        ctx: Context<UpdateIssuerMetadata>,
        new_name: String,
        new_website: String,
    ) -> Result<()> {
        instructions::update_issuer_metadata(ctx, new_name, new_website)
    }

    /// Выдаёт credential держателю: создаёт `Credential` PDA и soulbound MPL-Core Asset
    /// в коллекции эмитента. Требует верифицированного и активного эмитента
    pub fn issue_credential(
        ctx: Context<IssueCredential>,
        skill: SkillCategory,
        level: u8,
        name: String,
        expires_at: Option<i64>,
        metadata_uri: String,
    ) -> Result<()> {
        instructions::issue_credential(ctx, skill, level, name, expires_at, metadata_uri)
    }

    /// Отзывает credential: сжигает soulbound MPL-Core Asset и выставляет `revoked = true`
    /// Asset исчезает из кошелька держателя сразу — BurnV1 CPI в той же транзакции
    pub fn revoke_credential(ctx: Context<RevokeCredential>) -> Result<()> {
        instructions::revoke_credential(ctx)
    }

    /// Записывает эндорсмент. Рента эндорсера (~0.002 SOL) заблокирована на 30 дней —
    /// это цена атаки, делающей Sybil-эндорсинг дорогим. Само-эндорсмент и дубли отклоняются
    pub fn endorse_credential(ctx: Context<EndorseCredential>) -> Result<()> {
        instructions::endorse_credential(ctx)
    }

    /// Закрывает Endorsement PDA и возвращает ренту эндорсеру.
    /// Отклоняется, если с момента эндорсмента прошло меньше 30 дней
    pub fn close_endorsement(ctx: Context<CloseEndorsement>) -> Result<()> {
        instructions::close_endorsement(ctx)
    }

    /// Closes a revoked Credential PDA and returns rent to the issuer.
    /// Fails if the credential is not revoked or has active endorsements
    pub fn close_credential(ctx: Context<CloseCredential>) -> Result<()> {
        instructions::close_credential(ctx)
    }
}
