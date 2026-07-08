import {CHAIN_API_ENDPOINT} from '@raofoundation/api';

export const API_ENDPOINT = CHAIN_API_ENDPOINT;
export const CHAIN_STATS_ENDPOINT =
  'https://rs2ifhr594.execute-api.us-east-1.amazonaws.com/prod/chainstatus/';

export const rao2tao = (rao: number) => {
  return rao / 1e9;
};

export const tao2rao = (tao: number) => {
  return tao * 1e9;
};

export const rao2taoString = (rao: number, digits = 9) => {
  const tao = rao / 1e9;
  const taoString = tao.toLocaleString(undefined, {
    maximumSignificantDigits: digits,
  });
  return taoString.replace(/\.?0+$/, '');
};
