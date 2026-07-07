use std::sync::{Arc, atomic::AtomicBool};

use crate::{
    chain_spec,
    cli::{Cli, HistoryBackfill, Subcommand, SupportedConsensusMechanism},
    consensus::BabeConsensus,
    ethereum::db_config_dir,
    service,
};
use fc_db::{DatabaseSource, kv::frontier_database_dir};

use crate::consensus::AuraConsensus;
use clap::{ArgMatches, CommandFactory, FromArgMatches, parser::ValueSource};
use node_subtensor_runtime::Block;
use sc_cli::SubstrateCli;
use sc_service::{
    Configuration,
    config::{ExecutorConfiguration, RpcConfiguration},
};

impl SubstrateCli for Cli {
    fn impl_name() -> String {
        "Subtensor Node".into()
    }

    fn impl_version() -> String {
        env!("SUBSTRATE_CLI_IMPL_VERSION").into()
    }

    fn description() -> String {
        env!("CARGO_PKG_DESCRIPTION").into()
    }

    fn author() -> String {
        env!("CARGO_PKG_AUTHORS").into()
    }

    fn support_url() -> String {
        "support.anonymous.an".into()
    }

    fn copyright_start_year() -> i32 {
        2017
    }

    fn load_spec(&self, id: &str) -> Result<Box<dyn sc_service::ChainSpec>, String> {
        Ok(match id {
            "dev" => Box::new(chain_spec::localnet::localnet_config(true)?),
            "local" => Box::new(chain_spec::localnet::localnet_config(false)?),
            "finney" => Box::new(chain_spec::finney::finney_mainnet_config()?),
            "devnet" => Box::new(chain_spec::devnet::devnet_config()?),
            "" | "test_finney" => Box::new(chain_spec::testnet::finney_testnet_config()?),
            path => Box::new(chain_spec::ChainSpec::from_json_file(
                std::path::PathBuf::from(path),
            )?),
        })
    }
}

// Parse and run command line arguments
pub fn run() -> sc_cli::Result<()> {
    let cmd = Cli::command();
    let arg_matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&arg_matches)?;
    let skip_history_backfill = resolve_skip_history_backfill(&cli, &arg_matches);

    match &cli.subcommand {
        Some(Subcommand::Key(cmd)) => cmd.run(&cli),
        Some(Subcommand::BuildSpec(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| cmd.run(config.chain_spec, config.network))
        }
        Some(Subcommand::CheckBlock(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|mut config| {
                let (client, _, import_queue, task_manager, _) = cli
                    .initial_consensus
                    .new_chain_ops(&mut config, &cli.eth, skip_history_backfill)?;
                Ok((cmd.run(client, import_queue), task_manager))
            })
        }
        Some(Subcommand::ExportBlocks(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|mut config| {
                let (client, _, _, task_manager, _) = cli.initial_consensus.new_chain_ops(
                    &mut config,
                    &cli.eth,
                    skip_history_backfill,
                )?;
                Ok((cmd.run(client, config.database), task_manager))
            })
        }
        Some(Subcommand::ExportState(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|mut config| {
                let (client, _, _, task_manager, _) = cli.initial_consensus.new_chain_ops(
                    &mut config,
                    &cli.eth,
                    skip_history_backfill,
                )?;
                Ok((cmd.run(client, config.chain_spec), task_manager))
            })
        }
        Some(Subcommand::ImportBlocks(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|mut config| {
                let (client, _, import_queue, task_manager, _) = cli
                    .initial_consensus
                    .new_chain_ops(&mut config, &cli.eth, skip_history_backfill)?;
                Ok((cmd.run(client, import_queue), task_manager))
            })
        }
        Some(Subcommand::PurgeChain(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| {
                // Remove Frontier offchain db
                let db_config_dir = db_config_dir(&config);
                match cli.eth.frontier_backend_type {
                    crate::ethereum::BackendType::KeyValue => {
                        let frontier_database_config = match config.database {
                            DatabaseSource::RocksDb { .. } => DatabaseSource::RocksDb {
                                path: frontier_database_dir(&db_config_dir, "db"),
                                cache_size: 0,
                            },
                            DatabaseSource::ParityDb { .. } => DatabaseSource::ParityDb {
                                path: frontier_database_dir(&db_config_dir, "paritydb"),
                            },
                            _ => {
                                return Err(format!(
                                    "Cannot purge `{:?}` database",
                                    config.database
                                )
                                .into());
                            }
                        };
                        cmd.run(frontier_database_config)?;
                    }
                    crate::ethereum::BackendType::Sql => {
                        let db_path = db_config_dir.join("sql");
                        match std::fs::remove_dir_all(&db_path) {
                            Ok(_) => {
                                println!("{:?} removed.", &db_path);
                            }
                            Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => {
                                eprintln!("{:?} did not exist.", &db_path);
                            }
                            Err(err) => {
                                return Err(format!(
                                    "Cannot purge `{db_path:?}` database: {err:?}",
                                )
                                .into());
                            }
                        };
                    }
                };
                cmd.run(config.database)
            })
        }
        Some(Subcommand::Revert(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.async_run(|mut config| {
                let (client, backend, _, task_manager, _) = cli.initial_consensus.new_chain_ops(
                    &mut config,
                    &cli.eth,
                    skip_history_backfill,
                )?;
                let aux_revert = Box::new(move |client, _, blocks| {
                    sc_consensus_grandpa::revert(client, blocks)?;
                    Ok(())
                });
                Ok((cmd.run(client, backend, Some(aux_revert)), task_manager))
            })
        }
        #[cfg(feature = "runtime-benchmarks")]
        Some(Subcommand::Benchmark(cmd)) => {
            use crate::benchmarking::{
                RemarkBuilder, TransferKeepAliveBuilder, inherent_benchmark_data,
            };
            use frame_benchmarking_cli::{
                BenchmarkCmd, ExtrinsicFactory, SUBSTRATE_REFERENCE_HARDWARE,
            };
            use node_subtensor_runtime::EXISTENTIAL_DEPOSIT;
            use sc_service::PartialComponents;
            use sp_keyring::Sr25519Keyring;
            use sp_runtime::traits::HashingFor;

            let runner = cli.create_runner(cmd)?;

            runner.sync_run(|config| {
                let PartialComponents {
                    client, backend, ..
                } = crate::service::new_partial(
                    &config,
                    &cli.eth,
                    Box::new(crate::service::build_manual_seal_import_queue),
                )?;

                // This switch needs to be in the client, since the client decides
                // which sub-commands it wants to support.
                match cmd {
                    BenchmarkCmd::Pallet(cmd) => cmd
                        .run_with_spec::<HashingFor<Block>, crate::client::HostFunctions>(Some(
                            config.chain_spec,
                        )),
                    BenchmarkCmd::Block(cmd) => cmd.run(client),
                    BenchmarkCmd::Storage(cmd) => {
                        let db = backend.expose_db();
                        let storage = backend.expose_storage();
                        let shared_cache = backend.expose_shared_trie_cache();

                        cmd.run(config, client, db, storage, shared_cache)
                    }
                    BenchmarkCmd::Overhead(cmd) => {
                        let ext_builder = RemarkBuilder::new(client.clone());

                        cmd.run(
                            config.chain_spec.name().into(),
                            client,
                            inherent_benchmark_data()?,
                            Vec::new(),
                            &ext_builder,
                            false,
                        )
                    }
                    BenchmarkCmd::Extrinsic(cmd) => {
                        // Register the *Remark* and *TKA* builders.
                        let ext_factory = ExtrinsicFactory(vec![
                            Box::new(RemarkBuilder::new(client.clone())),
                            Box::new(TransferKeepAliveBuilder::new(
                                client.clone(),
                                Sr25519Keyring::Alice.to_account_id(),
                                EXISTENTIAL_DEPOSIT.into(),
                            )),
                        ]);

                        cmd.run(client, inherent_benchmark_data()?, Vec::new(), &ext_factory)
                    }
                    BenchmarkCmd::Machine(cmd) => {
                        cmd.run(&config, SUBSTRATE_REFERENCE_HARDWARE.clone())
                    }
                }
            })
        }
        Some(Subcommand::ChainInfo(cmd)) => {
            let runner = cli.create_runner(cmd)?;
            runner.sync_run(|config| cmd.run::<Block>(&config))
        }
        Some(Subcommand::CloneState(cmd)) => {
            let runner = cli.create_runner(&cli.run)?;
            let cmd = cmd.clone();
            runner.sync_run(move |_| crate::clone_spec::run(&cmd, skip_history_backfill))
        }
        // Start with the initial consensus type asked.
        None => match cli.initial_consensus {
            SupportedConsensusMechanism::Babe => {
                start_babe_service(&arg_matches, skip_history_backfill)
            }
            SupportedConsensusMechanism::Aura => {
                start_aura_service(&arg_matches, skip_history_backfill)
            }
        },
    }
}

fn resolve_skip_history_backfill(cli: &Cli, arg_matches: &ArgMatches) -> bool {
    // We keep a single global `--history-backfill` flag, but `build-patched-spec` should default to
    // `skip` when the operator didn't set the flag explicitly. This preserves `keep` as the default
    // for normal node runs.
    if matches!(
        arg_matches.value_source("history_backfill"),
        Some(ValueSource::CommandLine)
    ) {
        return matches!(cli.history_backfill, HistoryBackfill::Skip);
    }

    matches!(&cli.subcommand, Some(Subcommand::CloneState(_)))
}

#[allow(clippy::expect_used)]
fn start_babe_service(
    arg_matches: &ArgMatches,
    skip_history_backfill: bool,
) -> Result<(), sc_cli::Error> {
    let cli = Cli::from_arg_matches(arg_matches).expect("Bad arg_matches");
    let runner = cli.create_runner(&cli.run)?;
    match runner.run_node_until_exit(|config| async move {
        let config = customise_config(arg_matches, config);
        service::build_full::<BabeConsensus>(
            config,
            cli.eth,
            cli.sealing,
            None,
            skip_history_backfill,
        )
        .await
    }) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Handle node needs to be in Aura mode.
            if matches!(
                e,
                sc_service::Error::Client(sp_blockchain::Error::VersionInvalid(ref msg))
                    if msg == "Unsupported or invalid BabeApi version"
            ) {
                log::info!(
                    "💡 Chain is using Aura consensus. Switching to Aura service until Babe block is detected.",
                );
                start_aura_service(arg_matches, skip_history_backfill)
            // Handle Aura service still has DB lock. This never has been observed to take more
            // than 1s to drop.
            } else if matches!(e, sc_service::Error::Client(sp_blockchain::Error::Backend(ref msg))
                if msg.starts_with("IO error: lock hold by current process"))
            {
                log::info!("Failed to aquire DB lock, trying again in 1s...");
                std::thread::sleep(std::time::Duration::from_secs(1));
                start_babe_service(arg_matches, skip_history_backfill)
            // Unknown error, return it.
            } else {
                log::error!("Failed to start Babe service: {e:?}");
                Err(e.into())
            }
        }
    }
}

#[allow(clippy::expect_used)]
fn start_aura_service(
    arg_matches: &ArgMatches,
    skip_history_backfill: bool,
) -> Result<(), sc_cli::Error> {
    let cli = Cli::from_arg_matches(arg_matches).expect("Bad arg_matches");
    let runner = cli.create_runner(&cli.run)?;

    // Unlike when the Babe node fails to build due to missing BabeApi in the runtime,
    // there is no way to detect the exit reason for the Aura node when it encounters a Babe block.
    //
    // Passing this atomic bool is a hacky solution, allowing the node to set it to true to indicate
    // a Babe service should be spawned on exit instead of a regular shutdown.
    let custom_service_signal = Arc::new(AtomicBool::new(false));
    let custom_service_signal_clone = custom_service_signal.clone();
    match runner.run_node_until_exit(|config| async move {
        let config = customise_config(arg_matches, config);
        service::build_full::<AuraConsensus>(
            config,
            cli.eth,
            cli.sealing,
            Some(custom_service_signal_clone),
            skip_history_backfill,
        )
        .await
    }) {
        Ok(()) => Ok(()),
        Err(e) => {
            if custom_service_signal.load(std::sync::atomic::Ordering::Relaxed) {
                start_babe_service(arg_matches, skip_history_backfill)
            } else {
                Err(e.into())
            }
        }
    }
}

#[allow(clippy::expect_used)]
fn customise_config(arg_matches: &ArgMatches, config: Configuration) -> Configuration {
    let cli = Cli::from_arg_matches(arg_matches).expect("Bad arg_matches");

    // Polkadot SDK doubles the on-chain heap strategy during block import.
    // Keep Subtensor's historical effective import heap at 60k pages without
    // asking Wasm to pre-grow past its maximum memory.
    let mut config = override_default_heap_pages(config, 30_000);

    // If the operator did **not** supply `--rpc-rate-limit`, disable the limiter.
    if cli.run.rpc_params.rpc_rate_limit.is_none() {
        config.rpc.rate_limit = None;
    }

    // If the operator did **not** supply `--rpc-max-subscriptions-per-connection` set to high value.
    config.rpc.max_subs_per_conn = match arg_matches
        .value_source("rpc_max_subscriptions_per_connection")
    {
        Some(ValueSource::CommandLine) => cli.run.rpc_params.rpc_max_subscriptions_per_connection,
        _ => 10000,
    };

    // If the operator did **not** supply `--rpc-max-connections` set to high value.
    config.rpc.max_connections = match arg_matches.value_source("rpc_max_connections") {
        Some(ValueSource::CommandLine) => cli.run.rpc_params.rpc_max_connections,
        _ => 10000,
    };

    config
}

/// Override default heap pages
fn override_default_heap_pages(config: Configuration, pages: u64) -> Configuration {
    Configuration {
        impl_name: config.impl_name,
        impl_version: config.impl_version,
        role: config.role,
        tokio_handle: config.tokio_handle,
        transaction_pool: config.transaction_pool,
        network: config.network,
        keystore: config.keystore,
        database: config.database,
        trie_cache_maximum_size: config.trie_cache_maximum_size,
        warm_up_trie_cache: config.warm_up_trie_cache,
        state_pruning: config.state_pruning,
        blocks_pruning: config.blocks_pruning,
        chain_spec: config.chain_spec,
        wasm_runtime_overrides: config.wasm_runtime_overrides,
        prometheus_config: config.prometheus_config,
        telemetry_endpoints: config.telemetry_endpoints,
        offchain_worker: config.offchain_worker,
        force_authoring: config.force_authoring,
        disable_grandpa: config.disable_grandpa,
        dev_key_seed: config.dev_key_seed,
        tracing_targets: config.tracing_targets,
        tracing_receiver: config.tracing_receiver,
        announce_block: config.announce_block,
        data_path: config.data_path,
        base_path: config.base_path,
        executor: ExecutorConfiguration {
            default_heap_pages: Some(pages),
            wasm_method: config.executor.wasm_method,
            max_runtime_instances: config.executor.max_runtime_instances,
            runtime_cache_size: config.executor.runtime_cache_size,
        },
        rpc: RpcConfiguration {
            addr: config.rpc.addr,
            max_connections: config.rpc.max_connections,
            cors: config.rpc.cors,
            methods: config.rpc.methods,
            max_request_size: config.rpc.max_request_size,
            max_response_size: config.rpc.max_response_size,
            id_provider: config.rpc.id_provider,
            max_subs_per_conn: config.rpc.max_subs_per_conn,
            port: config.rpc.port,
            message_buffer_capacity: config.rpc.message_buffer_capacity,
            batch_config: config.rpc.batch_config,
            rate_limit: config.rpc.rate_limit,
            rate_limit_whitelisted_ips: config.rpc.rate_limit_whitelisted_ips,
            rate_limit_trust_proxy_headers: config.rpc.rate_limit_trust_proxy_headers,
            request_logger_limit: config.rpc.request_logger_limit,
        },
    }
}
