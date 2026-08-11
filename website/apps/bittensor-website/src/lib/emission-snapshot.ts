import snapshotData from '../../public/catalog/emission-snapshot.json';
import {blockEmissionTao} from './emission-math';

export type SubnetEmissionRow = {
  netuid: number;
  name: string;
  spotPrice: number;
  emaPrice: number;
  minerBurned: number;
  emissionEnabled: boolean;
  taoIn: number;
  alphaIn: number;
  alphaOut: number;
  demandShare: number;
  burnAdjustedShare: number;
  gateFactor: number;
  taoShare: number;
  taoPerBlock: number;
};

export type EmissionInput = {
  netuid: number;
  emaPrice: number;
  minerBurned: number;
  emissionEnabled: boolean;
};

export type EmissionSnapshot = {
  fetchedAt: string;
  network: string;
  chainSpecVersion: number;
  emissionMode: 'price_ema_miner_burn_hill_gate' | string;
  emissionGateSource: 'chain_storage' | 'v444_defaults_recomputed';
  blockEmissionTao: number;
  totalIssuanceTao: number;
  totalIssuanceRao?: number;
  rootTao: number;
  emaPriceSum: number;
  rootDividendGateOpen: boolean;
  taoWeight: number;
  emissionGateRank: number;
  emissionGateQuantile: number;
  emissionGateExponent: number;
  emissionGateBar: number;
  emissionInputs: EmissionInput[];
  dataSource?: {
    subnets: string;
    chain: string;
    tmcEndpoint?: string;
  };
  featuredSubnet: SubnetEmissionRow;
  topSubnets: SubnetEmissionRow[];
};

export const DEFAULT_EMISSION_SNAPSHOT = snapshotData as EmissionSnapshot;

export function alphaIssuance(subnet: SubnetEmissionRow): number {
  return subnet.alphaIn + subnet.alphaOut;
}

export function alphaEmissionPerBlock(subnet: SubnetEmissionRow): number {
  return blockEmissionTao(alphaIssuance(subnet));
}

export async function fetchEmissionSnapshot(): Promise<EmissionSnapshot> {
  const response = await fetch('/catalog/emission-snapshot.json', {cache: 'no-store'});
  if (!response.ok) {
    return DEFAULT_EMISSION_SNAPSHOT;
  }
  return (await response.json()) as EmissionSnapshot;
}

export function formatSnapshotAge(iso: string): string {
  const fetched = new Date(iso);
  if (Number.isNaN(fetched.getTime())) return 'unknown';
  return fetched.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}
