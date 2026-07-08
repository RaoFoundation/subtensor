import {SUBNET_SYMBOLS_MAP} from './constants';
import {Buffer} from 'buffer';

export const rao2tao = (raoAmount: number) => {
  return raoAmount / 10e8;
};

export const tao2rao = (taoAmount: number) => {
  return taoAmount * 10e8;
};

export function calculateApyFromDailyReturn(
  returnPer1000: number,
  compoundingPeriodsPerDay: number,
): number {
  const dailyRate = returnPer1000 / 1000;
  const apy = Math.pow(1 + dailyRate / compoundingPeriodsPerDay, 365) - 1;
  return apy;
}

export const calculateAlphaReturnFromTaoAmount = (
  taoAmount: number,
  subnetAlphaIn: number,
  subnetTAO: number,
  isRootOrStable: boolean,
) => {
  if (isRootOrStable) return taoAmount;
  else {
    const newSubnetTao = subnetTAO + tao2rao(taoAmount);
    if (!newSubnetTao) return 0;
    const k = subnetTAO * subnetAlphaIn;
    const newSubnetAlphaIn = k / newSubnetTao;
    return rao2tao(subnetAlphaIn - newSubnetAlphaIn);
  }
};

export const calculateTaoAmountFromAlphaReturn = (
  alphaReturned: number,
  subnetAlphaIn: number,
  subnetTAO: number,
  isRootOrStable: boolean,
): number => {
  if (isRootOrStable) return alphaReturned; // Direct return if stable

  if (tao2rao(alphaReturned) >= subnetAlphaIn) return 0; // No valid solution

  const k = subnetTAO * subnetAlphaIn;
  const taoAmount = k / (subnetAlphaIn - tao2rao(alphaReturned)) - subnetTAO;

  return taoAmount > 0 ? rao2tao(taoAmount) : 0; // Ensure non-negative taoAmount
};

export const calculateStakeSlippage = (
  taoAmount: number,
  subnetAlphaIn: number,
  subnetTAO: number,
  isRootOrStable: boolean,
): number => {
  if (isRootOrStable) return 0;
  else {
    const alphaReturned = calculateAlphaReturnFromTaoAmount(
      taoAmount,
      subnetAlphaIn,
      subnetTAO,
      isRootOrStable,
    );
    const expectedAlphaReturned = tao2rao(taoAmount) * (subnetAlphaIn / subnetTAO);
    if (expectedAlphaReturned > tao2rao(alphaReturned)) {
      return 100 * ((expectedAlphaReturned - tao2rao(alphaReturned)) / expectedAlphaReturned);
    } else {
      return 0;
    }
  }
};

export const calculateTaoReturnFromAlphaAmount = (
  alphaAmount: number,
  subnetAlphaIn: number,
  subnetTAO: number,
  isRootOrStable: boolean,
): number => {
  if (isRootOrStable) return alphaAmount;
  else {
    const newAlphaIn = subnetAlphaIn + tao2rao(alphaAmount);
    if (!newAlphaIn) return 0;
    const k = subnetTAO * subnetAlphaIn;
    const newTaoReserve = k / newAlphaIn;
    return rao2tao(subnetTAO - newTaoReserve);
  }
};

export const calculateAlphaAmountFromTaoReturn = (
  taoReturned: number,
  subnetAlphaIn: number,
  subnetTAO: number,
  isRootOrStable: boolean,
): number => {
  if (isRootOrStable) return taoReturned;
  else {
    if (tao2rao(taoReturned) >= subnetTAO) return 0; // No valid solution

    const k = subnetTAO * subnetAlphaIn;
    const newTaoReserve = subnetTAO - tao2rao(taoReturned);
    const newAlphaIn = k / newTaoReserve;
    const alphaAmount = newAlphaIn - subnetAlphaIn;

    return alphaAmount > 0 ? rao2tao(alphaAmount) : 0; // Ensure non-negative alphaAmount
  }
};

export const calculateUnstakeSlippage = (
  alphaAmount: number,
  subnetAlphaIn: number,
  subnetTAO: number,
  isRootOrStable: boolean,
): number => {
  if (isRootOrStable) return 0;
  else {
    const taoReturned = calculateTaoReturnFromAlphaAmount(
      alphaAmount,
      subnetAlphaIn,
      subnetTAO,
      isRootOrStable,
    );
    const expectedTaoReturned = tao2rao(alphaAmount) * (subnetTAO / subnetAlphaIn);
    if (expectedTaoReturned > tao2rao(taoReturned)) {
      return 100 * ((expectedTaoReturned - tao2rao(taoReturned)) / expectedTaoReturned);
    } else {
      return 0;
    }
  }
};

export const capitalize = (s: string) => {
  if (typeof s !== 'string') return '';
  return s.charAt(0).toUpperCase() + s.slice(1);
};

export const netuidToSymbol = (netuid: number): string => {
  const bytesStr =
    SUBNET_SYMBOLS_MAP[netuid as keyof typeof SUBNET_SYMBOLS_MAP] || SUBNET_SYMBOLS_MAP[0];
  const bytes = new Uint8Array(bytesStr.split('').map((char) => char.charCodeAt(0)));

  try {
    return Buffer.from(bytes).toString('utf-8');
  } catch (e) {
    return String.fromCharCode(
      ...new Uint8Array(SUBNET_SYMBOLS_MAP[0].split('').map((char) => char.charCodeAt(0))),
    );
  }
};

export const timestamp2Date = (timestamp: number) => {
  return new Date(timestamp).toLocaleDateString();
};
