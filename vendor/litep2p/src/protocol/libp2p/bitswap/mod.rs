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

//! [`/ipfs/bitswap/1.2.0`](https://github.com/ipfs/specs/blob/main/BITSWAP.md) implementation.

use crate::{
    error::Error,
    protocol::{Direction, TransportEvent, TransportService},
    substream::Substream,
    types::SubstreamId,
    PeerId,
};

use cid::Version;
use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};
use multihash::Code;
use prost::Message;
use tokio::sync::mpsc::{Receiver, Sender};

use std::{collections::HashMap, time::Duration};

pub use cid::Cid;
pub use config::Config;
pub use handle::{BitswapCommand, BitswapEvent, BitswapHandle, ResponseType};
pub use schema::bitswap::{wantlist::WantType, BlockPresenceType};

mod config;
mod handle;

mod schema {
    pub(super) mod bitswap {
        include!(concat!(env!("OUT_DIR"), "/bitswap.rs"));
    }
}

/// Log target for the file.
const LOG_TARGET: &str = "litep2p::ipfs::bitswap";

/// Write timeout for outbound messages.
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);

/// Bitswap metadata.
#[derive(Debug)]
struct Prefix {
    /// CID version.
    version: Version,

    /// CID codec.
    codec: u64,

    /// CID multihash type.
    multihash_type: u64,

    /// CID multihash length.
    multihash_len: u8,
}

impl Prefix {
    /// Convert the prefix to encoded bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut res = Vec::with_capacity(4 * 10);

        let mut buf = unsigned_varint::encode::u64_buffer();
        let version = unsigned_varint::encode::u64(self.version.into(), &mut buf);
        res.extend_from_slice(version);

        let mut buf = unsigned_varint::encode::u64_buffer();
        let codec = unsigned_varint::encode::u64(self.codec, &mut buf);
        res.extend_from_slice(codec);

        let mut buf = unsigned_varint::encode::u64_buffer();
        let multihash_type = unsigned_varint::encode::u64(self.multihash_type, &mut buf);
        res.extend_from_slice(multihash_type);

        let mut buf = unsigned_varint::encode::u64_buffer();
        let multihash_len = unsigned_varint::encode::u64(self.multihash_len as u64, &mut buf);
        res.extend_from_slice(multihash_len);
        res
    }
}

/// Bitswap protocol.
pub(crate) struct Bitswap {
    // Connection service.
    service: TransportService,

    /// TX channel for sending events to the user protocol.
    event_tx: Sender<BitswapEvent>,

    /// RX channel for receiving commands from `BitswapHandle`.
    cmd_rx: Receiver<BitswapCommand>,

    /// Pending outbound substreams.
    pending_outbound: HashMap<SubstreamId, Vec<ResponseType>>,

    /// Pending inbound substreams.
    pending_inbound:
        FuturesUnordered<BoxFuture<'static, crate::Result<(PeerId, Vec<(Cid, WantType)>)>>>,
}

impl Bitswap {
    /// Create new [`Bitswap`] protocol.
    pub(crate) fn new(service: TransportService, config: Config) -> Self {
        Self {
            service,
            cmd_rx: config.cmd_rx,
            event_tx: config.event_tx,
            pending_outbound: HashMap::new(),
            pending_inbound: FuturesUnordered::new(),
        }
    }

    /// Substream opened to remote peer.
    fn on_inbound_substream(&mut self, peer: PeerId, mut substream: Substream) {
        tracing::debug!(target: LOG_TARGET, ?peer, "handle inbound substream");

        self.pending_inbound.push(Box::pin(async move {
            let message = substream.next().await.ok_or(Error::ConnectionClosed)??;
            let message = schema::bitswap::Message::decode(message)?;

            let Some(wantlist) = message.wantlist else {
                tracing::debug!(target: LOG_TARGET, "bitswap message doesn't contain `WantList`");
                return Err(Error::InvalidData);
            };

            Ok((
                peer,
                wantlist
                    .entries
                    .into_iter()
                    .filter_map(|entry| {
                        let cid = Cid::read_bytes(entry.block.as_slice()).ok()?;

                        let want_type = match entry.want_type {
                            0 => WantType::Block,
                            1 => WantType::Have,
                            _ => return None,
                        };

                        (cid.version() == cid::Version::V1
                            && cid.hash().code() == u64::from(Code::Blake2b256)
                            && cid.hash().size() == 32)
                            .then_some((cid, want_type))
                    })
                    .collect::<Vec<_>>(),
            ))
        }));
    }

    /// Send response to bitswap request.
    async fn on_outbound_substream(
        &mut self,
        peer: PeerId,
        substream_id: SubstreamId,
        mut substream: Substream,
    ) {
        let Some(entries) = self.pending_outbound.remove(&substream_id) else {
            tracing::warn!(target: LOG_TARGET, ?peer, ?substream_id, "pending outbound entry doesn't exist");
            return;
        };

        let mut response = schema::bitswap::Message::default();

        for entry in entries {
            match entry {
                ResponseType::Block { cid, block } => {
                    let prefix = Prefix {
                        version: cid.version(),
                        codec: cid.codec(),
                        multihash_type: cid.hash().code(),
                        multihash_len: cid.hash().size(),
                    }
                    .to_bytes();

                    response.payload.push(schema::bitswap::Block {
                        prefix,
                        data: block,
                    });
                }
                ResponseType::Presence { cid, presence } => {
                    response.block_presences.push(schema::bitswap::BlockPresence {
                        cid: cid.to_bytes(),
                        r#type: presence as i32,
                    });
                }
            }
        }

        let message = response.encode_to_vec().into();
        let _ = tokio::time::timeout(WRITE_TIMEOUT, substream.send_framed(message)).await;
    }

    /// Handle bitswap response.
    fn on_bitswap_response(&mut self, peer: PeerId, responses: Vec<ResponseType>) {
        match self.service.open_substream(peer) {
            Err(error) => {
                tracing::debug!(target: LOG_TARGET, ?peer, ?error, "failed to open substream to peer")
            }
            Ok(substream_id) => {
                self.pending_outbound.insert(substream_id, responses);
            }
        }
    }

    /// Start [`Bitswap`] event loop.
    pub async fn run(mut self) {
        tracing::debug!(target: LOG_TARGET, "starting bitswap event loop");

        loop {
            tokio::select! {
                event = self.service.next() => match event {
                    Some(TransportEvent::SubstreamOpened {
                        peer,
                        substream,
                        direction,
                        ..
                    }) => match direction {
                        Direction::Inbound => self.on_inbound_substream(peer, substream),
                        Direction::Outbound(substream_id) =>
                            self.on_outbound_substream(peer, substream_id, substream).await,
                    },
                    None => return,
                    event => tracing::trace!(target: LOG_TARGET, ?event, "unhandled event"),
                },
                command = self.cmd_rx.recv() => match command {
                    Some(BitswapCommand::SendResponse { peer, responses }) => {
                        self.on_bitswap_response(peer, responses);
                    }
                    None => return,
                },
                event = self.pending_inbound.next(), if !self.pending_inbound.is_empty() => {
                    if let Some(Ok((peer, cids))) = event {
                        let _ = self.event_tx.send(BitswapEvent::Request { peer, cids }).await;
                    }
                }
            }
        }
    }
}
