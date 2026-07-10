import native, { type NativeEpochScheduleState } from './native'
import { nativeCall } from './errors'
import { toBigInt, toBuffer } from './wire'
import type {
  ByteLike,
  CiphertextRound,
  DrandResponse,
  EpochScheduleState,
  IntegerLike,
  TimelockUserData,
  WeightsTlockPayload,
} from './types'

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

export const MAX_TEMPO = native.timelockMaxTempo()
export const MAX_TEMPO_U64 = native.timelockMaxTempoU64()
export const DRAND_PUBLIC_KEY = native.timelockDrandPublicKey()
export const DRAND_GENESIS_TIME = native.timelockGenesisTime()
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

export function encryptAndCompress(data: ByteLike, revealRound: IntegerLike): Buffer {
  return nativeCall(() =>
    native.timelockEncryptAndCompress(
      toBuffer(data, 'data'),
      toBigInt(revealRound, 'revealRound'),
    ),
  )
}

export function decryptAndDecompress(
  encryptedData: ByteLike,
  signatureBytes: ByteLike,
): Buffer {
  return nativeCall(() =>
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
): CiphertextRound {
  return nativeCall(() =>
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

export function encryptCommitment(
  data: string,
  blocksUntilReveal: IntegerLike,
  blockTime: number,
): CiphertextRound {
  return nativeCall(() =>
    native.timelockEncryptCommitment(
      data,
      toBigInt(blocksUntilReveal, 'blocksUntilReveal'),
      blockTime,
    ),
  )
}

export function encryptNBlocks(
  data: ByteLike,
  nBlocks: IntegerLike,
  blockTime: number,
): CiphertextRound {
  return nativeCall(() =>
    native.timelockEncryptNBlocks(
      toBuffer(data, 'data'),
      toBigInt(nBlocks, 'nBlocks'),
      blockTime,
    ),
  )
}

export function encryptAtRound(data: ByteLike, revealRound: IntegerLike): CiphertextRound {
  return nativeCall(() =>
    native.timelockEncryptAtRound(
      toBuffer(data, 'data'),
      toBigInt(revealRound, 'revealRound'),
    ),
  )
}

export function getRoundInfo(round?: IntegerLike | null): DrandResponse {
  return nativeCall(() =>
    native.timelockGetRoundInfo(round == null ? undefined : toBigInt(round, 'round')),
  )
}

export function getRevealRoundSignature(
  revealRound?: IntegerLike | null,
  noErrors = true,
): string | null {
  return nativeCall(
    () =>
      native.timelockGetRevealRoundSignature(
        revealRound == null ? undefined : toBigInt(revealRound, 'revealRound'),
        noErrors,
      ) ?? null,
  )
}

export function decrypt(encryptedData: ByteLike, noErrors = true): Buffer | null {
  return nativeCall(
    () => native.timelockDecrypt(toBuffer(encryptedData, 'encryptedData'), noErrors) ?? null,
  )
}

export function decryptWithSignature(
  encryptedData: ByteLike,
  signatureHex: string,
): Buffer {
  return nativeCall(() =>
    native.timelockDecryptWithSignature(
      toBuffer(encryptedData, 'encryptedData'),
      signatureHex,
    ),
  )
}

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
  encryptCommitment,
  encryptNBlocks,
  encryptAtRound,
  getRoundInfo,
  getRevealRoundSignature,
  decrypt,
  decryptWithSignature,
  shouldRunEpoch,
  currentEpochPreRunCoinbase,
  simulateRunCoinbase,
  advanceBlocks,
  predictFirstRevealBlock,
  encodeWeightsTlockPayload,
  decodeWeightsTlockPayload,
  encodeTimelockUserData,
  decodeTimelockUserData,
  maxSimulationBlocks,
})
