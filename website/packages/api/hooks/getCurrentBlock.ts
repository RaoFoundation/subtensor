import {WsProvider} from '@polkadot/api';
import {CHAIN_API_ENDPOINT} from '../utils/constants';
import {getChainApiHandler} from './getChainApiHandler';

export type Block = {
  header?: {
    number?: number;
  };
};

const isBlock = (block: any): block is Block => {
  return block?.header?.number !== undefined;
};

export const getCurrentBlock = async () => {
  const provider = new WsProvider(CHAIN_API_ENDPOINT);
  const api = await getChainApiHandler(provider);
  const signedBlock = await api.rpc.chain.getBlock();
  const jsonSignedBlock = signedBlock.toJSON();

  const {block} = jsonSignedBlock;

  return (block && isBlock(block) && block?.header?.number) || 0;
};
