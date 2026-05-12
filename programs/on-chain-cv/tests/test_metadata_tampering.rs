// programs/on-chain-cv/tests/test_metadata_tampering.rs
//
// Тест на неизменность метаданных: эмитент не должен уметь поменять URI
// ассета после выдачи. update_authority ассета — это program_signer PDA,
// а не кошелёк эмитента. Прямой UpdateV1 от имени эмитента падает
// на стороне MPL-Core с ошибкой авторизации.
//
// Без этого теста гарантию неизменности нельзя было бы назвать проверенной:
// happy-path тесты её не касаются.
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{
    constants::seeds,
    state::SkillCategory,
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
            level: 4,
            name: "Senior Engineer".to_string(),
            expires_at: None,
            metadata_uri: "ar://original_uri".to_string(),
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

// ── Test ──────────────────────────────────────────────────────────────────────

/// После выдачи ассета update_authority — это program_signer PDA, не эмитент.
/// Прямой UpdateV1 от имени эмитента должен упасть на стороне MPL-Core:
/// программа не может подписать за чужой PDA.
#[test]
fn test_issuer_cannot_tamper_asset_uri() {
    let (mut svm, platform_kp) = setup_svm();
    let issuer_kp = Keypair::new();
    let collection_kp = Keypair::new();
    let recipient_kp = Keypair::new();
    let asset_kp = Keypair::new();
    svm.airdrop(&issuer_kp.pubkey(), 10_000_000_000).unwrap();

    // Platform init + register + verify issuer
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

    // Issue a credential — MPL-Core Asset is created with program_signer as update_authority
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient_kp.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
        )],
        &[&issuer_kp, &asset_kp],
        issuer_kp.pubkey(),
    )
    .expect("issue_credential");

    // Verify the asset was actually created
    assert!(
        svm.get_account(&asset_kp.pubkey()).is_some(),
        "MPL-Core Asset must exist after issue"
    );

    // Issuer attempts to call UpdateV1 directly with their own keypair as authority.
    // The asset's update_authority is `program_signer` PDA, not the issuer — MPL-Core must reject.
    let tamper_ix = mpl_core::instructions::UpdateV1 {
        asset: asset_kp.pubkey(),
        collection: Some(collection_kp.pubkey()),
        payer: issuer_kp.pubkey(),
        authority: Some(issuer_kp.pubkey()),   // issuer is NOT the update_authority
        system_program: system_program::ID,
        log_wrapper: None,
    }
    .instruction(mpl_core::instructions::UpdateV1InstructionArgs {
        new_name: None,
        new_uri: Some("ar://tampered_uri".to_string()),
        new_update_authority: None,
    });

    let res = send(&mut svm, &[tamper_ix], &[&issuer_kp], issuer_kp.pubkey());
    assert!(
        res.is_err(),
        "Issuer must not be able to update asset URI — they are not the update_authority"
    );
}
