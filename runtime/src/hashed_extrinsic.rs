//! Deliberate wire-format routing: legacy v4 bytes remain byte-for-byte unchanged.
//! Hasheds use v5 General transactions with extension version 1. The pinned SDK
//! otherwise decodes one extension type for both formats, which cannot preserve
//! v4 compatibility when an authorization proof is added to v5.

use alloc::{boxed::Box, vec::Vec};
use codec::{
    Compact, CountedInput, Decode, DecodeLimit, DecodeWithMemTracking, Encode, Input,
    MemTrackingInput,
};
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
        PostDispatchInfoOf, TransactionExtension, ValidateUnsigned,
    },
    transaction_validity::{
        InvalidTransaction, TransactionSource, TransactionValidity, TransactionValidityError,
    },
};

use crate::hashed_auth::{AuthorizeAccount, AuthorizeMlDsa};
use crate::{AccountId, Address, ChainContext, Runtime, RuntimeCall, Signature, TxExtension};

pub type HashedTxExtension = (AuthorizeAccount, TxExtension);
pub type MlDsaTxExtension = (AuthorizeMlDsa, TxExtension);
pub type MlDsaUnchecked =
    fp_self_contained::UncheckedExtrinsic<Address, RuntimeCall, Signature, MlDsaTxExtension>;
type MlDsaChecked =
    fp_self_contained::CheckedExtrinsic<AccountId, RuntimeCall, MlDsaTxExtension, H160>;
pub type LegacyUnchecked =
    fp_self_contained::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;
pub type HashedUnchecked =
    fp_self_contained::UncheckedExtrinsic<Address, RuntimeCall, Signature, HashedTxExtension>;
type LegacyChecked = fp_self_contained::CheckedExtrinsic<AccountId, RuntimeCall, TxExtension, H160>;
type HashedChecked =
    fp_self_contained::CheckedExtrinsic<AccountId, RuntimeCall, HashedTxExtension, H160>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UncheckedExtrinsic {
    Legacy(LegacyUnchecked),
    Hashed(HashedUnchecked),
    MlDsa(Box<MlDsaUnchecked>),
}

impl UncheckedExtrinsic {
    pub fn new_signed(
        call: RuntimeCall,
        address: Address,
        signature: Signature,
        extension: TxExtension,
    ) -> Self {
        Self::Legacy(LegacyUnchecked::new_signed(
            call, address, signature, extension,
        ))
    }

    pub fn new_bare(call: RuntimeCall) -> Self {
        Self::Legacy(LegacyUnchecked::new_bare(call))
    }

    pub fn new_hashed(
        call: RuntimeCall,
        authorization: AuthorizeAccount,
        extension: TxExtension,
    ) -> Self {
        Self::Hashed(
            generic::UncheckedExtrinsic {
                preamble: Preamble::General(1, (authorization, extension)),
                function: call,
            }
            .into(),
        )
    }

    pub fn new_mldsa(
        call: RuntimeCall,
        authorization: AuthorizeMlDsa,
        extension: TxExtension,
    ) -> Self {
        Self::MlDsa(Box::new(
            generic::UncheckedExtrinsic {
                preamble: Preamble::General(2, (authorization, extension)),
                function: call,
            }
            .into(),
        ))
    }

    pub fn into_call(self) -> RuntimeCall {
        match self {
            Self::Legacy(xt) => xt.0.function,
            Self::Hashed(xt) => xt.0.function,
            Self::MlDsa(xt) => xt.0.function,
        }
    }
}

/// Preserve v14/v15's legacy envelope while publishing the complete General-v5
/// pipeline in v16. The pinned FRAME generator only emits extension version 0,
/// so augment its IR before portable types are registered, then add version 1.
pub fn metadata_at_version(version: u32) -> Option<sp_core::OpaqueMetadata> {
    if version != 16 {
        return Runtime::metadata_at_version(version);
    }
    use frame_support::__private::{metadata::RuntimeMetadata, metadata_ir};
    use sp_runtime::traits::TransactionExtension;

    let mut ir = Runtime::metadata_ir();
    let authorization_index = u32::try_from(ir.extrinsic.extensions.len()).ok()?;
    ir.extrinsic.extensions.extend(
        <AuthorizeAccount as TransactionExtension<RuntimeCall>>::metadata()
            .into_iter()
            .map(|extension| metadata_ir::TransactionExtensionMetadataIR {
                identifier: extension.identifier,
                ty: extension.ty,
                implicit: extension.implicit,
            }),
    );
    let mldsa_index = u32::try_from(ir.extrinsic.extensions.len()).ok()?;
    ir.extrinsic.extensions.extend(
        <AuthorizeMlDsa as TransactionExtension<RuntimeCall>>::metadata()
            .into_iter()
            .map(|extension| metadata_ir::TransactionExtensionMetadataIR {
                identifier: extension.identifier,
                ty: extension.ty,
                implicit: extension.implicit,
            }),
    );
    let mut prefixed = metadata_ir::into_v16(ir);
    let RuntimeMetadata::V16(metadata) = &mut prefixed.1 else {
        return None;
    };
    let legacy = metadata
        .extrinsic
        .transaction_extensions_by_version
        .get_mut(&0)?;
    legacy.retain(|index| index.0 < authorization_index);
    let mut hashed = alloc::vec![codec::Compact(authorization_index)];
    hashed.extend(legacy.iter().copied());
    let mut mldsa = alloc::vec![codec::Compact(mldsa_index)];
    mldsa.extend(legacy.iter().copied());
    metadata
        .extrinsic
        .transaction_extensions_by_version
        .insert(2, mldsa);
    metadata
        .extrinsic
        .transaction_extensions_by_version
        .insert(1, hashed);
    Some(sp_core::OpaqueMetadata::new(prefixed.into()))
}

impl Encode for UncheckedExtrinsic {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Legacy(xt) => xt.encode(),
            Self::Hashed(xt) => xt.encode(),
            Self::MlDsa(xt) => xt.encode(),
        }
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

impl Decode for UncheckedExtrinsic {
    fn decode<I: Input>(input: &mut I) -> Result<Self, codec::Error> {
        let expected: Compact<u32> = Decode::decode(input)?;
        let mut input = CountedInput::new(input);
        let format = input.read_byte()?;
        let result = match format {
            0x84 => {
                let address = Address::decode(&mut input)?;
                let signature = Signature::decode(&mut input)?;
                let extension = TxExtension::decode(&mut input)?;
                let function = decode_call(&mut input)?;
                Self::new_signed(function, address, signature, extension)
            }
            4 | 5 => {
                let function = decode_call(&mut input)?;
                Self::Legacy(
                    generic::UncheckedExtrinsic {
                        preamble: Preamble::Bare(format),
                        function,
                    }
                    .into(),
                )
            }
            0x45 => match input.read_byte()? {
                0 => {
                    let extension = TxExtension::decode(&mut input)?;
                    let function = decode_call(&mut input)?;
                    Self::Legacy(
                        generic::UncheckedExtrinsic {
                            preamble: Preamble::General(0, extension),
                            function,
                        }
                        .into(),
                    )
                }
                1 => {
                    let extension = HashedTxExtension::decode(&mut input)?;
                    let function = decode_call(&mut input)?;
                    Self::new_hashed(function, extension.0, extension.1)
                }
                2 => {
                    // The variant is boxed so legacy transactions remain small.
                    // Charge that allocation to the enclosing decoder's budget.
                    input.on_before_alloc_mem(core::mem::size_of::<MlDsaUnchecked>())?;
                    let extension = MlDsaTxExtension::decode(&mut input)?;
                    let function = decode_call(&mut input)?;
                    Self::new_mldsa(function, extension.0, extension.1)
                }
                _ => return Err("Unsupported transaction extension version".into()),
            },
            _ => return Err("Invalid extrinsic format".into()),
        };
        if input.count() != u64::from(expected.0) {
            return Err("Invalid extrinsic length prefix".into());
        }
        Ok(result)
    }
}

impl DecodeWithMemTracking for UncheckedExtrinsic {}

impl TypeInfo for UncheckedExtrinsic {
    type Identity = Self;
    fn type_info() -> Type {
        // Metadata v14/v15 describes the legacy signed layout. Hashed's fixed
        // v5 format is additionally described by the public account-auth types.
        LegacyUnchecked::type_info()
    }
}

impl ExtrinsicLike for UncheckedExtrinsic {
    fn is_bare(&self) -> bool {
        match self {
            Self::Legacy(xt) => xt.is_bare(),
            Self::Hashed(_) => false,
            Self::MlDsa(_) => false,
        }
    }
}

impl ExtrinsicCall for UncheckedExtrinsic {
    type Call = RuntimeCall;
    fn call(&self) -> &RuntimeCall {
        match self {
            Self::Legacy(xt) => &xt.0.function,
            Self::Hashed(xt) => &xt.0.function,
            Self::MlDsa(xt) => &xt.0.function,
        }
    }
}

impl GetDispatchInfo for UncheckedExtrinsic {
    fn get_dispatch_info(&self) -> DispatchInfo {
        let mut info = self.call().get_dispatch_info();
        match self {
            Self::Legacy(xt) if matches!(xt.0.preamble, Preamble::Signed(..)) => {
                info.extension_weight = info
                    .extension_weight
                    .saturating_add(<Runtime as frame_system::Config>::DbWeight::get().reads(1));
            }
            Self::Hashed(xt) => info.extension_weight = xt.0.extension_weight(),
            Self::MlDsa(xt) => info.extension_weight = xt.0.extension_weight(),
            _ => {}
        }
        info
    }
}

impl ExtrinsicMetadata for UncheckedExtrinsic {
    const VERSIONS: &'static [u8] = &[4, 5];
    type TransactionExtensions = TxExtension;
}

impl SignedTransactionBuilder for UncheckedExtrinsic {
    type Address = Address;
    type Signature = Signature;
    type Extension = TxExtension;
    fn new_signed_transaction(
        call: RuntimeCall,
        signed: Address,
        signature: Signature,
        tx_ext: TxExtension,
    ) -> Self {
        Self::new_signed(call, signed, signature, tx_ext)
    }
}

impl InherentBuilder for UncheckedExtrinsic {
    fn new_inherent(call: RuntimeCall) -> Self {
        Self::new_bare(call)
    }
}

impl From<UncheckedExtrinsic> for OpaqueExtrinsic {
    fn from(xt: UncheckedExtrinsic) -> Self {
        match xt {
            UncheckedExtrinsic::Legacy(xt) => xt.into(),
            UncheckedExtrinsic::Hashed(xt) => xt.into(),
            UncheckedExtrinsic::MlDsa(xt) => (*xt).into(),
        }
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

pub enum CheckedExtrinsic {
    Legacy(LegacyChecked),
    Hashed(HashedChecked),
    MlDsa(Box<MlDsaChecked>),
}

impl Checkable<ChainContext> for UncheckedExtrinsic {
    type Checked = CheckedExtrinsic;
    fn check(self, context: &ChainContext) -> Result<Self::Checked, TransactionValidityError> {
        match self {
            Self::Legacy(xt) => Ok(CheckedExtrinsic::Legacy(xt.check(context)?)),
            Self::Hashed(xt) => {
                // Frontier's self-contained branch bypasses extensions entirely.
                // EVM access by a Hashed must use an authenticated native route.
                if matches!(xt.0.function, RuntimeCall::Ethereum(_)) {
                    return Err(InvalidTransaction::Call.into());
                }
                if !matches!(xt.0.preamble, Preamble::General(1, _)) {
                    return Err(InvalidTransaction::BadProof.into());
                }
                Ok(CheckedExtrinsic::Hashed(xt.check(context)?))
            }
            Self::MlDsa(xt) => {
                // Frontier's self-contained branch bypasses extensions entirely.
                // EVM access by a Hashed must use an authenticated native route.
                if matches!(xt.0.function, RuntimeCall::Ethereum(_)) {
                    return Err(InvalidTransaction::Call.into());
                }
                if !matches!(xt.0.preamble, Preamble::General(2, _)) {
                    return Err(InvalidTransaction::BadProof.into());
                }
                Ok(CheckedExtrinsic::MlDsa(Box::new((*xt).check(context)?)))
            }
        }
    }

    #[cfg(feature = "try-runtime")]
    fn unchecked_into_checked_i_know_what_i_am_doing(
        self,
        context: &ChainContext,
    ) -> Result<Self::Checked, TransactionValidityError> {
        match self {
            Self::Legacy(xt) => Ok(CheckedExtrinsic::Legacy(
                xt.unchecked_into_checked_i_know_what_i_am_doing(context)?,
            )),
            // Hashed authorization is never bypassed during upgrade replay.
            Self::Hashed(_) | Self::MlDsa(_) => self.check(context),
        }
    }
}

fn reject_legacy_hashed(xt: &LegacyChecked) -> Result<(), TransactionValidityError> {
    if let fp_self_contained::CheckedSignature::GenericDelegated(ExtrinsicFormat::Signed(
        account,
        _,
    )) = &xt.signed
        && pallet_hashed_accounts::Accounts::<Runtime>::contains_key(account)
    {
        return Err(InvalidTransaction::BadSigner.into());
    }
    Ok(())
}

impl GetDispatchInfo for CheckedExtrinsic {
    fn get_dispatch_info(&self) -> DispatchInfo {
        let mut info = self.call().get_dispatch_info();
        match self {
            Self::Legacy(xt)
                if matches!(
                    xt.signed,
                    fp_self_contained::CheckedSignature::GenericDelegated(ExtrinsicFormat::Signed(
                        ..
                    ))
                ) =>
            {
                info.extension_weight = info
                    .extension_weight
                    .saturating_add(<Runtime as frame_system::Config>::DbWeight::get().reads(1));
            }
            Self::Hashed(xt) => {
                info.extension_weight = match &xt.signed {
                    fp_self_contained::CheckedSignature::GenericDelegated(
                        ExtrinsicFormat::General(_, extension),
                    ) => extension.weight(&xt.function),
                    _ => frame_support::weights::Weight::MAX,
                };
            }
            Self::MlDsa(xt) => {
                info.extension_weight = match &xt.signed {
                    fp_self_contained::CheckedSignature::GenericDelegated(
                        ExtrinsicFormat::General(_, extension),
                    ) => extension.weight(&xt.function),
                    _ => frame_support::weights::Weight::MAX,
                };
            }
            _ => {}
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
        match self {
            Self::Legacy(xt) => {
                reject_legacy_hashed(xt)?;
                xt.validate::<U>(source, info, len)
            }
            Self::Hashed(xt) => xt.validate::<U>(source, info, len),
            Self::MlDsa(xt) => xt.validate::<U>(source, info, len),
        }
    }

    fn apply<U: ValidateUnsigned<Call = RuntimeCall>>(
        self,
        info: &DispatchInfoOf<RuntimeCall>,
        len: usize,
    ) -> sp_runtime::ApplyExtrinsicResultWithInfo<PostDispatchInfoOf<RuntimeCall>> {
        match self {
            Self::Legacy(xt) => {
                reject_legacy_hashed(&xt)?;
                xt.apply::<U>(info, len)
            }
            Self::Hashed(xt) => {
                frame_support::storage::transactional::with_transaction_opaque_err(|| {
                    let result = xt.apply::<U>(info, len);
                    if result.is_ok() {
                        frame_support::storage::TransactionOutcome::Commit(result)
                    } else {
                        frame_support::storage::TransactionOutcome::Rollback(result)
                    }
                })
                .map_err(|_| InvalidTransaction::ExhaustsResources)?
            }
            Self::MlDsa(xt) => {
                frame_support::storage::transactional::with_transaction_opaque_err(|| {
                    let result = xt.apply::<U>(info, len);
                    if result.is_ok() {
                        frame_support::storage::TransactionOutcome::Commit(result)
                    } else {
                        frame_support::storage::TransactionOutcome::Rollback(result)
                    }
                })
                .map_err(|_| InvalidTransaction::ExhaustsResources)?
            }
        }
    }

    fn call(&self) -> &RuntimeCall {
        match self {
            Self::Legacy(xt) => &xt.function,
            Self::Hashed(xt) => &xt.function,
            Self::MlDsa(xt) => &xt.function,
        }
    }
}
