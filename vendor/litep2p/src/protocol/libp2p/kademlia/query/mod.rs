// Copyright 2023 litep2p developers
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use crate::{
    protocol::libp2p::kademlia::{
        message::KademliaMessage,
        query::{
            find_node::{FindNodeConfig, FindNodeContext},
            get_providers::{GetProvidersConfig, GetProvidersContext},
            get_record::{GetRecordConfig, GetRecordContext},
        },
        record::{ContentProvider, Key as RecordKey, Record},
        types::{KademliaPeer, Key},
        PeerRecord, Quorum,
    },
    PeerId,
};

use bytes::Bytes;

use std::collections::{HashMap, VecDeque};

use self::find_many_nodes::FindManyNodesContext;

mod find_many_nodes;
mod find_node;
mod get_providers;
mod get_record;

/// Logging target for the file.
const LOG_TARGET: &str = "litep2p::ipfs::kademlia::query";

/// Type representing a query ID.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "fuzz", derive(serde::Serialize, serde::Deserialize))]
pub struct QueryId(pub usize);

/// Query type.
#[derive(Debug)]
enum QueryType {
    /// `FIND_NODE` query.
    FindNode {
        /// Context for the `FIND_NODE` query.
        context: FindNodeContext<PeerId>,
    },

    /// `PUT_VALUE` query.
    PutRecord {
        /// Record that needs to be stored.
        record: Record,

        /// Context for the `FIND_NODE` query.
        context: FindNodeContext<RecordKey>,
    },

    /// `PUT_VALUE` query to specified peers.
    PutRecordToPeers {
        /// Record that needs to be stored.
        record: Record,

        /// Context for finding peers.
        context: FindManyNodesContext,
    },

    /// `GET_VALUE` query.
    GetRecord {
        /// Context for the `GET_VALUE` query.
        context: GetRecordContext,
    },

    /// `ADD_PROVIDER` query.
    AddProvider {
        /// Provided key.
        provided_key: RecordKey,

        /// Provider record that need to be stored.
        provider: ContentProvider,

        /// Context for the `FIND_NODE` query.
        context: FindNodeContext<RecordKey>,
    },

    /// `GET_PROVIDERS` query.
    GetProviders {
        /// Context for the `GET_PROVIDERS` query.
        context: GetProvidersContext,
    },
}

/// Query action.
#[derive(Debug)]
pub enum QueryAction {
    /// Send message to peer.
    SendMessage {
        /// Query ID.
        query: QueryId,

        /// Peer.
        peer: PeerId,

        /// Message.
        message: Bytes,
    },

    /// `FIND_NODE` query succeeded.
    FindNodeQuerySucceeded {
        /// ID of the query that succeeded.
        query: QueryId,

        /// Target peer.
        target: PeerId,

        /// Peers that were found.
        peers: Vec<KademliaPeer>,
    },

    /// Store the record to nodes closest to target key.
    PutRecordToFoundNodes {
        /// Target peer.
        record: Record,

        /// Peers for whom the `PUT_VALUE` must be sent to.
        peers: Vec<KademliaPeer>,
    },

    /// Add the provider record to nodes closest to the target key.
    AddProviderToFoundNodes {
        /// Provided key.
        provided_key: RecordKey,

        /// Provider record.
        provider: ContentProvider,

        /// Peers for whom the `ADD_PROVIDER` must be sent to.
        peers: Vec<KademliaPeer>,
    },

    /// `GET_VALUE` query succeeded.
    GetRecordQueryDone {
        /// Query ID.
        query_id: QueryId,
    },

    /// `GET_VALUE` inflight query produced a result.
    ///
    /// This event is emitted when a peer responds to the query with a record.
    GetRecordPartialResult {
        /// Query ID.
        query_id: QueryId,

        /// Found record.
        record: PeerRecord,
    },

    /// `GET_PROVIDERS` query succeeded.
    GetProvidersQueryDone {
        /// Query ID.
        query_id: QueryId,

        /// Provided key.
        provided_key: RecordKey,

        /// Found providers.
        providers: Vec<ContentProvider>,
    },

    /// Query succeeded.
    QuerySucceeded {
        /// ID of the query that succeeded.
        query: QueryId,
    },

    /// Query failed.
    QueryFailed {
        /// ID of the query that failed.
        query: QueryId,
    },
}

/// Kademlia query engine.
pub struct QueryEngine {
    /// Local peer ID.
    local_peer_id: PeerId,

    /// Replication factor.
    replication_factor: usize,

    /// Parallelism factor.
    parallelism_factor: usize,

    /// Active queries.
    queries: HashMap<QueryId, QueryType>,
}

impl QueryEngine {
    /// Create new [`QueryEngine`].
    pub fn new(
        local_peer_id: PeerId,
        replication_factor: usize,
        parallelism_factor: usize,
    ) -> Self {
        Self {
            local_peer_id,
            replication_factor,
            parallelism_factor,
            queries: HashMap::new(),
        }
    }

    /// Start `FIND_NODE` query.
    pub fn start_find_node(
        &mut self,
        query_id: QueryId,
        target: PeerId,
        candidates: VecDeque<KademliaPeer>,
    ) -> QueryId {
        tracing::debug!(
            target: LOG_TARGET,
            ?query_id,
            ?target,
            num_peers = ?candidates.len(),
            "start `FIND_NODE` query"
        );

        let target = Key::from(target);
        let config = FindNodeConfig {
            local_peer_id: self.local_peer_id,
            replication_factor: self.replication_factor,
            parallelism_factor: self.parallelism_factor,
            query: query_id,
            target,
        };

        self.queries.insert(
            query_id,
            QueryType::FindNode {
                context: FindNodeContext::new(config, candidates),
            },
        );

        query_id
    }

    /// Start `PUT_VALUE` query.
    pub fn start_put_record(
        &mut self,
        query_id: QueryId,
        record: Record,
        candidates: VecDeque<KademliaPeer>,
    ) -> QueryId {
        tracing::debug!(
            target: LOG_TARGET,
            ?query_id,
            target = ?record.key,
            num_peers = ?candidates.len(),
            "start `PUT_VALUE` query"
        );

        let target = Key::new(record.key.clone());
        let config = FindNodeConfig {
            local_peer_id: self.local_peer_id,
            replication_factor: self.replication_factor,
            parallelism_factor: self.parallelism_factor,
            query: query_id,
            target,
        };

        self.queries.insert(
            query_id,
            QueryType::PutRecord {
                record,
                context: FindNodeContext::new(config, candidates),
            },
        );

        query_id
    }

    /// Start `PUT_VALUE` query to specified peers.
    pub fn start_put_record_to_peers(
        &mut self,
        query_id: QueryId,
        record: Record,
        peers_to_report: Vec<KademliaPeer>,
    ) -> QueryId {
        tracing::debug!(
            target: LOG_TARGET,
            ?query_id,
            target = ?record.key,
            num_peers = ?peers_to_report.len(),
            "start `PUT_VALUE` query to peers"
        );

        self.queries.insert(
            query_id,
            QueryType::PutRecordToPeers {
                record,
                context: FindManyNodesContext::new(query_id, peers_to_report),
            },
        );

        query_id
    }

    /// Start `GET_VALUE` query.
    pub fn start_get_record(
        &mut self,
        query_id: QueryId,
        target: RecordKey,
        candidates: VecDeque<KademliaPeer>,
        quorum: Quorum,
        local_record: bool,
    ) -> QueryId {
        tracing::debug!(
            target: LOG_TARGET,
            ?query_id,
            ?target,
            num_peers = ?candidates.len(),
            "start `GET_VALUE` query"
        );

        let target = Key::new(target);
        let config = GetRecordConfig {
            local_peer_id: self.local_peer_id,
            known_records: if local_record { 1 } else { 0 },
            quorum,
            replication_factor: self.replication_factor,
            parallelism_factor: self.parallelism_factor,
            query: query_id,
            target,
        };

        self.queries.insert(
            query_id,
            QueryType::GetRecord {
                context: GetRecordContext::new(config, candidates, local_record),
            },
        );

        query_id
    }

    /// Start `ADD_PROVIDER` query.
    pub fn start_add_provider(
        &mut self,
        query_id: QueryId,
        provided_key: RecordKey,
        provider: ContentProvider,
        candidates: VecDeque<KademliaPeer>,
    ) -> QueryId {
        tracing::debug!(
            target: LOG_TARGET,
            ?query_id,
            ?provider,
            num_peers = ?candidates.len(),
            "start `ADD_PROVIDER` query",
        );

        let config = FindNodeConfig {
            local_peer_id: self.local_peer_id,
            replication_factor: self.replication_factor,
            parallelism_factor: self.parallelism_factor,
            query: query_id,
            target: Key::new(provided_key.clone()),
        };

        self.queries.insert(
            query_id,
            QueryType::AddProvider {
                provided_key,
                provider,
                context: FindNodeContext::new(config, candidates),
            },
        );

        query_id
    }

    /// Start `GET_PROVIDERS` query.
    pub fn start_get_providers(
        &mut self,
        query_id: QueryId,
        key: RecordKey,
        candidates: VecDeque<KademliaPeer>,
        known_providers: Vec<ContentProvider>,
    ) -> QueryId {
        tracing::debug!(
            target: LOG_TARGET,
            ?query_id,
            ?key,
            num_peers = ?candidates.len(),
            "start `GET_PROVIDERS` query",
        );

        let target = Key::new(key);
        let config = GetProvidersConfig {
            local_peer_id: self.local_peer_id,
            parallelism_factor: self.parallelism_factor,
            query: query_id,
            target,
            known_providers: known_providers.into_iter().map(Into::into).collect(),
        };

        self.queries.insert(
            query_id,
            QueryType::GetProviders {
                context: GetProvidersContext::new(config, candidates),
            },
        );

        query_id
    }

    /// Register response failure from a queried peer.
    pub fn register_response_failure(&mut self, query: QueryId, peer: PeerId) {
        tracing::trace!(target: LOG_TARGET, ?query, ?peer, "register response failure");

        match self.queries.get_mut(&query) {
            None => {
                tracing::trace!(target: LOG_TARGET, ?query, ?peer, "response failure for a stale query");
            }
            Some(QueryType::FindNode { context }) => {
                context.register_response_failure(peer);
            }
            Some(QueryType::PutRecord { context, .. }) => {
                context.register_response_failure(peer);
            }
            Some(QueryType::PutRecordToPeers { context, .. }) => {
                context.register_response_failure(peer);
            }
            Some(QueryType::GetRecord { context }) => {
                context.register_response_failure(peer);
            }
            Some(QueryType::AddProvider { context, .. }) => {
                context.register_response_failure(peer);
            }
            Some(QueryType::GetProviders { context }) => {
                context.register_response_failure(peer);
            }
        }
    }

    /// Register that `response` received from `peer`.
    pub fn register_response(
        &mut self,
        query: QueryId,
        peer: PeerId,
        message: KademliaMessage,
    ) -> Option<QueryAction> {
        tracing::trace!(target: LOG_TARGET, ?query, ?peer, "register response");

        match self.queries.get_mut(&query) {
            None => {
                tracing::trace!(target: LOG_TARGET, ?query, ?peer, "response failure for a stale query");
            }
            Some(QueryType::FindNode { context }) => match message {
                KademliaMessage::FindNode { peers, .. } => {
                    context.register_response(peer, peers);
                }
                _ => unreachable!(),
            },
            Some(QueryType::PutRecord { context, .. }) => match message {
                KademliaMessage::FindNode { peers, .. } => {
                    context.register_response(peer, peers);
                }
                _ => unreachable!(),
            },
            Some(QueryType::PutRecordToPeers { context, .. }) => match message {
                KademliaMessage::FindNode { peers, .. } => {
                    context.register_response(peer, peers);
                }
                _ => unreachable!(),
            },
            Some(QueryType::GetRecord { context }) => match message {
                KademliaMessage::GetRecord { record, peers, .. } =>
                    context.register_response(peer, record, peers),
                _ => unreachable!(),
            },
            Some(QueryType::AddProvider { context, .. }) => match message {
                KademliaMessage::FindNode { peers, .. } => {
                    context.register_response(peer, peers);
                }
                _ => unreachable!(),
            },
            Some(QueryType::GetProviders { context }) => match message {
                KademliaMessage::GetProviders {
                    key: _,
                    providers,
                    peers,
                } => {
                    context.register_response(peer, providers, peers);
                }
                _ => unreachable!(),
            },
        }

        None
    }

    /// Get next action for `peer` from the [`QueryEngine`].
    pub fn next_peer_action(&mut self, query: &QueryId, peer: &PeerId) -> Option<QueryAction> {
        tracing::trace!(target: LOG_TARGET, ?query, ?peer, "get next peer action");

        match self.queries.get_mut(query) {
            None => {
                tracing::trace!(target: LOG_TARGET, ?query, ?peer, "response failure for a stale query");
                None
            }
            Some(QueryType::FindNode { context }) => context.next_peer_action(peer),
            Some(QueryType::PutRecord { context, .. }) => context.next_peer_action(peer),
            Some(QueryType::PutRecordToPeers { context, .. }) => context.next_peer_action(peer),
            Some(QueryType::GetRecord { context }) => context.next_peer_action(peer),
            Some(QueryType::AddProvider { context, .. }) => context.next_peer_action(peer),
            Some(QueryType::GetProviders { context }) => context.next_peer_action(peer),
        }
    }

    /// Handle query success by returning the queried value(s)
    /// and removing the query from [`QueryEngine`].
    fn on_query_succeeded(&mut self, query: QueryId) -> QueryAction {
        match self.queries.remove(&query).expect("query to exist") {
            QueryType::FindNode { context } => QueryAction::FindNodeQuerySucceeded {
                query,
                target: context.config.target.into_preimage(),
                peers: context.responses.into_values().collect::<Vec<_>>(),
            },
            QueryType::PutRecord { record, context } => QueryAction::PutRecordToFoundNodes {
                record,
                peers: context.responses.into_values().collect::<Vec<_>>(),
            },
            QueryType::PutRecordToPeers { record, context } => QueryAction::PutRecordToFoundNodes {
                record,
                peers: context.peers_to_report,
            },
            QueryType::GetRecord { context } => QueryAction::GetRecordQueryDone {
                query_id: context.config.query,
            },
            QueryType::AddProvider {
                provided_key,
                provider,
                context,
            } => QueryAction::AddProviderToFoundNodes {
                provided_key,
                provider,
                peers: context.responses.into_values().collect::<Vec<_>>(),
            },
            QueryType::GetProviders { context } => QueryAction::GetProvidersQueryDone {
                query_id: context.config.query,
                provided_key: context.config.target.clone().into_preimage(),
                providers: context.found_providers(),
            },
        }
    }

    /// Handle query failure by removing the query from [`QueryEngine`] and
    /// returning the appropriate [`QueryAction`] to user.
    fn on_query_failed(&mut self, query: QueryId) -> QueryAction {
        let _ = self.queries.remove(&query).expect("query to exist");

        QueryAction::QueryFailed { query }
    }

    /// Get next action from the [`QueryEngine`].
    pub fn next_action(&mut self) -> Option<QueryAction> {
        for (_, state) in self.queries.iter_mut() {
            let action = match state {
                QueryType::FindNode { context } => context.next_action(),
                QueryType::PutRecord { context, .. } => context.next_action(),
                QueryType::PutRecordToPeers { context, .. } => context.next_action(),
                QueryType::GetRecord { context } => context.next_action(),
                QueryType::AddProvider { context, .. } => context.next_action(),
                QueryType::GetProviders { context } => context.next_action(),
            };

            match action {
                Some(QueryAction::QuerySucceeded { query }) => {
                    return Some(self.on_query_succeeded(query));
                }
                Some(QueryAction::QueryFailed { query }) =>
                    return Some(self.on_query_failed(query)),
                Some(_) => return action,
                _ => continue,
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use multihash::{Code, Multihash};

    use super::*;
    use crate::protocol::libp2p::kademlia::types::ConnectionType;

    // make fixed peer id
    fn make_peer_id(first: u8, second: u8) -> PeerId {
        let mut peer_id = vec![0u8; 32];
        peer_id[0] = first;
        peer_id[1] = second;

        PeerId::from_bytes(
            &Multihash::wrap(Code::Identity.into(), &peer_id)
                .expect("The digest size is never too large")
                .to_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn query_fails() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        let mut engine = QueryEngine::new(PeerId::random(), 20usize, 3usize);
        let target_peer = PeerId::random();
        let _target_key = Key::from(target_peer);

        let query = engine.start_find_node(
            QueryId(1337),
            target_peer,
            vec![
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
            ]
            .into(),
        );

        for _ in 0..4 {
            if let Some(QueryAction::SendMessage { query, peer, .. }) = engine.next_action() {
                engine.register_response_failure(query, peer);
            }
        }

        if let Some(QueryAction::QueryFailed { query: failed }) = engine.next_action() {
            assert_eq!(failed, query);
        }

        assert!(engine.next_action().is_none());
    }

    #[test]
    fn lookup_paused() {
        let mut engine = QueryEngine::new(PeerId::random(), 20usize, 3usize);
        let target_peer = PeerId::random();
        let _target_key = Key::from(target_peer);

        let _ = engine.start_find_node(
            QueryId(1338),
            target_peer,
            vec![
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
                KademliaPeer::new(PeerId::random(), vec![], ConnectionType::NotConnected),
            ]
            .into(),
        );

        for _ in 0..3 {
            let _ = engine.next_action();
        }

        assert!(engine.next_action().is_none());
    }

    #[test]
    fn find_node_query_succeeds() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        let mut engine = QueryEngine::new(PeerId::random(), 20usize, 3usize);
        let target_peer = make_peer_id(0, 0);
        let target_key = Key::from(target_peer);

        let distances = {
            let mut distances = std::collections::BTreeMap::new();

            for i in 1..64 {
                let peer = make_peer_id(i, 0);
                let key = Key::from(peer);

                distances.insert(target_key.distance(&key), peer);
            }

            distances
        };
        let mut iter = distances.iter();

        // start find node with one known peer
        let _query = engine.start_find_node(
            QueryId(1339),
            target_peer,
            vec![KademliaPeer::new(
                *iter.next().unwrap().1,
                vec![],
                ConnectionType::NotConnected,
            )]
            .into(),
        );

        let action = engine.next_action();
        assert!(engine.next_action().is_none());

        // the one known peer responds with 3 other peers it knows
        match action {
            Some(QueryAction::SendMessage { query, peer, .. }) => {
                engine.register_response(
                    query,
                    peer,
                    KademliaMessage::FindNode {
                        target: Vec::new(),
                        peers: vec![
                            KademliaPeer::new(
                                *iter.next().unwrap().1,
                                vec![],
                                ConnectionType::NotConnected,
                            ),
                            KademliaPeer::new(
                                *iter.next().unwrap().1,
                                vec![],
                                ConnectionType::NotConnected,
                            ),
                            KademliaPeer::new(
                                *iter.next().unwrap().1,
                                vec![],
                                ConnectionType::NotConnected,
                            ),
                        ],
                    },
                );
            }
            _ => panic!("invalid event received"),
        }

        // send empty response for the last three nodes
        for _ in 0..3 {
            match engine.next_action() {
                Some(QueryAction::SendMessage { query, peer, .. }) => {
                    println!("next send message to {peer:?}");
                    engine.register_response(
                        query,
                        peer,
                        KademliaMessage::FindNode {
                            target: Vec::new(),
                            peers: vec![],
                        },
                    );
                }
                _ => panic!("invalid event received"),
            }
        }

        match engine.next_action() {
            Some(QueryAction::FindNodeQuerySucceeded { peers, .. }) => {
                assert_eq!(peers.len(), 4);
            }
            _ => panic!("invalid event received"),
        }

        assert!(engine.next_action().is_none());
    }

    #[test]
    fn put_record_succeeds() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        let mut engine = QueryEngine::new(PeerId::random(), 20usize, 3usize);
        let record_key = RecordKey::new(&vec![1, 2, 3, 4]);
        let target_key = Key::new(record_key.clone());
        let original_record = Record::new(record_key.clone(), vec![1, 3, 3, 7, 1, 3, 3, 8]);

        let distances = {
            let mut distances = std::collections::BTreeMap::new();

            for i in 1..64 {
                let peer = make_peer_id(i, 0);
                let key = Key::from(peer);

                distances.insert(target_key.distance(&key), peer);
            }

            distances
        };
        let mut iter = distances.iter();

        // start find node with one known peer
        let _query = engine.start_put_record(
            QueryId(1340),
            original_record.clone(),
            vec![KademliaPeer::new(
                *iter.next().unwrap().1,
                vec![],
                ConnectionType::NotConnected,
            )]
            .into(),
        );

        let action = engine.next_action();
        assert!(engine.next_action().is_none());

        // the one known peer responds with 3 other peers it knows
        match action {
            Some(QueryAction::SendMessage { query, peer, .. }) => {
                engine.register_response(
                    query,
                    peer,
                    KademliaMessage::FindNode {
                        target: Vec::new(),
                        peers: vec![
                            KademliaPeer::new(
                                *iter.next().unwrap().1,
                                vec![],
                                ConnectionType::NotConnected,
                            ),
                            KademliaPeer::new(
                                *iter.next().unwrap().1,
                                vec![],
                                ConnectionType::NotConnected,
                            ),
                            KademliaPeer::new(
                                *iter.next().unwrap().1,
                                vec![],
                                ConnectionType::NotConnected,
                            ),
                        ],
                    },
                );
            }
            _ => panic!("invalid event received"),
        }

        // send empty response for the last three nodes
        for _ in 0..3 {
            match engine.next_action() {
                Some(QueryAction::SendMessage { query, peer, .. }) => {
                    println!("next send message to {peer:?}");
                    engine.register_response(
                        query,
                        peer,
                        KademliaMessage::FindNode {
                            target: Vec::new(),
                            peers: vec![],
                        },
                    );
                }
                _ => panic!("invalid event received"),
            }
        }

        let peers = match engine.next_action() {
            Some(QueryAction::PutRecordToFoundNodes { peers, record }) => {
                assert_eq!(peers.len(), 4);
                assert_eq!(record.key, original_record.key);
                assert_eq!(record.value, original_record.value);
                peers
            }
            _ => panic!("invalid event received"),
        };

        assert!(engine.next_action().is_none());

        // get records from those peers.
        let _query = engine.start_get_record(
            QueryId(1341),
            record_key.clone(),
            vec![
                KademliaPeer::new(peers[0].peer, vec![], ConnectionType::NotConnected),
                KademliaPeer::new(peers[1].peer, vec![], ConnectionType::NotConnected),
                KademliaPeer::new(peers[2].peer, vec![], ConnectionType::NotConnected),
                KademliaPeer::new(peers[3].peer, vec![], ConnectionType::NotConnected),
            ]
            .into(),
            Quorum::All,
            false,
        );

        let mut records = Vec::new();
        for _ in 0..4 {
            match engine.next_action() {
                Some(QueryAction::SendMessage { query, peer, .. }) => {
                    assert_eq!(query, QueryId(1341));
                    engine.register_response(
                        query,
                        peer,
                        KademliaMessage::GetRecord {
                            record: Some(original_record.clone()),
                            peers: vec![],
                            key: Some(record_key.clone()),
                        },
                    );
                }
                event => panic!("invalid event received {:?}", event),
            }

            // GetRecordPartialResult is emitted after the `register_response` if the record is
            // valid.
            match engine.next_action() {
                Some(QueryAction::GetRecordPartialResult { query_id, record }) => {
                    println!("Partial result {:?}", record);
                    assert_eq!(query_id, QueryId(1341));
                    records.push(record);
                }
                event => panic!("invalid event received {:?}", event),
            }
        }

        let peers: std::collections::HashSet<_> = peers.into_iter().map(|p| p.peer).collect();
        match engine.next_action() {
            Some(QueryAction::GetRecordQueryDone { .. }) => {
                println!("Records {:?}", records);
                let query_peers = records
                    .iter()
                    .map(|peer_record| peer_record.peer)
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(peers, query_peers);

                let records: std::collections::HashSet<_> =
                    records.into_iter().map(|peer_record| peer_record.record).collect();
                // One single record found across peers.
                assert_eq!(records.len(), 1);
                let record = records.into_iter().next().unwrap();

                assert_eq!(record.key, original_record.key);
                assert_eq!(record.value, original_record.value);
            }
            event => panic!("invalid event received {:?}", event),
        }
    }
}
