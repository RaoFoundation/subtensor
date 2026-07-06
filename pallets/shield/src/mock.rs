use crate as pallet_shield;
use stp_shield::MLKEM768_ENC_KEY_LEN;

use codec::Decode;
use frame_support::pallet_prelude::DispatchError;
use frame_support::traits::{ConstBool, ConstU32, ConstU64};
use frame_support::{BoundedVec, construct_runtime, derive_impl, parameter_types};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::sr25519;
use sp_runtime::{BuildStorage, generic, testing::TestSignature};
use std::cell::RefCell;
use stp_shield::ShieldEncKey;

pub type Block = frame_system::mocking::MockBlock<Test>;

pub type DecodableExtrinsic = generic::UncheckedExtrinsic<u64, RuntimeCall, TestSignature, ()>;
pub type DecodableBlock =
    generic::Block<generic::Header<u64, sp_runtime::traits::BlakeTwo256>, DecodableExtrinsic>;

construct_runtime!(
    pub enum Test {
        System: frame_system = 0,
        Timestamp: pallet_timestamp = 1,
        Aura: pallet_aura = 2,
        MevShield: pallet_shield = 3,
        Utility: pallet_subtensor_utility = 4,
        Balances: pallet_balances = 5,
    }
);

const SLOT_DURATION: u64 = 6000;

parameter_types! {
    pub const SlotDuration: u64 = SLOT_DURATION;
    pub const MaxAuthorities: u32 = 32;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountData = pallet_balances::AccountData<u64>;
}

impl pallet_timestamp::Config for Test {
    type Moment = u64;
    type OnTimestampSet = ();
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
    type WeightInfo = ();
}

impl pallet_aura::Config for Test {
    type AuthorityId = AuraId;
    type DisabledValidators = ();
    type MaxAuthorities = MaxAuthorities;
    type AllowMultipleBlocksPerSlot = ConstBool<false>;
    type SlotDuration = SlotDuration;
}

impl pallet_balances::Config for Test {
    type RuntimeEvent = RuntimeEvent;
    type Balance = u64;
    type DustRemoval = ();
    type ExistentialDeposit = ConstU64<1>;
    type AccountStore = System;
    type WeightInfo = ();
    type MaxLocks = ConstU32<50>;
    type MaxReserves = ConstU32<50>;
    type ReserveIdentifier = [u8; 8];
    type RuntimeHoldReason = RuntimeHoldReason;
    type RuntimeFreezeReason = RuntimeFreezeReason;
    type FreezeIdentifier = RuntimeFreezeReason;
    type MaxFreezes = ConstU32<50>;
    type DoneSlashHandler = ();
}

impl pallet_subtensor_utility::Config for Test {
    type RuntimeCall = RuntimeCall;
    type PalletsOrigin = OriginCaller;
    type WeightInfo = ();
}

thread_local! {
    static MOCK_DKG_AUTHORITIES: RefCell<Vec<mev_shield_ibe_runtime_api::DkgAuthorityInfo>> = RefCell::new(Vec::new());
    static MOCK_CURRENT: RefCell<Option<AuraId>> = const { RefCell::new(None) };
    static MOCK_NEXT_NEXT: RefCell<Option<Option<AuraId>>> = const { RefCell::new(None) };
}

pub struct MockFindAuthors;

impl pallet_shield::FindAuthors<Test> for MockFindAuthors {
    fn find_current_author() -> Option<AuraId> {
        // Thread-local override (unit tests) → Aura fallback (benchmarks).
        MOCK_CURRENT.with(|c| c.borrow().clone()).or_else(|| {
            let slot = Aura::current_slot_from_digests()?;
            let auths = pallet_aura::Authorities::<Test>::get().into_inner();
            let len = auths.len();
            if len == 0 {
                return None;
            }
            let slot_index = usize::try_from(*slot).ok()?;
            let author_index = slot_index.checked_rem(len)?;
            auths.get(author_index).cloned()
        })
    }

    fn find_next_next_author() -> Option<AuraId> {
        if let Some(val) = MOCK_NEXT_NEXT.with(|n| n.borrow().clone()) {
            return val;
        }
        let slot = Aura::current_slot_from_digests()?.checked_add(2)?;
        let auths = pallet_aura::Authorities::<Test>::get().into_inner();
        let len = auths.len();
        if len == 0 {
            return None;
        }
        let slot_index = usize::try_from(slot).ok()?;
        let author_index = slot_index.checked_rem(len)?;
        auths.get(author_index).cloned()
    }
}

/// Mock decryptor that just decodes the bytes without decryption.
pub struct MockDecryptor;

impl pallet_shield::ExtrinsicDecryptor<RuntimeCall> for MockDecryptor {
    fn decrypt(data: &[u8]) -> Result<RuntimeCall, DispatchError> {
        RuntimeCall::decode(&mut &data[..]).map_err(|_| DispatchError::Other("decode failed"))
    }
}

pub struct MockIbeEncryptedTxDecryptor;
impl pallet_shield::IbeEncryptedTxDecryptor<RuntimeCall> for MockIbeEncryptedTxDecryptor {
    fn decrypt(data: &[u8]) -> pallet_shield::IbeDecryptOutcome<RuntimeCall> {
        let Ok(envelope) = stp_mev_shield_ibe::IbeEncryptedExtrinsicV1::decode_v2(data) else {
            return pallet_shield::IbeDecryptOutcome::InvalidAfterKeyAvailable;
        };
        match envelope.ciphertext.first().copied() {
            Some(0xAA) => pallet_shield::IbeDecryptOutcome::Ready(RuntimeCall::System(
                frame_system::Call::remark { remark: vec![0xAA] },
            )),
            Some(0xBB) => pallet_shield::IbeDecryptOutcome::Ready(RuntimeCall::System(
                frame_system::Call::set_heap_pages { pages: 64 },
            )),
            Some(0xCC) => pallet_shield::IbeDecryptOutcome::NotReady,
            _ => pallet_shield::IbeDecryptOutcome::InvalidAfterKeyAvailable,
        }
    }
}

pub struct MockDecryptedExtrinsicExecutor;
impl pallet_shield::DecryptedExtrinsicExecutor<RuntimeCall> for MockDecryptedExtrinsicExecutor {
    fn dispatch_info(inner: &RuntimeCall) -> Option<frame_support::dispatch::DispatchInfo> {
        Some(frame_support::dispatch::GetDispatchInfo::get_dispatch_info(
            inner,
        ))
    }

    fn apply(inner: RuntimeCall) -> pallet_shield::IbeAppliedExtrinsic {
        let info = frame_support::dispatch::GetDispatchInfo::get_dispatch_info(&inner);
        let result = sp_runtime::traits::Dispatchable::dispatch(inner, RuntimeOrigin::signed(1));
        let success = result.is_ok();
        let consumed_weight = result
            .map(|post_info| post_info.actual_weight.unwrap_or(info.call_weight))
            .unwrap_or(info.call_weight);
        pallet_shield::IbeAppliedExtrinsic {
            consumed_weight,
            success,
        }
    }
}

pub struct MockIbeDkgAuthorityProvider;
impl pallet_shield::IbeDkgAuthorityProvider for MockIbeDkgAuthorityProvider {
    fn authorities_for_epoch(_: u64) -> Vec<mev_shield_ibe_runtime_api::DkgAuthorityInfo> {
        MOCK_DKG_AUTHORITIES.with(|authorities| authorities.borrow().clone())
    }
    fn consensus_source_for_epoch(_: u64) -> mev_shield_ibe_runtime_api::DkgConsensusSource {
        mev_shield_ibe_runtime_api::DkgConsensusSource::PoaAuraRootValidators
    }

    fn verify_authority_signature(authority_id: &[u8], _: sp_core::H256, _: &[u8]) -> bool {
        MOCK_DKG_AUTHORITIES.with(|authorities| {
            authorities
                .borrow()
                .iter()
                .any(|authority| authority.authority_id.as_slice() == authority_id)
        })
    }
}

frame_support::parameter_types! {
    pub const TestIbeEpochLength: u64 = 100;
    pub const TestMaxDkgAtoms: u32 = 64;
    pub const TestMaxPendingIbePerSender: u32 = 8;
    pub const TestIbeSubmissionDeposit: u64 = 10;
}

impl pallet_shield::Config for Test {
    type AuthorityId = AuraId;
    type FindAuthors = MockFindAuthors;
    type RuntimeCall = RuntimeCall;
    type ExtrinsicDecryptor = MockDecryptor;
    type InnerExtrinsic = RuntimeCall;
    type IbeEncryptedTxDecryptor = MockIbeEncryptedTxDecryptor;
    type DecryptedExtrinsicExecutor = MockDecryptedExtrinsicExecutor;
    type IbeKeyVerifier = ();
    type WeightInfo = ();

    type EpochLength = TestIbeEpochLength;
    type MaxDkgAtoms = TestMaxDkgAtoms;
    type MaxPendingIbePerSender = TestMaxPendingIbePerSender;
    type Currency = Balances;
    type SubmissionDeposit = TestIbeSubmissionDeposit;
    type IbeDkgAuthorityProvider = MockIbeDkgAuthorityProvider;
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    MOCK_DKG_AUTHORITIES.with(|slot| slot.borrow_mut().clear());
    let mut storage = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("valid genesis");
    pallet_balances::GenesisConfig::<Test> {
        balances: vec![
            (1, 1_000_000_000),
            (2, 1_000_000_000),
            (3, 1_000_000_000),
            (100, 1_000_000_000),
            (200, 1_000_000_000),
        ],
        dev_accounts: None,
    }
    .assimilate_storage(&mut storage)
    .expect("balances genesis");
    let mut ext: sp_io::TestExternalities = storage.into();
    ext.register_extension(sp_keystore::KeystoreExt::new(
        sp_keystore::testing::MemoryKeystore::new(),
    ));
    ext
}

pub fn valid_pk() -> ShieldEncKey {
    BoundedVec::truncate_from(vec![0x42; MLKEM768_ENC_KEY_LEN])
}

pub fn valid_pk_b() -> ShieldEncKey {
    BoundedVec::truncate_from(vec![0x99; MLKEM768_ENC_KEY_LEN])
}

/// Create a deterministic `AuraId` from a simple index for tests.
pub fn author(n: u8) -> AuraId {
    AuraId::from(sr25519::Public::from_raw([n; 32]))
}

pub fn set_dkg_authorities(authorities: Vec<mev_shield_ibe_runtime_api::DkgAuthorityInfo>) {
    MOCK_DKG_AUTHORITIES.with(|slot| *slot.borrow_mut() = authorities);
}
pub fn set_authors(current: Option<AuraId>, next_next: Option<AuraId>) {
    MOCK_CURRENT.with(|c| *c.borrow_mut() = current);
    MOCK_NEXT_NEXT.with(|n| *n.borrow_mut() = Some(next_next));
}

pub fn nest_call(call: RuntimeCall, depth: usize) -> RuntimeCall {
    (0..depth).fold(call, |inner, _| {
        RuntimeCall::Utility(pallet_subtensor_utility::Call::batch { calls: vec![inner] })
    })
}

pub fn build_wire_ciphertext(
    key_hash: &[u8; 16],
    kem_ct: &[u8],
    nonce: &[u8; 24],
    aead_ct: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(key_hash);
    buf.extend_from_slice(&(kem_ct.len() as u16).to_le_bytes());
    buf.extend_from_slice(kem_ct);
    buf.extend_from_slice(nonce);
    buf.extend_from_slice(aead_ct);
    buf
}

pub fn babe_dkg_authority(id: u8, stake: u128) -> mev_shield_ibe_runtime_api::DkgAuthorityInfo {
    mev_shield_ibe_runtime_api::DkgAuthorityInfo {
        hotkey_account_id: vec![id; 32],
        consensus_key_kind: mev_shield_ibe_runtime_api::DkgConsensusKeyKind::BabeSr25519,
        authority_id: vec![id.wrapping_add(100); 32],
        stake,
        dkg_x25519_public_key: [id.wrapping_add(200); 32],
    }
}

pub fn test_ibe_epoch_key(
    epoch: u64,
    key_id: [u8; stp_mev_shield_ibe::KEY_ID_LEN],
    first_block: u64,
    last_block: u64,
) -> stp_mev_shield_ibe::IbeEpochPublicKey {
    stp_mev_shield_ibe::IbeEpochPublicKey {
        epoch,
        key_id,
        master_public_key: BoundedVec::truncate_from(
            vec![0x55; stp_mev_shield_ibe::COMPRESSED_MASTER_PUBLIC_KEY_LEN],
        ),
        total_weight: 100,
        threshold_weight: 67,
        public_atoms: Default::default(),
        first_block,
        last_block,
    }
}

pub fn test_ibe_envelope(
    epoch: u64,
    target_block: u64,
    key_id: [u8; stp_mev_shield_ibe::KEY_ID_LEN],
    commitment_byte: u8,
) -> BoundedVec<u8, crate::MaxEncryptedCallSize> {
    let envelope = stp_mev_shield_ibe::IbeEncryptedExtrinsicV1 {
        magic: stp_mev_shield_ibe::MEV_SHIELD_IBE_MAGIC,
        version: stp_mev_shield_ibe::MEV_SHIELD_IBE_VERSION,
        epoch,
        target_block,
        key_id,
        commitment: sp_core::H256::repeat_byte(commitment_byte),
        ciphertext: vec![commitment_byte; 32],
    };
    BoundedVec::truncate_from(codec::Encode::encode(&envelope))
}
