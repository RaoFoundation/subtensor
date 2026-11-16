// Runtime resolution belongs to package.json's browser/native conditions.
// A tsconfig path would also override those conditions in browser bundlers.
declare module '#bittensor-policy' {
  export function policyEvaluate(operation: string, input: string): string
}
