import type {HexString} from '@polkadot/util/types';
import {useChainStore} from './useChainStore';

export async function queryRuntimeApi(method: string, params: Array<any>): Promise<any> {
  const api = useChainStore.getState().api;

  if (!api) {
    throw new Error('No chain API found');
  }

  const split_idx = method.indexOf('_');
  const api_name = method.slice(0, split_idx);
  const method_name = method.slice(split_idx + 1);

  const api_def = api.runtimeMetadata.asV15.apis.find(
    (api: any) => api.name.toHuman() === api_name,
  );
  const call_def = api_def?.methods.find((method: any) => method.name.toHuman() === method_name);

  const output_type = call_def?.output;
  if (!output_type) {
    throw new Error(`Output type not found for method ${method_name} in api ${api_name}`);
  }

  const param_types = call_def?.inputs.map((input: any) => input.type);

  const params_bytes_hex: HexString = params
    .map((param, idx) => {
      const typeDef = api.registry.createLookupType(param_types[idx]);
      return api.createType(typeDef, param).toHex();
    })
    .reduce((acc: HexString, curr: HexString) => {
      return acc.concat(curr.slice(2)) as HexString;
    }, '0x');

  const result_bytes = await api.rpc.state.call(method, params_bytes_hex);

  const typeDef = api.registry.createLookupType(output_type);
  const result = api.createType(typeDef, result_bytes);

  return result;
}
