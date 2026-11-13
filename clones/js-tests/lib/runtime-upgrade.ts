/** Submit a checked sudo upgrade and verify the exact code installed at finalization. */
export async function submitRuntimeUpgrade(api, signer, wasm: Uint8Array): Promise<string> {
  const code = `0x${Buffer.from(wasm).toString("hex")}`;
  const tx = api.tx.sudo.sudo(api.tx.system.setCode(code));
  const blockHash = await new Promise<string>((resolve, reject) => {
    let unsubscribe;
    let settled = false;

    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      unsubscribe?.();
      fn(value);
    };
    const fail = (error) => finish(
      reject,
      new Error(`runtime upgrade failed: ${formatDispatchError(api, error)}`)
    );

    tx.signAndSend(signer, ({ status, events, dispatchError }) => {
      if (settled) return;
      if (dispatchError) {
        fail(dispatchError);
        return;
      }

      if (status.isInBlock || status.isFinalized) {
        let sudoSucceeded = false;
        for (const { event } of events) {
          if (event.section === "system" && event.method === "ExtrinsicFailed") {
            fail(event.data[0]);
            return;
          }
          if (event.section === "sudo" && event.method === "Sudid") {
            const result = event.data[0];
            if (result.isErr) {
              fail(result.asErr);
              return;
            }
            sudoSucceeded = result.isOk === true;
          }
        }
        if (status.isFinalized) {
          if (!sudoSucceeded) {
            finish(reject, new Error("runtime upgrade finalized without a successful Sudo.Sudid event"));
            return;
          }
          finish(resolve, status.asFinalized.toString());
        }
      }
    })
      .then((unsub) => {
        if (settled) unsub();
        else unsubscribe = unsub;
      })
      .catch((error) => finish(reject, error));
  });

  const installed = await api.rpc.state.getStorage("0x3a636f6465", blockHash);
  if (installed.isNone || installed.unwrap().toHex().toLowerCase() !== code) {
    throw new Error(
      `runtime upgrade finalized at ${blockHash}, but installed :code does not match the supplied Wasm`
    );
  }
  return blockHash;
}

function formatDispatchError(api, error): string {
  if (!error.isModule) return error.toString();
  const decoded = api.registry.findMetaError(error.asModule);
  return `${decoded.section}.${decoded.name}: ${decoded.docs.join(" ")}`;
}
