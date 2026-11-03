#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::too_many_arguments
)]

use bittensor_core::timelock::constants;
use bittensor_core::timelock::epoch_schedule::{self, EpochScheduleError, EpochScheduleState};
use bittensor_core::timelock::{self, UserData, WeightsTlockPayload};
use codec::{Decode, Encode};
use napi::bindgen_prelude::{AsyncTask, BigInt, Buffer};
use napi::{Env, Task};
use napi_derive::napi;

use crate::errors::{invalid_arg, CoreResultExt, NapiResult};

#[napi(object)]
pub struct NativeEpochScheduleState {
    pub last_epoch_block: BigInt,
    pub pending_epoch_at: BigInt,
    pub subnet_epoch_index: BigInt,
    pub tempo: u16,
    pub blocks_since_last_step: BigInt,
    pub current_block: BigInt,
}

#[napi(object)]
pub struct NativeCiphertextRound {
    pub ciphertext: Buffer,
    pub reveal_round: BigInt,
}

#[napi(object)]
pub struct NativeDrandResponse {
    pub round: BigInt,
    pub signature: String,
}

#[napi(object)]
pub struct NativeEpochScheduleResult {
    pub ok: bool,
    pub block: Option<BigInt>,
    pub error: Option<String>,
}

#[napi(object)]
pub struct NativeWeightsTlockPayload {
    pub hotkey: Buffer,
    pub uids: Vec<u16>,
    pub values: Vec<u16>,
    pub version_key: BigInt,
}

#[napi(object)]
pub struct NativeUserData {
    pub encrypted_data: Buffer,
    pub reveal_round: BigInt,
}

fn bigint_u64(name: &str, value: &BigInt) -> NapiResult<u64> {
    let (negative, value, lossless) = value.get_u64();
    if negative || !lossless {
        return Err(invalid_arg(format!(
            "{name} must be an unsigned 64-bit bigint"
        )));
    }
    Ok(value)
}

fn state_from_native(value: &NativeEpochScheduleState) -> NapiResult<EpochScheduleState> {
    Ok(EpochScheduleState {
        last_epoch_block: bigint_u64("lastEpochBlock", &value.last_epoch_block)?,
        pending_epoch_at: bigint_u64("pendingEpochAt", &value.pending_epoch_at)?,
        subnet_epoch_index: bigint_u64("subnetEpochIndex", &value.subnet_epoch_index)?,
        tempo: value.tempo,
        blocks_since_last_step: bigint_u64("blocksSinceLastStep", &value.blocks_since_last_step)?,
        current_block: bigint_u64("currentBlock", &value.current_block)?,
    })
}

fn state_to_native(value: EpochScheduleState) -> NativeEpochScheduleState {
    NativeEpochScheduleState {
        last_epoch_block: BigInt::from(value.last_epoch_block),
        pending_epoch_at: BigInt::from(value.pending_epoch_at),
        subnet_epoch_index: BigInt::from(value.subnet_epoch_index),
        tempo: value.tempo,
        blocks_since_last_step: BigInt::from(value.blocks_since_last_step),
        current_block: BigInt::from(value.current_block),
    }
}

pub struct CiphertextRoundOutput {
    ciphertext: Vec<u8>,
    reveal_round: u64,
}

impl From<(Vec<u8>, u64)> for CiphertextRoundOutput {
    fn from(value: (Vec<u8>, u64)) -> Self {
        Self {
            ciphertext: value.0,
            reveal_round: value.1,
        }
    }
}

pub struct TimelockEncryptAndCompressTask {
    data: Vec<u8>,
    reveal_round: u64,
}

impl Task for TimelockEncryptAndCompressTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::encrypt_and_compress(&self.data, self.reveal_round).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

pub struct TimelockDecryptAndDecompressTask {
    encrypted_data: Vec<u8>,
    signature_bytes: Vec<u8>,
}

impl Task for TimelockDecryptAndDecompressTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::decrypt_and_decompress(&self.encrypted_data, &self.signature_bytes).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

pub struct TimelockGenerateCommitV2Task {
    uids: Vec<u16>,
    values: Vec<u16>,
    version_key: u64,
    state: EpochScheduleState,
    subnet_reveal_period_epochs: u64,
    block_time: f64,
    hotkey: Vec<u8>,
}

impl Task for TimelockGenerateCommitV2Task {
    type Output = CiphertextRoundOutput;
    type JsValue = NativeCiphertextRound;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::generate_commit_v2(
            self.uids.clone(),
            self.values.clone(),
            self.version_key,
            self.state.clone(),
            self.subnet_reveal_period_epochs,
            self.block_time,
            self.hotkey.clone(),
        )
        .napi()
        .map(Into::into)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeCiphertextRound {
            ciphertext: output.ciphertext.into(),
            reveal_round: BigInt::from(output.reveal_round),
        })
    }
}

pub struct TimelockEncryptCommitmentTask {
    data: String,
    blocks_until_reveal: u64,
    block_time: f64,
}

impl Task for TimelockEncryptCommitmentTask {
    type Output = CiphertextRoundOutput;
    type JsValue = NativeCiphertextRound;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::encrypt_commitment(&self.data, self.blocks_until_reveal, self.block_time)
            .napi()
            .map(Into::into)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeCiphertextRound {
            ciphertext: output.ciphertext.into(),
            reveal_round: BigInt::from(output.reveal_round),
        })
    }
}

pub struct TimelockEncryptNBlocksTask {
    data: Vec<u8>,
    n_blocks: u64,
    block_time: f64,
}

impl Task for TimelockEncryptNBlocksTask {
    type Output = CiphertextRoundOutput;
    type JsValue = NativeCiphertextRound;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::encrypt_n_blocks(&self.data, self.n_blocks, self.block_time)
            .napi()
            .map(Into::into)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeCiphertextRound {
            ciphertext: output.ciphertext.into(),
            reveal_round: BigInt::from(output.reveal_round),
        })
    }
}

pub struct TimelockEncryptAtRoundTask {
    data: Vec<u8>,
    reveal_round: u64,
}

impl Task for TimelockEncryptAtRoundTask {
    type Output = CiphertextRoundOutput;
    type JsValue = NativeCiphertextRound;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::encrypt_at_round(&self.data, self.reveal_round)
            .napi()
            .map(Into::into)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeCiphertextRound {
            ciphertext: output.ciphertext.into(),
            reveal_round: BigInt::from(output.reveal_round),
        })
    }
}

pub struct DrandResponseOutput {
    round: u64,
    signature: String,
}

pub struct TimelockGetRoundInfoTask {
    round: Option<u64>,
}

impl Task for TimelockGetRoundInfoTask {
    type Output = DrandResponseOutput;
    type JsValue = NativeDrandResponse;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let response = timelock::get_round_info(self.round).napi()?;
        Ok(DrandResponseOutput {
            round: response.round,
            signature: response.signature,
        })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeDrandResponse {
            round: BigInt::from(output.round),
            signature: output.signature,
        })
    }
}

pub struct TimelockGetRevealRoundSignatureTask {
    reveal_round: Option<u64>,
    no_errors: bool,
}

impl Task for TimelockGetRevealRoundSignatureTask {
    type Output = Option<String>;
    type JsValue = Option<String>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::get_reveal_round_signature(self.reveal_round, self.no_errors).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct TimelockDecryptTask {
    encrypted_data: Vec<u8>,
    no_errors: bool,
}

impl Task for TimelockDecryptTask {
    type Output = Option<Vec<u8>>;
    type JsValue = Option<Buffer>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::decrypt(&self.encrypted_data, self.no_errors).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.map(Into::into))
    }
}

pub struct TimelockDecryptWithSignatureTask {
    encrypted_data: Vec<u8>,
    signature_hex: String,
}

impl Task for TimelockDecryptWithSignatureTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        timelock::decrypt_with_signature(&self.encrypted_data, &self.signature_hex).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

#[napi(js_name = "timelockEncryptAndCompress")]
pub fn encrypt_and_compress(
    data: Buffer,
    reveal_round: BigInt,
) -> NapiResult<AsyncTask<TimelockEncryptAndCompressTask>> {
    Ok(AsyncTask::new(TimelockEncryptAndCompressTask {
        data: data.to_vec(),
        reveal_round: bigint_u64("revealRound", &reveal_round)?,
    }))
}

#[napi(js_name = "timelockDecryptAndDecompress")]
pub fn decrypt_and_decompress(
    encrypted_data: Buffer,
    signature_bytes: Buffer,
) -> AsyncTask<TimelockDecryptAndDecompressTask> {
    AsyncTask::new(TimelockDecryptAndDecompressTask {
        encrypted_data: encrypted_data.to_vec(),
        signature_bytes: signature_bytes.to_vec(),
    })
}

#[napi(js_name = "timelockGenerateCommitV2")]
pub fn generate_commit_v2(
    uids: Vec<u16>,
    values: Vec<u16>,
    version_key: BigInt,
    state: NativeEpochScheduleState,
    subnet_reveal_period_epochs: BigInt,
    block_time: f64,
    hotkey: Buffer,
) -> NapiResult<AsyncTask<TimelockGenerateCommitV2Task>> {
    Ok(AsyncTask::new(TimelockGenerateCommitV2Task {
        uids,
        values,
        version_key: bigint_u64("versionKey", &version_key)?,
        state: state_from_native(&state)?,
        subnet_reveal_period_epochs: bigint_u64(
            "subnetRevealPeriodEpochs",
            &subnet_reveal_period_epochs,
        )?,
        block_time,
        hotkey: hotkey.to_vec(),
    }))
}

#[napi(js_name = "timelockEncryptCommitment")]
pub fn encrypt_commitment(
    data: String,
    blocks_until_reveal: BigInt,
    block_time: f64,
) -> NapiResult<AsyncTask<TimelockEncryptCommitmentTask>> {
    Ok(AsyncTask::new(TimelockEncryptCommitmentTask {
        data,
        blocks_until_reveal: bigint_u64("blocksUntilReveal", &blocks_until_reveal)?,
        block_time,
    }))
}

#[napi(js_name = "timelockEncryptNBlocks")]
pub fn encrypt_n_blocks(
    data: Buffer,
    n_blocks: BigInt,
    block_time: f64,
) -> NapiResult<AsyncTask<TimelockEncryptNBlocksTask>> {
    Ok(AsyncTask::new(TimelockEncryptNBlocksTask {
        data: data.to_vec(),
        n_blocks: bigint_u64("nBlocks", &n_blocks)?,
        block_time,
    }))
}

#[napi(js_name = "timelockEncryptAtRound")]
pub fn encrypt_at_round(
    data: Buffer,
    reveal_round: BigInt,
) -> NapiResult<AsyncTask<TimelockEncryptAtRoundTask>> {
    Ok(AsyncTask::new(TimelockEncryptAtRoundTask {
        data: data.to_vec(),
        reveal_round: bigint_u64("revealRound", &reveal_round)?,
    }))
}

#[napi(js_name = "timelockGetRoundInfo")]
pub fn get_round_info(round: Option<BigInt>) -> NapiResult<AsyncTask<TimelockGetRoundInfoTask>> {
    let round = round
        .as_ref()
        .map(|value| bigint_u64("round", value))
        .transpose()?;
    Ok(AsyncTask::new(TimelockGetRoundInfoTask { round }))
}

#[napi(js_name = "timelockGetRevealRoundSignature")]
pub fn get_reveal_round_signature(
    reveal_round: Option<BigInt>,
    no_errors: bool,
) -> NapiResult<AsyncTask<TimelockGetRevealRoundSignatureTask>> {
    let reveal_round = reveal_round
        .as_ref()
        .map(|value| bigint_u64("revealRound", value))
        .transpose()?;
    Ok(AsyncTask::new(TimelockGetRevealRoundSignatureTask {
        reveal_round,
        no_errors,
    }))
}

#[napi(js_name = "timelockDecrypt")]
pub fn decrypt(encrypted_data: Buffer, no_errors: bool) -> AsyncTask<TimelockDecryptTask> {
    AsyncTask::new(TimelockDecryptTask {
        encrypted_data: encrypted_data.to_vec(),
        no_errors,
    })
}

#[napi(js_name = "timelockDecryptWithSignature")]
pub fn decrypt_with_signature(
    encrypted_data: Buffer,
    signature_hex: String,
) -> AsyncTask<TimelockDecryptWithSignatureTask> {
    AsyncTask::new(TimelockDecryptWithSignatureTask {
        encrypted_data: encrypted_data.to_vec(),
        signature_hex,
    })
}

#[napi(js_name = "epochShouldRun")]
pub fn should_run_epoch(state: NativeEpochScheduleState, block: BigInt) -> NapiResult<bool> {
    Ok(epoch_schedule::should_run_epoch(
        &state_from_native(&state)?,
        bigint_u64("block", &block)?,
    ))
}

#[napi(js_name = "epochCurrentPreRunCoinbase")]
pub fn current_epoch_pre_run_coinbase(
    state: NativeEpochScheduleState,
    block: BigInt,
) -> NapiResult<BigInt> {
    Ok(BigInt::from(
        epoch_schedule::current_epoch_pre_run_coinbase(
            &state_from_native(&state)?,
            bigint_u64("block", &block)?,
        ),
    ))
}

#[napi(js_name = "epochSimulateRunCoinbase")]
pub fn simulate_run_coinbase(
    state: NativeEpochScheduleState,
    block: BigInt,
) -> NapiResult<NativeEpochScheduleState> {
    Ok(state_to_native(epoch_schedule::simulate_run_coinbase(
        &state_from_native(&state)?,
        bigint_u64("block", &block)?,
    )))
}

#[napi(js_name = "epochAdvanceBlocks")]
pub fn advance_blocks(
    state: NativeEpochScheduleState,
    start: BigInt,
    end: BigInt,
) -> NapiResult<NativeEpochScheduleState> {
    Ok(state_to_native(epoch_schedule::advance_blocks(
        &state_from_native(&state)?,
        bigint_u64("start", &start)?,
        bigint_u64("end", &end)?,
    )))
}

#[napi(js_name = "epochPredictFirstRevealBlock")]
pub fn predict_first_reveal_block(
    state: NativeEpochScheduleState,
    reveal_period_epochs: BigInt,
) -> NapiResult<BigInt> {
    epoch_schedule::predict_first_reveal_block(
        &state_from_native(&state)?,
        bigint_u64("revealPeriodEpochs", &reveal_period_epochs)?,
    )
    .map(BigInt::from)
    .map_err(|error| invalid_arg(error.to_string()))
}

#[napi(js_name = "epochPredictFirstRevealBlockResult")]
pub fn predict_first_reveal_block_result(
    state: NativeEpochScheduleState,
    reveal_period_epochs: BigInt,
) -> NapiResult<NativeEpochScheduleResult> {
    let result = epoch_schedule::predict_first_reveal_block(
        &state_from_native(&state)?,
        bigint_u64("revealPeriodEpochs", &reveal_period_epochs)?,
    );
    Ok(match result {
        Ok(block) => NativeEpochScheduleResult {
            ok: true,
            block: Some(BigInt::from(block)),
            error: None,
        },
        Err(error) => NativeEpochScheduleResult {
            ok: false,
            block: None,
            error: Some(
                match error {
                    EpochScheduleError::BoundExceeded => "BoundExceeded",
                    EpochScheduleError::TempoIsZero => "TempoIsZero",
                }
                .to_owned(),
            ),
        },
    })
}

#[napi(js_name = "encodeWeightsTlockPayload")]
pub fn encode_weights_tlock_payload(value: NativeWeightsTlockPayload) -> NapiResult<Buffer> {
    Ok(WeightsTlockPayload {
        hotkey: value.hotkey.as_ref().to_vec(),
        uids: value.uids,
        values: value.values,
        version_key: bigint_u64("versionKey", &value.version_key)?,
    }
    .encode()
    .into())
}

#[napi(js_name = "decodeWeightsTlockPayload")]
pub fn decode_weights_tlock_payload(data: Buffer) -> NapiResult<NativeWeightsTlockPayload> {
    let value = WeightsTlockPayload::decode(&mut &data.as_ref()[..])
        .map_err(|error| invalid_arg(format!("invalid weights timelock payload: {error}")))?;
    Ok(NativeWeightsTlockPayload {
        hotkey: value.hotkey.into(),
        uids: value.uids,
        values: value.values,
        version_key: BigInt::from(value.version_key),
    })
}

#[napi(js_name = "encodeTimelockUserData")]
pub fn encode_user_data(value: NativeUserData) -> NapiResult<Buffer> {
    Ok(UserData {
        encrypted_data: value.encrypted_data.as_ref().to_vec(),
        reveal_round: bigint_u64("revealRound", &value.reveal_round)?,
    }
    .encode()
    .into())
}

#[napi(js_name = "decodeTimelockUserData")]
pub fn decode_user_data(data: Buffer) -> NapiResult<NativeUserData> {
    let value = UserData::decode(&mut &data.as_ref()[..])
        .map_err(|error| invalid_arg(format!("invalid timelock user data: {error}")))?;
    Ok(NativeUserData {
        encrypted_data: value.encrypted_data.into(),
        reveal_round: BigInt::from(value.reveal_round),
    })
}

#[napi(js_name = "timelockMaxTempo")]
pub fn max_tempo() -> u16 {
    constants::MAX_TEMPO
}

#[napi(js_name = "timelockMaxTempoU64")]
pub fn max_tempo_u64() -> BigInt {
    BigInt::from(constants::MAX_TEMPO_U64)
}

#[napi(js_name = "timelockDrandPublicKey")]
pub fn drand_public_key() -> String {
    constants::DRAND_PUBLIC_KEY.to_owned()
}

#[napi(js_name = "timelockGenesisTime")]
pub fn genesis_time() -> BigInt {
    BigInt::from(constants::GENESIS_TIME)
}

#[napi(js_name = "timelockDrandPeriod")]
pub fn drand_period() -> BigInt {
    BigInt::from(constants::DRAND_PERIOD)
}

#[napi(js_name = "timelockQuicknetChainHash")]
pub fn quicknet_chain_hash() -> String {
    constants::QUICKNET_CHAIN_HASH.to_owned()
}

#[napi(js_name = "timelockDrandEndpoints")]
pub fn drand_endpoints() -> Vec<String> {
    constants::DRAND_ENDPOINTS
        .iter()
        .map(|endpoint| (*endpoint).to_owned())
        .collect()
}

#[napi(js_name = "timelockSecurityBlockOffset")]
pub fn security_block_offset() -> BigInt {
    BigInt::from(constants::SECURITY_BLOCK_OFFSET)
}

#[napi(js_name = "timelockCommitInclusionBlockOffset")]
pub fn commit_inclusion_block_offset() -> BigInt {
    BigInt::from(constants::COMMIT_INCLUSION_BLOCK_OFFSET)
}

#[napi(js_name = "timelockMaxSimulationBlocks")]
pub fn max_simulation_blocks(reveal_period_epochs: BigInt) -> NapiResult<BigInt> {
    Ok(BigInt::from(constants::max_simulation_blocks(bigint_u64(
        "revealPeriodEpochs",
        &reveal_period_epochs,
    )?)))
}
