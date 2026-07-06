//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use crate::consensus::ConsensusMechanism;
use futures::{FutureExt, StreamExt as _, channel::mpsc};
use node_subtensor_runtime::{RuntimeApi, TransactionConverter, opaque::Block};
use sc_chain_spec::ChainType;
use sc_client_api::{Backend as BackendT, BlockBackend};
use sc_consensus::{BasicQueue, BoxBlockImport};
use sc_consensus_grandpa::BlockNumberOps;
use sc_consensus_slots::BackoffAuthoringOnFinalizedHeadLagging;
use sc_consensus_slots::SlotProportion;
use sc_keystore::LocalKeystore;
use sc_network::config::SyncMode;
use sc_network_sync::strategy::warp::{WarpSyncConfig, WarpSyncProvider};
use sc_service::{Configuration, PartialComponents, TaskManager, error::Error as ServiceError};
use sc_telemetry::{Telemetry, TelemetryHandle, TelemetryWorker, log};
use sc_transaction_pool::TransactionPoolHandle;
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sc_transaction_pool_api::TransactionPool as _;
use sp_core::H256;
use sp_core::crypto::KeyTypeId;
use sp_keystore::Keystore;
use sp_runtime::key_types;
use sp_runtime::traits::{Block as BlockT, NumberFor};
use stc_shield::{self, MemoryShieldKeystore};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::{cell::RefCell, path::Path};
use std::{sync::Arc, time::Duration};
use stp_shield::ShieldKeystorePtr;
use substrate_prometheus_endpoint::Registry;

use crate::cli::Sealing;
use crate::client::{FullBackend, FullClient, HostFunctions, RuntimeExecutor};
use crate::ethereum::{
    BackendType, EthConfiguration, FrontierBackend, FrontierPartialComponents, StorageOverride,
    StorageOverrideHandler, db_config_dir, new_frontier_partial, spawn_frontier_tasks,
};

const LOG_TARGET: &str = "node-service";

/// The minimum period of blocks on which justifications will be
/// imported and generated.
const GRANDPA_JUSTIFICATION_PERIOD: u32 = 512;

pub type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;
pub type GrandpaBlockImport =
    sc_consensus_grandpa::GrandpaBlockImport<FullBackend, Block, FullClient, FullSelectChain>;
type GrandpaLinkHalf = sc_consensus_grandpa::LinkHalf<Block, FullClient, FullSelectChain>;
#[allow(clippy::upper_case_acronyms)]
pub type BIQ<'a> = Box<
    dyn FnOnce(
            Arc<FullClient>,
            Arc<FullBackend>,
            &Configuration,
            &EthConfiguration,
            &TaskManager,
            Option<TelemetryHandle>,
            GrandpaBlockImport,
            Arc<TransactionPoolHandle<Block, FullClient>>,
        ) -> Result<(BasicQueue<Block>, BoxBlockImport<Block>), sc_service::Error>
        + 'a,
>;

#[allow(clippy::expect_used)]
pub fn new_partial(
    config: &Configuration,
    eth_config: &EthConfiguration,
    build_import_queue: BIQ,
) -> Result<
    PartialComponents<
        FullClient,
        FullBackend,
        FullSelectChain,
        BasicQueue<Block>,
        TransactionPoolHandle<Block, FullClient>,
        (
            Option<Telemetry>,
            BoxBlockImport<Block>,
            GrandpaLinkHalf,
            FrontierBackend,
            Arc<dyn StorageOverride<Block>>,
        ),
    >,
    ServiceError,
> {
    let telemetry = config
        .telemetry_endpoints
        .clone()
        .filter(|x| !x.is_empty())
        .map(|endpoints| -> Result<_, sc_telemetry::Error> {
            let worker = TelemetryWorker::new(16)?;
            let telemetry = worker.handle().new_telemetry(endpoints);
            Ok((worker, telemetry))
        })
        .transpose()?;

    let executor = sc_service::new_wasm_executor::<HostFunctions>(&config.executor);
    let (client, backend, keystore_container, task_manager) =
        sc_service::new_full_parts::<Block, RuntimeApi, RuntimeExecutor>(
            config,
            telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
            executor,
        )?;

    // Prepare keystore for authoring Babe blocks.
    copy_keys(
        &keystore_container.local_keystore(),
        key_types::AURA,
        key_types::BABE,
    )?;

    let client = Arc::new(client);

    let telemetry = telemetry.map(|(worker, telemetry)| {
        task_manager
            .spawn_handle()
            .spawn("telemetry", None, worker.run());
        telemetry
    });

    let select_chain = sc_consensus::LongestChain::new(backend.clone());

    let skip_block_justifications = if config.chain_spec.chain_type() == ChainType::Live {
        // Mainnet patch
        let hash_5614869 =
            H256::from_str("0xb49f8cd2a49b51a493fc55a8c9524ba08c3aa6b702f20af02878c1ec68e3ff5f")
                .expect("Invalid hash string.");
        let hash_5614888 =
            H256::from_str("0x04c71efb77060bfacfb49cd61826d594148ccda2ee23d66c7db819b16b55911c")
                .expect("Invalid hash string.");

        Some(HashSet::from([hash_5614869, hash_5614888]))
    } else {
        // Testnet patch
        let hash_4589660 =
            H256::from_str("0x819a5e54ffa2d267d469c6da44de5e8819b1aad1717a1389c959eab4349722ca")
                .expect("Invalid hash string.");

        Some(HashSet::from([hash_4589660]))
    };

    log::warn!(
        "Grandpa block import patch enabled. Chain type = {:?}. Skip justifications for blocks = {skip_block_justifications:?}",
        config.chain_spec.chain_type()
    );

    let (grandpa_block_import, grandpa_link) = sc_consensus_grandpa::block_import(
        client.clone(),
        GRANDPA_JUSTIFICATION_PERIOD,
        &client,
        select_chain.clone(),
        telemetry.as_ref().map(|x| x.handle()),
        skip_block_justifications,
    )?;

    let storage_override = Arc::new(StorageOverrideHandler::<_, _, _>::new(client.clone()));
    let frontier_backend = match eth_config.frontier_backend_type {
        BackendType::KeyValue => FrontierBackend::KeyValue(Arc::new(fc_db::kv::Backend::open(
            Arc::clone(&client),
            &config.database,
            &db_config_dir(config),
        )?)),
        BackendType::Sql => {
            let db_path = db_config_dir(config).join("sql");
            std::fs::create_dir_all(&db_path).expect("failed creating sql db directory");
            let backend = futures::executor::block_on(fc_db::sql::Backend::new(
                fc_db::sql::BackendConfig::Sqlite(fc_db::sql::SqliteBackendConfig {
                    path: Path::new("sqlite:///")
                        .join(db_path)
                        .join("frontier.db3")
                        .to_str()
                        .unwrap_or(""),
                    create_if_missing: true,
                    thread_count: eth_config.frontier_sql_backend_thread_count,
                    cache_size: eth_config.frontier_sql_backend_cache_size,
                }),
                eth_config.frontier_sql_backend_pool_size,
                std::num::NonZeroU32::new(eth_config.frontier_sql_backend_num_ops_timeout),
                storage_override.clone(),
            ))
            .unwrap_or_else(|err| panic!("failed creating sql backend: {err:?}"));
            FrontierBackend::Sql(Arc::new(backend))
        }
    };

    let transaction_pool = Arc::from(
        sc_transaction_pool::Builder::new(
            task_manager.spawn_essential_handle(),
            client.clone(),
            config.role.is_authority().into(),
        )
        .with_options(config.transaction_pool.clone())
        .with_prometheus(config.prometheus_registry())
        .build(),
    );

    let (import_queue, block_import) = build_import_queue(
        client.clone(),
        backend.clone(),
        config,
        eth_config,
        &task_manager,
        telemetry.as_ref().map(|x| x.handle()),
        grandpa_block_import,
        transaction_pool.clone(),
    )?;

    Ok(PartialComponents {
        client,
        backend,
        keystore_container,
        task_manager,
        select_chain,
        import_queue,
        transaction_pool,
        other: (
            telemetry,
            block_import,
            grandpa_link,
            frontier_backend,
            storage_override,
        ),
    })
}

/// Build the import queue for the template runtime (manual seal).
#[allow(clippy::too_many_arguments)]
#[cfg(feature = "runtime-benchmarks")]
pub fn build_manual_seal_import_queue(
    client: Arc<FullClient>,
    _backend: Arc<FullBackend>,
    config: &Configuration,
    _eth_config: &EthConfiguration,
    task_manager: &TaskManager,
    _telemetry: Option<TelemetryHandle>,
    grandpa_block_import: GrandpaBlockImport,
    _transaction_pool_handle: Arc<TransactionPoolHandle<Block, FullClient>>,
) -> Result<(BasicQueue<Block>, BoxBlockImport<Block>), ServiceError> {
    let conditional_block_import =
        crate::conditional_evm_block_import::ConditionalEVMBlockImport::new(
            grandpa_block_import.clone(),
            fc_consensus::FrontierBlockImport::new(grandpa_block_import.clone(), client.clone()),
            false,
        );
    Ok((
        sc_consensus_manual_seal::import_queue(
            Box::new(conditional_block_import.clone()),
            &task_manager.spawn_essential_handle(),
            config.prometheus_registry(),
        ),
        Box::new(conditional_block_import),
    ))
}

/// Builds a new service for a full client.
#[allow(clippy::expect_used)]
pub async fn new_full<NB, CM>(
    mut config: Configuration,
    eth_config: EthConfiguration,
    sealing: Option<Sealing>,
    custom_service_signal: Option<Arc<AtomicBool>>,
    skip_history_backfill: bool,
) -> Result<TaskManager, ServiceError>
where
    NumberFor<Block>: BlockNumberOps,
    NB: sc_network::NetworkBackend<Block, <Block as BlockT>::Hash>,
    CM: ConsensusMechanism,
{
    // Substrate doesn't seem to support fast sync option in our configuration.
    if matches!(config.network.sync_mode, SyncMode::LightState { .. }) {
        log::error!(
            "Supported sync modes: full, warp. Provided: {:?}",
            config.network.sync_mode
        );
        return Err(ServiceError::Other("Unsupported sync mode".to_string()));
    }

    let mut consensus_mechanism = CM::new();
    let build_import_queue = consensus_mechanism.build_biq(skip_history_backfill)?;

    let PartialComponents {
        client,
        backend,
        mut task_manager,
        import_queue,
        keystore_container,
        select_chain,
        transaction_pool,
        other: (mut telemetry, block_import, grandpa_link, frontier_backend, storage_override),
    } = new_partial(&config, &eth_config, build_import_queue)?;

    let FrontierPartialComponents {
        filter_pool,
        fee_history_cache,
        fee_history_cache_limit,
    } = new_frontier_partial(&eth_config)?;

    let maybe_registry = config.prometheus_config.as_ref().map(|cfg| &cfg.registry);
    let mut net_config = sc_network::config::FullNetworkConfiguration::<_, _, NB>::new(
        &config.network,
        maybe_registry.cloned(),
    );
    let peer_store_handle = net_config.peer_store_handle();
    let metrics = NB::register_notification_metrics(maybe_registry);

    let grandpa_protocol_name = sc_consensus_grandpa::protocol_standard_name(
        &client.block_hash(0u32)?.expect("Genesis block exists; qed"),
        &config.chain_spec,
    );

    let (grandpa_protocol_config, grandpa_notification_service) =
        sc_consensus_grandpa::grandpa_peers_set_config::<_, NB>(
            grandpa_protocol_name.clone(),
            metrics.clone(),
            peer_store_handle,
        );

    let warp_sync_config = if sealing.is_some() {
        None
    } else {
        let set_id = match config.chain_spec.chain_type() {
            // Finney patch
            ChainType::Live => 3,
            // Testnet patch
            ChainType::Development => 2,
            // All others (e.g. localnet)
            _ => 0,
        };
        log::warn!(
            "Grandpa warp sync patch enabled. Chain type = {:?}. Set ID = {set_id}",
            config.chain_spec.chain_type()
        );
        net_config.add_notification_protocol(grandpa_protocol_config);
        let warp_sync: Arc<dyn WarpSyncProvider<Block>> =
            Arc::new(sc_consensus_grandpa::warp_proof::NetworkProvider::new(
                backend.clone(),
                grandpa_link.shared_authority_set().clone(),
                sc_consensus_grandpa::warp_proof::HardForks::new_initial_set_id(set_id),
            ));

        Some(WarpSyncConfig::WithProvider(warp_sync))
    };

    let (network, system_rpc_tx, tx_handler_controller, sync_service) =
        sc_service::build_network(sc_service::BuildNetworkParams {
            config: &config,
            net_config,
            client: client.clone(),
            transaction_pool: transaction_pool.clone(),
            spawn_handle: task_manager.spawn_handle(),
            import_queue,
            block_announce_validator_builder: None,
            warp_sync_config,
            block_relay: None,
            metrics,
        })?;

    consensus_mechanism.spawn_essential_handles(
        &mut task_manager,
        client.clone(),
        custom_service_signal,
        sync_service.clone(),
    )?;

    if config.offchain_worker.enabled && config.role.is_authority() {
        let public_keys = keystore_container
            .keystore()
            .sr25519_public_keys(pallet_drand::KEY_TYPE);

        if public_keys.is_empty() {
            match sp_keystore::Keystore::sr25519_generate_new(
                &*keystore_container.keystore(),
                pallet_drand::KEY_TYPE,
                None,
            ) {
                Ok(_) => {
                    log::debug!("Offchain worker key generated");
                }
                Err(e) => {
                    log::error!("Failed to create SR25519 key for offchain worker: {e:?}");
                }
            }
        } else {
            log::debug!("Offchain worker key already exists");
        }

        task_manager.spawn_essential_handle().spawn(
            "offchain-workers-runner",
            None,
            sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
                runtime_api_provider: client.clone(),
                is_validator: config.role.is_authority(),
                keystore: Some(keystore_container.keystore()),
                offchain_db: backend.offchain_storage(),
                transaction_pool: Some(OffchainTransactionPoolFactory::new(
                    transaction_pool.clone(),
                )),
                network_provider: Arc::new(network.clone()),
                enable_http_requests: true,
                custom_extensions: |_| vec![],
            })?
            .run(client.clone(), task_manager.spawn_handle())
            .boxed(),
        );
    }

    let role = config.role;
    let force_authoring = config.force_authoring;
    let backoff_authoring_blocks =
        Some(BackoffAuthoringOnFinalizedHeadLagging::<NumberFor<Block>> {
            unfinalized_slack: 6u32,
            ..Default::default()
        });
    let name = config.network.node_name.clone();
    let frontier_backend = Arc::new(frontier_backend);
    let enable_grandpa = !config.disable_grandpa && sealing.is_none();
    let prometheus_registry = config.prometheus_registry().cloned();

    // Channel for the rpc handler to communicate with the authorship task.
    let (command_sink, commands_stream) = mpsc::channel(1000);

    // Sinks for pubsub notifications.
    // Everytime a new subscription is created, a new mpsc channel is added to the sink pool.
    // The MappingSyncWorker sends through the channel on block import and the subscription emits a notification to the subscriber on receiving a message through this channel.
    // This way we avoid race conditions when using native substrate block import notification stream.
    let pubsub_notification_sinks: fc_mapping_sync::EthereumBlockNotificationSinks<
        fc_mapping_sync::EthereumBlockNotification<Block>,
    > = Default::default();
    let pubsub_notification_sinks = Arc::new(pubsub_notification_sinks);

    // for ethereum-compatibility rpc.
    config.rpc.id_provider = Some(Box::new(fc_rpc::EthereumSubIdProvider));

    let rpc_builder = {
        let client = client.clone();
        let pool = transaction_pool.clone();
        let network = network.clone();
        let sync_service = sync_service.clone();

        let is_authority = role.is_authority();
        let enable_dev_signer = eth_config.enable_dev_signer;
        let max_past_logs = eth_config.max_past_logs;
        let execute_gas_limit_multiplier = eth_config.execute_gas_limit_multiplier;
        let filter_pool = filter_pool.clone();
        let frontier_backend = frontier_backend.clone();
        let pubsub_notification_sinks = pubsub_notification_sinks.clone();
        let storage_override = storage_override.clone();
        let fee_history_cache = fee_history_cache.clone();
        let block_data_cache = Arc::new(fc_rpc::EthBlockDataCacheTask::new(
            task_manager.spawn_handle(),
            storage_override.clone(),
            eth_config.eth_log_block_cache,
            eth_config.eth_statuses_cache,
            prometheus_registry.clone(),
        ));

        let shield_keystore = Arc::new(MemoryShieldKeystore::new());
        let slot_duration = consensus_mechanism.slot_duration(&client)?;
        let pending_create_inherent_data_providers = move |_, ()| {
            let keystore = shield_keystore.clone();
            async move { CM::pending_create_inherent_data_providers(slot_duration, keystore) }
        };

        let rpc_methods = consensus_mechanism.rpc_methods(
            client.clone(),
            keystore_container.keystore(),
            select_chain.clone(),
        )?;
        Box::new(move |subscription_task_executor| {
            let eth_deps = crate::rpc::EthDeps {
                client: client.clone(),
                pool: pool.clone(),
                graph: pool.clone(),
                converter: Some(TransactionConverter::<Block>::default()),
                is_authority,
                enable_dev_signer,
                network: network.clone(),
                sync: sync_service.clone(),
                frontier_backend: match &*frontier_backend {
                    fc_db::Backend::KeyValue(b) => b.clone(),
                    fc_db::Backend::Sql(b) => b.clone(),
                },
                storage_override: storage_override.clone(),
                block_data_cache: block_data_cache.clone(),
                filter_pool: filter_pool.clone(),
                max_past_logs,
                fee_history_cache: fee_history_cache.clone(),
                fee_history_cache_limit,
                execute_gas_limit_multiplier,
                forced_parent_hashes: None,
                pending_create_inherent_data_providers: pending_create_inherent_data_providers
                    .clone(),
            };
            let deps = crate::rpc::FullDeps {
                client: client.clone(),
                pool: pool.clone(),
                command_sink: if sealing.is_some() {
                    Some(command_sink.clone())
                } else {
                    None
                },
                eth: eth_deps,
            };
            crate::rpc::create_full(
                deps,
                subscription_task_executor,
                pubsub_notification_sinks.clone(),
                CM::frontier_consensus_data_provider(client.clone())?,
                rpc_methods.as_slice(),
            )
            .map_err(Into::into)
        })
    };

    let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
        config,
        client: client.clone(),
        backend: backend.clone(),
        task_manager: &mut task_manager,
        keystore: keystore_container.keystore(),
        transaction_pool: transaction_pool.clone(),
        rpc_builder,
        network: network.clone(),
        system_rpc_tx,
        tx_handler_controller,
        sync_service: sync_service.clone(),
        telemetry: telemetry.as_mut(),
    })?;

    spawn_frontier_tasks(
        &task_manager,
        client.clone(),
        backend,
        frontier_backend,
        filter_pool,
        storage_override,
        fee_history_cache,
        fee_history_cache_limit,
        sync_service.clone(),
        pubsub_notification_sinks,
    )
    .await;

    if role.is_authority() {
        // manual-seal authorship — use a fixed keystore so the single-validator dev
        // node doesn't drift: MemoryShieldKeystore rolls on every own-block import
        // (every block in single-validator mode), advancing current_dec_key() 2 pairs
        // ahead of PendingKey on-chain. DevShieldKeystore avoids this by keeping the
        // same keypair for both next_enc_key() and current_dec_key().
        if let Some(sealing) = sealing {
            let dev_shield_keystore: stp_shield::ShieldKeystorePtr =
                Arc::new(crate::dev_keystore::DevShieldKeystore::new());
            run_manual_seal_authorship(
                sealing,
                client,
                transaction_pool,
                select_chain.clone(),
                block_import,
                &task_manager,
                prometheus_registry.as_ref(),
                telemetry.as_ref(),
                commands_stream,
                dev_shield_keystore,
            )?;
            log::info!("Manual Seal Ready");
            return Ok(task_manager);
        }

        let shield_keystore = Arc::new(MemoryShieldKeystore::new());

        stc_shield::spawn_key_rotation_on_own_import(
            &task_manager.spawn_handle(),
            client.clone(),
            shield_keystore.clone(),
        );

        let proposer_factory = sc_basic_authorship::ProposerFactory::new(
            task_manager.spawn_handle(),
            client.clone(),
            transaction_pool.clone(),
            prometheus_registry.as_ref(),
            telemetry.as_ref().map(|x| x.handle()),
            shield_keystore.clone(),
        );

        let slot_duration = consensus_mechanism.slot_duration(&client)?;

        let create_inherent_data_providers = move |_, ()| {
            let keystore = shield_keystore.clone();
            async move { CM::create_inherent_data_providers(slot_duration, keystore) }
        };

        consensus_mechanism.start_authoring(
            &mut task_manager,
            crate::consensus::StartAuthoringParams {
                slot_duration,
                client,
                select_chain,
                block_import,
                proposer_factory,
                sync_oracle: sync_service.clone(),
                justification_sync_link: sync_service.clone(),
                create_inherent_data_providers,
                force_authoring,
                backoff_authoring_blocks,
                keystore: keystore_container.keystore(),
                block_proposal_slot_portion: SlotProportion::new(2f32 / 3f32),
                max_block_proposal_slot_portion: None,
                telemetry: telemetry.as_ref().map(|x| x.handle()),
            },
        )?;
    }

    if enable_grandpa {
        // if the node isn't actively participating in consensus then it doesn't
        // need a keystore, regardless of which protocol we use below.
        let keystore = if role.is_authority() {
            Some(keystore_container.keystore())
        } else {
            None
        };

        let grandpa_config = sc_consensus_grandpa::Config {
            // FIXME #1578 make this available through chainspec
            gossip_duration: Duration::from_millis(333),
            justification_generation_period: GRANDPA_JUSTIFICATION_PERIOD,
            name: Some(name),
            observer_enabled: false,
            keystore,
            local_role: role,
            telemetry: telemetry.as_ref().map(|x| x.handle()),
            protocol_name: grandpa_protocol_name,
        };

        // start the full GRANDPA voter
        // NOTE: non-authorities could run the GRANDPA observer protocol, but at
        // this point the full voter should provide better guarantees of block
        // and vote data availability than the observer. The observer has not
        // been tested extensively yet and having most nodes in a network run it
        // could lead to finality stalls.
        let grandpa_voter =
            sc_consensus_grandpa::run_grandpa_voter(sc_consensus_grandpa::GrandpaParams {
                config: grandpa_config,
                link: grandpa_link,
                network,
                sync: sync_service,
                notification_service: grandpa_notification_service,
                voting_rule: sc_consensus_grandpa::VotingRulesBuilder::default().build(),
                prometheus_registry,
                shared_voter_state: sc_consensus_grandpa::SharedVoterState::empty(),
                telemetry: telemetry.as_ref().map(|x| x.handle()),
                offchain_tx_pool_factory: OffchainTransactionPoolFactory::new(transaction_pool),
            })?;

        // the GRANDPA voter task is considered infallible, i.e.
        // if it fails we take down the service with it.
        task_manager
            .spawn_essential_handle()
            .spawn_blocking("grandpa-voter", None, grandpa_voter);
    }

    Ok(task_manager)
}

pub async fn build_full<CM: ConsensusMechanism>(
    config: Configuration,
    eth_config: EthConfiguration,
    sealing: Option<Sealing>,
    custom_service_signal: Option<Arc<AtomicBool>>,
    skip_history_backfill: bool,
) -> Result<TaskManager, ServiceError> {
    match config.network.network_backend {
        sc_network::config::NetworkBackendType::Libp2p => {
            new_full::<sc_network::NetworkWorker<_, _>, CM>(
                config,
                eth_config,
                sealing,
                custom_service_signal,
                skip_history_backfill,
            )
            .await
        }
        sc_network::config::NetworkBackendType::Litep2p => {
            new_full::<sc_network::Litep2pNetworkBackend, CM>(
                config,
                eth_config,
                sealing,
                custom_service_signal,
                skip_history_backfill,
            )
            .await
        }
    }
}

pub fn new_chain_ops<CM: ConsensusMechanism>(
    config: &mut Configuration,
    eth_config: &EthConfiguration,
    skip_history_backfill: bool,
) -> Result<
    (
        Arc<FullClient>,
        Arc<FullBackend>,
        BasicQueue<Block>,
        TaskManager,
        FrontierBackend,
    ),
    ServiceError,
> {
    config.keystore = sc_service::config::KeystoreConfig::InMemory;
    let mut consensus_mechanism = CM::new();
    let PartialComponents {
        client,
        backend,
        import_queue,
        task_manager,
        other,
        ..
    } = new_partial(
        config,
        eth_config,
        consensus_mechanism.build_biq(skip_history_backfill)?,
    )?;
    Ok((client, backend, import_queue, task_manager, other.3))
}

type SealStream = std::pin::Pin<
    Box<
        dyn futures::Stream<
                Item = sc_consensus_manual_seal::rpc::EngineCommand<<Block as BlockT>::Hash>,
            > + Send,
    >,
>;

#[allow(clippy::too_many_arguments)]
fn run_manual_seal_authorship(
    sealing: Sealing,
    client: Arc<FullClient>,
    transaction_pool: Arc<TransactionPoolHandle<Block, FullClient>>,
    select_chain: FullSelectChain,
    block_import: BoxBlockImport<Block>,
    task_manager: &TaskManager,
    prometheus_registry: Option<&Registry>,
    telemetry: Option<&Telemetry>,
    commands_stream: mpsc::Receiver<
        sc_consensus_manual_seal::rpc::EngineCommand<<Block as BlockT>::Hash>,
    >,
    shield_keystore: ShieldKeystorePtr,
) -> Result<(), ServiceError> {
    let proposer_factory = sc_basic_authorship::ProposerFactory::new(
        task_manager.spawn_handle(),
        client.clone(),
        transaction_pool.clone(),
        prometheus_registry,
        telemetry.as_ref().map(|x| x.handle()),
        shield_keystore.clone(),
    );

    thread_local!(static TIMESTAMP: RefCell<u64> = const { RefCell::new(0) });

    /// Provide a mock duration starting at 0 in millisecond for timestamp inherent.
    /// Each call will increment timestamp by slot_duration making the consensus logic think time has passed.
    struct MockTimestampInherentDataProvider;

    #[async_trait::async_trait]
    impl sp_inherents::InherentDataProvider for MockTimestampInherentDataProvider {
        async fn provide_inherent_data(
            &self,
            inherent_data: &mut sp_inherents::InherentData,
        ) -> Result<(), sp_inherents::Error> {
            TIMESTAMP.with(|x| {
                let mut x_ref = x.borrow_mut();
                *x_ref = x_ref.saturating_add(subtensor_runtime_common::time::SLOT_DURATION);
                inherent_data.put_data(sp_timestamp::INHERENT_IDENTIFIER, &*x_ref)
            })
        }

        async fn try_handle_error(
            &self,
            _identifier: &sp_inherents::InherentIdentifier,
            _error: &[u8],
        ) -> Option<Result<(), sp_inherents::Error>> {
            // The pallet never reports error.
            None
        }
    }

    let create_inherent_data_providers = move |_, ()| {
        let keystore = shield_keystore.clone();
        async move {
            Ok((
                MockTimestampInherentDataProvider,
                stc_shield::InherentDataProvider::new(keystore),
            ))
        }
    };

    let aura_data_provider =
        sc_consensus_manual_seal::consensus::aura::AuraConsensusDataProvider::new(client.clone());

    let seal_stream: SealStream = match sealing {
        Sealing::Manual => Box::pin(commands_stream),
        Sealing::Instant => Box::pin(transaction_pool.import_notification_stream().map(|_| {
            sc_consensus_manual_seal::rpc::EngineCommand::SealNewBlock {
                create_empty: false,
                finalize: false,
                parent_hash: None,
                sender: None,
            }
        })),
        Sealing::Interval(millis) => Box::pin(
            futures::stream::unfold(
                tokio::time::interval(std::time::Duration::from_millis(millis)),
                |mut interval| async move {
                    interval.tick().await;
                    Some(((), interval))
                },
            )
            .map(
                |_| sc_consensus_manual_seal::rpc::EngineCommand::SealNewBlock {
                    create_empty: true,
                    finalize: true,
                    parent_hash: None,
                    sender: None,
                },
            ),
        ),
    };

    let manual_seal =
        sc_consensus_manual_seal::run_manual_seal(sc_consensus_manual_seal::ManualSealParams {
            block_import,
            env: proposer_factory,
            client,
            pool: transaction_pool,
            commands_stream: seal_stream,
            select_chain,
            consensus_data_provider: Some(Box::new(aura_data_provider)),
            create_inherent_data_providers,
        });

    // we spawn the future on a background thread managed by service.
    task_manager
        .spawn_essential_handle()
        .spawn_blocking("manual-seal", None, manual_seal);
    Ok(())
}

/// Copy `from_key_type` keys to also exist as `to_key_type`.
///
/// Used for the Aura to Babe migration, where Aura validators need their keystore to copy their
/// Aura keys over to Babe. This works because Aura and Babe keys use identical crypto.
///
/// While not required to retain beyond the initial Aura to Babe migration, it is nice to leave it
/// so the node always retains the ability to perform Aura to Babe migrations in the future, in case
/// there is a requirement to do something like regenesis testnet.
fn copy_keys(
    keystore: &LocalKeystore,
    from_key_type: KeyTypeId,
    to_key_type: KeyTypeId,
) -> sc_keystore::Result<()> {
    use std::collections::HashSet;

    let from_keys: HashSet<_> = keystore
        .raw_public_keys(from_key_type)?
        .into_iter()
        .collect();
    let to_keys: HashSet<_> = keystore.raw_public_keys(to_key_type)?.into_iter().collect();
    let to_copy: Vec<_> = from_keys.difference(&to_keys).collect();

    log::debug!(target: LOG_TARGET, "from_keys: {from_keys:?}");
    log::debug!(target: LOG_TARGET, "to_keys: {to_keys:?}");
    log::debug!(target: LOG_TARGET, "to_copy: {:?} from {:?} to {:?}", &to_copy, from_key_type, to_key_type);

    for public in to_copy {
        if let Some(phrase) = keystore.key_phrase_by_type(public, from_key_type)? {
            if keystore.insert(to_key_type, &phrase, public).is_err() {
                log::error!(
                    target: LOG_TARGET,
                    "Failed to copy key {:?} into keystore, insert operation failed.",
                    &public,
                );
            };
        } else {
            log::error!(
                target: LOG_TARGET,
                "Failed to copy key from {from_key_type:?} to {to_key_type:?} as the key phrase is not available"
            );
        }
    }

    Ok(())
}
