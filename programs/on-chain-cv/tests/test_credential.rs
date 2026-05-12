// programs/on-chain-cv/tests/test_credential.rs
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{
    constants::seeds,
    state::{Credential, SkillCategory},
};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

// ── SVM factory ───────────────────────────────────────────────────────────────

fn setup_with_mpl_core() -> (LiteSVM, Keypair) {
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
    skill: SkillCategory,
    level: u8,
    name: String,
    expires_at: Option<i64>,
    metadata_uri: String,
) -> Instruction {
    let issuer_pda = issuer_registry_pda(issuer_authority);
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::IssueCredential {
            skill,
            level,
            name,
            expires_at,
            metadata_uri,
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

// ── Shared setup: platform + issuer registered + issuer verified ──────────────

struct IssuerCtx {
    svm: LiteSVM,
    platform_kp: Keypair,
    issuer_kp: Keypair,
    collection_kp: Keypair,
}

fn setup_verified_issuer() -> IssuerCtx {
    let (mut svm, platform_kp) = setup_with_mpl_core();
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

#[test]
fn test_issue_credential_happy_path() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,  // first credential → index 0
            SkillCategory::Work,
            4,
            "Senior Rust Developer".to_string(),
            None,
            "ar://test_metadata_uri".to_string(),
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_ok(), "issue_credential failed: {:?}", res.err());

    // Verify Credential PDA fields
    let cred_pda = credential_pda(issuer_registry_pda(issuer_kp.pubkey()), recipient.pubkey(), 0);
    let account = svm.get_account(&cred_pda).expect("Credential PDA not found");
    let cred = Credential::try_deserialize(&mut account.data.as_slice())
        .expect("Deserialize Credential");

    assert_eq!(cred.recipient, recipient.pubkey());
    assert_eq!(cred.core_asset, asset.pubkey());
    assert!(cred.skill == SkillCategory::Work, "skill mismatch");
    assert_eq!(cred.level, 4);
    assert!(!cred.revoked);
    assert_eq!(cred.endorsement_count, 0);
    assert_eq!(cred.metadata_uri, "ar://test_metadata_uri");
    assert_eq!(cred.index, 0);
    assert_eq!(cred.issuer, issuer_registry_pda(issuer_kp.pubkey()));

    // Verify MPL-Core Asset account was created
    assert!(
        svm.get_account(&asset.pubkey()).is_some(),
        "MPL-Core Asset account not found after issue"
    );

    // Verify credentials_issued counter incremented
    let issuer_account = svm
        .get_account(&issuer_registry_pda(issuer_kp.pubkey()))
        .unwrap();
    let issuer = on_chain_cv::state::IssuerRegistry::try_deserialize(
        &mut issuer_account.data.as_slice(),
    )
    .unwrap();
    assert_eq!(issuer.credentials_issued, 1);
}

#[test]
fn test_issue_second_credential_uses_index_1() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset0 = Keypair::new();
    let asset1 = Keypair::new();

    // Issue first credential (index 0)
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset0.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Education,
            3,
            "Python Developer".to_string(),
            None,
            "ar://first".to_string(),
        )],
        &[&issuer_kp, &asset0],
        issuer_kp.pubkey(),
    )
    .expect("first issue");

    // Issue second credential (index 1)
    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset1.pubkey(),
            collection_kp.pubkey(),
            1,  // counter is now 1
            SkillCategory::Certificate,
            5,
            "AWS Certified".to_string(),
            Some(1893456000), // far future expiry
            "ar://second".to_string(),
        )],
        &[&issuer_kp, &asset1],
        issuer_kp.pubkey(),
    );
    assert!(res.is_ok(), "second issue failed: {:?}", res.err());

    let cred1_pda = credential_pda(
        issuer_registry_pda(issuer_kp.pubkey()),
        recipient.pubkey(),
        1,
    );
    let account = svm.get_account(&cred1_pda).expect("second Credential PDA not found");
    let cred = Credential::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(cred.index, 1);
    assert_eq!(cred.expires_at, Some(1893456000));
    assert!(cred.skill == SkillCategory::Certificate, "skill mismatch");
}

#[test]
fn test_issue_credential_unverified_issuer() {
    let (mut svm, platform_kp) = setup_with_mpl_core();
    let issuer_kp = Keypair::new();
    let collection_kp = Keypair::new();
    let recipient = Keypair::new();
    let asset = Keypair::new();
    svm.airdrop(&issuer_kp.pubkey(), 10_000_000_000).unwrap();

    // Platform init + register — but NO verify_issuer
    send(&mut svm, &[initialize_platform_ix(platform_kp.pubkey())], &[&platform_kp], platform_kp.pubkey()).unwrap();
    send(&mut svm, &[register_issuer_ix(issuer_kp.pubkey(), "EPAM".to_string(), "https://epam.com".to_string())], &[&issuer_kp], issuer_kp.pubkey()).unwrap();

    // issuer_collection constraint will fail first (collection is None → Some(x) != None)
    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            1,
            "Test".to_string(),
            None,
            "ar://x".to_string(),
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "Should fail for unverified issuer");
}

#[test]
fn test_issue_credential_deactivated_issuer() {
    let IssuerCtx { mut svm, platform_kp, issuer_kp, collection_kp } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    // Deactivate the issuer
    let deactivate_ix = Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::DeactivateIssuer {}.data(),
        on_chain_cv::accounts::DeactivateIssuer {
            platform_config: platform_config_pda(),
            issuer_registry: issuer_registry_pda(issuer_kp.pubkey()),
            authority: platform_kp.pubkey(),
        }
        .to_account_metas(None),
    );
    send(&mut svm, &[deactivate_ix], &[&platform_kp], platform_kp.pubkey()).unwrap();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            1,
            "Test".to_string(),
            None,
            "ar://x".to_string(),
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "Should fail for deactivated issuer");
}

#[test]
fn test_issue_credential_invalid_level_zero() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            0,  // invalid: must be 1–5
            "Test".to_string(),
            None,
            "ar://x".to_string(),
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "Level 0 should be rejected");
}

#[test]
fn test_issue_credential_invalid_level_six() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            6,  // invalid: must be 1–5
            "Test".to_string(),
            None,
            "ar://x".to_string(),
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "Level 6 should be rejected");
}

#[test]
fn test_issue_credential_invalid_uri() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            3,
            "Test".to_string(),
            None,
            "https://ipfs.io/ipfs/Qm...".to_string(),  // not Arweave
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "Non-Arweave URI should be rejected");
}

#[test]
fn test_issue_credential_irys_uri_accepted() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Achievement,
            2,
            "Test Achievement".to_string(),
            None,
            "https://gateway.irys.xyz/some_tx_id".to_string(),
        )],
        &[&issuer_kp, &asset],
        issuer_kp.pubkey(),
    );
    assert!(res.is_ok(), "Irys URI should be accepted: {:?}", res.err());
}

#[test]
fn test_issue_credential_frozen_asset_transfer_rejected() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();
    let new_owner = Keypair::new();
    svm.airdrop(&recipient.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&new_owner.pubkey(), 1_000_000_000).unwrap();

    // Issue credential — produces a FreezeDelegate-frozen MPL-Core Asset
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            3,
            "Soulbound Test".to_string(),
            None,
            "ar://frozen_transfer_test".to_string(),
        )],
        &[&issuer_kp, &asset_kp],
        issuer_kp.pubkey(),
    )
    .expect("issue credential");

    // Attempt transfer — must fail because FreezeDelegate.frozen = true
    let transfer_ix = mpl_core::instructions::TransferV1 {
        asset: asset_kp.pubkey(),
        collection: Some(collection_kp.pubkey()),
        payer: recipient.pubkey(),
        authority: None,
        new_owner: new_owner.pubkey(),
        system_program: Some(system_program::ID),
        log_wrapper: None,
    }
    .instruction(mpl_core::instructions::TransferV1InstructionArgs {
        compression_proof: None,
    });

    let result = send(&mut svm, &[transfer_ix], &[&recipient], recipient.pubkey());
    assert!(result.is_err(), "Transfer of frozen soulbound Asset must be rejected by MPL-Core");
}

#[test]
fn test_issue_credential_wrong_authority() {
    // has_one = authority must reject a signer who is not the registered issuer authority
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();
    let recipient = Keypair::new();
    let asset = Keypair::new();

    let res = send(
        &mut svm,
        &[issue_credential_ix(
            attacker.pubkey(),  // wrong authority — not the registered issuer
            attacker.pubkey(),
            recipient.pubkey(),
            asset.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            3,
            "Fake Credential".to_string(),
            None,
            "ar://x".to_string(),
        )],
        &[&attacker, &asset],
        attacker.pubkey(),
    );
    assert!(res.is_err(), "has_one = authority must reject wrong signer");

    // Verify no credential was created under the real issuer's PDA
    let cred_pda = credential_pda(issuer_registry_pda(issuer_kp.pubkey()), recipient.pubkey(), 0);
    assert!(svm.get_account(&cred_pda).is_none(), "Credential PDA must not exist after rejected tx");
}

fn close_credential_ix(issuer_authority: Pubkey, credential_pda_key: Pubkey) -> Instruction {
    let issuer_pda = issuer_registry_pda(issuer_authority);
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::CloseCredential {}.data(),
        on_chain_cv::accounts::CloseCredential {
            issuer_registry: issuer_pda,
            credential: credential_pda_key,
            authority: issuer_authority,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn endorse_credential_ix(
    endorser: Pubkey,
    credential_pda_key: Pubkey,
) -> Instruction {
    // Endorsement PDA seeds: [b"endorsement", credential.key(), endorser.key()]
    let (endorsement_pda, _) = Pubkey::find_program_address(
        &[seeds::ENDORSEMENT, credential_pda_key.as_ref(), endorser.as_ref()],
        &on_chain_cv::id(),
    );
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::EndorseCredential {}.data(),
        on_chain_cv::accounts::EndorseCredential {
            credential: credential_pda_key,
            endorsement: endorsement_pda,
            endorser,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

// ── revoke_credential helpers + tests ────────────────────────────────────────

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

#[test]
fn test_revoke_credential_happy_path() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue a credential first
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            3,
            "Rust Developer".to_string(),
            None,
            "ar://test_revoke".to_string(),
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

    // Revoke it
    let res = send(
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
    );
    assert!(res.is_ok(), "revoke_credential failed: {:?}", res.err());

    // Credential PDA: revoked=true, revoked_at is set
    let account = svm.get_account(&cred_pda).expect("Credential PDA not found after revoke");
    let cred = Credential::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert!(cred.revoked, "credential.revoked must be true");
    assert!(cred.revoked_at.is_some(), "credential.revoked_at must be set");
    assert!(
        cred.revoked_at.unwrap() >= cred.issued_at,
        "revoked_at must be >= issued_at"
    );

    // MPL-Core burn reduces the asset account to a 1-byte tombstone (the collect mechanism
    // keeps lamports pending collection rather than zeroing them). We verify the asset
    // is no longer a valid MPL-Core asset: either the account is gone or its data is
    // collapsed to ≤1 byte (the burned-marker state the mpl-core fixture produces).
    let asset_after = svm.get_account(&asset_kp.pubkey());
    // NOTE: The mpl_core.so test fixture leaves a 1-byte tombstone rather than
    // closing the account fully. data.len() <= 1 is the correct burned check
    // for this fixture; production MPL-Core may close the account (None branch).
    let asset_is_burned = asset_after
        .as_ref()
        .map(|a| a.data.len() <= 1)
        .unwrap_or(true); // None also counts as burned
    assert!(asset_is_burned, "MPL-Core Asset must be burned after revoke");
}

#[test]
fn test_revoke_credential_wrong_issuer() {
    // A second issuer cannot revoke a credential issued by a different issuer
    let IssuerCtx { mut svm, issuer_kp, collection_kp, platform_kp } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue a credential as the first issuer
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            3,
            "Rust Developer".to_string(),
            None,
            "ar://test_revoke2".to_string(),
        )],
        &[&issuer_kp, &asset_kp],
        issuer_kp.pubkey(),
    )
    .expect("issue credential");

    // Register and verify a second issuer (the attacker)
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
    .expect("register attacker issuer");
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
    .expect("verify attacker issuer");

    let cred_pda = credential_pda(
        issuer_registry_pda(issuer_kp.pubkey()),
        recipient.pubkey(),
        0,
    );

    // Attacker tries to revoke using their own valid registry+collection pair.
    // The credential PDA is derived from the victim's issuer, so Anchor's PDA
    // re-derivation with attacker_registry as seed will fail before any CPI.
    let attacker_registry = issuer_registry_pda(attacker_kp.pubkey());
    let ix = Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::RevokeCredential {}.data(),
        on_chain_cv::accounts::RevokeCredential {
            issuer_registry: attacker_registry,
            credential: cred_pda,
            asset: asset_kp.pubkey(),
            issuer_collection: attacker_collection_kp.pubkey(), // valid for attacker's registry
            program_signer: program_signer_pda(),
            authority: attacker_kp.pubkey(),
            payer: attacker_kp.pubkey(),
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    let res = send(&mut svm, &[ix], &[&attacker_kp], attacker_kp.pubkey());
    assert!(res.is_err(), "Wrong issuer must not be able to revoke");

    // Asset and credential are untouched
    assert!(svm.get_account(&asset_kp.pubkey()).is_some(), "Asset must still exist");
    let account = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert!(!cred.revoked, "Credential must not be revoked");
}

#[test]
fn test_revoke_credential_already_revoked() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue then revoke once
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            2,
            "Already Revoked Test".to_string(),
            None,
            "ar://double_revoke".to_string(),
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
    .expect("first revoke must succeed");

    // Second revoke must fail — !credential.revoked constraint fires before CPI
    let res = send(
        &mut svm,
        &[revoke_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            cred_pda,
            asset_kp.pubkey(), // account no longer exists, but AlreadyRevoked fires first
            collection_kp.pubkey(),
        )],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "Second revoke must be rejected with AlreadyRevoked");
}

// ── close_credential tests ────────────────────────────────────────────────────

#[test]
fn test_close_credential_happy_path() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue credential
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Work,
            3,
            "Senior Rust Engineer".to_string(),
            None,
            "ar://close_happy_path".to_string(),
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

    // Revoke credential
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

    let balance_before = svm.get_balance(&issuer_kp.pubkey()).unwrap();

    // Close credential — should succeed (revoked=true, endorsement_count=0)
    let res = send(
        &mut svm,
        &[close_credential_ix(issuer_kp.pubkey(), cred_pda)],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_ok(), "close_credential failed: {:?}", res.err());

    // Credential PDA should be closed
    assert!(
        svm.get_account(&cred_pda).is_none(),
        "credential account should be closed"
    );

    // Authority should have received lamports back (minus tx fee)
    let balance_after = svm.get_balance(&issuer_kp.pubkey()).unwrap();
    assert!(
        balance_after > balance_before,
        "authority should receive rent lamports back (before={}, after={})",
        balance_before,
        balance_after,
    );
}

#[test]
fn test_close_credential_not_revoked() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue credential but do NOT revoke it
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Education,
            2,
            "Go Engineer".to_string(),
            None,
            "ar://close_not_revoked".to_string(),
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

    // Try to close without revoking — should fail with NotRevoked (6004)
    let res = send(
        &mut svm,
        &[close_credential_ix(issuer_kp.pubkey(), cred_pda)],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "close_credential should fail when credential is not revoked");
    let err_str = format!("{:?}", res.err());
    assert!(
        err_str.contains("6004") || err_str.contains("NotRevoked"),
        "unexpected error: {}",
        err_str,
    );
}

#[test]
fn test_close_credential_has_endorsements() {
    let IssuerCtx { mut svm, issuer_kp, collection_kp, .. } = setup_verified_issuer();
    let recipient = Keypair::new();
    let asset_kp = Keypair::new();

    // Issue credential
    send(
        &mut svm,
        &[issue_credential_ix(
            issuer_kp.pubkey(),
            issuer_kp.pubkey(),
            recipient.pubkey(),
            asset_kp.pubkey(),
            collection_kp.pubkey(),
            0,
            SkillCategory::Certificate,
            4,
            "Python Engineer".to_string(),
            None,
            "ar://close_has_endorsements".to_string(),
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

    // Endorse the credential with a different keypair (not the recipient)
    let endorser_kp = Keypair::new();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();
    send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("endorse credential");

    // Verify endorsement_count is now 1
    let account = svm.get_account(&cred_pda).expect("credential PDA must exist");
    let cred = Credential::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 1, "endorsement_count should be 1");

    // Revoke the credential
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

    // Try to close — should fail with HasEndorsements (6009)
    let res = send(
        &mut svm,
        &[close_credential_ix(issuer_kp.pubkey(), cred_pda)],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "close_credential should fail when there are active endorsements");
    let err_str = format!("{:?}", res.err());
    assert!(
        err_str.contains("6009") || err_str.contains("HasEndorsements"),
        "unexpected error: {}",
        err_str,
    );
}
