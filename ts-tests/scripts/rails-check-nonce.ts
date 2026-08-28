// Quick check: was a gateway envelope nonce processed by pallet-usd-psm?
import { ApiPromise, WsProvider } from "@polkadot/api";

async function main() {
  const api = await ApiPromise.create({
    provider: new WsProvider(process.env.BT_RPC_WS ?? "ws://127.0.0.1:9944"),
    noInitWarn: true,
  });
  const nonce = process.argv[2];
  const processed = await api.query.usdPsm.processedNonces(nonce);
  console.log(`processedNonces(${nonce}):`, processed.toHuman());
  process.exit(0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
