import { ApiPromise, WsProvider } from "@polkadot/api";

const DEFAULT_CONNECT_TIMEOUT_MS = Number(process.env.API_CONNECT_TIMEOUT_MS ?? 60_000);

export async function connectApi(endpoint, { log = () => {}, timeoutMs = DEFAULT_CONNECT_TIMEOUT_MS } = {}) {
  log(`Connecting to ${endpoint} ...`);
  const provider = new WsProvider(endpoint);
  let api;

  try {
    api = await withTimeout(ApiPromise.create({ provider }), timeoutMs, `Timed out creating API for ${endpoint}`);
    await withTimeout(api.isReady, timeoutMs, `Timed out waiting for API readiness for ${endpoint}`);
    log("Connected.");
    return api;
  } catch (error) {
    await api?.disconnect();
    throw error;
  }
}

function withTimeout(promise, timeoutMs, message) {
  let timeout;
  const timeoutPromise = new Promise((_, reject) => {
    timeout = setTimeout(() => reject(new Error(message)), timeoutMs);
  });

  return Promise.race([promise, timeoutPromise]).finally(() => clearTimeout(timeout));
}
