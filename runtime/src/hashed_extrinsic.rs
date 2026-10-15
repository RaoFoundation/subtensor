//! Standard signed-v4 framing for both ordinary and rotating accounts.
//! No call is intercepted or rewritten while decoding.
use crate::{
    AccountId, Address, ChainContext, Runtime, RuntimeCall, Signature, TxExtension,
    hashed_auth::AccountTxExtension,
};
use codec::{
    Compact, CountedInput, Decode, DecodeLimit, DecodeWithMemTracking, Encode, Input,
    MemTrackingInput,
};
use fp_self_contained::SelfContainedCall;
use frame_support::{
    dispatch::{DispatchInfo, GetDispatchInfo},
    traits::{InherentBuilder, SignedTransactionBuilder},
};
use scale_info::{Type, TypeInfo};
use sp_core::H160;
use sp_runtime::{
    OpaqueExtrinsic,
    generic::{self, ExtrinsicFormat, Preamble},
    traits::{
        Applyable, Checkable, DispatchInfoOf, ExtrinsicCall, ExtrinsicLike, ExtrinsicMetadata,
        PostDispatchInfoOf, TransactionExtension, ValidateUnsigned, Verify,
    },
    transaction_validity::{
        InvalidTransaction, TransactionSource, TransactionValidity, TransactionValidityError,
    },
};
use subtensor_hashed::SignerAddress;
pub type LegacyUnchecked =
    fp_self_contained::UncheckedExtrinsic<Address, RuntimeCall, Signature, AccountTxExtension>;
type LegacyChecked =
    fp_self_contained::CheckedExtrinsic<AccountId, RuntimeCall, AccountTxExtension, H160>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UncheckedExtrinsic {
    Legacy(LegacyUnchecked),
}
impl UncheckedExtrinsic {
    pub fn new_signed(
        call: RuntimeCall,
        address: Address,
        signature: Signature,
        extension: TxExtension,
    ) -> Self {
        let mut extension: AccountTxExtension = extension.into();
        if let Address::Raw(bytes) = &address {
            extension.1 = SignerAddress::from_bytes(bytes).ok();
        }
        Self::Legacy(LegacyUnchecked::new_signed(
            call, address, signature, extension,
        ))
    }
    pub fn new_bare(call: RuntimeCall) -> Self {
        Self::Legacy(LegacyUnchecked::new_bare(call))
    }
    pub fn into_call(self) -> RuntimeCall {
        let Self::Legacy(xt) = self;
        xt.0.function
    }
}
pub fn metadata_at_version(version: u32) -> Option<sp_core::OpaqueMetadata> {
    Runtime::metadata_at_version(version)
}
impl Encode for UncheckedExtrinsic {
    fn encode(&self) -> alloc::vec::Vec<u8> {
        let Self::Legacy(xt) = self;
        xt.encode()
    }
}
fn decode_call<I: Input>(input: &mut I) -> Result<RuntimeCall, codec::Error> {
    // The upstream extrinsic decoder bounds allocations; Executive separately
    // bounds depth when re-decoding a transaction. Also protect direct Decode
    // and serde entry points before they reach Executive. Both wrappers forward
    // the caller's tracking hooks, preserving any stricter enclosing limits.
    let mut input = MemTrackingInput::new(input, 16 * 1024 * 1024 + 1);
    RuntimeCall::decode_with_depth_limit(frame_support::MAX_EXTRINSIC_DEPTH, &mut input)
}

// Bound the variable-length signer before allocation or signature recovery.
fn decode_address<I: Input>(input: &mut I) -> Result<Address, codec::Error> {
    match input.read_byte()? {
        0 => Ok(Address::Id(Decode::decode(input)?)),
        1 => {
            Compact::<()>::decode(input)?;
            Ok(Address::Index(()))
        }
        2 => {
            let len = Compact::<u32>::decode(input)?.0;
            if len != SignerAddress::ENCODED_LEN as u32 {
                return Err("Invalid hashed signer address length".into());
            }
            let bytes: [u8; SignerAddress::ENCODED_LEN] = Decode::decode(input)?;
            SignerAddress::from_bytes(&bytes)?;
            input.on_before_alloc_mem(SignerAddress::ENCODED_LEN)?;
            Ok(Address::Raw(bytes.to_vec()))
        }
        3 => Ok(Address::Address32(Decode::decode(input)?)),
        4 => Ok(Address::Address20(Decode::decode(input)?)),
        _ => Err("Invalid address variant".into()),
    }
}

/// Exact payload shared with the SDK: no call wrapper or extra wire fields.
pub(crate) fn signing_hash(
    address: &SignerAddress,
    call: &RuntimeCall,
    extension: &TxExtension,
) -> Result<[u8; 32], TransactionValidityError> {
    Ok(address.signing_hash(&(call, extension, extension.implicit()?).encode()))
}

impl Decode for UncheckedExtrinsic {
    fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
        let expected: Compact<u32> = Decode::decode(input)?;
        let mut input = CountedInput::new(input);
        let preamble = match input.read_byte()? {
            0x84 => {
                let address = decode_address(&mut input)?;
                let signature = Signature::decode(&mut input)?;
                let mut extension = AccountTxExtension::decode(&mut input)?;
                if let Address::Raw(bytes) = &address {
                    extension.1 = Some(SignerAddress::from_bytes(bytes)?);
                }
                Preamble::Signed(address, signature, extension)
            }
            version @ (4 | 5) => Preamble::Bare(version),
            0x45 => {
                if input.read_byte()? != 0 {
                    return Err("Unsupported transaction extension version".into());
                }
                Preamble::General(0, AccountTxExtension::decode(&mut input)?)
            }
            _ => return Err("Invalid extrinsic format".into()),
        };
        let function = decode_call(&mut input)?;
        if input.count() != u64::from(expected.0) {
            return Err("Invalid extrinsic length prefix".into());
        }
        Ok(Self::Legacy(
            generic::UncheckedExtrinsic { preamble, function }.into(),
        ))
    }
}
impl DecodeWithMemTracking for UncheckedExtrinsic {}
impl TypeInfo for UncheckedExtrinsic {
    type Identity = Self;
    fn type_info() -> Type {
        LegacyUnchecked::type_info()
    }
}
impl ExtrinsicLike for UncheckedExtrinsic {
    fn is_bare(&self) -> bool {
        let Self::Legacy(xt) = self;
        xt.is_bare()
    }
}
impl ExtrinsicCall for UncheckedExtrinsic {
    type Call = RuntimeCall;
    fn call(&self) -> &RuntimeCall {
        let Self::Legacy(xt) = self;
        &xt.0.function
    }
}
impl GetDispatchInfo for UncheckedExtrinsic {
    fn get_dispatch_info(&self) -> DispatchInfo {
        let Self::Legacy(xt) = self;
        let mut info = xt.0.function.get_dispatch_info();
        if let Preamble::Signed(_, _, extension) = &xt.0.preamble {
            info.extension_weight = extension.weight(&xt.0.function);
        }
        info
    }
}
impl ExtrinsicMetadata for UncheckedExtrinsic {
    const VERSIONS: &'static [u8] = &[4, 5];
    type TransactionExtensions = AccountTxExtension;
}
impl SignedTransactionBuilder for UncheckedExtrinsic {
    type Address = Address;
    type Signature = Signature;
    type Extension = TxExtension;
    fn new_signed_transaction(
        call: RuntimeCall,
        signed: Address,
        signature: Signature,
        ext: TxExtension,
    ) -> Self {
        Self::new_signed(call, signed, signature, ext)
    }
}
impl InherentBuilder for UncheckedExtrinsic {
    fn new_inherent(call: RuntimeCall) -> Self {
        Self::new_bare(call)
    }
}
impl From<UncheckedExtrinsic> for OpaqueExtrinsic {
    #[allow(clippy::expect_used)]
    fn from(xt: UncheckedExtrinsic) -> Self {
        OpaqueExtrinsic::from_bytes(&xt.encode())
            .expect("locally encoded extrinsic is length framed")
    }
}
impl serde::Serialize for UncheckedExtrinsic {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.encode())
    }
}

impl<'de> serde::Deserialize<'de> for UncheckedExtrinsic {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = sp_core::bytes::deserialize(deserializer)?;
        let mut input = bytes.as_slice();
        let xt = Self::decode(&mut input).map_err(serde::de::Error::custom)?;
        if !input.is_empty() {
            return Err(serde::de::Error::custom("Trailing extrinsic bytes"));
        }
        Ok(xt)
    }
}

pub struct CheckedExtrinsic(LegacyChecked);
impl Checkable<ChainContext> for UncheckedExtrinsic {
    type Checked = CheckedExtrinsic;
    fn check(self, context: &ChainContext) -> Result<Self::Checked, TransactionValidityError> {
        let Self::Legacy(mut xt) = self;
        if let Preamble::Signed(address, signature, extension) = &mut xt.0.preamble {
            // Context is derived only from the signed address, never caller-supplied state.
            extension.1 = None;
            if let Address::Raw(bytes) = address {
                if xt.0.function.is_self_contained() {
                    return Err(InvalidTransaction::Call.into());
                }
                let address =
                    SignerAddress::from_bytes(bytes).map_err(|_| InvalidTransaction::BadProof)?;
                let matches = matches!(
                    (&*signature, address.scheme),
                    (Signature::Sr25519(_), subtensor_hashed::Scheme::Sr25519)
                        | (Signature::Ed25519(_), subtensor_hashed::Scheme::Ed25519)
                );
                if !matches
                    || !signature.verify(
                        signing_hash(&address, &xt.0.function, &extension.0)?.as_slice(),
                        &AccountId::new(address.public_key),
                    )
                {
                    return Err(InvalidTransaction::BadProof.into());
                }
                let account = AccountId::new(address.account);
                extension.1 = Some(address);
                return Ok(CheckedExtrinsic(LegacyChecked {
                    signed: fp_self_contained::CheckedSignature::GenericDelegated(
                        ExtrinsicFormat::Signed(account, extension.clone()),
                    ),
                    function: xt.0.function,
                }));
            }
        }
        Ok(CheckedExtrinsic(xt.check(context)?))
    }
    #[cfg(feature = "try-runtime")]
    fn unchecked_into_checked_i_know_what_i_am_doing(
        self,
        context: &ChainContext,
    ) -> Result<Self::Checked, TransactionValidityError> {
        let Self::Legacy(xt) = &self;
        if matches!(&xt.0.preamble, Preamble::Signed(Address::Raw(_), _, _)) {
            return self.check(context);
        }
        let Self::Legacy(xt) = self;
        Ok(CheckedExtrinsic(
            xt.unchecked_into_checked_i_know_what_i_am_doing(context)?,
        ))
    }
}
impl GetDispatchInfo for CheckedExtrinsic {
    fn get_dispatch_info(&self) -> DispatchInfo {
        let mut info = self.0.function.get_dispatch_info();
        if let fp_self_contained::CheckedSignature::GenericDelegated(ExtrinsicFormat::Signed(
            _,
            extension,
        )) = &self.0.signed
        {
            info.extension_weight = extension.weight(&self.0.function);
        }
        info
    }
}
impl Applyable for CheckedExtrinsic {
    type Call = RuntimeCall;
    fn validate<U: ValidateUnsigned<Call = RuntimeCall>>(
        &self,
        source: TransactionSource,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> TransactionValidity {
        self.0.validate::<U>(source, info, len)
    }
    fn apply<U: ValidateUnsigned<Call = RuntimeCall>>(
        self,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> sp_runtime::ApplyExtrinsicResultWithInfo<PostDispatchInfoOf<RuntimeCall>> {
        let authorized = matches!(&self.0.signed,
            fp_self_contained::CheckedSignature::GenericDelegated(ExtrinsicFormat::Signed(_, extension)) if extension.1.is_some());
        if !authorized {
            return self.0.apply::<U>(info, len);
        }
        frame_support::storage::transactional::with_transaction_opaque_err(|| {
            let result = self.0.apply::<U>(info, len);
            if result.is_ok() {
                frame_support::storage::TransactionOutcome::Commit(result)
            } else {
                frame_support::storage::TransactionOutcome::Rollback(result)
            }
        })
        .map_err(|_| InvalidTransaction::ExhaustsResources)?
    }
    fn call(&self) -> &RuntimeCall {
        &self.0.function
    }
}

/// The actual signed call is inspected directly; no wrapper is decoded.
pub(crate) fn shielded_transaction(
    xt: UncheckedExtrinsic,
) -> Option<stp_shield::ShieldedTransaction> {
    crate::MevShield::try_decode_shielded_tx::<crate::Block, ChainContext>(xt)
}
