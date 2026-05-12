// Тесты для initialize_platform и transfer_platform_authority
//
// Используем LiteSVM — in-process эмулятор Solana, работает без solana-test-validator
// Программа загружается из скомпилированного .so, поэтому перед запуском нужен `anchor build`
use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use litesvm::LiteSVM;
use on_chain_cv::{constants::seeds, state::PlatformConfig};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

/// Поднимает LiteSVM с задеплоенной программой и выдаёт payer'у 10 SOL
/// Каждый тест вызывает setup() заново — изоляция полная, состояние между тестами не протекает
fn setup() -> (LiteSVM, Keypair) {
    let program_id = on_chain_cv::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    // include_bytes! вшивает .so прямо в бинарник теста на этапе компиляции —
    // никакого чтения с диска в рантайме
    let bytes = include_bytes!("../../../target/deploy/on_chain_cv.so");
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    (svm, payer)
}

/// Вычисляет адрес PlatformConfig PDA
/// Детерминировано: одни и те же seeds + program_id всегда дают один адрес
fn platform_config_pda() -> Pubkey {
    Pubkey::find_program_address(&[seeds::PLATFORM_CONFIG], &on_chain_cv::id()).0
}

/// Собирает инструкцию initialize_platform для переданного authority
/// Anchor генерирует типы `accounts::InitializePlatform` и `instruction::InitializePlatform`
/// из IDL — поэтому структура аккаунтов здесь совпадает с тем, что в программе
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

/// Удачный кейс: проверяем, что после успешного вызова initialize_platform
/// PlatformConfig создан и поле authority совпадает с подписантом
#[test]
fn test_initialize_platform_happy_path() {
    let (mut svm, authority) = setup();
    let authority_pubkey = authority.pubkey();

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[initialize_platform_ix(authority_pubkey)],
        Some(&authority_pubkey),
        &blockhash,
    );
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[authority]).unwrap();
    let res = svm.send_transaction(tx);
    assert!(res.is_ok(), "initialize_platform failed: {:?}", res.err());

    // Читаем аккаунт напрямую из SVM и десериализуем байты в структуру
    let account = svm
        .get_account(&platform_config_pda())
        .expect("PlatformConfig account not found");
    let config = PlatformConfig::try_deserialize(&mut account.data.as_slice())
        .expect("Failed to deserialize PlatformConfig");

    assert_eq!(config.authority, authority_pubkey);
}

/// Негативный кейс: посторонний не может забрать себе права на платформу
/// `has_one = authority` в #[derive(Accounts)] должен отклонить транзакцию
/// ещё до вызова хендлера
#[test]
fn test_transfer_platform_authority_unauthorized() {
    let (mut svm, authority) = setup();
    let impostor = Keypair::new();
    let new_authority = Keypair::new();
    svm.airdrop(&impostor.pubkey(), 1_000_000_000).unwrap();

    // Сначала инициализируем платформу под легитимным authority
    let authority_pubkey = authority.pubkey();
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[initialize_platform_ix(authority_pubkey)],
        Some(&authority_pubkey),
        &blockhash,
    );
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[authority]).unwrap();
    svm.send_transaction(tx).expect("Initial setup failed");

    // Пытаемся передать права от имени impostor'а — должно упасть с Unauthorized
    let transfer_ix = Instruction::new_with_bytes(
        on_chain_cv::id(),
        &on_chain_cv::instruction::TransferPlatformAuthority {}.data(),
        on_chain_cv::accounts::TransferPlatformAuthority {
            platform_config: platform_config_pda(),
            // impostor.pubkey() не совпадает с platform_config.authority — has_one это поймает
            authority: impostor.pubkey(),
            new_authority: new_authority.pubkey(),
        }
        .to_account_metas(None),
    );

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(
        &[transfer_ix],
        Some(&impostor.pubkey()),
        &blockhash,
    );
    let tx =
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[impostor]).unwrap();
    let res = svm.send_transaction(tx);
    assert!(res.is_err(), "Unauthorized transfer should have been rejected");
}
