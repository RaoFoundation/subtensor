import native, { type NativeEpochScheduleState } from './native'
import { nativeAsync, nativeCall } from './errors'
import { toBigInt, toBuffer } from './wire'
import type {
  ByteLike,
  CiphertextRound,
  DrandResponse,
  EpochScheduleResult,
  EpochScheduleState,
  IntegerLike,
  TimelockUserData,
  WeightsTlockPayload,
} from './types'

export type CiphertextRoundTuple = [Buffer, bigint]

function nativeState(state: EpochScheduleState): NativeEpochScheduleState {
  return {
    lastEpochBlock: toBigInt(state.lastEpochBlock, 'lastEpochBlock'),
    pendingEpochAt: toBigInt(state.pendingEpochAt, 'pendingEpochAt'),
    subnetEpochIndex: toBigInt(state.subnetEpochIndex, 'subnetEpochIndex'),
    tempo: state.tempo,
    blocksSinceLastStep: toBigInt(state.blocksSinceLastStep, 'blocksSinceLastStep'),
    currentBlock: toBigInt(state.currentBlock, 'currentBlock'),
  }
}

function publicState(state: NativeEpochScheduleState): EpochScheduleState {
  return {
    lastEpochBlock: state.lastEpochBlock,
    pendingEpochAt: state.pendingEpochAt,
    subnetEpochIndex: state.subnetEpochIndex,
    tempo: state.tempo,
    blocksSinceLastStep: state.blocksSinceLastStep,
    currentBlock: state.currentBlock,
  }
}

function roundTuple(value: CiphertextRound): CiphertextRoundTuple {
  return [Buffer.from(value.ciphertext), value.revealRound]
}

async function roundTupleAsync(value: Promise<CiphertextRound>): Promise<CiphertextRoundTuple> {
  return roundTuple(await value)
}

export const MAX_TEMPO = native.timelockMaxTempo()
export const MAX_TEMPO_U64 = native.timelockMaxTempoU64()
export const DRAND_PUBLIC_KEY = native.timelockDrandPublicKey()
export const DRAND_GENESIS_TIME = native.timelockGenesisTime()
/** Rust-name alias. */
export const GENESIS_TIME = DRAND_GENESIS_TIME
export const DRAND_PERIOD = native.timelockDrandPeriod()
export const QUICKNET_CHAIN_HASH = native.timelockQuicknetChainHash()
export const DRAND_ENDPOINTS = Object.freeze(native.timelockDrandEndpoints())
export const SECURITY_BLOCK_OFFSET = native.timelockSecurityBlockOffset()
export const COMMIT_INCLUSION_BLOCK_OFFSET = native.timelockCommitInclusionBlockOffset()

export function maxSimulationBlocks(revealPeriodEpochs: IntegerLike): bigint {
  return nativeCall(() =>
    native.timelockMaxSimulationBlocks(
      toBigInt(revealPeriodEpochs, 'revealPeriodEpochs'),
    ),
  )
}

export function encryptAndCompress(data: ByteLike, revealRound: IntegerLike): Promise<Buffer> {
  return nativeAsync(() =>
    native.timelockEncryptAndCompress(
      toBuffer(data, 'data'),
      toBigInt(revealRound, 'revealRound'),
    ),
  )
}

export function decryptAndDecompress(
  encryptedData: ByteLike,
  signatureBytes: ByteLike,
): Promise<Buffer> {
  return nativeAsync(() =>
    native.timelockDecryptAndDecompress(
      toBuffer(encryptedData, 'encryptedData'),
      toBuffer(signatureBytes, 'signatureBytes'),
    ),
  )
}

export function generateCommitV2(
  uids: number[],
  values: number[],
  versionKey: IntegerLike,
  state: EpochScheduleState,
  subnetRevealPeriodEpochs: IntegerLike,
  blockTime: number,
  hotkey: ByteLike,
): Promise<CiphertextRound> {
  return nativeAsync(() =>
    native.timelockGenerateCommitV2(
      uids,
      values,
      toBigInt(versionKey, 'versionKey'),
      nativeState(state),
      toBigInt(subnetRevealPeriodEpochs, 'subnetRevealPeriodEpochs'),
      blockTime,
      toBuffer(hotkey, 'hotkey'),
    ),
  )
}

export function get_encrypted_commit_v2(
  uids: number[],
  weights: number[],
  versionKey: IntegerLike,
  lastEpochBlock: IntegerLike,
  pendingEpochAt: IntegerLike,
  subnetEpochIndex: IntegerLike,
  tempo: number,
  blocksSinceLastStep: IntegerLike,
  currentBlock: IntegerLike,
  subnetRevealPeriodEpochs: IntegerLike,
  blockTime: number,
  hotkey: ByteLike,
): Promise<CiphertextRoundTuple> {
  return roundTupleAsync(
    generateCommitV2(
      uids,
      weights,
      versionKey,
      {
        lastEpochBlock,
        pendingEpochAt,
        subnetEpochIndex,
        tempo,
        blocksSinceLastStep,
        currentBlock,
      },
      subnetRevealPeriodEpochs,
      blockTime,
      hotkey,
    ),
  )
}

export function encryptCommitment(
  data: string,
  blocksUntilReveal: IntegerLike,
  blockTime: number,
): Promise<CiphertextRound> {
  return nativeAsync(() =>
    native.timelockEncryptCommitment(
      data,
      toBigInt(blocksUntilReveal, 'blocksUntilReveal'),
      blockTime,
    ),
  )
}

export function get_encrypted_commitment(
  data: string,
  blocksUntilReveal: IntegerLike,
  blockTime = 12.0,
): Promise<CiphertextRoundTuple> {
  return roundTupleAsync(encryptCommitment(data, blocksUntilReveal, blockTime))
}

export function encryptNBlocks(
  data: ByteLike,
  nBlocks: IntegerLike,
  blockTime: number,
): Promise<CiphertextRound> {
  return nativeAsync(() =>
    native.timelockEncryptNBlocks(
      toBuffer(data, 'data'),
      toBigInt(nBlocks, 'nBlocks'),
      blockTime,
    ),
  )
}

export function encrypt(
  data: ByteLike,
  nBlocks: IntegerLike,
  blockTime = 12.0,
): Promise<CiphertextRoundTuple> {
  return roundTupleAsync(encryptNBlocks(data, nBlocks, blockTime))
}

export function encryptAtRound(data: ByteLike, revealRound: IntegerLike): Promise<CiphertextRound> {
  return nativeAsync(() =>
    native.timelockEncryptAtRound(
      toBuffer(data, 'data'),
      toBigInt(revealRound, 'revealRound'),
    ),
  )
}

export function encrypt_at_round(
  data: ByteLike,
  revealRound: IntegerLike,
): Promise<CiphertextRoundTuple> {
  return roundTupleAsync(encryptAtRound(data, revealRound))
}

export function getRoundInfo(round?: IntegerLike | null): Promise<DrandResponse> {
  return nativeAsync(() =>
    native.timelockGetRoundInfo(round == null ? undefined : toBigInt(round, 'round')),
  )
}

export async function get_latest_round(): Promise<bigint> {
  return (await getRoundInfo()).round
}

export function getRevealRoundSignature(
  revealRound?: IntegerLike | null,
  noErrors = true,
): Promise<string | null> {
  return nativeAsync(async () =>
    (await native.timelockGetRevealRoundSignature(
      revealRound == null ? undefined : toBigInt(revealRound, 'revealRound'),
      noErrors,
    )) ?? null,
  )
}

export async function get_signature_for_round(revealRound: IntegerLike): Promise<string> {
  const signature = await getRevealRoundSignature(revealRound, false)
  if (signature == null) {
    throw new Error('Signature not available')
  }
  return signature
}

export function decrypt(encryptedData: ByteLike, noErrors = true): Promise<Buffer | null> {
  return nativeAsync(async () =>
    (await native.timelockDecrypt(toBuffer(encryptedData, 'encryptedData'), noErrors)) ?? null,
  )
}

export function decryptWithSignature(
  encryptedData: ByteLike,
  signatureHex: string,
): Promise<Buffer> {
  return nativeAsync(() =>
    native.timelockDecryptWithSignature(
      toBuffer(encryptedData, 'encryptedData'),
      signatureHex,
    ),
  )
}

export const decrypt_with_signature = decryptWithSignature

export function shouldRunEpoch(state: EpochScheduleState, block: IntegerLike): boolean {
  return native.epochShouldRun(nativeState(state), toBigInt(block, 'block'))
}

export function currentEpochPreRunCoinbase(
  state: EpochScheduleState,
  block: IntegerLike,
): bigint {
  return native.epochCurrentPreRunCoinbase(nativeState(state), toBigInt(block, 'block'))
}

export function simulateRunCoinbase(
  state: EpochScheduleState,
  block: IntegerLike,
): EpochScheduleState {
  return publicState(
    native.epochSimulateRunCoinbase(nativeState(state), toBigInt(block, 'block')),
  )
}

export function advanceBlocks(
  state: EpochScheduleState,
  start: IntegerLike,
  end: IntegerLike,
): EpochScheduleState {
  return publicState(
    native.epochAdvanceBlocks(
      nativeState(state),
      toBigInt(start, 'start'),
      toBigInt(end, 'end'),
    ),
  )
}

export function predictFirstRevealBlock(
  state: EpochScheduleState,
  revealPeriodEpochs: IntegerLike,
): bigint {
  return nativeCall(() =>
    native.epochPredictFirstRevealBlock(
      nativeState(state),
      toBigInt(revealPeriodEpochs, 'revealPeriodEpochs'),
    ),
  )
}

export const EpochScheduleError = Object.freeze([
  'BoundExceeded',
  'TempoIsZero',
] as const)

export function predictFirstRevealBlockResult(
  state: EpochScheduleState,
  revealPeriodEpochs: IntegerLike,
): EpochScheduleResult {
  return nativeCall(() => {
    const result = native.epochPredictFirstRevealBlockResult(
      nativeState(state),
      toBigInt(revealPeriodEpochs, 'revealPeriodEpochs'),
    )
    return {
      ok: result.ok,
      block: result.block ?? null,
      error: (result.error as EpochScheduleResult['error'] | undefined) ?? null,
    }
  })
}

export function encodeWeightsTlockPayload(payload: WeightsTlockPayload): Buffer {
  return nativeCall(() =>
    native.encodeWeightsTlockPayload({
      hotkey: toBuffer(payload.hotkey, 'hotkey'),
      uids: payload.uids,
      values: payload.values,
      versionKey: toBigInt(payload.versionKey, 'versionKey'),
    }),
  )
}

export function decodeWeightsTlockPayload(data: ByteLike): WeightsTlockPayload {
  return nativeCall(() => native.decodeWeightsTlockPayload(toBuffer(data, 'data')))
}

export function encodeTimelockUserData(value: TimelockUserData): Buffer {
  return nativeCall(() =>
    native.encodeTimelockUserData({
      encryptedData: toBuffer(value.encryptedData, 'encryptedData'),
      revealRound: toBigInt(value.revealRound, 'revealRound'),
    }),
  )
}

export function decodeTimelockUserData(data: ByteLike): TimelockUserData {
  return nativeCall(() => native.decodeTimelockUserData(toBuffer(data, 'data')))
}

export const timelock = Object.freeze({
  encryptAndCompress,
  decryptAndDecompress,
  generateCommitV2,
  get_encrypted_commit_v2,
  encryptCommitment,
  get_encrypted_commitment,
  encryptNBlocks,
  encrypt,
  encryptAtRound,
  encrypt_at_round,
  getRoundInfo,
  get_latest_round,
  getRevealRoundSignature,
  get_signature_for_round,
  decrypt,
  decryptWithSignature,
  decrypt_with_signature,
  shouldRunEpoch,
  currentEpochPreRunCoinbase,
  simulateRunCoinbase,
  advanceBlocks,
  predictFirstRevealBlock,
  predictFirstRevealBlockResult,
  encodeWeightsTlockPayload,
  decodeWeightsTlockPayload,
  encodeTimelockUserData,
  decodeTimelockUserData,
  maxSimulationBlocks,
})
