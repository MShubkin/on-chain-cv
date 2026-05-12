use anchor_lang::prelude::*;

/// Кастомные коды ошибок программы.
///
/// Anchor возвращает их клиенту как `Custom(N)`, где N = variant_index + 6000.
/// Текст из `#[msg]` попадает в логи транзакции — виден в Explorer и LiteSVM тестах.
#[error_code]
pub enum OnChainCVError {
    /// Подписант не совпадает с сохранённым полем authority. Обычно означает,
    /// что кто-то пытается вызвать admin-инструкцию с чужим кошельком.
    #[msg("Unauthorized: signer is not platform_authority")]
    Unauthorized,

    /// Эмитент не прошёл верификацию платформой — выдавать credentials нельзя.
    #[msg("Issuer is not verified")]
    IssuerNotVerified,

    /// Эмитент деактивирован — новые credentials не принимаются, старые не затронуты.
    #[msg("Issuer has been deactivated")]
    IssuerDeactivated,

    /// Уровень вне диапазона 1–5. Проверяется в хендлере до записи аккаунта.
    #[msg("Credential level must be in range 1-5")]
    InvalidLevel,

    /// `close_credential` вызван для credential, который ещё не отозван.
    #[msg("Cannot close credential that is not revoked")]
    NotRevoked,

    /// Повторный `revoke_credential` для уже отозванного credential.
    #[msg("Credential is already revoked")]
    AlreadyRevoked,

    /// Получатель credential пытается эндорсировать собственную запись.
    /// Нарушило бы смысл эндорсмента как внешней оценки.
    #[msg("Recipient cannot endorse own credential")]
    SelfEndorsementForbidden,

    /// URI не начинается с разрешённого префикса Arweave.
    /// Только постоянные хранилища гарантируют доступность метаданных.
    #[msg("Metadata URI must use ar://, https://arweave.net/, or https://gateway.irys.xyz/")]
    InvalidMetadataUri,

    /// Прошло меньше 30 дней с момента эндорсмента — рента ещё заблокирована.
    #[msg("Endorsement is still in lockup period (30 days)")]
    EndorsementLocked,

    /// У credential есть живые Endorsement PDAs — сначала их нужно закрыть.
    /// Иначе `close_credential` создал бы висячие ссылки.
    #[msg("Cannot close credential with active endorsements")]
    HasEndorsements,

    /// Переполнение при `checked_add` или `checked_sub`. На практике не достижимо
    /// при разумных объёмах данных, но проверка обязательна для корректности.
    #[msg("Arithmetic overflow")]
    Overflow,
}
