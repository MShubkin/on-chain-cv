/// Все инструкции в одном файле

use anchor_lang::prelude::*;
use crate::constants::seeds;
use crate::error::OnChainCVError;
use crate::state::{Credential, CredentialClosed, CredentialIssued, CredentialRevoked, Endorsement, EndorsementAdded, EndorsementClosed, IssuerRegistry, PlatformConfig, SkillCategory};


/// Аккаунты для инициализации платформы
///
/// `init` создаёт PDA-аккаунт и списывает ренту с `authority`
/// Вызвать дважды не получится — при повторном вызове Anchor упадёт с `AccountAlreadyInUse`
#[derive(Accounts)]
pub struct InitializePlatform<'info> {
    /// Сам аккаунт конфига. Anchor автоматически проверит seeds и вычислит bump
    /// `space = 8 + INIT_SPACE` — 8 байт это дискриминатор Anchor'а в начале каждого аккаунта
    #[account(
        init,
        payer = authority,
        space = 8 + PlatformConfig::INIT_SPACE,
        seeds = [seeds::PLATFORM_CONFIG],
        bump,
    )]
    pub platform_config: Account<'info, PlatformConfig>,

    /// Тот, кто платит за ренту и становится `platform_authority`
    /// Проверка подписи: проверяет, что аккаунт подписал транзакцию(account.is_signer == true)
    #[account(mut)]
    pub authority: Signer<'info>,

    /// Нужен Anchor'у для создания аккаунта через CPI к System Program
    pub system_program: Program<'info, System>,
}

/// Записывает `authority` и `bump` в только что созданный `PlatformConfig`.
/// Сам аккаунт уже создан через `#[account(init)]` — хендлер только заполняет поля.
pub fn initialize_platform(ctx: Context<InitializePlatform>) -> Result<()> {
    let config = &mut ctx.accounts.platform_config;
    config.authority = ctx.accounts.authority.key();
    // Сохраняем bump, чтобы не искать его каждый раз через find_program_address
    config.bump = ctx.bumps.platform_config;
    Ok(())
}

// ── transfer_platform_authority ──────────────────────────────────────────────

/// Аккаунты для передачи прав на платформу другому кошельку.
///
/// `has_one = authority` — Anchor проверяет `platform_config.authority == authority.key()`
/// ещё до входа в хендлер. Если не совпадает — бросает `Unauthorized`.
#[derive(Accounts)]
pub struct TransferPlatformAuthority<'info> {
    #[account(
        mut,
        seeds = [seeds::PLATFORM_CONFIG],
        bump = platform_config.bump,
        // Проверка принадлежности: подписант должен совпадать с сохранённым authority
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub platform_config: Account<'info, PlatformConfig>,

    /// Текущий authority — должен подписать транзакцию
    pub authority: Signer<'info>,

    /// CHECK: нам нужен только pubkey нового владельца, проверять владение аккаунтом не нужно
    pub new_authority: UncheckedAccount<'info>,
}

/// Перезаписывает `authority`. Bump не меняется — PDA один и тот же.
pub fn transfer_platform_authority(ctx: Context<TransferPlatformAuthority>) -> Result<()> {
    ctx.accounts.platform_config.authority = ctx.accounts.new_authority.key();
    Ok(())
}

// ── register_issuer ──────────────────────────────────────────────────────────

/// Аккаунты для регистрации нового эмитента.
///
/// `#[instruction(...)]` здесь не влияет на вычисление space — он фиксирован через `INIT_SPACE`.
/// Атрибут оставлен на случай, если понадобится добавить динамические ограничения на аргументы.
#[derive(Accounts)]
#[instruction(name: String, website: String)]
pub struct RegisterIssuer<'info> {
    /// Создаваемый аккаунт реестра эмитента.
    /// Seeds включают `authority.key()`, поэтому один кошелёк — один реестр.
    #[account(
        init,
        payer = authority,
        space = 8 + IssuerRegistry::INIT_SPACE,
        seeds = [seeds::ISSUER_REGISTRY, authority.key().as_ref()],
        bump,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// Кошелёк, который платит ренту и становится `authority` нового реестра.
    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// Инициализирует `IssuerRegistry`: записывает все поля в исходное состояние.
///
/// После регистрации `is_verified = false` — выдавать credentials нельзя до верификации платформой.
pub fn register_issuer(
    ctx: Context<RegisterIssuer>,
    name: String,
    website: String,
) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer_registry;
    issuer.authority = ctx.accounts.authority.key();
    issuer.name = name;
    issuer.website = website;
    issuer.is_verified = false;
    issuer.verified_by = None;
    issuer.verified_at = None;
    issuer.deactivated_at = None;
    issuer.collection = None;
    issuer.credentials_issued = 0;
    issuer.bump = ctx.bumps.issuer_registry;
    Ok(())
}

// ── verify_issuer ─────────────────────────────────────────────────────────────

/// Аккаунты для верификации эмитента администратором платформы.
///
/// При первой верификации создаёт MPL-Core Collection через CPI.
/// `collection` — отдельный keypair, потому что MPL-Core требует подписи создаваемого аккаунта
/// при его инициализации через System Program. После создания `update_authority`
/// передаётся `program_signer` — с этого момента URI коллекции изменить снаружи нельзя.
#[derive(Accounts)]
pub struct VerifyIssuer<'info> {
    /// Конфиг платформы. `has_one = authority` гарантирует, что подписант — текущий администратор.
    #[account(
        seeds = [seeds::PLATFORM_CONFIG],
        bump = platform_config.bump,
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub platform_config: Account<'info, PlatformConfig>,

    /// Аккаунт эмитента, которого верифицируют.
    #[account(
        mut,
        seeds = [seeds::ISSUER_REGISTRY, issuer_registry.authority.as_ref()],
        bump = issuer_registry.bump,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// PDA-подписант. Передаётся как `update_authority` создаваемой коллекции,
    /// поэтому только программа может менять метаданные через CPI — ни issuer, ни admin напрямую.
    /// CHECK: PDA used as update_authority for all MPL-Core Collections.
    /// Only the program can sign CPIs with these seeds — issuers cannot alter metadata.
    #[account(seeds = [seeds::PROGRAM_SIGNER], bump)]
    pub program_signer: UncheckedAccount<'info>,

    /// Keypair новой коллекции. Должен подписать транзакцию — MPL-Core требует подписи аккаунта
    /// при его создании через System Program.
    #[account(mut)]
    pub collection: Signer<'info>,

    /// Администратор платформы — инициатор верификации.
    pub authority: Signer<'info>,

    /// Плательщик ренты за создание аккаунта коллекции. Обычно тот же, что `authority`.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: must be MPL-Core program (id = CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d)
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Верифицирует эмитента и при необходимости создаёт MPL-Core коллекцию.
///
/// Идемпотентен в части создания коллекции: если `issuer.collection` уже задан
/// (повторная верификация после деактивации), CPI пропускается — обновляются только флаги.
///
/// `collection_uri` должен начинаться с `ar://`, `https://arweave.net/` или
/// `https://gateway.irys.xyz/` — только постоянное хранилище гарантирует доступность
/// метаданных на годы вперёд.
pub fn verify_issuer(ctx: Context<VerifyIssuer>, collection_uri: String) -> Result<()> {
    require!(
        crate::constants::ALLOWED_URI_PREFIXES
            .iter()
            .any(|p| collection_uri.starts_with(p)),
        OnChainCVError::InvalidMetadataUri
    );

    let issuer = &mut ctx.accounts.issuer_registry;
    let signer_seeds: &[&[&[u8]]] =
        &[&[seeds::PROGRAM_SIGNER, &[ctx.bumps.program_signer]]];

    if issuer.collection.is_none() {
        mpl_core::instructions::CreateCollectionV2CpiBuilder::new(
            &ctx.accounts.mpl_core_program,
        )
        .collection(&ctx.accounts.collection)
        .update_authority(Some(&ctx.accounts.program_signer))
        .payer(&ctx.accounts.payer)
        .system_program(&ctx.accounts.system_program)
        .name(format!("{} Credentials", issuer.name))
        .uri(collection_uri)
        .invoke_signed(signer_seeds)?;

        issuer.collection = Some(ctx.accounts.collection.key());
    }

    issuer.is_verified = true;
    issuer.verified_by = Some(ctx.accounts.authority.key());
    issuer.verified_at = Some(Clock::get()?.unix_timestamp);

    emit!(crate::state::IssuerVerified {
        issuer: issuer.key(),
        collection: issuer.collection.unwrap(),
        timestamp: Clock::get()?.unix_timestamp,
    });

    Ok(())
}

// ── deactivate_issuer ─────────────────────────────────────────────────────────

/// Аккаунты для деактивации эмитента.
///
/// Деактивация не удаляет аккаунт и не затрагивает коллекцию или уже выданные credentials.
/// Они остаются в кошельках держателей. Проверку `deactivated_at` при выдаче
/// выполняет инструкция `issue_credential`.
#[derive(Accounts)]
pub struct DeactivateIssuer<'info> {
    /// Конфиг платформы. `has_one = authority` — только администратор может деактивировать эмитентов.
    #[account(
        seeds = [seeds::PLATFORM_CONFIG],
        bump = platform_config.bump,
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub platform_config: Account<'info, PlatformConfig>,

    /// Деактивируемый эмитент.
    #[account(
        mut,
        seeds = [seeds::ISSUER_REGISTRY, issuer_registry.authority.as_ref()],
        bump = issuer_registry.bump,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// Администратор платформы — инициатор деактивации.
    pub authority: Signer<'info>,
}

/// Записывает текущий Unix timestamp в `deactivated_at`.
/// После этого эмитент не может выдавать новые credentials, пока не будет верифицирован снова.
pub fn deactivate_issuer(ctx: Context<DeactivateIssuer>) -> Result<()> {
    ctx.accounts.issuer_registry.deactivated_at = Some(Clock::get()?.unix_timestamp);
    Ok(())
}

// ── update_issuer_metadata ────────────────────────────────────────────────────

/// Аккаунты для обновления метаданных эмитента.
///
/// При переименовании синхронизирует название коллекции через CPI к MPL-Core
/// и сбрасывает флаги верификации. Смысл сброса: организация с новым названием
/// должна снова пройти проверку у администратора платформы.
#[derive(Accounts)]
pub struct UpdateIssuerMetadata<'info> {
    /// Аккаунт эмитента. `has_one = authority` — только владелец может обновлять метаданные.
    #[account(
        mut,
        seeds = [seeds::ISSUER_REGISTRY, authority.key().as_ref()],
        bump = issuer_registry.bump,
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// Аккаунт MPL-Core коллекции. Если коллекция ещё не создана (до верификации),
    /// `unwrap_or(true)` пропускает constraint — CPI в этом случае тоже не вызывается.
    /// `mut` нужен для UpdateCollectionV1 CPI — MPL-Core обновляет данные коллекции.
    /// CHECK: must match issuer_registry.collection if set.
    /// When collection is None (not yet verified), any pubkey passes — CPI is skipped.
    #[account(
        mut,
        constraint = issuer_registry.collection
            .map(|c| c == collection.key())
            .unwrap_or(true)
            @ OnChainCVError::Unauthorized
    )]
    pub collection: UncheckedAccount<'info>,

    /// PDA-подписант для CPI к MPL-Core при обновлении названия коллекции.
    /// CHECK: PDA-signer used as update_authority of the Collection.
    #[account(seeds = [seeds::PROGRAM_SIGNER], bump)]
    pub program_signer: UncheckedAccount<'info>,

    /// Владелец реестра эмитента — единственный, кто может вызвать эту инструкцию.
    pub authority: Signer<'info>,

    /// CHECK: must be MPL-Core program (id = CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d)
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,

    /// Нужна для UpdateCollectionV1 CPI (перераспределение ренты при изменении размера аккаунта).
    pub system_program: Program<'info, System>,
}

/// Обновляет `name` и `website`. При изменении `name`:
/// - если коллекция уже создана, синхронизирует её название через `UpdateCollectionV1` CPI;
/// - сбрасывает `is_verified`, `verified_by` и `verified_at`.
///
/// Сброс верификации при переименовании намеренный: смена названия это смена идентичности,
/// которую платформа должна подтвердить заново.
pub fn update_issuer_metadata(
    ctx: Context<UpdateIssuerMetadata>,
    new_name: String,
    new_website: String,
) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer_registry;
    let name_changed = issuer.name != new_name;

    issuer.name = new_name;
    issuer.website = new_website;

    if name_changed {
        if issuer.collection.is_some() {
            let signer_seeds: &[&[&[u8]]] =
                &[&[seeds::PROGRAM_SIGNER, &[ctx.bumps.program_signer]]];

            mpl_core::instructions::UpdateCollectionV1CpiBuilder::new(
                &ctx.accounts.mpl_core_program,
            )
            .collection(&ctx.accounts.collection)
            .payer(&ctx.accounts.authority)
            .authority(Some(&ctx.accounts.program_signer))
            .system_program(&ctx.accounts.system_program)
            .new_name(format!("{} Credentials", issuer.name))
            .invoke_signed(signer_seeds)?;
        }

        issuer.is_verified = false;
        issuer.verified_by = None;
        issuer.verified_at = None;
    }

    Ok(())
}

// ── issue_credential ──────────────────────────────────────────────────────────

/// Аккаунты для выдачи credential.
///
/// За одну транзакцию создаются два аккаунта: `Credential` PDA (Anchor `init`)
/// и MPL-Core Asset (CPI к `CreateV2`). Asset помечается как frozen через `FreezeDelegate`,
/// чтобы держатель не мог его передать. `update_authority` — `program_signer`, а не эмитент.
#[derive(Accounts)]
#[instruction(skill: SkillCategory, level: u8, name: String, expires_at: Option<i64>, metadata_uri: String)]
pub struct IssueCredential<'info> {
    /// Реестр эмитента. `has_one = authority` — только владелец реестра может выдавать.
    /// `credentials_issued` используется как часть seed нового Credential PDA.
    #[account(
        mut,
        seeds = [seeds::ISSUER_REGISTRY, authority.key().as_ref()],
        bump = issuer_registry.bump,
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// Новый Credential PDA. Seed включает `credentials_issued` на момент вызова —
    /// атомарность транзакции гарантирует уникальность адреса.
    #[account(
        init,
        payer = payer,
        space = 8 + Credential::INIT_SPACE,
        seeds = [
            seeds::CREDENTIAL,
            issuer_registry.key().as_ref(),
            recipient.key().as_ref(),
            issuer_registry.credentials_issued.to_le_bytes().as_ref(),
        ],
        bump,
    )]
    pub credential: Account<'info, Credential>,

    /// CHECK: кошелёк получателя. Подпись не нужна — эмитент выдаёт на любой адрес.
    pub recipient: UncheckedAccount<'info>,

    /// CHECK: эфемерный keypair нового MPL-Core Asset. Должен подписать транзакцию,
    /// чтобы MPL-Core смог создать аккаунт через System Program.
    #[account(mut, signer)]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: MPL-Core коллекция эмитента. Должна совпадать с `issuer_registry.collection`,
    /// установленным при `verify_issuer`. Asset будет привязан к этой коллекции.
    #[account(
        mut,
        constraint = Some(issuer_collection.key()) == issuer_registry.collection
            @ OnChainCVError::Unauthorized
    )]
    pub issuer_collection: UncheckedAccount<'info>,

    /// CHECK: PDA-подписант для CPI к MPL-Core. Передаётся как `update_authority` нового Asset.
    #[account(seeds = [seeds::PROGRAM_SIGNER], bump)]
    pub program_signer: UncheckedAccount<'info>,

    /// Кошелёк эмитента — инициатор выдачи.
    pub authority: Signer<'info>,

    /// Плательщик ренты за Credential PDA и MPL-Core Asset.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: должен быть программой MPL-Core (id = CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d)
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Создаёт Credential PDA и soulbound MPL-Core Asset.
///
/// Проверяет: эмитент верифицирован, активен, уровень 1–5, URI из Arweave.
/// Инкрементирует `issuer_registry.credentials_issued` — счётчик нужен для seed следующего credential.
pub fn issue_credential(
    ctx: Context<IssueCredential>,
    skill: SkillCategory,
    level: u8,
    name: String,
    expires_at: Option<i64>,
    metadata_uri: String,
) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer_registry;
    require!(issuer.is_verified, OnChainCVError::IssuerNotVerified);
    require!(issuer.deactivated_at.is_none(), OnChainCVError::IssuerDeactivated);
    require!((1..=5).contains(&level), OnChainCVError::InvalidLevel);
    require!(
        crate::constants::ALLOWED_URI_PREFIXES
            .iter()
            .any(|p| metadata_uri.starts_with(p)),
        OnChainCVError::InvalidMetadataUri
    );

    // `name` — название должности (например, "Senior Rust Developer"). Хранится в MPL-Core Asset
    // (виден в Phantom), а не в Credential PDA. Получить его после выдачи: запросить аккаунт
    // по адресу `credential.core_asset`.
    let signer_seeds: &[&[&[u8]]] =
        &[&[seeds::PROGRAM_SIGNER, &[ctx.bumps.program_signer]]];

    mpl_core::instructions::CreateV2CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .asset(&ctx.accounts.asset)
        .collection(Some(&ctx.accounts.issuer_collection))
        .authority(Some(&ctx.accounts.program_signer))
        .payer(&ctx.accounts.payer)
        .owner(Some(&ctx.accounts.recipient))
        .name(name)
        .uri(metadata_uri.clone())
        .plugins(vec![
            mpl_core::types::PluginAuthorityPair {
                plugin: mpl_core::types::Plugin::FreezeDelegate(
                    mpl_core::types::FreezeDelegate { frozen: true },
                ),
                authority: Some(mpl_core::types::PluginAuthority::Address {
                    address: ctx.accounts.program_signer.key(),
                }),
            },
            mpl_core::types::PluginAuthorityPair {
                plugin: mpl_core::types::Plugin::PermanentBurnDelegate(
                    mpl_core::types::PermanentBurnDelegate {},
                ),
                authority: Some(mpl_core::types::PluginAuthority::Address {
                    address: ctx.accounts.program_signer.key(),
                }),
            },
        ])
        .system_program(&ctx.accounts.system_program)
        .invoke_signed(signer_seeds)?;

    let issuer_key = issuer.key();
    let now = Clock::get()?.unix_timestamp;
    let credential = &mut ctx.accounts.credential;
    credential.issuer = issuer_key;
    credential.recipient = ctx.accounts.recipient.key();
    credential.core_asset = ctx.accounts.asset.key();
    credential.skill = skill;
    credential.level = level;
    credential.issued_at = now;
    credential.expires_at = expires_at;
    credential.revoked = false;
    credential.revoked_at = None;
    credential.endorsement_count = 0;
    credential.metadata_uri = metadata_uri;
    credential.index = issuer.credentials_issued;
    credential.bump = ctx.bumps.credential;

    issuer.credentials_issued = issuer.credentials_issued
        .checked_add(1)
        .ok_or(OnChainCVError::Overflow)?;

    emit!(CredentialIssued {
        credential: credential.key(),
        core_asset: credential.core_asset,
        issuer: issuer_key,
        recipient: credential.recipient,
        skill: credential.skill.clone(),
        level: credential.level,
        timestamp: now,
    });

    Ok(())
}

// ── revoke_credential ─────────────────────────────────────────────────────────

/// Аккаунты для отзыва credential.
///
/// Эмитент сжигает MPL-Core Asset (он исчезает из Phantom) и выставляет `revoked = true`.
/// Отозвать может только эмитент, выдавший credential: `has_one = authority`.
/// `program_signer` — одновременно `FreezeDelegate` authority и `update_authority` коллекции,
/// поэтому может сжечь замороженный Asset без предварительного CPI разморозки.
#[derive(Accounts)]
pub struct RevokeCredential<'info> {
    /// Реестр эмитента. `has_one = authority` — только выдавший может отозвать.
    #[account(
        mut,
        seeds = [seeds::ISSUER_REGISTRY, authority.key().as_ref()],
        bump = issuer_registry.bump,
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// Отзываемый Credential. Два inline-ограничения:
    /// - `credential.issuer == issuer_registry.key()` — принадлежит этому реестру.
    /// - `!credential.revoked` — нельзя отозвать уже отозванный.
    #[account(
        mut,
        seeds = [
            seeds::CREDENTIAL,
            issuer_registry.key().as_ref(),
            credential.recipient.as_ref(),
            credential.index.to_le_bytes().as_ref(),
        ],
        bump = credential.bump,
        constraint = credential.issuer == issuer_registry.key() @ OnChainCVError::Unauthorized,
        constraint = !credential.revoked @ OnChainCVError::AlreadyRevoked,
    )]
    pub credential: Account<'info, Credential>,

    /// CHECK: MPL-Core Asset для сжигания. Должен совпадать с `credential.core_asset`.
    #[account(
        mut,
        constraint = asset.key() == credential.core_asset @ OnChainCVError::Unauthorized,
    )]
    pub asset: UncheckedAccount<'info>,

    /// CHECK: MPL-Core коллекция эмитента. Нужна для BurnV1 CPI.
    #[account(
        mut,
        constraint = Some(issuer_collection.key()) == issuer_registry.collection
            @ OnChainCVError::Unauthorized
    )]
    pub issuer_collection: UncheckedAccount<'info>,

    /// CHECK: PDA-подписант для BurnV1 CPI. Одновременно `FreezeDelegate` authority
    /// и `update_authority` коллекции — может сжигать замороженные ассеты.
    #[account(seeds = [seeds::PROGRAM_SIGNER], bump)]
    pub program_signer: UncheckedAccount<'info>,

    /// Кошелёк эмитента — инициатор отзыва.
    pub authority: Signer<'info>,

    /// Плательщик комиссии за BurnV1 CPI.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: должен быть программой MPL-Core
    #[account(address = mpl_core::ID)]
    pub mpl_core_program: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

/// Сжигает MPL-Core Asset и помечает Credential как отозванный.
pub fn revoke_credential(ctx: Context<RevokeCredential>) -> Result<()> {
    let signer_seeds: &[&[&[u8]]] =
        &[&[seeds::PROGRAM_SIGNER, &[ctx.bumps.program_signer]]];

    // PermanentBurnDelegate использует forceApprove-семантику и обходит FreezeDelegate.frozen=true.
    // Отдельный CPI разморозки перед сжиганием не нужен.
    // Деактивированные эмитенты сохраняют право отзыва — они не могут выдавать новые credentials,
    // но могут отзывать ранее выданные.
    mpl_core::instructions::BurnV1CpiBuilder::new(&ctx.accounts.mpl_core_program)
        .asset(&ctx.accounts.asset)
        .collection(Some(&ctx.accounts.issuer_collection))
        .authority(Some(&ctx.accounts.program_signer))
        .payer(&ctx.accounts.payer)
        .system_program(Some(&ctx.accounts.system_program))
        .invoke_signed(signer_seeds)?;

    let now = Clock::get()?.unix_timestamp;
    let credential = &mut ctx.accounts.credential;
    credential.revoked = true;
    credential.revoked_at = Some(now);

    emit!(CredentialRevoked {
        credential: credential.key(),
        core_asset: credential.core_asset,
        issuer: credential.issuer,
        recipient: credential.recipient,
        timestamp: now,
    });

    Ok(())
}

// ── endorse_credential ────────────────────────────────────────────────────────

/// Аккаунты для эндорсирования credential.
///
/// Создаёт Endorsement PDA. `init` отклонит транзакцию, если аккаунт уже существует —
/// это предотвращает двойной эндорсмент без дополнительных проверок в хендлере.
/// Seeds credential используют поля из самого аккаунта — это безопасно, потому что
/// только наша программа может создавать аккаунты с нашим discriminator'ом.
#[derive(Accounts)]
pub struct EndorseCredential<'info> {
    /// Эндорсируемый Credential. PDA переподтверждается по полям аккаунта.
    /// Отозванный credential эндорсировать нельзя.
    #[account(
        mut,
        seeds = [
            seeds::CREDENTIAL,
            credential.issuer.as_ref(),
            credential.recipient.as_ref(),
            credential.index.to_le_bytes().as_ref(),
        ],
        bump = credential.bump,
        constraint = !credential.revoked @ OnChainCVError::AlreadyRevoked,
    )]
    pub credential: Account<'info, Credential>,

    /// Новый Endorsement PDA. Seeds привязывают его к паре credential+endorser —
    /// один эндорсер не может создать два аккаунта для одного credential.
    #[account(
        init,
        payer = endorser,
        space = 8 + Endorsement::INIT_SPACE,
        seeds = [seeds::ENDORSEMENT, credential.key().as_ref(), endorser.key().as_ref()],
        bump,
    )]
    pub endorsement: Account<'info, Endorsement>,

    /// Эндорсер платит ренту и подписывает. Не должен быть получателем credential.
    #[account(
        mut,
        constraint = endorser.key() != credential.recipient @ OnChainCVError::SelfEndorsementForbidden,
    )]
    pub endorser: Signer<'info>,

    pub system_program: Program<'info, System>,
}

/// Записывает эндорсмент и инкрементирует `endorsement_count` у Credential.
pub fn endorse_credential(ctx: Context<EndorseCredential>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let endorsement = &mut ctx.accounts.endorsement;
    endorsement.credential = ctx.accounts.credential.key();
    endorsement.endorser = ctx.accounts.endorser.key();
    endorsement.endorsed_at = now;
    endorsement.bump = ctx.bumps.endorsement;

    let credential = &mut ctx.accounts.credential;
    credential.endorsement_count = credential.endorsement_count
        .checked_add(1)
        .ok_or(OnChainCVError::Overflow)?;

    emit!(EndorsementAdded {
        endorsement: endorsement.key(),
        credential: credential.key(),
        endorser: endorsement.endorser,
        timestamp: now,
    });

    Ok(())
}

// ── close_endorsement ─────────────────────────────────────────────────────────

/// Аккаунты для закрытия эндорсмента и возврата заблокированной ренты.
///
/// `close = endorser` — сокращение Anchor: переводит лампорты аккаунта на `endorser`
/// после Ok(()). Блокировка 30 дней проверяется в хендлере.
/// Credential к этому моменту может быть отозван или истёкшим — рента возвращается в любом случае.
#[derive(Accounts)]
pub struct CloseEndorsement<'info> {
    /// Credential, к которому относится эндорсмент. `endorsement_count` будет уменьшен на 1.
    #[account(
        mut,
        seeds = [
            seeds::CREDENTIAL,
            credential.issuer.as_ref(),
            credential.recipient.as_ref(),
            credential.index.to_le_bytes().as_ref(),
        ],
        bump = credential.bump,
    )]
    pub credential: Account<'info, Credential>,

    /// Закрываемый Endorsement PDA. `has_one = endorser` — только создавший может закрыть.
    /// `close = endorser` — Anchor переведёт лампорты на endorser после Ok(()).
    #[account(
        mut,
        seeds = [seeds::ENDORSEMENT, credential.key().as_ref(), endorser.key().as_ref()],
        bump = endorsement.bump,
        has_one = endorser @ OnChainCVError::Unauthorized,
        close = endorser,
    )]
    pub endorsement: Account<'info, Endorsement>,

    /// Оригинальный эндорсер — PDA seeds жёстко привязаны к его кошельку.
    /// Получает ренту после закрытия.
    #[account(mut)]
    pub endorser: Signer<'info>,
}

/// Проверяет 30-дневную блокировку, декрементирует `endorsement_count`.
/// Anchor закрывает Endorsement PDA и переводит лампорты уже после Ok(()).
pub fn close_endorsement(ctx: Context<CloseEndorsement>) -> Result<()> {
    // 30-дневная блокировка: эндорсер не может вернуть ренту раньше endorsed_at + 30 дней.
    let lockup_end = ctx.accounts.endorsement.endorsed_at
        .checked_add(30 * 24 * 60 * 60)
        .ok_or(OnChainCVError::Overflow)?;
    require!(
        Clock::get()?.unix_timestamp >= lockup_end,
        OnChainCVError::EndorsementLocked
    );

    // Уменьшаем счётчик. Anchor (close = endorser) сам переводит лампорты.
    let credential = &mut ctx.accounts.credential;
    credential.endorsement_count = credential.endorsement_count
        .checked_sub(1)
        .ok_or(OnChainCVError::Overflow)?;

    emit!(EndorsementClosed {
        endorsement: ctx.accounts.endorsement.key(),
        credential: credential.key(),
        endorser: ctx.accounts.endorser.key(),
        timestamp: Clock::get()?.unix_timestamp,
    });

    Ok(())
}

// ── close_credential ─────────────────────────────────────────────────────────

/// Закрывает PDA отозванного Credential и возвращает ренту эмитенту.
///
/// Два условия проверяются до любого изменения состояния:
/// 1. `credential.revoked` — нельзя закрыть активный Credential.
/// 2. `credential.endorsement_count == 0` — сначала все эндорсеры закрывают
///    свои PDAs. Иначе `close = authority` уничтожил бы PDA, на который они
///    ещё ссылаются: висячие ссылки в блокчейне ничем не детектируются.
///
/// `close = authority` — сокращение Anchor: после Ok(()) рантайм обнуляет
/// данные аккаунта и переводит его лампорты на authority.
#[derive(Accounts)]
pub struct CloseCredential<'info> {
    /// IssuerRegistry эмитента. `has_one = authority` проверяет,
    /// что закрывает именно владелец реестра, а не посторонний.
    #[account(
        mut,
        seeds = [seeds::ISSUER_REGISTRY, authority.key().as_ref()],
        bump = issuer_registry.bump,
        has_one = authority @ OnChainCVError::Unauthorized,
    )]
    pub issuer_registry: Account<'info, IssuerRegistry>,

    /// Закрываемый Credential. Три inline-ограничения на уровне аккаунта:
    /// - `credential.issuer == issuer_registry.key()` — Credential принадлежит этому реестру.
    /// - `credential.revoked` — только отозванные можно удалять.
    /// - `credential.endorsement_count == 0` — нет живых Endorsement PDAs.
    #[account(
        mut,
        seeds = [
            seeds::CREDENTIAL,
            issuer_registry.key().as_ref(),
            credential.recipient.as_ref(),
            credential.index.to_le_bytes().as_ref(),
        ],
        bump = credential.bump,
        constraint = credential.issuer == issuer_registry.key() @ OnChainCVError::Unauthorized,
        constraint = credential.revoked @ OnChainCVError::NotRevoked,
        constraint = credential.endorsement_count == 0 @ OnChainCVError::HasEndorsements,
        close = authority,
    )]
    pub credential: Account<'info, Credential>,

    /// Получает ренту закрытого PDA. `mut` обязателен: Anchor переводит
    /// лампорты именно на этот аккаунт.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// Нужна Anchor для `close = authority` — перевод лампортов идёт через неё.
    pub system_program: Program<'info, System>,
}

/// Отправляет событие CredentialClosed. Всю остальную работу берёт Anchor:
/// `close = authority` обнуляет данные аккаунта и переводит лампорты уже
/// после возврата Ok(()) из этой функции — не внутри неё.
pub fn close_credential(ctx: Context<CloseCredential>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    emit!(CredentialClosed {
        credential: ctx.accounts.credential.key(),
        issuer: ctx.accounts.credential.issuer,
        recipient: ctx.accounts.credential.recipient,
        timestamp: now,
    });
    Ok(())
}
