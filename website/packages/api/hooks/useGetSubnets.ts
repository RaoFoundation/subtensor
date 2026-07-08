import {useChainStore} from '@raofoundation/api';
import {ApiRpc} from '@raofoundation/api/hooks/useAccountStake';

export type SubnetType = {
  netUid: number;
  name: string;
  symbol: number;
  apy: number;
  address: string;
  difficulty: number;
  contact: string;
  githubLink: string;
  subnetTAO: number;
  subnetAlphaOut: number;
};

export type SubnetInfoType = {
  netuid: number;
  rho: number;
  kappa: number;
  difficulty: number;
  immunity_period: number;
  max_allowed_validators: number;
  min_allowed_weights: number;
  max_weights_limit: number;
  scaling_law_power: number;
  subnetwork_n: number;
  max_allowed_uids: number;
  blocks_since_last_step: number;
  tempo: number;
  network_modality: number;
  network_connect: any[];
  emission_values: number;
  burn: number;
  owner: string;
};

export type SubnetIdentityType = {
  githubRepo: string;
  subnetContact: string;
  subnetName: string;
};
const isSubnetInfoType = (data: any): data is SubnetInfoType => {
  return typeof data === 'object' && 'netuid' in data && typeof data.netuid === 'number';
};

const isSubnetInfoTypeArray = (data: any): data is SubnetInfoType[] => {
  return Array.isArray(data) && data.every(isSubnetInfoType);
};
const isApiRpc = (x: any): x is ApiRpc => {
  return typeof x.neuronInfo === 'object' && typeof x.neuronInfo.getNeuronsLite === 'function';
};

export const useGetSubnets = async () => {
  try {
    let api = useChainStore.getState().api;
    if (!api || !isApiRpc(api.rpc)) {
      return {error: "Can't connect server", subnets: []};
    }
    const resultBytes = await (api?.rpc as any).subnetInfo?.getSubnetsInfo();

    const result = api?.createType('Vec<SubnetInfo>', resultBytes);
    const subnet_info = result?.toJSON();

    if (!isSubnetInfoTypeArray(subnet_info)) {
      throw new Error('Cannot fetch subnets list from the chain');
    }

    let subnetInfo = await Promise.all(
      subnet_info.map(async (info) => {
        const subnetIdentityBytes = await api?.query['subtensorModule']['subnetIdentities'](
          info.netuid / 256,
        );
        const subnetIdentity = subnetIdentityBytes?.toHuman() as SubnetIdentityType;
        const subnetTAOBytes = await api?.query['subtensorModule']['subnetTAO'](info.netuid / 256);
        const subnetTAO = (subnetTAOBytes?.toHuman() as string).replaceAll(',', '');
        const subnetAlphaOutBytes = await api?.query['subtensorModule']['subnetAlphaOut'](
          info.netuid / 256,
        );
        const subnetAlphaOut = (subnetAlphaOutBytes?.toHuman() as string).replaceAll(',', '');

        if (!!subnetIdentity) {
          return {
            netUid: info.netuid / 256,
            name: subnetIdentity.subnetName,
            symbol: info.netuid / 256,
            apy: 0,
            address: info.owner,
            difficulty: info.difficulty,
            contact: subnetIdentity.subnetContact,
            githubLink: subnetIdentity.githubRepo,
            subnetTAO: parseFloat(subnetTAO),
            subnetAlphaOut: parseFloat(subnetAlphaOut),
            emissionValues: info.emission_values,
          };
        } else {
          return {
            netUid: info.netuid / 256,
            name: info.owner,
            symbol: info.netuid / 256,
            apy: 0,
            address: info.owner,
            difficulty: info.difficulty,
            contact: '',
            githubLink: '',
            subnetTAO: parseFloat(subnetTAO),
            subnetAlphaOut: parseFloat(subnetAlphaOut),
            emissionValues: info.emission_values,
          };
        }
      }),
    );
    return {
      subnets: subnetInfo.sort(
        (a, b) => b.subnetTAO / b.subnetAlphaOut - a.subnetTAO / a.subnetAlphaOut,
      ),
    };
  } catch (error) {
    return {error: 'Cannot fetch subnets list', subnets: []};
  }
};

// const globalWeightBytes = await api?.query['subtensorModule']['globalWeight'](
//   info.netuid / 256,
// );
// const globalWeight = (globalWeightBytes?.toHuman() as string).replaceAll(',', '');
// const U64_MAX = BigInt(2 ** 64 - 1);

// // Normalize GlobalWeight to [0, 1] range
// const globalWeightNormalized = Number(globalWeight) / Number(U64_MAX);
