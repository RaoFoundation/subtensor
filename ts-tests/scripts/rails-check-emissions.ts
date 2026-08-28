import { ApiPromise, WsProvider } from "@polkadot/api";

async function main() {
  const api = await ApiPromise.create({
    provider: new WsProvider("ws://127.0.0.1:9944"),
    noInitWarn: true,
  });
  const alice = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
  const read = async () =>
    BigInt((await api.query.subtensorModule.totalHotkeyAlpha(alice, 7)).toString());
  const a = await read();
  await new Promise((r) => setTimeout(r, 20000));
  const b = await read();
  console.log("t0:", a.toString());
  console.log("t+20s:", b.toString());
  console.log("delta:", (b - a).toString());
  await api.disconnect();
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
