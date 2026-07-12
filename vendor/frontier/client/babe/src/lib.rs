use std::{marker::PhantomData, sync::Arc};

use fc_rpc::pending::ConsensusDataProvider;
use sc_client_api::{AuxStore, UsageProvider};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::Error;
use sp_consensus_babe::{digests::CompatibleDigestItem, BabeApi, BabeConfiguration, Slot};
use sp_inherents::InherentData;
use sp_runtime::{traits::Block as BlockT, Digest, DigestItem};
use sp_timestamp::TimestampInherentData;

pub struct BabeConsensusDataProvider<B, C> {
	babe_config: BabeConfiguration,
	_phantom: PhantomData<(B, C)>,
}

impl<B, C> BabeConsensusDataProvider<B, C>
where
	B: BlockT,
	C: AuxStore + ProvideRuntimeApi<B> + UsageProvider<B>,
	C::Api: BabeApi<B>,
{
	/// Creates a new instance of the [`BabeConsensusDataProvider`], requires that `client`
	/// implements [`sp_consensus_babe::BabeApi`]
	pub fn new(client: Arc<C>) -> Result<Self, Error> {
		let babe_config = sc_consensus_babe::configuration(&*client)?;
		Ok(Self {
			babe_config,
			_phantom: PhantomData,
		})
	}
}

impl<B: BlockT, C: Send + Sync> ConsensusDataProvider<B> for BabeConsensusDataProvider<B, C> {
	fn create_digest(
		&self,
		_parent: &B::Header,
		data: &InherentData,
	) -> Result<Digest, sp_inherents::Error> {
		let timestamp = data
			.timestamp_inherent_data()?
			.expect("Timestamp is always present; qed");

		let slot_duration = self.babe_config.slot_duration();
		let slot = Slot::from_timestamp(timestamp, slot_duration);

		let digest_item = <DigestItem as CompatibleDigestItem>::babe_pre_digest(
			sp_consensus_babe::digests::PreDigest::SecondaryPlain(
				sp_consensus_babe::digests::SecondaryPlainPreDigest {
					slot,
					authority_index: 0, // Use first authority for pending blocks
				},
			),
		);

		Ok(Digest {
			logs: vec![digest_item],
		})
	}
}
