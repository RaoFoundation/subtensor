import type { FixedSizeBinary, SS58String, ResultPayload, Enum, Binary, FixedSizeArray } from 'polkadot-api';
import type { InkDescriptors } from 'polkadot-api/ink';
type AnonymousEnum<T extends {}> = T & {
    __anonymous: true;
};
type MyTuple<T> = [T, ...T[]];
type SeparateUndefined<T> = undefined extends T ? undefined | Exclude<T, undefined> : T;
type Anonymize<T> = SeparateUndefined<T extends FixedSizeBinary<infer L> ? number extends L ? Binary : FixedSizeBinary<L> : T extends string | number | bigint | boolean | void | undefined | null | symbol | Uint8Array | Enum<any> ? T : T extends AnonymousEnum<infer V> ? Enum<V> : T extends MyTuple<any> ? {
    [K in keyof T]: T[K];
} : T extends [] ? [] : T extends FixedSizeArray<infer L, infer T> ? number extends L ? Array<T> : FixedSizeArray<L, T> : {
    [K in keyof T & string]: T[K];
}>;
type T0 = ResultPayload<undefined, Anonymize<T1>>;
type T1 = Enum<{
    "CouldNotReadInput": undefined;
}>;
type T2 = Enum<{
    "ReadFailed": undefined;
    "WriteFailed": undefined;
}>;
type T3 = ResultPayload<ResultPayload<undefined, Anonymize<T2>>, Anonymize<T1>>;
type StorageDescriptor = {
    "": {
        "key": undefined;
        "value": undefined;
    };
};
type MessagesDescriptor = {
    "get_stake_info_for_hotkey_coldkey_netuid": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "coldkey": FixedSizeBinary<32>;
            "netuid": number;
        };
        "response": ResultPayload<ResultPayload<({
            "hotkey": SS58String;
            "coldkey": SS58String;
            "netuid": number;
            "stake": bigint;
            "locked": bigint;
            "emission": bigint;
            "tao_emission": bigint;
            "drain": bigint;
            "is_registered": boolean;
        }) | undefined, Anonymize<T2>>, Anonymize<T1>>;
    };
    "add_stake": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "netuid": number;
            "amount": bigint;
        };
        "response": Anonymize<T3>;
    };
    "remove_stake": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "netuid": number;
            "amount": bigint;
        };
        "response": Anonymize<T3>;
    };
    "unstake_all": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
        };
        "response": Anonymize<T3>;
    };
    "unstake_all_alpha": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
        };
        "response": Anonymize<T3>;
    };
    "move_stake": {
        "message": {
            "origin_hotkey": FixedSizeBinary<32>;
            "destination_hotkey": FixedSizeBinary<32>;
            "origin_netuid": number;
            "destination_netuid": number;
            "amount": bigint;
        };
        "response": Anonymize<T3>;
    };
    "transfer_stake": {
        "message": {
            "destination_coldkey": FixedSizeBinary<32>;
            "hotkey": FixedSizeBinary<32>;
            "origin_netuid": number;
            "destination_netuid": number;
            "amount": bigint;
        };
        "response": Anonymize<T3>;
    };
    "swap_stake": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "origin_netuid": number;
            "destination_netuid": number;
            "amount": bigint;
        };
        "response": Anonymize<T3>;
    };
    "add_stake_limit": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "netuid": number;
            "amount": bigint;
            "limit_price": bigint;
            "allow_partial": boolean;
        };
        "response": Anonymize<T3>;
    };
    "remove_stake_limit": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "netuid": number;
            "amount": bigint;
            "limit_price": bigint;
            "allow_partial": boolean;
        };
        "response": Anonymize<T3>;
    };
    "swap_stake_limit": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "origin_netuid": number;
            "destination_netuid": number;
            "amount": bigint;
            "limit_price": bigint;
            "allow_partial": boolean;
        };
        "response": Anonymize<T3>;
    };
    "remove_stake_full_limit": {
        "message": {
            "hotkey": FixedSizeBinary<32>;
            "netuid": number;
            "limit_price": bigint;
        };
        "response": Anonymize<T3>;
    };
    "set_coldkey_auto_stake_hotkey": {
        "message": {
            "netuid": number;
            "hotkey": FixedSizeBinary<32>;
        };
        "response": Anonymize<T3>;
    };
    "add_proxy": {
        "message": {
            "delegate": FixedSizeBinary<32>;
        };
        "response": Anonymize<T3>;
    };
    "remove_proxy": {
        "message": {
            "delegate": FixedSizeBinary<32>;
        };
        "response": Anonymize<T3>;
    };
    "get_alpha_price": {
        "message": {
            "netuid": number;
        };
        "response": ResultPayload<ResultPayload<bigint, Anonymize<T2>>, Anonymize<T1>>;
    };
};
type ConstructorsDescriptor = {
    /**
     * Constructor
     */
    "new": {
        "message": {};
        "response": Anonymize<T0>;
    };
    /**
     * Constructor
     */
    "default": {
        "message": {};
        "response": Anonymize<T0>;
    };
};
type EventDescriptor = Enum<{}>;
export declare const descriptor: InkDescriptors<StorageDescriptor, MessagesDescriptor, ConstructorsDescriptor, EventDescriptor>;
export {};
