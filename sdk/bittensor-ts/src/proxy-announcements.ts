import type { IntentDescriptor } from './intents'
import { evaluatePolicy, evaluatePolicyPreservingValues } from './policy'

/** Runtime call inspection and trusted replay policy are shared with Python. */
export function validateAnnouncedCall(call: IntentDescriptor, depth = 0): void { evaluatePolicy('protected.validate_call', { call, depth }) }
export function decodedCallDescriptor(value: unknown, depth = 0): IntentDescriptor { return evaluatePolicyPreservingValues('protected.decoded_call', { call: value, depth }) }
export function announcedCallRequiresDirect(value: unknown, depth = 0): boolean { return evaluatePolicy('protected.requires_direct', { call: value, depth }) }
export function* announcedCallLeaves(call: IntentDescriptor, depth = 0): Generator<IntentDescriptor> { yield* evaluatePolicyPreservingValues<IntentDescriptor[]>('protected.call_leaves', { call, depth }) }
export function sameCallSemantics(left: unknown, right: unknown): boolean { return evaluatePolicy('protected.same_semantics', { left, right }) }
