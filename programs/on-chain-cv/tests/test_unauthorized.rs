// programs/on-chain-cv/tests/test_unauthorized.rs
//
// Тесты на авторизацию: чужой кошелёк не должен уметь закрыть чужой
// Credential или перехватить управление платформой.
//
// Плюс happy-path для transfer_platform_authority — проверяем, что
// легитимная передача прав реально меняет PlatformConfig.authority.
// Без него непонятно: тест на ошибку мог пройти потому, что инструкция
// сломана в принципе, а не потому, что авторизация работает.
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{
    constants::seeds,
    state::{PlatformConfig, SkillCategory},
};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

// ── SVM factory ───────────────────────────────────────────────────────────────

fn setup_svm() -> (LiteSVM, Keypair) {
    let program_id = on_chain_cv::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/on_chain_cv.so"))
        .unwrap();
    let mpl_core_id: Pubkey = "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
        .parse()
        .unwrap();
    svm.add_program(mpl_core_id, include_bytes!("fixtures/mpl_core.so"))
        .unwrap();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    (svm, payer)
}

// ── PDA helpers ───────────────────────────────────────────────────────────────

fn platform_config_pda() -> Pubkey {
    Pubkey::find_program_address(&[seeds::PLATFORM_CONFIG], &on_chain_cv::id()).0
}

fn program_signer_pda() -> Pubkey {
    Pubkey::find_program_address(&[seeds::PROGRAM_SIGNER], &on_chain_cv::id()).0
}

fn issuer_registry_pda(authority: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[seeds::ISSUER_REGISTRY, authority.as_ref()],
        &on_chain_cv::id(),
    )
    .0
}

fn credential_pda(issuer_registry: Pubkey, recipient: Pubkey, index: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            seeds::CREDENTIAL,
            issuer_registry.as_ref(),
            recipient.as_ref(),
            &index.to_le_bytes(),
        ],
        &on_chain_cv::id(),
    )
    .0
}

fn mpl_core_id() -> Pubkey {
    "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
        .parse()
        .unwrap()
}

// ── Transaction helper ────────────────────────────────────────────────────────

fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    signers: &[&Keypair],
    fee_payer: Pubkey,
) -> Result<(), litesvm::types::FailedTransactionMetadata> {
    let bh = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&fee_payer), &bh);
    svm.send_transaction(
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap(),
    )
    .map(|_| ())
}

// ── Instruction builders ──────────────────────────────────────────────────────

fn initialize_platform_ix(authority: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::InitializePlatform {}.data(),
        on_chain_cv::accounts::InitializePlatform {
            platform_config: platform_config_pda(),
            authority,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn register_issuer_ix(authority: Pubkey, name: String, website: String) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::RegisterIssuer { name, website }.data(),
        on_chain_cv::accounts::RegisterIssuer {
            issuer_registry: issuer_registry_pda(authority),
            authority,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn verify_issuer_ix(
    platform_authority: Pubkey,
    issuer_authority: Pubkey,
    collection: Pubkey,
    uri: String,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::VerifyIssuer { collection_uri: uri }.data(),
        on_chain_cv::accounts::VerifyIssuer {
            platform_config: platform_config_pda(),
            issuer_registry: issuer_registry_pda(issuer_authority),
            program_signer: program_signer_pda(),
            collection,
            authority: platform_authority,
            payer: platform_authority,
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn issue_credential_ix(
    issuer_authority: Pubkey,
    payer: Pubkey,
    recipient: Pubkey,
    asset: Pubkey,
    collection: Pubkey,
    index: u64,
) -> Instruction {
    let issuer_pda = issuer_registry_pda(issuer_authority);
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::IssueCredential {
            skill: SkillCategory::Work,
            level: 3,
            name: "Rust Developer".to_string(),
            expires_at: None,
            metadata_uri: "ar://test_uri".to_string(),
        }
        .data(),
        on_chain_cv::accounts::IssueCredential {
            issuer_registry: issuer_pda,
            credential: credential_pda(issuer_pda, recipient, index),
            recipient,
            asset,
            issuer_collection: collection,
            program_signer: program_signer_pda(),
            authority: issuer_authority,
            payer,
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn revoke_credential_ix(
    issuer_authority: Pubkey,
    payer: Pubkey,
    credential_pda_key: Pubkey,
    asset: Pubkey,
    collection: Pubkey,
) -> Instruction {
    let issuer_pda = issuer_registry_pda(issuer_authority);
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::RevokeCredential {}.data(),
        on_chain_cv::accounts::RevokeCredential {
            issuer_registry: issuer_pda,
            credential: credential_pda_key,
            asset,
            issuer_collection: collection,
            program_signer: program_signer_pda(),
            authority: issuer_authority,
            payer,
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn transfer_platform_authority_ix(
    current_authority: Pubkey,
    new_authority: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::TransferPlatformAuthority {}.data(),
        on_chain_cv::accounts::TransferPlatformAuthority {
            platform_config: platform_config_pda(),
            authority: current_authority,
            new_authority,
        }
        .to_account_metas(None),
    )
}

// ── Shared setup: platform + issuer registered + issuer verified ──────────────

struct IssuerCtx {
    svm: LiteSVM,
    platform_kp: Keypair,
    issuer_kp: Keypair,
    collection_kp: Keypair,
}

fn setup_verified_issuer() -> IssuerCtx {
    let (mut svm, platform_kp) = setup_svm();
    let issuer_kp = Keypair::new();
    let collection_kp = Keypair::new();
    svm.airdrop(&issuer_kp.pubkey(), 10_000_000_000).unwrap();

    send(
        &mut svm,
        &[initialize_platform_ix(platform_kp.pubkey())],
        &[&platform_kp],
        platform_kp.pubkey(),
    )
    .expect("initialize_platform");

    send(
        &mut svm,
        &[register_issuer_ix(
            issuer_kp.pubkey(),
            "EPAM".to_string(),
            "https://epam.com".to_string(),
        )],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    )
    .expect("register_issuer");

    send(
        &mut svm,
        &[verify_issuer_ix(
            platform_kp.pubkey(),
            issuer_kp.pubkey(),
            collection_kp.pubkey(),
            "ar://collection_uri".to_string(),
        )],
        &[&platform_kp, &collection_kp],
        platform_kp.pubkey(),
    )
    .expect("verify_issuer");

    IssuerCtx { svm, platform_kp, issuer_kp, collection_kp }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Атакующий пытается закрыть чужой Credential. Строит инструкцию
/// со своим issuer_registry PDA, но указывает Credential жертвы.
/// Anchor отклоняет: `credential.issuer != attacker_registry`.
#[test]
fn test_close_credential_wrong_authority() {
    let IssuerCtx { mut svm, platform_kp, issuer_kp, collection_kp } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue a credential as the real issuer
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
        )],
        &[&issuer_kp, &asset_kp],
        issuer_kp.pubkey(),
    )
    .expect("issue credential");

    let cred_pda = credential_pda(
        issuer_registry_pda(issuer_kp.pubkey()),
        recipient.pubkey(),
        0,
    );

    // Revoke it (so it would be closeable if authority were correct)
    send(
        &mut svm,
        &[revoke_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            cred_pda,
            asset_kp.pubkey(),
            collection_kp.pubkey(),
        )],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    )
    .expect("revoke credential");

    // Register attacker as a separate issuer
    let attacker_kp = Keypair::new();
    let attacker_collection_kp = Keypair::new();
    svm.airdrop(&attacker_kp.pubkey(), 10_000_000_000).unwrap();
    send(
        &mut svm,
        &[register_issuer_ix(
            attacker_kp.pubkey(),
            "Evil Corp".to_string(),
            "https://evil.com".to_string(),
        )],
        &[&attacker_kp],
        attacker_kp.pubkey(),
    )
    .expect("register attacker");
    send(
        &mut svm,
        &[verify_issuer_ix(
            platform_kp.pubkey(),
            attacker_kp.pubkey(),
            attacker_collection_kp.pubkey(),
            "ar://attacker_collection".to_string(),
        )],
        &[&platform_kp, &attacker_collection_kp],
        platform_kp.pubkey(),
    )
    .expect("verify attacker");

    // Attacker tries to close the victim's credential using their own registry
    let attacker_registry = issuer_registry_pda(attacker_kp.pubkey());
    let malicious_close_ix = Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::CloseCredential {}.data(),
        on_chain_cv::accounts::CloseCredential {
            issuer_registry: attacker_registry,
            credential: cred_pda,       // victim's credential
            authority: attacker_kp.pubkey(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );

    let res = send(&mut svm, &[malicious_close_ix], &[&attacker_kp], attacker_kp.pubkey());
    assert!(res.is_err(), "Wrong authority must not be able to close someone else's credential");

    // Credential PDA must still exist
    assert!(
        svm.get_account(&cred_pda).is_some(),
        "Victim's credential must still exist after rejected close"
    );
}

/// Атакующий пытается захватить управление платформой без прав.
/// `has_one = authority` в PlatformConfig отклоняет транзакцию,
/// потому что подписант — не текущий admin.
#[test]
fn test_transfer_platform_authority_wrong_authority() {
    let (mut svm, platform_kp) = setup_svm();

    send(
        &mut svm,
        &[initialize_platform_ix(platform_kp.pubkey())],
        &[&platform_kp],
        platform_kp.pubkey(),
    )
    .expect("initialize_platform");

    let attacker_kp = Keypair::new();
    let new_authority_kp = Keypair::new();
    svm.airdrop(&attacker_kp.pubkey(), 10_000_000_000).unwrap();

    // Attacker claims to be the authority
    let res = send(
        &mut svm,
        &[transfer_platform_authority_ix(
            attacker_kp.pubkey(),   // not the real authority
            new_authority_kp.pubkey(),
        )],
        &[&attacker_kp],
        attacker_kp.pubkey(),
    );
    assert!(res.is_err(), "has_one = authority must reject wrong signer");

    // PlatformConfig.authority must still be the original admin
    let config_acc = svm.get_account(&platform_config_pda()).expect("PlatformConfig not found");
    let config = PlatformConfig::try_deserialize(&mut config_acc.data.as_slice()).unwrap();
    assert_eq!(
        config.authority,
        platform_kp.pubkey(),
        "authority must remain unchanged after rejected transfer"
    );
}

/// Легитимный admin передаёт права новому кошельку.
/// После транзакции десериализуем PlatformConfig и проверяем,
/// что authority реально поменялся — не просто is_ok().
#[test]
fn test_transfer_platform_authority_happy_path() {
    let (mut svm, platform_kp) = setup_svm();

    send(
        &mut svm,
        &[initialize_platform_ix(platform_kp.pubkey())],
        &[&platform_kp],
        platform_kp.pubkey(),
    )
    .expect("initialize_platform");

    let new_authority_kp = Keypair::new();

    let res = send(
        &mut svm,
        &[transfer_platform_authority_ix(
            platform_kp.pubkey(),
            new_authority_kp.pubkey(),
        )],
        &[&platform_kp],
        platform_kp.pubkey(),
    );
    assert!(res.is_ok(), "transfer_platform_authority failed: {:?}", res.err());

    // Verify PlatformConfig.authority is now the new wallet
    let config_acc = svm.get_account(&platform_config_pda()).expect("PlatformConfig not found");
    let config = PlatformConfig::try_deserialize(&mut config_acc.data.as_slice()).unwrap();
    assert_eq!(
        config.authority,
        new_authority_kp.pubkey(),
        "PlatformConfig.authority must be updated to new_authority"
    );

    // Old admin should no longer be able to do admin actions (e.g. transfer again)
    let another_kp = Keypair::new();
    let res2 = send(
        &mut svm,
        &[transfer_platform_authority_ix(
            platform_kp.pubkey(),   // old admin — no longer valid
            another_kp.pubkey(),
        )],
        &[&platform_kp],
        platform_kp.pubkey(),
    );
    assert!(res2.is_err(), "Old admin must be rejected after authority transfer");
}
