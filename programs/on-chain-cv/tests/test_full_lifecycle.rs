// programs/on-chain-cv/tests/test_full_lifecycle.rs
//
// Интеграционный тест: прогоняет все инструкции программы по порядку.
// Задача — убедиться, что весь жизненный цикл работает как единое целое,
// а не только отдельные инструкции в изоляции.
//
//   initialize_platform → register_issuer → verify_issuer → issue_credential
//   → endorse_credential → revoke_credential → close_credential (блокируется)
//   → перемотка времени +30 дней → close_endorsement → close_credential (успех)
//
// После каждого шага десериализуем состояние аккаунтов и проверяем поля —
// иначе тест доказывает только отсутствие паник, но не корректность данных.
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{
    constants::seeds,
    state::{Credential, Endorsement, IssuerRegistry, SkillCategory},
};
use solana_clock::Clock;
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
            level: 4,
            name: "Senior Rust Developer".to_string(),
            expires_at: None,
            metadata_uri: "ar://lifecycle_test_metadata".to_string(),
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

fn revoke_credential_ix(
    issuer_authority: Pubkey,
    payer: Pubkey,
    cred_pda: Pubkey,
    asset: Pubkey,
    collection: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::RevokeCredential {}.data(),
        on_chain_cv::accounts::RevokeCredential {
            issuer_registry: issuer_registry_pda(issuer_authority),
            credential: cred_pda,
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

fn close_credential_ix(issuer_authority: Pubkey, cred_pda: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::CloseCredential {}.data(),
        on_chain_cv::accounts::CloseCredential {
            issuer_registry: issuer_registry_pda(issuer_authority),
            credential: cred_pda,
            authority: issuer_authority,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

// ── Full lifecycle test ───────────────────────────────────────────────────────

/// Полный жизненный цикл: инициализация → регистрация → верификация → выдача →
/// эндорсирование → отзыв → попытка закрытия (блокируется) → закрытие
/// эндорсмента → успешное закрытие Credential.
///
/// Каждый шаг сопровождается десериализацией аккаунта и проверкой полей.
/// `svm.expire_blockhash()` перед повторным close_credential нужен, чтобы
/// LiteSVM не отверг структурно идентичную транзакцию как AlreadyProcessed.
#[test]
fn test_full_lifecycle() {
    let (mut svm, platform_kp) = setup_svm();
    let issuer_kp = Keypair::new();
    let collection_kp = Keypair::new();
    let recipient_kp = Keypair::new();
    let asset_kp = Keypair::new();
    let endorser_kp = Keypair::new();

    svm.airdrop(&issuer_kp.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&endorser_kp.pubkey(), 10_000_000_000).unwrap();

    let issuer_pda = issuer_registry_pda(issuer_kp.pubkey());
    let cred_pda = credential_pda(issuer_pda, recipient_kp.pubkey(), 0);
    let endorsement_pda_key = endorsement_pda(cred_pda, endorser_kp.pubkey());

    // ── Step 1: initialize_platform ───────────────────────────────────────────
    send(
        &mut svm,
        &[initialize_platform_ix(platform_kp.pubkey())],
        &[&platform_kp],
        platform_kp.pubkey(),
    )
    .expect("initialize_platform failed");

    // ── Step 2: register_issuer ───────────────────────────────────────────────
    send(
        &mut svm,
        &[register_issuer_ix(
            issuer_kp.pubkey(),
            "EPAM Systems".to_string(),
            "https://epam.com".to_string(),
        )],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    )
    .expect("register_issuer failed");

    // Verify issuer is registered but not yet verified
    let issuer_acc = svm.get_account(&issuer_pda).expect("issuer_registry PDA not found");
    let issuer = IssuerRegistry::try_deserialize(&mut issuer_acc.data.as_slice())
        .expect("deserialize IssuerRegistry");
    assert!(!issuer.is_verified, "issuer should not be verified yet");
    assert_eq!(issuer.credentials_issued, 0);

    // ── Step 3: verify_issuer ─────────────────────────────────────────────────
    send(
        &mut svm,
        &[verify_issuer_ix(
            platform_kp.pubkey(),
            issuer_kp.pubkey(),
            collection_kp.pubkey(),
            "ar://lifecycle_collection_uri".to_string(),
        )],
        &[&platform_kp, &collection_kp],
        platform_kp.pubkey(),
    )
    .expect("verify_issuer failed");

    // Verify issuer is now verified with collection set
    let issuer_acc = svm.get_account(&issuer_pda).unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut issuer_acc.data.as_slice()).unwrap();
    assert!(issuer.is_verified, "issuer should be verified");
    assert_eq!(issuer.collection, Some(collection_kp.pubkey()));

    // ── Step 4: issue_credential ──────────────────────────────────────────────
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
    .expect("issue_credential failed");

    // Verify credential was created correctly
    let cred_acc = svm.get_account(&cred_pda).expect("credential PDA not found");
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice())
        .expect("deserialize Credential");
    assert_eq!(cred.issuer, issuer_pda);
    assert_eq!(cred.recipient, recipient_kp.pubkey());
    assert_eq!(cred.core_asset, asset_kp.pubkey());
    assert!(!cred.revoked);
    assert_eq!(cred.endorsement_count, 0);
    assert_eq!(cred.index, 0);

    // Verify MPL-Core Asset was minted
    assert!(
        svm.get_account(&asset_kp.pubkey()).is_some(),
        "MPL-Core Asset account not found after issue"
    );

    // Verify issuer counter incremented
    let issuer_acc = svm.get_account(&issuer_pda).unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut issuer_acc.data.as_slice()).unwrap();
    assert_eq!(issuer.credentials_issued, 1);

    // ── Step 5: endorse_credential ────────────────────────────────────────────
    send(
        &mut svm,
        &[endorse_credential_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("endorse_credential failed");

    // ── Step 6: verify endorsement_count == 1 ────────────────────────────────
    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 1, "endorsement_count must be 1 after endorsement");

    // Verify endorsement PDA was created
    let end_acc = svm
        .get_account(&endorsement_pda_key)
        .expect("endorsement PDA not found");
    let end = Endorsement::try_deserialize(&mut end_acc.data.as_slice())
        .expect("deserialize Endorsement");
    assert_eq!(end.credential, cred_pda);
    assert_eq!(end.endorser, endorser_kp.pubkey());

    // ── Step 7: revoke_credential ─────────────────────────────────────────────
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
    .expect("revoke_credential failed");

    // Verify credential is revoked
    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert!(cred.revoked, "credential must be revoked");
    assert!(cred.revoked_at.is_some(), "revoked_at must be set");

    // ── Step 8: close_credential must fail (has endorsements) ─────────────────
    let res = send(
        &mut svm,
        &[close_credential_ix(issuer_kp.pubkey(), cred_pda)],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_err(), "close_credential must fail when endorsements exist");

    // Credential PDA must still exist
    assert!(
        svm.get_account(&cred_pda).is_some(),
        "credential PDA must still exist after blocked close"
    );

    // ── Step 9: time-travel 30 days + 1 second ────────────────────────────────
    let mut clock: Clock = svm.get_sysvar();
    clock.unix_timestamp += 30 * 24 * 60 * 60 + 1;
    svm.set_sysvar(&clock);

    // ── Step 10: close_endorsement ────────────────────────────────────────────
    send(
        &mut svm,
        &[close_endorsement_ix(endorser_kp.pubkey(), cred_pda)],
        &[&endorser_kp],
        endorser_kp.pubkey(),
    )
    .expect("close_endorsement must succeed after 30-day lockup");

    // Endorsement PDA must be closed
    assert!(
        svm.get_account(&endorsement_pda_key).is_none(),
        "endorsement PDA must be closed after close_endorsement"
    );

    // endorsement_count must be back to 0
    let cred_acc = svm.get_account(&cred_pda).unwrap();
    let cred = Credential::try_deserialize(&mut cred_acc.data.as_slice()).unwrap();
    assert_eq!(cred.endorsement_count, 0, "endorsement_count must be 0 after close_endorsement");

    // ── Step 11: close_credential must now succeed ────────────────────────────
    // Expire the blockhash so this transaction gets a different hash from the
    // failed attempt in step 8, avoiding AlreadyProcessed rejection.
    svm.expire_blockhash();
    let res = send(
        &mut svm,
        &[close_credential_ix(issuer_kp.pubkey(), cred_pda)],
        &[&issuer_kp],
        issuer_kp.pubkey(),
    );
    assert!(res.is_ok(), "close_credential must succeed: {:?}", res.err());

    // ── Step 12: verify credential PDA is gone ────────────────────────────────
    assert!(
        svm.get_account(&cred_pda).is_none(),
        "credential PDA must be closed after close_credential"
    );
}
