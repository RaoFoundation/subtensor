import {CHAIN_STATS_ENDPOINT} from './config';

export type Balance = {
  account: string;
  stake: number;
  free: number;
  total: number;
};

export type ChainStatusResponse = {
  total_stake: number;
  total_issuance: number;
  accounts: number;
  balances: Balance[];
};

const isChainStatusResponse = (value: any): value is ChainStatusResponse => {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  if (typeof value.total_stake !== 'number') {
    return false;
  }
  if (typeof value.total_issuance !== 'number') {
    return false;
  }
  if (typeof value.accounts !== 'number') {
    return false;
  }
  if (!Array.isArray(value.balances)) {
    return false;
  }
  return value.balances.every((item: any) => {
    return (
      typeof item === 'object' &&
      item !== null &&
      'account' in item &&
      'stake' in item &&
      'free' in item &&
      'total' in item
    );
  });
};

export const getAllChainStats = async () => {
  const response = await fetch(CHAIN_STATS_ENDPOINT + '?' + Math.random());
  const data = await response.json();

  const rawResponse = data?.response;

  if (!isChainStatusResponse(rawResponse)) {
    throw new Error('Invalid response');
  }

  return rawResponse;
};
