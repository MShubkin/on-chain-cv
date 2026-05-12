// programs/on-chain-cv/tests/test_endorsement.rs
//
// Time-travel note: `svm.set_sysvar::<Clock>()` is the LiteSVM equivalent of
// Surfpool's `surfnet_timeTravel` JSON-RPC cheatcode — both overwrite the
// Clock sysvar that `Clock::get()?` reads inside the program.
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, Space, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{
    constants::seeds,
    state::{Credential, Endorsement, SkillCategory},
};
use solana_clock::Clock;
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
    // Set a non-zero starting timestamp so Clock::get()?.unix_timestamp > 0 inside the program.
    let mut clock: Clock = svm.get_sysvar();
    clock.unix_timestamp = 1_700_000_000; // 2023-11-14, well in the past
    svm.set_sysvar(&clock);
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

fn endorsement_pda(credential: Pubkey, endorser: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[seeds::ENDORSEMENT, credential.as_ref(), endorser.as_ref()],
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
            name: "Endorsement Test Credential".to_string(),
            expires_at: None,
            metadata_uri: "ar://endorse_test".to_string(),
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
    credential: Pubkey,
    asset: Pubkey,
    collection: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::RevokeCredential {}.data(),
        on_chain_cv::accounts::RevokeCredential {
            issuer_registry: issuer_registry_pda(issuer_authority),
            credential,
            asset,
            issuer_collection: collection,
            program_signer: program_signer_pda(),
            authority: issuer_authority,
            payer: issuer_authority,
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn endorse_credential_ix(endorser: Pubkey, cred_pda: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::EndorseCredential {}.data(),
        on_chain_cv::accounts::EndorseCredential {
            credential: cred_pda,
            endorsement: endorsement_pda(cred_pda, endorser),
            endorser,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn close_endorsement_ix(endorser: Pubkey, cred_pda: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::CloseEndorsement {}.data(),
        on_chain_cv::accounts::CloseEndorsement {
            credential: cred_pda,
            endorsement: endorsement_pda(cred_pda, endorser),
            endorser,
        }
        .to_account_metas(None),
    )
}

// ── Shared setup ──────────────────────────────────────────────────────────────

struct EndorsementCtx {
    svm: LiteSVM,
    platform_kp: Keypair,
    issuer_kp: Keypair,
    collection_kp: Keypair,
    recipient_kp: Keypair,
    asset_kp: Keypair,
    cred_pda: Pubkey,
}

/// Spins up a full environment: platform → issuer verified → credential issued.
fn setup_endorsed_env() -> EndorsementCtx {
    let (mut svm, platform_kp) = setup_with_mpl_core();
    let issuer_kp = Keypair::new();
    let collection_kp = Keypair::new();
    let recipient_kp = Keypair::new();
    let asset_kp = Keypair::new();

    svm.airdrop(&issuer_kp.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&recipient_kp.pubkey(), 10_000_000_000).unwrap();

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
            "Endorsement Corp".to_string(),
            "https://endorse.example.com".to_string(),
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
            "ar://endorse_collection".to_string(),
        )],
        &[&platform_kp, &collection_kp],
        platform_kp.pubkey(),
    )
    .expect("verify_issuer");

    let issuer_pda = issuer_registry_pda(issuer_kp.pubkey());
    let cred_pda = credential_pda(issuer_pda, recipient_kp.pubkey(), 0);

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

    EndorsementCtx {
        svm,
        platform_kp,
        issuer_kp,
        collection_kp,
        recipient_kp,
        asset_kp,
        cred_pda,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_endorse_credential_happy_path() {
    let EndorsementCtx { mut svm, cred_pda, .. } = setup_endorsed_env();
    let endorser_kp = Keypair::new();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();

    send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("endorse_credential should succeed");

    // Credential.endorsement_count must be incremented to 1
    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 1, "endorsement_count must be 1");

    // Endorsement PDA must exist and have correct fields
    let end_pda = endorsement_pda(cred_pda, endorser_kp.pubkey());
    let end_acc = svm.get_account(&end_pda).expect("endorsement account must exist");
    let end = Endorsement::try_deserialize(&mut end_acc.data.as_slice()).unwrap();
    assert_eq!(end.credential, cred_pda, "endorsement.credential must match");
    assert_eq!(end.endorser, endorser_kp.pubkey(), "endorsement.endorser must match");
    assert_eq!(end.endorsed_at, 1_700_000_000, "endorsed_at must equal the initial clock value");
}

#[test]
fn test_endorse_credential_double_endorse() {
    let EndorsementCtx { mut svm, cred_pda, .. } = setup_endorsed_env();
    let endorser_kp = Keypair::new();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();

    send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("first endorse must succeed");

    // Second endorse from the same wallet must fail: `init` rejects existing account
    let res = send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    );
    assert!(res.is_err(), "double-endorse must be rejected");

    // Count stays at 1
    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 1, "endorsement_count must remain 1");
}

#[test]
fn test_endorse_credential_self_endorse() {
    let EndorsementCtx { mut svm, cred_pda, recipient_kp, .. } = setup_endorsed_env();

    // The recipient tries to endorse their own credential — must fail
    let res = send(
        &mut svm,
        &[endorse_credential_ix(recipient_kp.pubkey(), cred_pda)],
        &[&recipient_kp],
        recipient_kp.pubkey(),
    );
    let err = res.unwrap_err();
    assert!(
        format!("{err:?}").contains("SelfEndorsementForbidden"),
        "expected SelfEndorsementForbidden, got: {err:?}"
    );

    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 0, "endorsement_count must remain 0");
}

#[test]
fn test_endorse_credential_revoked() {
    let EndorsementCtx { mut svm, cred_pda, issuer_kp, collection_kp, asset_kp, .. } =
        setup_endorsed_env();
    let endorser_kp = Keypair::new();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();

    // Revoke the credential first
    send(
        &mut svm,
        &[revoke_credential_ix(
            issuer_kp.pubkey(),
            cred_pda,
            asset_kp.pubkey(),
            collection_kp.pubkey(),
        )],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    )
    .expect("revoke must succeed");

    // Now try to endorse the revoked credential — must fail with AlreadyRevoked
    let res = send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    );
    let err = res.unwrap_err();
    assert!(
        format!("{err:?}").contains("AlreadyRevoked"),
        "expected AlreadyRevoked, got: {err:?}"
    );
}

#[test]
fn test_close_endorsement_flash_close() {
    // "Flash close" — tries to reclaim deposit immediately after endorsing.
    // Must fail with EndorsementLocked because 30 days have not passed.
    let EndorsementCtx { mut svm, cred_pda, .. } = setup_endorsed_env();
    let endorser_kp = Keypair::new();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();

    send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("endorse must succeed");

    // Attempt immediate close — lockup has not elapsed
    let res = send(
        &mut svm,
        &[close_endorsement_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    );
    let err = res.unwrap_err();
    assert!(
        format!("{err:?}").contains("EndorsementLocked"),
        "expected EndorsementLocked, got: {err:?}"
    );
}

#[test]
fn test_close_endorsement_after_lockup() {
    // Time travel: advance the LiteSVM clock by 30 days + 1 second.
    // This is the in-process equivalent of Surfpool's `surfnet_timeTravel` cheatcode.
    let EndorsementCtx { mut svm, cred_pda, .. } = setup_endorsed_env();
    let endorser_kp = Keypair::new();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();

    send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("endorse must succeed");

    let balance_before = svm
        .get_account(&endorser_kp.pubkey())
        .map(|a| a.lamports)
        .unwrap_or(0);

    // ── Time travel: advance clock 30 days + 1 second ────────────────────────
    let mut clock: Clock = svm.get_sysvar();
    clock.unix_timestamp += 30 * 24 * 60 * 60 + 1;
    svm.set_sysvar(&clock);
    // ─────────────────────────────────────────────────────────────────────────

    send(
        &mut svm,
        &[close_endorsement_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("close_endorsement must succeed after 30-day lockup");

    // Endorsement PDA must be closed (no account)
    let end_pda = endorsement_pda(cred_pda, endorser_kp.pubkey());
    assert!(
        svm.get_account(&end_pda).is_none(),
        "endorsement account must be closed"
    );

    // Credential endorsement_count must be back to 0
    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 0, "endorsement_count must be 0 after close");

    // Endorser balance must have increased by approximately the rent deposit
    let balance_after = svm
        .get_account(&endorser_kp.pubkey())
        .map(|a| a.lamports)
        .unwrap_or(0);
    let rent = svm.minimum_balance_for_rent_exemption(8 + Endorsement::INIT_SPACE);
    assert!(
        balance_after >= balance_before + rent - 10_000,
        "endorser must receive full rent back: expected ~{rent} lamports, before={balance_before}, after={balance_after}"
    );
}
