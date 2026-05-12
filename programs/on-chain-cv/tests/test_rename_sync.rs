// programs/on-chain-cv/tests/test_rename_sync.rs
//
// При переименовании эмитента программа обновляет название MPL-Core
// коллекции через CPI к UpdateCollectionV1 и сбрасывает is_verified.
//
// Тест десериализует BaseCollectionV1 напрямую из данных аккаунта —
// это единственный способ убедиться, что CPI реально записал новое
// имя, а не просто вернул Ok(()).
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use mpl_core::accounts::BaseCollectionV1;
use on_chain_cv::{
    constants::seeds,
    state::IssuerRegistry,
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

fn update_issuer_metadata_ix(
    authority: Pubkey,
    collection: Pubkey,
    new_name: String,
    new_website: String,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::UpdateIssuerMetadata { new_name, new_website }.data(),
        on_chain_cv::accounts::UpdateIssuerMetadata {
            issuer_registry: issuer_registry_pda(authority),
            collection,
            program_signer: program_signer_pda(),
            authority,
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

// ── Test ──────────────────────────────────────────────────────────────────────

/// Переименование верифицированного эмитента:
/// - меняет IssuerRegistry.name;
/// - сбрасывает is_verified — смена названия это смена идентичности,
///   платформа должна перепроверить;
/// - обновляет название MPL-Core коллекции на "{new_name} Credentials"
///   через CPI к UpdateCollectionV1.
#[test]
fn test_rename_syncs_collection_name() {
    let (mut svm, platform_kp) = setup_svm();
    let issuer_kp = Keypair::new();
    let collection_kp = Keypair::new();
    svm.airdrop(&issuer_kp.pubkey(), 10_000_000_000).unwrap();

    // ── Setup: register + verify "EPAM" ──────────────────────────────────────
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

    // Confirm initial state: verified, collection name = "EPAM Credentials"
    let issuer_pda = issuer_registry_pda(issuer_kp.pubkey());
    let issuer_acc = svm.get_account(&issuer_pda).expect("issuer_registry not found");
    let issuer = IssuerRegistry::try_deserialize(&mut issuer_acc.data.as_slice()).unwrap();
    assert!(issuer.is_verified, "issuer must be verified before rename");
    assert_eq!(issuer.name, "EPAM");

    // ── Rename to "EPAM Systems" ──────────────────────────────────────────────
    let res = send(
        &mut svm,
        &[update_issuer_metadata_ix(
            issuer_kp.pubkey(),
            collection_kp.pubkey(),
            "EPAM Systems".to_string(),
            "https://epam.com".to_string(),
        )],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_ok(), "update_issuer_metadata failed: {:?}", res.err());

    // ── Assert IssuerRegistry state ───────────────────────────────────────────
    let issuer_acc = svm.get_account(&issuer_pda).expect("issuer_registry not found after rename");
    let issuer = IssuerRegistry::try_deserialize(&mut issuer_acc.data.as_slice()).unwrap();
    assert_eq!(issuer.name, "EPAM Systems", "IssuerRegistry.name must be updated");
    assert!(!issuer.is_verified, "is_verified must be reset to false after rename");

    // ── Assert MPL-Core collection name ──────────────────────────────────────
    // BaseCollectionV1 uses Borsh (no 8-byte Anchor discriminator)
    let col_account = svm
        .get_account(&collection_kp.pubkey())
        .expect("MPL-Core collection account not found");
    let collection = BaseCollectionV1::from_bytes(&col_account.data)
        .expect("Failed to deserialize BaseCollectionV1");
    assert_eq!(
        collection.name,
        "EPAM Systems Credentials",
        "MPL-Core collection name must be synced to 'EPAM Systems Credentials'"
    );
}
