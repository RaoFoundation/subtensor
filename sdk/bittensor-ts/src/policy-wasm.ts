type Evaluator = (operation: string, input: string) => string
let evaluate: Evaluator | undefined

export function configurePolicyWasm(binding: Evaluator): void { evaluate = binding }
export function policyEvaluate(operation: string, input: string): string {
  if (!evaluate) throw new Error('Call await initBrowser() before using SDK policy')
  return evaluate(operation, input)
}
