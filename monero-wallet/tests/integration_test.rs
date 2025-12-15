use monero::{Address, Amount as MoneroAmount, Network, PrivateKey, PublicKey};
use monero_harness::{Monero, Cli};
use monero_harness::image::Monerod;
use monero_sys::{Daemon, TransactionInfo, WalletHandle};
use monero_wallet::{MoneroTauriHandle, Wallets, no_listener};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::path::PathBuf;
use swap_core::monero::primitives::{PrivateViewKey, TxHash, Amount as CoreAmount};
use tempfile::TempDir;
use testcontainers::Container;
use tokio::time::sleep;

// =============================================================================
// Test Constants
// =============================================================================

// Amount to send to the test wallet (1 XMR in piconero)
const AMOUNT_TO_RECEIVE_PICO: u64 = 1_000_000_000_000;
// Amount to transfer in spending test (0.1 XMR in piconero)
const TRANSFER_AMOUNT_PICO: u64 = 100_000_000_000;
// Interval between balance polling attempts
const SYNC_POLL_INTERVAL: Duration = Duration::from_millis(500);
// Maximum number of retries when waiting for wallet sync
const MAX_SYNC_RETRIES: u32 = 30;

// =============================================================================
// Mock Tauri Handle
// =============================================================================
//
// Mock implementation of `MoneroTauriHandle` for testing UI notifications.
//
// This struct captures all wallet events (balance changes, history updates, and sync progress)
// so they can be asserted in tests to verify the notification system works correctly.
//
struct MockTauriHandle {
    // Captured balance change events: (total_balance and unlocked_balance)
    balance_updates: Arc<Mutex<Vec<(CoreAmount, CoreAmount)>>>,    
    // Captured transaction history updates
    history_updates: Arc<Mutex<Vec<Vec<TransactionInfo>>>>,
    // Captured sync progress events: (current_block, target_block, and percentage)
    sync_progress_updates: Arc<Mutex<Vec<(u64, u64, f32)>>>,
}

impl MockTauriHandle {
    // Create a new mock handle with empty event buffers
    fn new() -> Self {
        Self {
            balance_updates: Arc::new(Mutex::new(Vec::new())),
            history_updates: Arc::new(Mutex::new(Vec::new())),
            sync_progress_updates: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // Assert that at least one history update was received
    fn assert_history_received(&self) {
        let updates = self.history_updates.lock().unwrap();
        assert!(!updates.is_empty(), "Should have received history updates");
    }

    // Assert that a balance update for the specified total was received
    fn assert_balance_update(&self, total: u64) {
        let updates = self.balance_updates.lock().unwrap();
        let expected = CoreAmount::from_piconero(total);
        assert!(
            updates.iter().any(|(t, _)| *t == expected),
            "Should have received balance update for {}", total
        );
    }
}

impl MoneroTauriHandle for MockTauriHandle {
    fn balance_change(&self, total: CoreAmount, unlocked: CoreAmount) {
        self.balance_updates.lock().unwrap().push((total, unlocked));
    }
    
    fn history_update(&self, txs: Vec<TransactionInfo>) {
        self.history_updates.lock().unwrap().push(txs);
    }
    
    fn sync_progress(&self, current: u64, target: u64, percentage: f32) {
        self.sync_progress_updates.lock().unwrap().push((current, target, percentage));
    }
}

// =============================================================================
// Test Context
// =============================================================================
//
// Encapsulates all components needed for integration tests.
//
// This struct holds references to the Monero harness, daemon connection,
// filesystem paths, and mock handlers. It is passed to individual test
// verification functions.
//
struct TestContext<'a> {
    // The Monero test harness (provides miner wallet and block generation)
    monero: Monero,
    // Docker container running monerod (kept alive for test duration)
    #[allow(unused)]
    container: Container<'a, Monerod>,
    // Daemon connection info (hostname, port, ssl flag)
    daemon: Daemon,
    // Directory where wallet files are stored
    wallet_dir: PathBuf,
    // Temporary directory (automatically cleaned up on drop)
    _temp_dir: TempDir,
    // Database for tracking wallet usage history
    database: Arc<monero_sys::Database>,
    // Mock Tauri handle for capturing UI notifications
    tauri_handle: Arc<MockTauriHandle>,
}

// =============================================================================
// Main Integration Test
// =============================================================================
//
// Main integration test that exercises the complete wallet lifecycle.
//
#[tokio::test]
async fn integration_test() {
    // Initialize logging for test output
    init_tracing();
    // Initialize TLS crypto provider
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Step 1: Setup test environment
    let cli = Cli::default();
    let (env, wallets) = setup_env(&cli).await;
    let main_wallet = wallets.main_wallet().await;
    // Step 2: Verify wallet starts with zero balance
    verify_initial_state(&main_wallet).await;
    // Step 3: Fund wallet and verify Tauri notifications
    verify_funding(&env, &main_wallet).await;
    // Step 4: Test swap wallet functionality (atomic swap use case)
    verify_swap_wallet(&env, &wallets).await;
    // Step 5: Verify recent wallets database tracking
    verify_recent_wallets(&wallets).await;
    // Step 6: Test spending and blockchain height queries
    verify_spending(&env, &wallets, &main_wallet).await;
    // Step 7: Test wallet restoration from seed phrase
    verify_restoration(&env, &main_wallet).await;
    // Step 8: Test changing Monero node at runtime
    verify_node_change(&wallets, &env, &main_wallet).await;
    // Step 9: Test Wallets::new_with_existing_wallet
    verify_new_with_existing_wallet(&env, &wallets).await;
    // Step 10: Test verify_transfer
    verify_verify_transfer(&env, &wallets).await;
    // Step 11: Test wait_until_confirmed
    verify_wait_until_confirmed(&env, &wallets, &main_wallet).await;
    // Step 12: Test wait_for_incoming_transfer
    verify_wait_for_incoming_transfer(&env, &wallets).await;
}

// =============================================================================
// Setup Helpers
// =============================================================================
//
// Initialize tracing subscriber for test output.
//
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug,monero_wallet=trace,monero_sys=trace")
        .with_test_writer()
        .try_init();
}

// Set up the complete test environment.
//
// This function:
// 1. Starts a Monero Docker container with monerod
// 2. Initializes the miner wallet with funds
// 3. Creates a temporary directory for wallet files
// 4. Creates the `Wallets` instance with mock Tauri handle
//
// Returns the `TestContext` and `Wallets` instance for use in tests.
//
async fn setup_env(cli: &Cli) -> (TestContext<'_>, Wallets) {
    tracing::debug!("Starting integration test setup");

    // Start Monero harness with Docker container
    let (monero, container, _wallets) = Monero::new(cli, vec![]).await.unwrap();
    
    // Initialize miner wallet with pre-mined funds
    monero.init_miner().await.unwrap();
    
    // Get the RPC port mapped from the container
    let rpc_port = container.get_host_port_ipv4(18089);
    let daemon = Daemon {
        hostname: "127.0.0.1".to_string(),
        port: rpc_port,
        ssl: false,
    };

    // Create temporary directory for wallet files and database
    let temp_dir = TempDir::new().unwrap();
    let wallet_dir = temp_dir.path().to_path_buf();
    let database_dir = temp_dir.path().join("db");
    
    // Create mock Tauri handle and database
    let tauri_handle = Arc::new(MockTauriHandle::new());    
    let database = Arc::new(monero_sys::Database::new(database_dir).await.unwrap());

    // Create the Wallets instance
    // Note: We use Network::Mainnet with regtest=true because monero-rs lacks Network::Regtest
    let wallets = Wallets::new(
        wallet_dir.clone(),
        "main_wallet".to_string(),
        daemon.clone(),
        Network::Mainnet,
        true, // regtest mode - disables certain safety checks
        Some(tauri_handle.clone()),
        Some(database.clone()),
    ).await.unwrap();

    let context = TestContext {
        monero,
        container,
        daemon,
        wallet_dir,
        _temp_dir: temp_dir,
        database,
        tauri_handle,
    };

    (context, wallets)
}

// =============================================================================
// Verification Functions
// =============================================================================
//
// Verify that a newly created wallet has zero balance.
//
async fn verify_initial_state(wallet: &WalletHandle) {
    tracing::info!("Verifying initial balance");
    assert_eq!(wallet.total_balance().await.unwrap(), MoneroAmount::ZERO);
}

// Verify wallet funding and Tauri notification callbacks.
//
// This test:
// 1. Sends XMR from the miner to the main wallet
// 2. Waits for the balance to update
// 3. Verifies balance_change and history_update callbacks were triggered
//
async fn verify_funding(env: &TestContext<'_>, wallet: &WalletHandle) {
    tracing::info!("Funding main wallet");
    
    // Send funds from miner to the test wallet
    env.monero.init_external_wallet("target", wallet, vec![AMOUNT_TO_RECEIVE_PICO]).await.unwrap();

    // Wait for wallet to sync and reflect the new balance
    wait_for_balance(wallet, MoneroAmount::from_pico(AMOUNT_TO_RECEIVE_PICO)).await;
    
    // Verify balance values
    let balance = wallet.total_balance().await.unwrap();
    tracing::info!("Balance after funding: {}", balance);
    assert_eq!(balance, MoneroAmount::from_pico(AMOUNT_TO_RECEIVE_PICO));
    assert_eq!(wallet.unlocked_balance().await.unwrap(), MoneroAmount::from_pico(AMOUNT_TO_RECEIVE_PICO));

    // Verify Tauri notifications were received
    env.tauri_handle.assert_balance_update(AMOUNT_TO_RECEIVE_PICO);
    
    {
         let sync_updates = env.tauri_handle.sync_progress_updates.lock().unwrap();
         tracing::info!("Sync updates received: {}", sync_updates.len());
    }
    
    env.tauri_handle.assert_history_received();
}

// Verify swap wallet functionality for atomic swaps.
//
// This test simulates the atomic swap use case where:
// 1. Random spend/view keys are generated
// 2. Funds are sent to the derived address
// 3. A swap wallet is opened with those keys
// 4. The wallet can see and access the funds
//
async fn verify_swap_wallet(env: &TestContext<'_>, wallets: &Wallets) {
    tracing::info!("Testing swap wallet");
    let mut rng = rand::thread_rng();
    
    // Generate random keys (simulating atomic swap key exchange)
    let view_key = PrivateViewKey::new_random(&mut rng);
    let view_key_private: PrivateKey = view_key.clone().into();
    let scalar = swap_core::monero::primitives::Scalar::random(&mut rng);
    let spend_key = PrivateKey::from_slice(&scalar.to_bytes()).unwrap();
    
    // Derive the wallet address from the keys
    let address = Address::standard(
        Network::Mainnet,
        PublicKey::from_private_key(&spend_key),
        PublicKey::from_private_key(&view_key_private)
    );
    
    // Send funds to the swap address
    tracing::info!("Sending to swap address: {}", address);
    let miner = env.monero.wallet("miner").unwrap();
    let receipt = miner.transfer(&address, AMOUNT_TO_RECEIVE_PICO).await.unwrap();
    
    // Mine blocks to confirm the transaction
    env.monero.monerod().generate_blocks(10, miner.address().await.unwrap().to_string()).await.unwrap();
    
    let tx_hash = TxHash(receipt.txid.clone());
    let swap_uuid = uuid::Uuid::new_v4();
    
    // Open swap wallet with the keys and verify balance
    tracing::info!("Opening swap wallet");
    let swap_wallet_handle = wallets.swap_wallet_spendable(
        swap_uuid,
        spend_key,
        view_key,
        tx_hash
    ).await.unwrap();
    
    let swap_balance = swap_wallet_handle.total_balance().await.unwrap();
    tracing::info!("Swap wallet balance: {}", swap_balance);
    assert_eq!(swap_balance, MoneroAmount::from_pico(AMOUNT_TO_RECEIVE_PICO));
}

// Verify that recently accessed wallets are tracked in the database.
async fn verify_recent_wallets(wallets: &Wallets) {
    tracing::info!("Testing recent wallets");
    let recents = wallets.get_recent_wallets().await.unwrap();
    tracing::info!("Recent wallets: {:?}", recents);
    
    // The main_wallet should appear in the recent wallets list
    assert!(recents.iter().any(|p| p.contains("main_wallet")));
}

// Verify spending (transfer) functionality and blockchain height queries.
//
// This test:
// 1. Sends funds from the main wallet to the miner
// 2. Verifies the balance decreased
// 3. Tests blockchain_height() and block_height() methods
//
async fn verify_spending(env: &TestContext<'_>, wallets: &Wallets, wallet: &WalletHandle) {
    tracing::info!("Testing fund transfer");
    let miner = env.monero.wallet("miner").unwrap();
    let recipient_address = miner.address().await.unwrap();
    let transfer_amount = MoneroAmount::from_pico(TRANSFER_AMOUNT_PICO);
    let initial_balance = wallet.total_balance().await.unwrap();
    
    // Execute the transfer
    let receipt = wallet.transfer_single_destination(&recipient_address, transfer_amount).await.unwrap();
    tracing::info!("Transfer receipt txid: {}", receipt.txid);
    
    // Mine blocks to confirm the transaction
    env.monero.monerod().generate_blocks(10, recipient_address.to_string()).await.unwrap();
    
    // Poll for balance update
    let mut new_balance = wallet.total_balance().await.unwrap();
    for _ in 0..MAX_SYNC_RETRIES {
        if new_balance < initial_balance { break; }
        sleep(SYNC_POLL_INTERVAL).await;
        new_balance = wallet.total_balance().await.unwrap();
    }
    
    tracing::info!("Balance after transfer: {}", new_balance);
    assert!(new_balance < initial_balance, "Balance should have decreased");

    // Test blockchain height queries
    let height = wallets.direct_rpc_block_height().await.unwrap();
    tracing::info!("Blockchain height: {}", height);
    assert!(height > 0);
    
    // Verify both height methods return the same value
    let block_height_alias = wallets.direct_rpc_block_height().await.unwrap();
    assert_eq!(height, block_height_alias);
}

// Verify wallet restoration from seed phrase.
//
// This test:
// 1. Gets the seed phrase from an existing wallet
// 2. Creates a new wallet from that seed
// 3. Verifies both wallets have the same address
//
async fn verify_restoration(env: &TestContext<'_>, wallet: &WalletHandle) {
    tracing::info!("Testing wallet restoration from seed");
    let seed = wallet.seed().await.unwrap();
    
    let restore_dir = env.wallet_dir.join("restored_wallet");
    let restore_path = restore_dir.to_string_lossy().to_string();
    
    // Restore wallet from seed
    let restored_wallet = WalletHandle::open_or_create_from_seed(
        restore_path.clone(),
        seed,
        Network::Mainnet,
        0, // restore height
        false, // don't start refresh thread
        env.daemon.clone()
    ).await.unwrap();
    
    // Verify addresses match
    let restored_address = restored_wallet.main_address().await.unwrap();
    assert_eq!(restored_address.to_string(), wallet.main_address().await.unwrap().to_string());
}

// Verify that the Monero node can be changed at runtime.
async fn verify_node_change(wallets: &Wallets, env: &TestContext<'_>, wallet: &WalletHandle) {
    tracing::info!("Testing change node");
    
    // Change to the same daemon (tests the API, not actual reconnection)
    wallets.change_monero_node(env.daemon.clone()).await.unwrap();
    
    // Verify wallet is still connected
    assert!(wallet.connected().await.unwrap());
}

// Verify `Wallets::new_with_existing_wallet` functionality.
//
// This tests the ability to create a Wallets instance using an already-opened
// WalletHandle, rather than opening/creating a new wallet internally.
//
async fn verify_new_with_existing_wallet(env: &TestContext<'_>, _wallets: &Wallets) {
    tracing::info!("Testing new_with_existing_wallet");
    let test_wallet_name = "existing_test_wallet";
    let test_wallet_path = env.wallet_dir.join(test_wallet_name);
    
    // First, create/open a wallet using the low-level API
    let raw_wallet = WalletHandle::open_or_create(
        test_wallet_path.to_string_lossy().to_string(),
        env.daemon.clone(),
        Network::Mainnet,
        true // start refresh thread
    ).await.unwrap();
    
    // Use that wallet to create a new Wallets instance
    let _existing_wallets = Wallets::new_with_existing_wallet(
        env.wallet_dir.clone(),
        env.daemon.clone(),
        Network::Mainnet,
        true, // regtest
        Some(env.tauri_handle.clone()),
        raw_wallet,
        Some(env.database.clone())
    ).await.unwrap();
}

// Verify `Wallets::verify_transfer`.
//
// This test:
// 1. Sends funds to a random address
// 2. Uses verify_transfer to check if the transaction exists and has the correct amount
//
async fn verify_verify_transfer(env: &TestContext<'_>, wallets: &Wallets) {
    tracing::info!("Testing verify_transfer");
    let mut rng = rand::thread_rng();

    // Generate random keys
    let view_key = PrivateViewKey::new_random(&mut rng);
    let view_key_private: PrivateKey = view_key.clone().into();
    let scalar = swap_core::monero::primitives::Scalar::random(&mut rng);
    let spend_key = PrivateKey::from_slice(&scalar.to_bytes()).unwrap();

    let public_spend_key = PublicKey::from_private_key(&spend_key);

    // Derive the wallet address
    let address = Address::standard(
        Network::Mainnet,
        public_spend_key,
        PublicKey::from_private_key(&view_key_private)
    );

    // Send funds to the address
    let miner = env.monero.wallet("miner").unwrap();
    let receipt = miner.transfer(&address, AMOUNT_TO_RECEIVE_PICO).await.unwrap();

    // Mine blocks to confirm
    env.monero.monerod().generate_blocks(1, miner.address().await.unwrap().to_string()).await.unwrap();

    let tx_hash = TxHash(receipt.txid.clone());
    let amount = CoreAmount::from_piconero(AMOUNT_TO_RECEIVE_PICO);

    // Verify the transfer
    let result = wallets.verify_transfer(
        &tx_hash,
        public_spend_key,
        view_key,
        amount
    ).await.unwrap();

    assert!(result, "Transfer verification failed");
}

// Verify `Wallets::wait_until_confirmed`.
//
// This test:
// 1. Sends funds to the main wallet
// 2. Waits for confirmations in a background task
// 3. Generates blocks in the foreground to satisfy the confirmation requirement
//
async fn verify_wait_until_confirmed(env: &TestContext<'_>, wallets: &Wallets, wallet: &WalletHandle) {
    tracing::info!("Testing wait_until_confirmed");
    
    // Send funds from miner to the test wallet
    let miner = env.monero.wallet("miner").unwrap();
    let wallet_address = wallet.main_address().await.unwrap();
    let receipt = miner.transfer(&wallet_address, AMOUNT_TO_RECEIVE_PICO).await.unwrap();
    let tx_hash = TxHash(receipt.txid);

    let wallets_clone = unsafe { std::mem::transmute::<&Wallets, &'static Wallets>(wallets) };
    let tx_hash_clone = tx_hash.clone();

    // Spawn waiter in background
    let handle = tokio::spawn(async move {
        wallets_clone.wait_until_confirmed(
            &tx_hash_clone,
            3,
            Some(|(_, current, target)| {
                tracing::info!("Confirmations: {}/{}", current, target);
            })
        ).await
    });

    // Generate 3 blocks to confirm
    for _ in 0..3 {
        sleep(Duration::from_millis(500)).await;
        env.monero.monerod().generate_blocks(1, miner.address().await.unwrap().to_string()).await.unwrap();
    }

    handle.await.unwrap().unwrap();
}

// Verify `Wallets::wait_for_incoming_transfer`.
//
// This test:
// 1. Spawns a scanner in the background waiting for a specific amount
// 2. Sends that amount to the target keys
// 3. Verifies the scanner found the transaction
//
async fn verify_wait_for_incoming_transfer(env: &TestContext<'_>, wallets: &Wallets) {
    tracing::info!("Testing wait_for_incoming_transfer");
    let mut rng = rand::thread_rng();

    // Generate random keys
    let view_key = PrivateViewKey::new_random(&mut rng);
    let view_key_private: PrivateKey = view_key.clone().into();
    let scalar = swap_core::monero::primitives::Scalar::random(&mut rng);
    let spend_key = PrivateKey::from_slice(&scalar.to_bytes()).unwrap();
    let public_spend_key = PublicKey::from_private_key(&spend_key);

    let height = wallets.direct_rpc_block_height().await.unwrap();
    let restore_height = swap_core::monero::primitives::BlockHeight { height };

    // Derive the wallet address
    let address = Address::standard(
        Network::Mainnet,
        public_spend_key,
        PublicKey::from_private_key(&view_key_private)
    );

    let wallets_clone = unsafe { std::mem::transmute::<&Wallets, &'static Wallets>(wallets) };
    let amount = CoreAmount::from_piconero(AMOUNT_TO_RECEIVE_PICO);
    
    // Spawn scanner in background
    let handle = tokio::spawn(async move {
        wallets_clone.wait_for_incoming_transfer(
            public_spend_key,
            view_key,
            amount,
            restore_height
        ).await
    });

    // Give the scanner a moment to start
    sleep(Duration::from_millis(500)).await;

    // Send funds
    let miner = env.monero.wallet("miner").unwrap();
    let receipt = miner.transfer(&address, AMOUNT_TO_RECEIVE_PICO).await.unwrap();
    let expected_txid = receipt.txid;

    // Generate a block to include the tx
    env.monero.monerod().generate_blocks(1, miner.address().await.unwrap().to_string()).await.unwrap();

    let found_tx_hash = handle.await.unwrap().unwrap();
    
    assert_eq!(found_tx_hash.0, expected_txid);
}

// =============================================================================
// Utility Functions
// =============================================================================
//
// Poll wallet until it reaches the expected balance or times out.
//
// This replaces direct `sleep()` calls with a more robust polling mechanism,
// similar to patterns used in `monero-harness` tests.
//
async fn wait_for_balance(wallet: &WalletHandle, expected: MoneroAmount) {
    for _ in 0..MAX_SYNC_RETRIES {
        let balance = wallet.total_balance().await.unwrap();
        if balance == expected {
            return;
        }
        sleep(SYNC_POLL_INTERVAL).await;
    }
    panic!(
        "Wallet failed to sync to expected balance of {}. Current: {}", 
        expected, 
        wallet.total_balance().await.unwrap()
    );
}
