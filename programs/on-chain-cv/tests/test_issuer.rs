use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{constants::seeds, state::IssuerRegistry};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

fn setup() -> (LiteSVM, Keypair) {
    let program_id = on_chain_cv::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!("../../../target/deploy/on_chain_cv.so");
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    (svm, payer)
}

fn setup_with_mpl_core() -> (LiteSVM, Keypair) {
    let (mut svm, payer) = setup();
    let mpl_core_id: Pubkey = "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
        .parse()
        .unwrap();
    let mpl_core_bytes = include_bytes!("fixtures/mpl_core.so");
    svm.add_program(mpl_core_id, mpl_core_bytes).unwrap();
    (svm, payer)
}

fn issuer_registry_pda(authority: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[seeds::ISSUER_REGISTRY, authority.as_ref()],
        &on_chain_cv::id(),
    )
    .0
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

// ── helpers shared by verify + deactivate + update tests ─────────────────────

fn platform_config_pda() -> Pubkey {
    Pubkey::find_program_address(&[seeds::PLATFORM_CONFIG], &on_chain_cv::id()).0
}

fn program_signer_pda() -> Pubkey {
    Pubkey::find_program_address(&[seeds::PROGRAM_SIGNER], &on_chain_cv::id()).0
}

fn mpl_core_id() -> Pubkey {
    "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
        .parse()
        .unwrap()
}

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

fn verify_issuer_ix(
    platform_authority: Pubkey,
    issuer_authority: Pubkey,
    collection: Pubkey,
    collection_uri: String,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::VerifyIssuer { collection_uri }.data(),
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

// ── verify tests ──────────────────────────────────────────────────────────────

#[test]
fn test_verify_issuer_happy_path() {
    let (mut svm, platform_authority) = setup_with_mpl_core();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    let collection = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();

    // 1. Initialize platform
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[initialize_platform_ix(platform_pubkey)],
        Some(&platform_pubkey),
        &blockhash,
    );
    svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(msg),
            &[platform_authority.insecure_clone()],
        )
        .unwrap(),
    )
    .expect("initialize_platform failed");

    // 2. Register issuer
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[register_issuer_ix(
            issuer_authority.pubkey(),
            "EPAM".to_string(),
            "https://epam.com".to_string(),
        )],
        Some(&issuer_authority.pubkey()),
        &blockhash,
    );
    svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(msg),
            &[issuer_authority.insecure_clone()],
        )
        .unwrap(),
    )
    .expect("register_issuer failed");

    // 3. Verify issuer — platform_authority AND collection keypair must sign
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[verify_issuer_ix(
            platform_pubkey,
            issuer_authority.pubkey(),
            collection.pubkey(),
            "ar://some_collection_uri".to_string(),
        )],
        Some(&platform_pubkey),
        &blockhash,
    );
    let res = svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(msg),
            &[platform_authority, collection.insecure_clone()],
        )
        .unwrap(),
    );
    assert!(res.is_ok(), "verify_issuer failed: {:?}", res.err());

    let account = svm
        .get_account(&issuer_registry_pda(issuer_authority.pubkey()))
        .unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert!(issuer.is_verified);
    assert_eq!(issuer.verified_by, Some(platform_pubkey));
    assert_eq!(issuer.collection, Some(collection.pubkey()));
}

#[test]
fn test_verify_issuer_idempotent() {
    let (mut svm, platform_authority) = setup_with_mpl_core();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    let collection = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();

    // Setup: initialize + register
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[initialize_platform_ix(platform_pubkey)], Some(&platform_pubkey), &bh)), &[platform_authority.insecure_clone()]).unwrap()).unwrap();
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[register_issuer_ix(issuer_authority.pubkey(), "EPAM".to_string(), "https://epam.com".to_string())], Some(&issuer_authority.pubkey()), &bh)), &[issuer_authority.insecure_clone()]).unwrap()).unwrap();

    // First verify
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[verify_issuer_ix(platform_pubkey, issuer_authority.pubkey(), collection.pubkey(), "ar://uri1".to_string())], Some(&platform_pubkey), &bh)), &[platform_authority.insecure_clone(), collection.insecure_clone()]).unwrap()).unwrap();

    // Second verify with same collection — must succeed without re-creating Collection
    let collection_pubkey = collection.pubkey();
    let issuer_pubkey = issuer_authority.pubkey();
    let bh = svm.latest_blockhash();
    let res = svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(Message::new_with_blockhash(
                &[verify_issuer_ix(
                    platform_pubkey,
                    issuer_pubkey,
                    collection_pubkey,
                    "ar://uri2".to_string(),
                )],
                Some(&platform_pubkey),
                &bh,
            )),
            &[platform_authority, collection],
        )
        .unwrap(),
    );
    assert!(res.is_ok(), "Second verify (idempotent) failed: {:?}", res.err());

    let account = svm.get_account(&issuer_registry_pda(issuer_pubkey)).unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(issuer.collection, Some(collection_pubkey));
}

#[test]
fn test_verify_issuer_unauthorized() {
    let (mut svm, platform_authority) = setup_with_mpl_core();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    let impostor = Keypair::new();
    let collection = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&impostor.pubkey(), 1_000_000_000).unwrap();

    // Setup
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[initialize_platform_ix(platform_pubkey)], Some(&platform_pubkey), &bh)), &[platform_authority]).unwrap()).unwrap();
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[register_issuer_ix(issuer_authority.pubkey(), "EPAM".to_string(), "https://epam.com".to_string())], Some(&issuer_authority.pubkey()), &bh)), &[issuer_authority.insecure_clone()]).unwrap()).unwrap();

    // Impostor tries to verify — has_one = authority must reject
    let ix = Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::VerifyIssuer {
            collection_uri: "ar://x".to_string(),
        }
        .data(),
        on_chain_cv::accounts::VerifyIssuer {
            platform_config: platform_config_pda(),
            issuer_registry: issuer_registry_pda(issuer_authority.pubkey()),
            program_signer: program_signer_pda(),
            collection: collection.pubkey(),
            authority: impostor.pubkey(),
            payer: impostor.pubkey(),
            mpl_core_program: mpl_core_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    let bh = svm.latest_blockhash();
    let res = svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(Message::new_with_blockhash(
                &[ix],
                Some(&impostor.pubkey()),
                &bh,
            )),
            &[impostor, collection],
        )
        .unwrap(),
    );
    assert!(res.is_err(), "Unauthorized verify should be rejected");
}

#[test]
fn test_register_issuer_happy_path() {
    let (mut svm, authority) = setup();
    let authority_pubkey = authority.pubkey();

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[register_issuer_ix(
            authority_pubkey,
            "EPAM Systems".to_string(),
            "https://epam.com".to_string(),
        )],
        Some(&authority_pubkey),
        &blockhash,
    );
    let tx =
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[authority]).unwrap();
    let res = svm.send_transaction(tx);
    assert!(res.is_ok(), "register_issuer failed: {:?}", res.err());

    let account = svm
        .get_account(&issuer_registry_pda(authority_pubkey))
        .expect("IssuerRegistry not found");
    let issuer = IssuerRegistry::try_deserialize(&mut account.data.as_slice())
        .expect("Failed to deserialize IssuerRegistry");

    assert_eq!(issuer.authority, authority_pubkey);
    assert_eq!(issuer.name, "EPAM Systems");
    assert_eq!(issuer.website, "https://epam.com");
    assert!(!issuer.is_verified);
    assert!(issuer.collection.is_none());
    assert_eq!(issuer.credentials_issued, 0);
}

#[test]
fn test_register_issuer_already_exists() {
    let (mut svm, authority) = setup();
    let authority_pubkey = authority.pubkey();

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[register_issuer_ix(
            authority_pubkey,
            "EPAM".to_string(),
            "https://epam.com".to_string(),
        )],
        Some(&authority_pubkey),
        &blockhash,
    );
    svm.send_transaction(
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[authority.insecure_clone()])
            .unwrap(),
    )
    .expect("First registration failed");

    // Second registration with same authority must fail (account already in use)
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[register_issuer_ix(
            authority_pubkey,
            "EPAM".to_string(),
            "https://epam.com".to_string(),
        )],
        Some(&authority_pubkey),
        &blockhash,
    );
    let res = svm.send_transaction(
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[authority]).unwrap(),
    );
    assert!(res.is_err(), "Duplicate registration should fail");
}

// ── deactivate tests ──────────────────────────────────────────────────────────

fn deactivate_issuer_ix(platform_authority: Pubkey, issuer_authority: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::DeactivateIssuer {}.data(),
        on_chain_cv::accounts::DeactivateIssuer {
            platform_config: platform_config_pda(),
            issuer_registry: issuer_registry_pda(issuer_authority),
            authority: platform_authority,
        }
        .to_account_metas(None),
    )
}

#[test]
fn test_deactivate_issuer_happy_path() {
    let (mut svm, platform_authority) = setup();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();

    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[initialize_platform_ix(platform_pubkey)], Some(&platform_pubkey), &bh)), &[platform_authority.insecure_clone()]).unwrap()).unwrap();
    let issuer_pubkey = issuer_authority.pubkey();
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[register_issuer_ix(issuer_pubkey, "EPAM".to_string(), "https://epam.com".to_string())], Some(&issuer_pubkey), &bh)), &[issuer_authority]).unwrap()).unwrap();

    let bh = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[deactivate_issuer_ix(platform_pubkey, issuer_pubkey)],
        Some(&platform_pubkey),
        &bh,
    );
    let res = svm.send_transaction(
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[platform_authority])
            .unwrap(),
    );
    assert!(res.is_ok(), "deactivate_issuer failed: {:?}", res.err());

    let account = svm.get_account(&issuer_registry_pda(issuer_pubkey)).unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert!(issuer.deactivated_at.is_some());
}

#[test]
fn test_deactivate_issuer_unauthorized() {
    let (mut svm, platform_authority) = setup();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    let impostor = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&impostor.pubkey(), 1_000_000_000).unwrap();

    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[initialize_platform_ix(platform_pubkey)], Some(&platform_pubkey), &bh)), &[platform_authority]).unwrap()).unwrap();
    let issuer_pubkey = issuer_authority.pubkey();
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[register_issuer_ix(issuer_pubkey, "EPAM".to_string(), "https://epam.com".to_string())], Some(&issuer_pubkey), &bh)), &[issuer_authority]).unwrap()).unwrap();

    let ix = Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::DeactivateIssuer {}.data(),
        on_chain_cv::accounts::DeactivateIssuer {
            platform_config: platform_config_pda(),
            issuer_registry: issuer_registry_pda(issuer_pubkey),
            authority: impostor.pubkey(),
        }
        .to_account_metas(None),
    );
    let bh = svm.latest_blockhash();
    let res = svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(Message::new_with_blockhash(&[ix], Some(&impostor.pubkey()), &bh)),
            &[impostor],
        )
        .unwrap(),
    );
    assert!(res.is_err(), "Unauthorized deactivate should be rejected");
}

// ── update_issuer_metadata tests ──────────────────────────────────────────────

fn update_issuer_metadata_ix(
    authority: Pubkey,
    collection: Pubkey,
    new_name: String,
    new_website: String,
) -> Instruction {
    Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::UpdateIssuerMetadata {
            new_name,
            new_website,
        }
        .data(),
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

#[test]
fn test_update_issuer_metadata_website_only() {
    let (mut svm, platform_authority) = setup();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();

    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[initialize_platform_ix(platform_pubkey)], Some(&platform_pubkey), &bh)), &[platform_authority]).unwrap()).unwrap();
    let issuer_pubkey = issuer_authority.pubkey();
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[register_issuer_ix(issuer_pubkey, "EPAM".to_string(), "https://epam.com".to_string())], Some(&issuer_pubkey), &bh)), &[issuer_authority.insecure_clone()]).unwrap()).unwrap();

    let bh = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[update_issuer_metadata_ix(
            issuer_pubkey,
            issuer_pubkey, // collection ещё не создана (is_verified=false); любой writable pubkey проходит unwrap_or(true)
            "EPAM".to_string(),
            "https://new.epam.com".to_string(),
        )],
        Some(&issuer_pubkey),
        &bh,
    );
    let res = svm.send_transaction(
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[issuer_authority]).unwrap(),
    );
    assert!(res.is_ok(), "update_issuer_metadata failed: {:?}", res.err());

    let account = svm.get_account(&issuer_registry_pda(issuer_pubkey)).unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(issuer.website, "https://new.epam.com");
    assert_eq!(issuer.name, "EPAM");
}

#[test]
fn test_update_issuer_metadata_name_change_resets_verified() {
    let (mut svm, platform_authority) = setup();
    let platform_pubkey = platform_authority.pubkey();
    let issuer_authority = Keypair::new();
    svm.airdrop(&issuer_authority.pubkey(), 1_000_000_000).unwrap();

    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[initialize_platform_ix(platform_pubkey)], Some(&platform_pubkey), &bh)), &[platform_authority]).unwrap()).unwrap();
    let issuer_pubkey = issuer_authority.pubkey();
    let bh = svm.latest_blockhash();
    svm.send_transaction(VersionedTransaction::try_new(VersionedMessage::Legacy(Message::new_with_blockhash(&[register_issuer_ix(issuer_pubkey, "EPAM".to_string(), "https://epam.com".to_string())], Some(&issuer_pubkey), &bh)), &[issuer_authority.insecure_clone()]).unwrap()).unwrap();

    let bh = svm.latest_blockhash();
    let res = svm.send_transaction(
        VersionedTransaction::try_new(
            VersionedMessage::Legacy(Message::new_with_blockhash(
                &[update_issuer_metadata_ix(
                    issuer_pubkey,
                    issuer_pubkey, // collection ещё не создана; любой writable pubkey проходит unwrap_or(true)
                    "EPAM Systems".to_string(),
                    "https://epam.com".to_string(),
                )],
                Some(&issuer_pubkey),
                &bh,
            )),
            &[issuer_authority],
        )
        .unwrap(),
    );
    assert!(res.is_ok(), "update_issuer_metadata name change failed: {:?}", res.err());

    let account = svm.get_account(&issuer_registry_pda(issuer_pubkey)).unwrap();
    let issuer = IssuerRegistry::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(issuer.name, "EPAM Systems");
    assert!(!issuer.is_verified);
    assert!(issuer.verified_by.is_none());
    assert!(issuer.verified_at.is_none());
}
