import {ApiRpc} from '@raofoundation/api/hooks/useAccountStake';
import {useChainStore} from '@raofoundation/api';
import {calculateApyFromDailyReturn} from '@raofoundation/api/utils';
import {rao2tao} from '../../../apps/bittensor-website/src/app/(pages-without-header)/api/config';

export type DelegateType = {
  name: string;
  address: string;
  image?: string;
  description?: string;
  nominators: number;
  dailyReturn: number;
  owner?: string;
  url?: string;
  apy?: number;
  stake?: number;
};

export type DelegateIdentityType = {
  name: string;
  url: string;
  image: string;
  discord: string;
  description: string;
  additional: string;
};

export type DelegateInfoType = {
  delegate_ss58?: string;
  nominators?: [string, number][];
  owner_ss58?: string;
  total_daily_return?: number;
  take?: string;
  registrations?: string[];
  validator_permits?: string[];
  return_per_1000?: number;
};

const isDelegateInfoType = (data: any): data is DelegateInfoType => {
  return (
    typeof data === 'object' &&
    'delegate_ss58' in data &&
    typeof data.delegate_ss58 === 'string' &&
    'nominators' in data &&
    Array.isArray(data.nominators) &&
    'owner_ss58' in data &&
    typeof data.owner_ss58 === 'string' &&
    'total_daily_return' in data &&
    typeof data.total_daily_return === 'number'
  );
};

const isDelegateInfoTypeArray = (data: any): data is DelegateInfoType[] => {
  return Array.isArray(data) && data.every(isDelegateInfoType);
};

const isApiRpc = (x: any): x is ApiRpc => {
  return typeof x.neuronInfo === 'object' && typeof x.neuronInfo.getNeuronsLite === 'function';
};

const delegatesListUrl =
  'https://raw.githubusercontent.com/opentensor/bittensor-delegates/main/public/delegates.json';

type RawDelegateList = Record<string, Omit<DelegateType, 'address'>>;

const isRawDelegateList = (data: any): data is RawDelegateList => {
  return (
    typeof data === 'object' &&
    Object.values(data).every(
      (delegate) =>
        delegate &&
        typeof delegate === 'object' &&
        'name' in delegate &&
        typeof delegate.name === 'string',
    )
  );
};

export const useGetDelegates = async () => {
  try {
    let api = useChainStore.getState().api;
    if (!api || !isApiRpc(api.rpc)) {
      return {error: 'Not connected server', delegates: []};
    }
    const resultBytes = await (api?.rpc as any).delegateInfo?.getDelegates();

    const result = api?.createType('Vec<DelegateInfo>', resultBytes);
    const delegate_info = result?.toJSON();

    if (!isDelegateInfoTypeArray(delegate_info)) {
      throw new Error('Cannot fetch delegates list from the chain');
    }

    const onChainDelegateData = delegate_info.map((delegate) => {
      const totalStake = delegate.nominators?.reduce(
        (acc: number, nominator: [string, number]) => acc + nominator[1],
        0,
      );
      return {
        address: delegate.delegate_ss58,
        nominators: delegate.nominators?.length || 0,
        dailyReturn: delegate.total_daily_return || 0,
        owner: delegate.owner_ss58,
        returnPer1000: delegate.return_per_1000,
        take: delegate.take,
        nominatorArr: delegate.nominators,
        stake: rao2tao(totalStake || 0),
      };
    });

    const response = await fetch(delegatesListUrl);
    const rawDelegatesList = await response.json();
    if (!isRawDelegateList(rawDelegatesList)) {
      throw new Error('Cannot parse delegates list');
    }
    const delegatesAddresses = Object.keys(rawDelegatesList);
    const delegatesNames = Object.values(rawDelegatesList);
    const delegateMetadata = delegatesAddresses.map((address, index) => ({
      address,
      name: delegatesNames[index].name,
      description: delegatesNames[index]?.description,
      url: delegatesNames[index]?.url,
    }));

    const delegates = (
      await Promise.all(
        onChainDelegateData.map(async (onChainDelegate) => {
          const delegateDetailBytes = await api?.query['subtensorModule']['identities'](
            onChainDelegate.owner,
          );
          const delegateDetail = delegateDetailBytes?.toHuman() as DelegateIdentityType;

          const delegate = delegateMetadata.find(
            (delegate) => delegate.address === onChainDelegate.address,
          );

          const returnPer1000 = onChainDelegate.returnPer1000;
          const compoundingPeriodsPerDay = 7200; // '7200' if per block, '1' if daily
          const apy = calculateApyFromDailyReturn(returnPer1000 ?? 0, compoundingPeriodsPerDay);

          if (!delegateDetail) {
            return {
              address: onChainDelegate.address,
              name: onChainDelegate.address,
              nominators: onChainDelegate.nominators,
              dailyReturn: onChainDelegate.dailyReturn,
              owner: onChainDelegate.owner,
              take: onChainDelegate.take,
              stake: onChainDelegate.stake,
              apy: parseFloat((apy * 100).toFixed(3)),
              ...(delegate ?? {}),
            } as DelegateType;
          }

          return {
            ...delegateDetail,
            address: onChainDelegate.address,
            nominators: onChainDelegate.nominators,
            dailyReturn: onChainDelegate.dailyReturn,
            owner: onChainDelegate.owner,
            take: onChainDelegate.take,
            stake: onChainDelegate.stake,
            apy: parseFloat((apy * 100).toFixed(3)),
            ...(delegate ?? {}),
          } as DelegateType;
        }),
      )
    ).sort((a, b) => {
      return b.apy! - a.apy!;
    });
    return {delegates};
  } catch (error) {
    return {error: 'Cannot fetch delegates list', delegates: []};
  }
};
