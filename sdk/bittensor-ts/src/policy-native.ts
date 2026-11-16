import nativeBinding from './native'

export function policyEvaluate(operation: string, input: string): string {
  return nativeBinding.policyEvaluate(operation, input)
}
