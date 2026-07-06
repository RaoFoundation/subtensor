import { StorageDescriptor, PlainDescriptor, TxDescriptor, RuntimeDescriptor, Enum, ApisFromDef, QueryFromPalletsDef, TxFromPalletsDef, EventsFromPalletsDef, ErrorsFromPalletsDef, ConstFromPalletsDef, ViewFnsFromPalletsDef, SS58String, FixedSizeBinary, Binary, FixedSizeArray } from "polkadot-api";
import { I5sesotjlssv2d, Iffmde3ekjedi9, I4mddgoa69c0a2, Icr4vaj0vrd6je, I95g6i7ilua7lq, Ieniouoqkq4icf, Phase, Ibgl04rn6nbfm6, I4q39t5hn830vp, Ic5m5lp1oioo8r, GrandpaStoredState, I7pe2me3i3vtn9, I9jd27rnpm8ttv, I3geksg000c171, I1q8tnt1cluu5j, I8ds64oj6581v0, Ia7pdug7cdsg8g, I2hnk9r4ukuj1p, I9bin2jc70qt6q, TransactionPaymentReleases, I7svnfko10tq2e, I6ouflveob4eli, Idoeu5t0dum8va, I5n8gpu725k1nu, Ia2lhg7l2hilo3, I4p5t2krb1gmvp, I7svrbkiu01iec, Iabpgqcjikia83, I2j729bmgsdiuo, Iakavvne152v30, Ib9tptuv3cggfs, Icgljjb6j82uhn, I4h6ivgjtd51lv, I9eir063evtfb6, Iafqnechp3omqg, Ibc83gdj8hi3rc, I9lpjucl20l82d, Iaap7oohdmr1sb, Ifjlj958aeheic, I4tc54pa558g5n, Id32h28hjj1tch, Icrrf4uohj5gb0, I76jd8kl1mtn5g, I4ojmnsk1dchql, I4jqk5si14p5oi, I2na29tt2afp0j, If9jidduiuq7vv, I2brm5b9jij1st, Iapm6e7vtp0l6r, I7tof95tckt2r, Ieruonr5pk2d7h, Iag146hmjgqfgj, I8uo3fpd3bcc6f, PreimageOldRequestStatus, PreimageRequestStatus, I4pact7n2e9a0i, I11tetbe8ces3o, I56u24ncejr5kt, I6tqrno2gaos08, I9p9lq3rej5bhc, Ibq6c27da62s2q, Ib6u9l1gtc5l4t, I7nkl7ntqohel8, I3m6d7ohcp5n4v, Ib9pv5dg6upo6t, I27ub49plcvb4c, I8un1ap2r4hhbj, Ic3l568el19b24, Ib0hfhkohlekcj, I32lgu058i52q9, Ie7atdsih6q14b, I4totqt881mlti, I7jidl7qnnq87c, I82cps8ng2jtug, I4gqmlq9k6jlk3, I494mq1ertfc9k, Ialchst9lgd11u, If0p9hvn3kegj1, I8ac0r18acljm6, I5g2vv0ckl2m8b, I5mi4kb05lrsa9, Icsknfl0f6r973, I1ptic1rnhda0n, I5kulbesqc1h1t, I36dvimehsh2tm, I8t4pajubp34g3, Ifdiflqufkknl8, In7a38730s6qs, If15el53dd76v9, I9s0ave7t0vnrk, I4fo08joqmcqnm, I35p85j063s0il, I4arjljr6dpflb, Ijc5n210o8bbf, I3m5sq54sjdlso, I8ofcg5rbj0g2c, I4adgbll7gku4i, I6pjjpfvhvcfru, I9pj91mj79qekl, I39uah9nss64h9, Ik64dknsq7k08, Ib51vk42m1po4n, Idcr6u6361oad9, I3a5kuu5t5jj3g, I2hviml3snvhhn, I4ktuaksf5i1gk, I9bqtpv2ii35mp, I9j7pagd6d4bda, I2h9pmio37r7fb, Ibmr18suc9ikh9, I9iq22t0burs89, I5u8olqbbvfnvf, I5utcetro501ir, Icv6ofu4lqekr4, I48embv0n659kj, I8l6dbd18t5aja, I513du23unvan, I36o6oho99gjm8, If3mvus4cmnb7l, I3qrhi1ua10nnf, I2hpc4ev2drsf2, I73q6qh9ckhm04, Idia8cmqvul6et, Idardmhchnv8aa, Icud5m8j0nlgtj, I850u7ir5o34um, Ica88a899k1afk, I4tfn6eb3ekqt2, Ia5r6mm7trbg6a, I27gr0ss2ikvqh, Ie7hipi75c7vn0, I7f38r2vt6r9k1, I6b53cjq4m9nsr, I216fvnrl9nq6l, I9n4d52k0luroe, I3gk6eeddm0hsd, I6ue7qc27uhiev, Ifp8lgrkla2dig, I30l38oi9ed9dj, Ifj9gf4ekq9snm, If2k69ql8jgivj, I4378ieh1uba9u, I8e6f7r9dtk9c1, I9d117ni3tprb, I340k0hbj1hc6r, Ibapoov2fa817a, I2eon60c4gde7f, I7egr0053sjpci, I6r22p9usi2mkl, I6cm4c5a1euio9, I96k3nrdjfd63k, Ibg3cp8vjl5u55, Ibtu1gfmdnou5k, Iaoomvri5btde, Ic80igo4eds6rq, Iflrm8un6aibtn, I62rrikn5vj0p5, Ietm4rjshhu7sf, I1v9m3ms1elitm, I2t4b7068rtebl, I7a99hd3nbic2l, Ie8hpsm3jhsvo3, Ifcj247vgfdg56, Ic21uicfit5vcu, I375tmdui1ejfc, I4guv8rii4s6je, I2t2h3sjr2mdj0, I80tnmsfsu19sl, Ib7nn1mns0usdp, I4fivl1mrn0hhc, I2ead8rm0h16hm, I25l72483lbgf9, I9okvr56cd7277, I8k3rnvpeeh4hv, I56sht7incdimf, I8hge8nrufr05f, I5v0mk7rggegmh, Ideaemvoneh309, I3d9o9d7epp66v, I6lqh1vgb4mcja, I82nfqfkd48n10, I1jm8m1rh9e20v, I3o5j3bli1pd8e, Ivqkjqsbgj1dj, I5n4sebgkfr760, Ib6bm2ug64rldc, Ifs1i5fk9cqvr6, I5q3t0hm83a58h, I2gnaqoj2eimi0, Ieg3fd8p4pkt10, I8kg5ll427kfqq, I467333262q1l9, Idlqs144rc48hk, It11trpppbc3l, Ietml13sclqs1q, Iftfic7p3uban2, I2eb501t8s6hsq, Ianmuoljk2sk1u, I7hgtlnpelk0fc, I3p6khp3nv37cu, I6pnnj50tnq448, I57v1t6776pl3a, I1il5mj68vvsms, I42mob3hqe6j7h, Icdbq0j31b3g9c, I2t2rlclb0ce3e, Iar87gdqmug5o7, I3oullii9p80a1, I8t8ta6lfbia9e, I3akfmjle982qg, Ibaje86kdit7s6, I90lra4vl5j4db, I1q480m57ftcms, Ie2bjglo51atf6, Ievma38tc25kil, I2er75v4akf5cc, I5pldh0j0v0u4l, Ifhou5p0slv68r, I9m89dnau2i4tt, Ifunpjbsc4jrrr, I85uujfpnu8gum, I7bl5t0it6ck2m, I4iope0tjiqgu4, Iptqa236frcvo, I8hbi1vrve1i2, I1v9a50gjqk26k, Idv4d3rktbigfh, I56j1e9gqlq602, Ib6k4vik9ruq8h, I9u9gu9aa92l5m, Idmd4tos09qd68, Ia0sp2p68e9k16, Ie318529rgoagk, Iam4iou8r3isc1, I21ajnsdtbutjh, I203rofi4rpmo4, I1e290fmo892vi, Ie31ro5s5e089f, I71lu4gpn88cf0, I98iornf3ajrp9, I9893mbk9nh201, I623eo8t3jrbeo, Ieo8qamskgm4dk, Ift1efpssa32g2, Ibk3v0rrpo1bio, I1sj8huj7of8mb, I6av3sq9jkhmm3, I70cd7doki8rme, Iam7j42j9f1go6, Idco9ambhipg4i, I6s1nbislhk619, I9jtu7slb30qvs, Idv3j6a15pjc16, I206qvjkjun95i, I4qhb3plq4ifmq, Ic58lhlh1ocpm1, I6uopd4b2os90n, I6idbvi8v00o5j, Ifbgbhkj74b35k, Ibt4a800kb7frq, Icb4un8h4cokoo, I1up607q6ce947, I7hktg5sccf8op, Ib1d0bomkbrqv1, Iaflrold1ds0nq, I1ssp78ejl639m, I13qib3vtm9cs3, Idcabvplu05lea, Id38gdpcotl637, I73q3qf5u7nnqg, Idpm1bc2cr6dgj, I837c61fc07ine, I6m0oguilvhn8, I7vi74gbubc8u5, I3u0knmtb1ueq7, I87tlou92i0bot, Ifd3mkud9g8rb1, Iakvbbhvger3oa, I92t98snpjjcts, Iet4pe2le7ku09, I5dueehi6i2dg9, I64ev05f6q10es, Ikc5h15joooak, Ie8f436ua5fs59, I3mkis681qg30e, I2foqo7cbqf35v, I3mcu79ge1e54v, Icf66vuktncksu, Id69glo8rcjef, Ia2rnh5pfua40a, I3otc7e9a35k1k, I89ier5tb9ne0s, Im2f0numhevg3, I2agkcpojhkk43, I32rvg545edabm, I83fv0vi59md7i, I5tjjqcdd4tae0, I1894dm1lf1ae7, Idkfsqnep2hpeb, I2u5b4034ft9hp, I602p6mm30elei, Ia82mnkmeo2rhc, I6u3ru0d29kkj0, Icbccs0ug47ilf, I855j4i3kr8ko1, Ibk0nulspilods, I5768ac424h061, Icv68aq8841478, Ic262ibdoec56a, Iflcfm9b6nlmdd, Ijrsf4mnp3eka, Id5fm4p8lj5qgi, I8tjvj9uq4b7hi, I4cbvqmqadhrea, I3qt1hgg4djhgb, I4fooe9dun9o0t, Ier2cke86dqbr2, Io45lnue7n40k, I83e4tgdv5ohg1, I6o6dmud53u1fj, I39p6ln31i4n46, I95l2k9b1re95f, Ifkgc6cte1k96e, I6kvs2mb8unk0t, Idbuci3sr3i1f7, I73drt1hl9e70v, I1dm4sip108q0g, Iajgphfb1fka7l, I4hnmf90qkrer9, Ijsohbv0raf36, I4ga01hppthoe1, I4hckkcv10tcue, Ic871mj76419vm, If2ieedn10ujdv, Iaseh340tnovdh, I8m5umt6snnmlj, I5aeg4u9kpsp8o, I3fsv5f1boeqf3, Ifoov68qt28nbm, Ib937mhlbop6j7, I838gqvljm75tj, I1cu36qostj5d8, I4r2ptfsrl017r, Ielglukq9ekcit, I1clsdhcok4nle, Iemddv6u2buvfn, Ic149bnrif7lpr, I89dsvf7sdo4ko, I804q3c12638a0, Idguve298jnare, Idi3fb8585u2lp, I1327b77famnt3, If58ibsptjm2at, I5rtkmhm2dng4u, Iep27ialq4a7o7, Iasu5jvoqr43mv, I88p4dmln8611r, I5qolde99acmd1, I8gtde5abn1g9a, I3dvon8akhmsut, Ia3c82eadg79bj, Ienusoeb625ftq, Ibtsa3docbr9el, Iek6442ldi23n3, Idpdo54rotesu2, I2ur0oeqg495j8, Ibco2bqthggul0, I1bhd210c3phjj, Idcqgi2844k5he, Iej2173ou338sm, I20e9ph536u7ti, I8kcpmsh450rp, Iea4g5ovhnolus, Ifmc9boeeia623, Itmchvgqfl28g, I5tf7b5o64mfpl, If71d2q730qf6n, If0sk51c1n7ri8, I4b2eh3b1oi815, I57q620f4fu1bl, Ie5222qfrr24ek, I28g8sphdu312k, Idqbjt2c6r46t6, I853aigjva3f0t, I9uehhems5hkqm, I7q5qk4uoanhof, Iehpbs40l3jkit, Idht9upmipvd4j, Icns2sqr5hp8s3, I9n4hs8p3rlkag, I6a8j73186lfdf, I8vbtb6bd00lm0, I8v1041j74kmaj, Iaqet9jc3ihboe, Ic952bubvq4k7d, I2v50gu3s1aqk6, Ibmofsd95figtn, If7uv525tdvv7a, Itom7fk49o0c9, I2an1fs2eiebjp, Ie9sr1iqcg3cgm, I1mqgk2tmnn9i2, I6lr8sctk0bi4e, TransactionValidityTransactionSource, I9ask1o4tfvcvs, Icerf8h8pdu8ss, I9puqgoda8ofk4, I6spmpef2c7svf, Iei2mvq0mjvt81, If08sfhqn8ujfr, Ic4rgfgksgmm3e, I3dj14b7k3rkm5, Ic5egmm215ml6k, Ibg4am9lqg35ku, I7efspe2svrt0g, I5fvdd841odbi3, I35vouom6s9r2, Ie6kgk6f04rsvk, Ifgqf2rskq94om, Ie30stbbeaul1o, I7aold6s47n103, Ibjuap2vk03rp6, Iasb8k6ash5mjn, Ifla7g8u5j9k68, I9sijb8gfrns29, I17s97pb2d5tj3, I2dfliekq1ed7e, I4gah17u2nc33h, I9u22scd4ksrjm, Ibil6rvg3saeb3, I97cs1i8k87lnm, I874e758ge6pa9, I86tq0h1o8f1g5, I78cq8c9mego2f, I64hm01ml98m4p, I3gjbugrk45her, I9nvi04b7jiso4, I6s1052v0hl6mr, I31p8sd8onusg0, I2vgg418k9gfnm, I7dp6t7k7a8r36, Ibtpedbm9ai3hp, I8ivaf995pho4u, Icr6rj04unermu, I5gfdo8kg6rloq, Ibjoh8vk2j7bqd, I2u4s5o1c0r3fu, Ic0g2vnp5r296p, Ihfphjolmsqq1, Ic9fkrj2ggjleq, Ifi9cmevnosufh, I1i5jfmqcsjper, I3pbrjdm4vnbsa, Iems84l8lk2v0c, I1r5ke30ueqo0r, I68ii5ik8avr9o, I8slfm2rri67ri, I34n2itmpoq7on } from "./common-types";
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
type IStorage = {
    System: {
        /**
         * The full account information for a particular account ID.
         */
        Account: StorageDescriptor<[Key: SS58String], Anonymize<I5sesotjlssv2d>, false, never>;
        /**
         * Total extrinsics count for the current block.
         */
        ExtrinsicCount: StorageDescriptor<[], number, true, never>;
        /**
         * Whether all inherents have been applied.
         */
        InherentsApplied: StorageDescriptor<[], boolean, false, never>;
        /**
         * The current weight for the block.
         */
        BlockWeight: StorageDescriptor<[], Anonymize<Iffmde3ekjedi9>, false, never>;
        /**
         * Total length (in bytes) for all extrinsics put together, for the current block.
         */
        AllExtrinsicsLen: StorageDescriptor<[], number, true, never>;
        /**
         * Map of block numbers to block hashes.
         */
        BlockHash: StorageDescriptor<[Key: number], FixedSizeBinary<32>, false, never>;
        /**
         * Extrinsics data for the current block (maps an extrinsic's index to its data).
         */
        ExtrinsicData: StorageDescriptor<[Key: number], Binary, false, never>;
        /**
         * The current block number being processed. Set by `execute_block`.
         */
        Number: StorageDescriptor<[], number, false, never>;
        /**
         * Hash of the previous block.
         */
        ParentHash: StorageDescriptor<[], FixedSizeBinary<32>, false, never>;
        /**
         * Digest of the current block, also part of the block header.
         */
        Digest: StorageDescriptor<[], Anonymize<I4mddgoa69c0a2>, false, never>;
        /**
         * Events deposited for the current block.
         *
         * NOTE: The item is unbound and should therefore never be read on chain.
         * It could otherwise inflate the PoV size of a block.
         *
         * Events have a large in-memory size. Box the events to not go out-of-memory
         * just in case someone still reads them from within the runtime.
         */
        Events: StorageDescriptor<[], Anonymize<Icr4vaj0vrd6je>, false, never>;
        /**
         * The number of events in the `Events<T>` list.
         */
        EventCount: StorageDescriptor<[], number, false, never>;
        /**
         * Mapping between a topic (represented by T::Hash) and a vector of indexes
         * of events in the `<Events<T>>` list.
         *
         * All topic vectors have deterministic storage locations depending on the topic. This
         * allows light-clients to leverage the changes trie storage tracking mechanism and
         * in case of changes fetch the list of events of interest.
         *
         * The value has the type `(BlockNumberFor<T>, EventIndex)` because if we used only just
         * the `EventIndex` then in case if the topic has the same contents on the next block
         * no notification will be triggered thus the event might be lost.
         */
        EventTopics: StorageDescriptor<[Key: FixedSizeBinary<32>], Anonymize<I95g6i7ilua7lq>, false, never>;
        /**
         * Stores the `spec_version` and `spec_name` of when the last runtime upgrade happened.
         */
        LastRuntimeUpgrade: StorageDescriptor<[], Anonymize<Ieniouoqkq4icf>, true, never>;
        /**
         * True if we have upgraded so that `type RefCount` is `u32`. False (default) if not.
         */
        UpgradedToU32RefCount: StorageDescriptor<[], boolean, false, never>;
        /**
         * True if we have upgraded so that AccountInfo contains three types of `RefCount`. False
         * (default) if not.
         */
        UpgradedToTripleRefCount: StorageDescriptor<[], boolean, false, never>;
        /**
         * The execution phase of the block.
         */
        ExecutionPhase: StorageDescriptor<[], Phase, true, never>;
        /**
         * `Some` if a code upgrade has been authorized.
         */
        AuthorizedUpgrade: StorageDescriptor<[], Anonymize<Ibgl04rn6nbfm6>, true, never>;
        /**
         * The weight reclaimed for the extrinsic.
         *
         * This information is available until the end of the extrinsic execution.
         * More precisely this information is removed in `note_applied_extrinsic`.
         *
         * Logic doing some post dispatch weight reduction must update this storage to avoid duplicate
         * reduction.
         */
        ExtrinsicWeightReclaimed: StorageDescriptor<[], Anonymize<I4q39t5hn830vp>, false, never>;
    };
    RandomnessCollectiveFlip: {
        /**
         * Series of block headers from the last 81 blocks that acts as random seed material. This
         * is arranged as a ring buffer with `block_number % 81` being the index into the `Vec` of
         * the oldest hash.
         */
        RandomMaterial: StorageDescriptor<[], Anonymize<Ic5m5lp1oioo8r>, false, never>;
    };
    Timestamp: {
        /**
         * The current time for the current block.
         */
        Now: StorageDescriptor<[], bigint, false, never>;
        /**
         * Whether the timestamp has been updated in this block.
         *
         * This value is updated to `true` upon successful submission of a timestamp by a node.
         * It is then checked at the end of each block execution in the `on_finalize` hook.
         */
        DidUpdate: StorageDescriptor<[], boolean, false, never>;
    };
    Aura: {
        /**
         * The current authority set.
         */
        Authorities: StorageDescriptor<[], Anonymize<Ic5m5lp1oioo8r>, false, never>;
        /**
         * The current slot of this block.
         *
         * This will be set in `on_initialize`.
         */
        CurrentSlot: StorageDescriptor<[], bigint, false, never>;
    };
    Grandpa: {
        /**
         * State of the current authority set.
         */
        State: StorageDescriptor<[], GrandpaStoredState, false, never>;
        /**
         * Pending change: (signaled at, scheduled change).
         */
        PendingChange: StorageDescriptor<[], Anonymize<I7pe2me3i3vtn9>, true, never>;
        /**
         * next block number where we can force a change.
         */
        NextForced: StorageDescriptor<[], number, true, never>;
        /**
         * `true` if we are currently stalled.
         */
        Stalled: StorageDescriptor<[], Anonymize<I9jd27rnpm8ttv>, true, never>;
        /**
         * The number of changes (both in terms of keys and underlying economic responsibilities)
         * in the "set" of Grandpa validators from genesis.
         */
        CurrentSetId: StorageDescriptor<[], bigint, false, never>;
        /**
         * A mapping from grandpa set ID to the index of the *most recent* session for which its
         * members were responsible.
         *
         * This is only used for validating equivocation proofs. An equivocation proof must
         * contains a key-ownership proof for a given session, therefore we need a way to tie
         * together sessions and GRANDPA set ids, i.e. we need to validate that a validator
         * was the owner of a given key on a given session, and what the active set ID was
         * during that session.
         *
         * TWOX-NOTE: `SetId` is not under user control.
         */
        SetIdSession: StorageDescriptor<[Key: bigint], number, true, never>;
        /**
         * The current list of authorities.
         */
        Authorities: StorageDescriptor<[], Anonymize<I3geksg000c171>, false, never>;
    };
    Balances: {
        /**
         * The total units issued in the system.
         */
        TotalIssuance: StorageDescriptor<[], bigint, false, never>;
        /**
         * The total units of outstanding deactivated balance in the system.
         */
        InactiveIssuance: StorageDescriptor<[], bigint, false, never>;
        /**
         * The Balances pallet example of storing the balance of an account.
         *
         * # Example
         *
         * ```nocompile
         * impl pallet_balances::Config for Runtime {
         * type AccountStore = StorageMapShim<Self::Account<Runtime>, frame_system::Provider<Runtime>, AccountId, Self::AccountData<Balance>>
         * }
         * ```
         *
         * You can also store the balance of an account in the `System` pallet.
         *
         * # Example
         *
         * ```nocompile
         * impl pallet_balances::Config for Runtime {
         * type AccountStore = System
         * }
         * ```
         *
         * But this comes with tradeoffs, storing account balances in the system pallet stores
         * `frame_system` data alongside the account data contrary to storing account balances in the
         * `Balances` pallet, which uses a `StorageMap` to store balances data only.
         * NOTE: This is only used in the case that this pallet is used to store balances.
         */
        Account: StorageDescriptor<[Key: SS58String], Anonymize<I1q8tnt1cluu5j>, false, never>;
        /**
         * Any liquidity locks on some account balances.
         * NOTE: Should only be accessed when setting, changing and freeing a lock.
         *
         * Use of locks is deprecated in favour of freezes. See `https://github.com/paritytech/substrate/pull/12951/`
         */
        Locks: StorageDescriptor<[Key: SS58String], Anonymize<I8ds64oj6581v0>, false, never>;
        /**
         * Named reserves on some account balances.
         *
         * Use of reserves is deprecated in favour of holds. See `https://github.com/paritytech/substrate/pull/12951/`
         */
        Reserves: StorageDescriptor<[Key: SS58String], Anonymize<Ia7pdug7cdsg8g>, false, never>;
        /**
         * Holds on account balances.
         */
        Holds: StorageDescriptor<[Key: SS58String], Anonymize<I2hnk9r4ukuj1p>, false, never>;
        /**
         * Freeze locks on account balances.
         */
        Freezes: StorageDescriptor<[Key: SS58String], Anonymize<I9bin2jc70qt6q>, false, never>;
    };
    TransactionPayment: {
        /**
        
         */
        NextFeeMultiplier: StorageDescriptor<[], bigint, false, never>;
        /**
        
         */
        StorageVersion: StorageDescriptor<[], TransactionPaymentReleases, false, never>;
    };
    SubtensorModule: {
        /**
        
         */
        MinActivityCutoff: StorageDescriptor<[], number, false, never>;
        /**
         * Global window (in blocks) at the end of each tempo where admin ops are disallowed
         */
        AdminFreezeWindow: StorageDescriptor<[], number, false, never>;
        /**
         * Global number of epochs used to rate limit subnet owner hyperparameter updates
         */
        OwnerHyperparamRateLimit: StorageDescriptor<[], number, false, never>;
        /**
         * Duration of dissolve network schedule before execution
         */
        DissolveNetworkScheduleDuration: StorageDescriptor<[], number, false, never>;
        /**
         * --- DMap ( netuid, coldkey ) --> blocknumber | last hotkey swap on network.
         */
        LastHotkeySwapOnNetuid: StorageDescriptor<Anonymize<I7svnfko10tq2e>, bigint, false, never>;
        /**
         * Ensures unique IDs for StakeJobs storage map
         */
        NextStakeJobId: StorageDescriptor<[], bigint, false, never>;
        /**
         * ============================
         * ==== Staking Variables ====
         * ============================
         * The Subtensor [`TotalIssuance`] represents the total issuance of tokens on the Bittensor network.
         *
         * It is comprised of three parts:
         * - The total amount of issued tokens, tracked in the TotalIssuance of the Balances pallet
         * - The total amount of tokens staked in the system, tracked in [`TotalStake`]
         * - The total amount of tokens locked up for subnet reg, tracked in [`TotalSubnetLocked`] attained by iterating over subnet lock.
         *
         * Eventually, Bittensor should migrate to using Holds afterwhich time we will not require this
         * separate accounting.
         * --- ITEM --> Global weight
         */
        TaoWeight: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM --> CK burn
         */
        CKBurn: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM ( default_delegate_take )
         */
        MaxDelegateTake: StorageDescriptor<[], number, false, never>;
        /**
         * --- ITEM ( min_delegate_take )
         */
        MinDelegateTake: StorageDescriptor<[], number, false, never>;
        /**
         * --- ITEM ( default_childkey_take )
         */
        MaxChildkeyTake: StorageDescriptor<[], number, false, never>;
        /**
         * --- ITEM ( min_childkey_take )
         */
        MinChildkeyTake: StorageDescriptor<[], number, false, never>;
        /**
         * MAP ( hot ) --> cold | Returns the controlling coldkey for a hotkey
         */
        Owner: StorageDescriptor<[Key: SS58String], SS58String, false, never>;
        /**
         * MAP ( hot ) --> take | Returns the hotkey delegation take. And signals that this key is open for delegation
         */
        Delegates: StorageDescriptor<[Key: SS58String], number, false, never>;
        /**
         * DMAP ( hot, netuid ) --> take | Returns the hotkey childkey take for a specific subnet
         */
        ChildkeyTake: StorageDescriptor<Anonymize<I6ouflveob4eli>, number, false, never>;
        /**
         * DMAP ( netuid, parent ) --> (Vec<(proportion,child)>, cool_down_block)
         */
        PendingChildKeys: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<Idoeu5t0dum8va>, false, never>;
        /**
         * DMAP ( parent, netuid ) --> Vec<(proportion,child)>
         */
        ChildKeys: StorageDescriptor<Anonymize<I6ouflveob4eli>, Anonymize<I5n8gpu725k1nu>, false, never>;
        /**
         * DMAP ( child, netuid ) --> Vec<(proportion,parent)>
         */
        ParentKeys: StorageDescriptor<Anonymize<I6ouflveob4eli>, Anonymize<I5n8gpu725k1nu>, false, never>;
        /**
         * --- DMAP ( netuid, hotkey ) --> u64 | Last alpha dividend this hotkey got on tempo.
         */
        AlphaDividendsPerSubnet: StorageDescriptor<Anonymize<I7svnfko10tq2e>, bigint, false, never>;
        /**
         * --- DMAP ( netuid, hotkey ) --> u64 | Last root alpha dividend this hotkey got on tempo.
         */
        RootAlphaDividendsPerSubnet: StorageDescriptor<Anonymize<I7svnfko10tq2e>, bigint, false, never>;
        /**
         * ==================
         * ==== Coinbase ====
         * ==================
         * --- ITEM ( global_block_emission )
         */
        BlockEmission: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- DMap ( hot, netuid ) --> emission | last hotkey emission on network.
         */
        LastHotkeyEmissionOnNetuid: StorageDescriptor<Anonymize<I6ouflveob4eli>, bigint, false, never>;
        /**
         * ==========================
         * ==== Staking Counters ====
         * ==========================
         * The Subtensor [`TotalIssuance`] represents the total issuance of tokens on the Bittensor network.
         *
         * It is comprised of three parts:
         * - The total amount of issued tokens, tracked in the TotalIssuance of the Balances pallet
         * - The total amount of tokens staked in the system, tracked in [`TotalStake`]
         * - The total amount of tokens locked up for subnet reg, tracked in [`TotalSubnetLocked`] attained by iterating over subnet lock.
         *
         * Eventually, Bittensor should migrate to using Holds afterwhich time we will not require this
         * separate accounting.
         * --- ITEM ( maximum_number_of_networks )
         */
        SubnetLimit: StorageDescriptor<[], number, false, never>;
        /**
         * --- ITEM ( total_issuance )
         */
        TotalIssuance: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM ( total_stake )
         */
        TotalStake: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM ( moving_alpha ) -- subnet moving alpha.
         */
        SubnetMovingAlpha: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> moving_price | The subnet moving price.
         */
        SubnetMovingPrice: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> root_prop | The subnet root proportion.
         */
        RootProp: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> total_volume | The total amount of TAO bought and sold since the start of the network.
         */
        SubnetVolume: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> tao_in_subnet | Returns the amount of TAO in the subnet.
         */
        SubnetTAO: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> tao_in_user_subnet | Returns the amount of TAO in the subnet reserve provided by users as liquidity.
         */
        SubnetTaoProvided: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> alpha_in_emission | Returns the amount of alph in  emission into the pool per block.
         */
        SubnetAlphaInEmission: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> alpha_out_emission | Returns the amount of alpha out emission into the network per block.
         */
        SubnetAlphaOutEmission: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> tao_in_emission | Returns the amount of tao emitted into this subent on the last block.
         */
        SubnetTaoInEmission: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> alpha_supply_in_pool | Returns the amount of alpha in the pool.
         */
        SubnetAlphaIn: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> alpha_supply_user_in_pool | Returns the amount of alpha in the pool provided by users as liquidity.
         */
        SubnetAlphaInProvided: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> alpha_supply_in_subnet | Returns the amount of alpha in the subnet.
         */
        SubnetAlphaOut: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( cold ) --> Vec<hot> | Maps coldkey to hotkeys that stake to it
         */
        StakingHotkeys: StorageDescriptor<[Key: SS58String], Anonymize<Ia2lhg7l2hilo3>, false, never>;
        /**
         * --- MAP ( cold ) --> Vec<hot> | Returns the vector of hotkeys controlled by this coldkey.
         */
        OwnedHotkeys: StorageDescriptor<[Key: SS58String], Anonymize<Ia2lhg7l2hilo3>, false, never>;
        /**
         * --- DMAP ( cold, netuid )--> hot | Returns the hotkey a coldkey will autostake to with mining rewards.
         */
        AutoStakeDestination: StorageDescriptor<Anonymize<I6ouflveob4eli>, SS58String, true, never>;
        /**
         * --- DMAP ( hot, netuid )--> Vec<cold> | Returns a list of coldkeys that are autostaking to a hotkey
         */
        AutoStakeDestinationColdkeys: StorageDescriptor<Anonymize<I6ouflveob4eli>, Anonymize<Ia2lhg7l2hilo3>, false, never>;
        /**
         * The delay after an announcement before a coldkey swap can be performed.
         */
        ColdkeySwapAnnouncementDelay: StorageDescriptor<[], number, false, never>;
        /**
         * The delay after the initial delay has passed before a new announcement can be made.
         */
        ColdkeySwapReannouncementDelay: StorageDescriptor<[], number, false, never>;
        /**
         * A map of the coldkey swap announcements from a coldkey
         * to the block number the coldkey swap can be performed.
         */
        ColdkeySwapAnnouncements: StorageDescriptor<[Key: SS58String], Anonymize<I4p5t2krb1gmvp>, true, never>;
        /**
         * A map of the coldkey swap disputes from a coldkey to the
         * block number the coldkey swap was disputed.
         */
        ColdkeySwapDisputes: StorageDescriptor<[Key: SS58String], number, true, never>;
        /**
         * --- DMAP ( hot, netuid ) --> alpha | Returns the total amount of alpha a hotkey owns.
         */
        TotalHotkeyAlpha: StorageDescriptor<Anonymize<I6ouflveob4eli>, bigint, false, never>;
        /**
         * --- DMAP ( hot, netuid ) --> alpha | Returns the total amount of alpha a hotkey owned in the last epoch.
         */
        TotalHotkeyAlphaLastEpoch: StorageDescriptor<Anonymize<I6ouflveob4eli>, bigint, false, never>;
        /**
         * DMAP ( hot, netuid ) --> total_alpha_shares | Returns the number of alpha shares for a hotkey on a subnet.
         */
        TotalHotkeyShares: StorageDescriptor<Anonymize<I6ouflveob4eli>, bigint, false, never>;
        /**
         * --- NMAP ( hot, cold, netuid ) --> alpha | Returns the alpha shares for a hotkey, coldkey, netuid triplet.
         */
        Alpha: StorageDescriptor<Anonymize<I7svrbkiu01iec>, bigint, false, never>;
        /**
         * Contains last Alpha storage map key to iterate (check first)
         */
        AlphaMapLastKey: StorageDescriptor<[], Anonymize<Iabpgqcjikia83>, false, never>;
        /**
         * --- MAP ( netuid ) --> token_symbol | Returns the token symbol for a subnet.
         */
        TokenSymbol: StorageDescriptor<[Key: number], Binary, false, never>;
        /**
         * --- MAP ( netuid ) --> subnet_tao_flow | Returns the TAO inflow-outflow balance.
         */
        SubnetTaoFlow: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> subnet_ema_tao_flow | Returns the EMA of TAO inflow-outflow balance.
         */
        SubnetEmaTaoFlow: StorageDescriptor<[Key: number], Anonymize<I2j729bmgsdiuo>, true, never>;
        /**
         * --- ITEM --> TAO Flow Cutoff
         */
        TaoFlowCutoff: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM --> Flow Normalization Exponent (p)
         */
        FlowNormExponent: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM --> Flow EMA smoothing factor (flow alpha), u64 normalized
         */
        FlowEmaSmoothingFactor: StorageDescriptor<[], bigint, false, never>;
        /**
         * ============================
         * ==== Global Parameters =====
         * ============================
         * --- StorageItem Global Used Work.
         */
        UsedWork: StorageDescriptor<[Key: Binary], bigint, false, never>;
        /**
         * --- ITEM( global_max_registrations_per_block )
         */
        MaxRegistrationsPerBlock: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- ITEM( total_number_of_existing_networks )
         */
        TotalNetworks: StorageDescriptor<[], number, false, never>;
        /**
         * ITEM( network_immunity_period )
         */
        NetworkImmunityPeriod: StorageDescriptor<[], bigint, false, never>;
        /**
         * ITEM( start_call_delay )
         */
        StartCallDelay: StorageDescriptor<[], bigint, false, never>;
        /**
         * ITEM( min_network_lock_cost )
         */
        NetworkMinLockCost: StorageDescriptor<[], bigint, false, never>;
        /**
         * ITEM( last_network_lock_cost )
         */
        NetworkLastLockCost: StorageDescriptor<[], bigint, false, never>;
        /**
         * ITEM( network_lock_reduction_interval )
         */
        NetworkLockReductionInterval: StorageDescriptor<[], bigint, false, never>;
        /**
         * ITEM( subnet_owner_cut )
         */
        SubnetOwnerCut: StorageDescriptor<[], number, false, never>;
        /**
         * ITEM( network_rate_limit )
         */
        NetworkRateLimit: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM( nominator_min_required_stake ) --- Factor of DefaultMinStake in per-mill format.
         */
        NominatorMinRequiredStake: StorageDescriptor<[], bigint, false, never>;
        /**
         * ITEM( weights_version_key_rate_limit ) --- Rate limit in tempos.
         */
        WeightsVersionKeyRateLimit: StorageDescriptor<[], bigint, false, never>;
        /**
         * ============================
         * ==== Rate Limiting =====
         * ============================
         * --- MAP ( RateLimitKey ) --> Block number in which the last rate limited operation occured
         */
        LastRateLimitedBlock: StorageDescriptor<[Key: Anonymize<Iakavvne152v30>], bigint, false, never>;
        /**
         * ============================
         * ==== Subnet Locks =====
         * ============================
         * --- MAP ( netuid ) --> transfer_toggle
         */
        TransferToggle: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> total_subnet_locked
         */
        SubnetLocked: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> largest_locked
         */
        LargestLocked: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * =================
         * ==== Tempos =====
         * =================
         * --- MAP ( netuid ) --> tempo
         */
        Tempo: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * ============================
         * ==== Subnet Parameters =====
         * ============================
         * --- MAP ( netuid ) --> block number of first emission
         */
        FirstEmissionBlockNumber: StorageDescriptor<[Key: number], bigint, true, never>;
        /**
         * --- MAP ( netuid ) --> subnet mechanism
         */
        SubnetMechanism: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> subnetwork_n (Number of UIDs in the network).
         */
        SubnetworkN: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> network_is_added
         */
        NetworksAdded: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- DMAP ( hotkey, netuid ) --> bool
         */
        IsNetworkMember: StorageDescriptor<Anonymize<I6ouflveob4eli>, boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> network_registration_allowed
         */
        NetworkRegistrationAllowed: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> network_pow_allowed
         */
        NetworkPowRegistrationAllowed: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> block_created
         */
        NetworkRegisteredAt: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> pending_server_emission
         */
        PendingServerEmission: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> pending_validator_emission
         */
        PendingValidatorEmission: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> pending_root_alpha_emission
         */
        PendingRootAlphaDivs: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> pending_owner_cut
         */
        PendingOwnerCut: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> blocks_since_last_step
         */
        BlocksSinceLastStep: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> last_mechanism_step_block
         */
        LastMechansimStepBlock: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> subnet_owner
         */
        SubnetOwner: StorageDescriptor<[Key: number], SS58String, false, never>;
        /**
         * --- MAP ( netuid ) --> subnet_owner_hotkey
         */
        SubnetOwnerHotkey: StorageDescriptor<[Key: number], SS58String, false, never>;
        /**
         * --- MAP ( netuid ) --> recycle_or_burn
         */
        RecycleOrBurn: StorageDescriptor<[Key: number], Anonymize<Ib9tptuv3cggfs>, false, never>;
        /**
         * --- MAP ( netuid ) --> serving_rate_limit
         */
        ServingRateLimit: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> Rho
         */
        Rho: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> AlphaSigmoidSteepness
         */
        AlphaSigmoidSteepness: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> Kappa
         */
        Kappa: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> registrations_this_interval
         */
        RegistrationsThisInterval: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> pow_registrations_this_interval
         */
        POWRegistrationsThisInterval: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> burn_registrations_this_interval
         */
        BurnRegistrationsThisInterval: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> min_allowed_uids
         */
        MinAllowedUids: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> max_allowed_uids
         */
        MaxAllowedUids: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> immunity_period
         */
        ImmunityPeriod: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> activity_cutoff
         */
        ActivityCutoff: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> max_weight_limit
         */
        MaxWeightsLimit: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> weights_version_key
         */
        WeightsVersionKey: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> min_allowed_weights
         */
        MinAllowedWeights: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> max_allowed_validators
         */
        MaxAllowedValidators: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> adjustment_interval
         */
        AdjustmentInterval: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> bonds_moving_average
         */
        BondsMovingAverage: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> bonds_penalty
         */
        BondsPenalty: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> bonds_reset
         */
        BondsResetOn: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> weights_set_rate_limit
         */
        WeightsSetRateLimit: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> validator_prune_len
         */
        ValidatorPruneLen: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> scaling_law_power
         */
        ScalingLawPower: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> target_registrations_this_interval
         */
        TargetRegistrationsPerInterval: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> adjustment_alpha
         */
        AdjustmentAlpha: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> commit reveal v2 weights are enabled
         */
        CommitRevealWeightsEnabled: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> Burn
         */
        Burn: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> Difficulty
         */
        Difficulty: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> MinBurn
         */
        MinBurn: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> MaxBurn
         */
        MaxBurn: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> MinDifficulty
         */
        MinDifficulty: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> MaxDifficulty
         */
        MaxDifficulty: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) -->  Block at last adjustment.
         */
        LastAdjustmentBlock: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> Registrations of this Block.
         */
        RegistrationsThisBlock: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> Halving time of average moving price.
         */
        EMAPriceHalvingBlocks: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> global_RAO_recycled_for_registration
         */
        RAORecycledForRegistration: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- ITEM ( tx_rate_limit )
         */
        TxRateLimit: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM ( tx_delegate_take_rate_limit )
         */
        TxDelegateTakeRateLimit: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- ITEM ( tx_childkey_take_rate_limit )
         */
        TxChildkeyTakeRateLimit: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> Whether or not Liquid Alpha is enabled
         */
        LiquidAlphaOn: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> Whether or not Yuma3 is enabled
         */
        Yuma3On: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * MAP ( netuid ) --> (alpha_low, alpha_high)
         */
        AlphaValues: StorageDescriptor<[Key: number], Anonymize<I9jd27rnpm8ttv>, false, never>;
        /**
         * --- MAP ( netuid ) --> If subtoken trading enabled
         */
        SubtokenEnabled: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- DMAP ( netuid, hotkey ) --> voting_power | EMA of stake for voting
         * This tracks stake EMA updated every epoch when VotingPowerTrackingEnabled is true.
         * Used by smart contracts to determine validator voting power for subnet governance.
         */
        VotingPower: StorageDescriptor<Anonymize<I7svnfko10tq2e>, bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> bool | Whether voting power tracking is enabled for this subnet.
         * When enabled, VotingPower EMA is updated every epoch. Default is false.
         * When disabled with disable_at_block set, tracking continues until that block.
         */
        VotingPowerTrackingEnabled: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * --- MAP ( netuid ) --> block_number | Block at which voting power tracking will be disabled.
         * When set (non-zero), tracking continues until this block, then automatically disables
         * and clears VotingPower entries for the subnet. Provides a 14-day grace period.
         */
        VotingPowerDisableAtBlock: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> u64 | EMA alpha value for voting power calculation.
         * Higher alpha = faster response to stake changes.
         * Stored as u64 with 18 decimal precision (1.0 = 10^18).
         * Only settable by sudo/root.
         */
        VotingPowerEmaAlpha: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> Burn key limit
         */
        ImmuneOwnerUidsLimit: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * =======================================
         * ==== Subnetwork Consensus Storage  ====
         * =======================================
         * --- DMAP ( netuid ) --> stake_weight | weight for stake used in YC.
         */
        StakeWeight: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- DMAP ( netuid, hotkey ) --> uid
         */
        Uids: StorageDescriptor<Anonymize<I7svnfko10tq2e>, number, true, never>;
        /**
         * --- DMAP ( netuid, uid ) --> hotkey
         */
        Keys: StorageDescriptor<Anonymize<I9jd27rnpm8ttv>, SS58String, false, never>;
        /**
         * --- MAP ( netuid ) --> (hotkey, se, ve)
         */
        LoadedEmission: StorageDescriptor<[Key: number], Anonymize<I4h6ivgjtd51lv>, true, never>;
        /**
         * --- MAP ( netuid ) --> active
         */
        Active: StorageDescriptor<[Key: number], Anonymize<I9eir063evtfb6>, false, never>;
        /**
         * --- MAP ( netuid ) --> rank
         */
        Rank: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> trust
         */
        Trust: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> consensus
         */
        Consensus: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> incentive
         */
        Incentive: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> dividends
         */
        Dividends: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> emission
         */
        Emission: StorageDescriptor<[Key: number], Anonymize<Iafqnechp3omqg>, false, never>;
        /**
         * --- MAP ( netuid ) --> last_update
         */
        LastUpdate: StorageDescriptor<[Key: number], Anonymize<Iafqnechp3omqg>, false, never>;
        /**
         * --- MAP ( netuid ) --> validator_trust
         */
        ValidatorTrust: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> pruning_scores
         */
        PruningScores: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, false, never>;
        /**
         * --- MAP ( netuid ) --> validator_permit
         */
        ValidatorPermit: StorageDescriptor<[Key: number], Anonymize<I9eir063evtfb6>, false, never>;
        /**
         * --- DMAP ( netuid, uid ) --> weights
         */
        Weights: StorageDescriptor<Anonymize<I9jd27rnpm8ttv>, Anonymize<I95g6i7ilua7lq>, false, never>;
        /**
         * --- DMAP ( netuid, uid ) --> bonds
         */
        Bonds: StorageDescriptor<Anonymize<I9jd27rnpm8ttv>, Anonymize<I95g6i7ilua7lq>, false, never>;
        /**
         * --- DMAP ( netuid, uid ) --> block_at_registration
         */
        BlockAtRegistration: StorageDescriptor<Anonymize<I9jd27rnpm8ttv>, bigint, false, never>;
        /**
         * --- MAP ( netuid, hotkey ) --> axon_info
         */
        Axons: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<Ibc83gdj8hi3rc>, true, never>;
        /**
         * --- MAP ( netuid, hotkey ) --> certificate
         */
        NeuronCertificates: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<I9lpjucl20l82d>, true, never>;
        /**
         * --- MAP ( netuid, hotkey ) --> prometheus_info
         */
        Prometheus: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<Iaap7oohdmr1sb>, true, never>;
        /**
         * --- MAP ( coldkey ) --> identity
         */
        IdentitiesV2: StorageDescriptor<[Key: SS58String], Anonymize<Ifjlj958aeheic>, true, never>;
        /**
         * --- MAP ( netuid ) --> SubnetIdentityOfV3
         */
        SubnetIdentitiesV3: StorageDescriptor<[Key: number], Anonymize<I4tc54pa558g5n>, true, never>;
        /**
         * =================================
         * ==== Axon / Promo Endpoints =====
         * =================================
         * --- NMAP ( hot, netuid, name ) --> last_block | Returns the last block of a transaction for a given key, netuid, and name.
         */
        TransactionKeyLastBlock: StorageDescriptor<Anonymize<Id32h28hjj1tch>, bigint, false, never>;
        /**
         * --- MAP ( key ) --> last_block
         */
        LastTxBlock: StorageDescriptor<[Key: SS58String], bigint, false, never>;
        /**
         * --- MAP ( key ) --> last_tx_block_childkey_take
         */
        LastTxBlockChildKeyTake: StorageDescriptor<[Key: SS58String], bigint, false, never>;
        /**
         * --- MAP ( key ) --> last_tx_block_delegate_take
         */
        LastTxBlockDelegateTake: StorageDescriptor<[Key: SS58String], bigint, false, never>;
        /**
         * ITEM( weights_min_stake )
         */
        StakeThreshold: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- MAP (netuid, who) --> VecDeque<(hash, commit_block, first_reveal_block, last_reveal_block)> | Stores a queue of commits for an account on a given netuid.
         */
        WeightCommits: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<Icrrf4uohj5gb0>, true, never>;
        /**
         * MAP (netuid, epoch) → VecDeque<(who, commit_block, ciphertext, reveal_round)>
         * Stores a queue of weight commits for an account on a given subnet.
         */
        TimelockedWeightCommits: StorageDescriptor<Anonymize<I4ojmnsk1dchql>, Anonymize<I76jd8kl1mtn5g>, false, never>;
        /**
         * MAP (netuid, epoch) → VecDeque<(who, ciphertext, reveal_round)>
         * DEPRECATED for CRV3WeightCommitsV2
         */
        CRV3WeightCommits: StorageDescriptor<Anonymize<I4ojmnsk1dchql>, Anonymize<I4jqk5si14p5oi>, false, never>;
        /**
         * MAP (netuid, epoch) → VecDeque<(who, commit_block, ciphertext, reveal_round)>
         * DEPRECATED for TimelockedWeightCommits
         */
        CRV3WeightCommitsV2: StorageDescriptor<Anonymize<I4ojmnsk1dchql>, Anonymize<I76jd8kl1mtn5g>, false, never>;
        /**
         * --- Map (netuid) --> Number of epochs allowed for commit reveal periods
         */
        RevealPeriodEpochs: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- Map (coldkey, hotkey) --> u64 the last block at which stake was added/removed.
         */
        LastColdkeyHotkeyStakeBlock: StorageDescriptor<Anonymize<I2na29tt2afp0j>, bigint, true, never>;
        /**
         * DMAP ( hot, cold, netuid ) --> rate limits for staking operations
         * Value contains just a marker: we use this map as a set.
         */
        StakingOperationRateLimiter: StorageDescriptor<Anonymize<I7svrbkiu01iec>, boolean, false, never>;
        /**
        
         */
        RootClaimableThreshold: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
        
         */
        RootClaimable: StorageDescriptor<[Key: SS58String], Anonymize<If9jidduiuq7vv>, false, never>;
        /**
        
         */
        RootClaimed: StorageDescriptor<Anonymize<I2brm5b9jij1st>, bigint, false, never>;
        /**
        
         */
        RootClaimType: StorageDescriptor<[Key: SS58String], Anonymize<Iapm6e7vtp0l6r>, false, never>;
        /**
        
         */
        StakingColdkeysByIndex: StorageDescriptor<[Key: bigint], SS58String, true, never>;
        /**
        
         */
        StakingColdkeys: StorageDescriptor<[Key: SS58String], bigint, true, never>;
        /**
        
         */
        NumStakingColdkeys: StorageDescriptor<[], bigint, false, never>;
        /**
        
         */
        NumRootClaim: StorageDescriptor<[], bigint, false, never>;
        /**
         * =============================
         * ==== EVM related storage ====
         * =============================
         * --- DMAP (netuid, uid) --> (H160, last_block_where_ownership_was_proven)
         */
        AssociatedEvmAddress: StorageDescriptor<Anonymize<I9jd27rnpm8ttv>, Anonymize<I7tof95tckt2r>, true, never>;
        /**
         * ========================
         * ==== Subnet Leasing ====
         * ========================
         * --- MAP ( lease_id ) --> subnet lease | The subnet lease for a given lease id.
         */
        SubnetLeases: StorageDescriptor<[Key: number], Anonymize<Ieruonr5pk2d7h>, true, never>;
        /**
         * --- DMAP ( lease_id, contributor ) --> shares | The shares of a contributor for a given lease.
         */
        SubnetLeaseShares: StorageDescriptor<Anonymize<I7svnfko10tq2e>, bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> lease_id | The lease id for a given netuid.
         */
        SubnetUidToLeaseId: StorageDescriptor<[Key: number], number, true, never>;
        /**
         * --- ITEM ( next_lease_id ) | The next lease id.
         */
        NextSubnetLeaseId: StorageDescriptor<[], number, false, never>;
        /**
         * --- MAP ( lease_id ) --> accumulated_dividends | The accumulated dividends for a given lease that needs to be distributed.
         */
        AccumulatedLeaseDividends: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * --- ITEM ( CommitRevealWeightsVersion )
         */
        CommitRevealWeightsVersion: StorageDescriptor<[], number, false, never>;
        /**
         * ITEM( NetworkRegistrationStartBlock )
         */
        NetworkRegistrationStartBlock: StorageDescriptor<[], bigint, false, never>;
        /**
         * --- MAP ( netuid ) --> minimum required number of non-immortal & non-immune UIDs
         */
        MinNonImmuneUids: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * ITEM( max_mechanism_count )
         */
        MaxMechanismCount: StorageDescriptor<[], number, false, never>;
        /**
         * --- MAP ( netuid ) --> Current number of subnet mechanisms
         */
        MechanismCountCurrent: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * --- MAP ( netuid ) --> Normalized vector of emission split proportion between subnet mechanisms
         */
        MechanismEmissionSplit: StorageDescriptor<[Key: number], Anonymize<Icgljjb6j82uhn>, true, never>;
        /**
         * ==================
         * ==== Genesis =====
         * ==================
         * --- Storage for migration run status
         */
        HasMigrationRun: StorageDescriptor<[Key: Binary], boolean, false, never>;
        /**
         * Storage value for pending childkey cooldown, settable by root.
         */
        PendingChildKeyCooldown: StorageDescriptor<[], bigint, false, never>;
    };
    Sudo: {
        /**
         * The `AccountId` of the sudo key.
         */
        Key: StorageDescriptor<[], SS58String, true, never>;
    };
    Multisig: {
        /**
         * The set of open multisig operations.
         */
        Multisigs: StorageDescriptor<Anonymize<I8uo3fpd3bcc6f>, Anonymize<Iag146hmjgqfgj>, true, never>;
    };
    Preimage: {
        /**
         * The request status of a given hash.
         */
        StatusFor: StorageDescriptor<[Key: FixedSizeBinary<32>], PreimageOldRequestStatus, true, never>;
        /**
         * The request status of a given hash.
         */
        RequestStatusFor: StorageDescriptor<[Key: FixedSizeBinary<32>], PreimageRequestStatus, true, never>;
        /**
        
         */
        PreimageFor: StorageDescriptor<[Key: Anonymize<I4pact7n2e9a0i>], Binary, true, never>;
    };
    Scheduler: {
        /**
         * Block number at which the agenda began incomplete execution.
         */
        IncompleteSince: StorageDescriptor<[], number, true, never>;
        /**
         * Items to be executed, indexed by the block number that they should be executed on.
         */
        Agenda: StorageDescriptor<[Key: number], Anonymize<I11tetbe8ces3o>, false, never>;
        /**
         * Retry configurations for items to be executed, indexed by task address.
         */
        Retries: StorageDescriptor<[Key: Anonymize<I9jd27rnpm8ttv>], Anonymize<I56u24ncejr5kt>, true, never>;
        /**
         * Lookup from a name to the block number and index of the task.
         *
         * For v3 -> v4 the previously unbounded identities are Blake2-256 hashed to form the v4
         * identities.
         */
        Lookup: StorageDescriptor<[Key: FixedSizeBinary<32>], Anonymize<I9jd27rnpm8ttv>, true, never>;
    };
    Proxy: {
        /**
         * The set of account proxies. Maps the account which has delegated to the accounts
         * which are being delegated to, together with the amount held on deposit.
         */
        Proxies: StorageDescriptor<[Key: SS58String], Anonymize<I6tqrno2gaos08>, false, never>;
        /**
         * The announcements made by the proxy (key).
         */
        Announcements: StorageDescriptor<[Key: SS58String], Anonymize<I9p9lq3rej5bhc>, false, never>;
        /**
         * The result of the last call made by the proxy (key).
         */
        LastCallResult: StorageDescriptor<[Key: SS58String], Anonymize<Ibq6c27da62s2q>, true, never>;
    };
    Registry: {
        /**
         * Identity data by account
         */
        IdentityOf: StorageDescriptor<[Key: SS58String], Anonymize<Ib6u9l1gtc5l4t>, true, never>;
    };
    Commitments: {
        /**
         * Tracks all CommitmentOf that have at least one timelocked field.
         */
        TimelockedIndex: StorageDescriptor<[], Anonymize<I7nkl7ntqohel8>, false, never>;
        /**
         * Identity data by account
         */
        CommitmentOf: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<I3m6d7ohcp5n4v>, true, never>;
        /**
        
         */
        LastCommitment: StorageDescriptor<Anonymize<I7svnfko10tq2e>, number, true, never>;
        /**
        
         */
        LastBondsReset: StorageDescriptor<Anonymize<I7svnfko10tq2e>, number, true, never>;
        /**
        
         */
        RevealedCommitments: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<Ib9pv5dg6upo6t>, true, never>;
        /**
         * Maps (netuid, who) -> usage (how many “bytes” they've committed)
         * in the RateLimit window
         */
        UsedSpaceOf: StorageDescriptor<Anonymize<I7svnfko10tq2e>, Anonymize<I27ub49plcvb4c>, true, never>;
        /**
        
         */
        MaxSpace: StorageDescriptor<[], number, false, never>;
    };
    AdminUtils: {
        /**
         * Map PrecompileEnum --> enabled
         */
        PrecompileEnable: StorageDescriptor<[Key: Anonymize<I8un1ap2r4hhbj>], boolean, false, never>;
    };
    SafeMode: {
        /**
         * Contains the last block number that the safe-mode will remain entered in.
         *
         * Set to `None` when safe-mode is exited.
         *
         * Safe-mode is automatically exited when the current block number exceeds this value.
         */
        EnteredUntil: StorageDescriptor<[], number, true, never>;
        /**
         * Holds the reserve that was taken from an account at a specific block number.
         *
         * This helps governance to have an overview of outstanding deposits that should be returned or
         * slashed.
         */
        Deposits: StorageDescriptor<Anonymize<I6ouflveob4eli>, bigint, true, never>;
    };
    Ethereum: {
        /**
         * Mapping from transaction index to transaction in the current building block.
         */
        Pending: StorageDescriptor<[Key: number], Anonymize<Ic3l568el19b24>, true, never>;
        /**
         * Counter for the related counted storage map
         */
        CounterForPending: StorageDescriptor<[], number, false, never>;
        /**
         * The current Ethereum block.
         */
        CurrentBlock: StorageDescriptor<[], Anonymize<Ib0hfhkohlekcj>, true, never>;
        /**
         * The current Ethereum receipts.
         */
        CurrentReceipts: StorageDescriptor<[], Anonymize<I32lgu058i52q9>, true, never>;
        /**
         * The current transaction statuses.
         */
        CurrentTransactionStatuses: StorageDescriptor<[], Anonymize<Ie7atdsih6q14b>, true, never>;
        /**
        
         */
        BlockHash: StorageDescriptor<[Key: Anonymize<I4totqt881mlti>], FixedSizeBinary<32>, false, never>;
    };
    EVM: {
        /**
        
         */
        AccountCodes: StorageDescriptor<[Key: FixedSizeBinary<20>], Binary, false, never>;
        /**
        
         */
        AccountCodesMetadata: StorageDescriptor<[Key: FixedSizeBinary<20>], Anonymize<I7jidl7qnnq87c>, true, never>;
        /**
        
         */
        AccountStorages: StorageDescriptor<Anonymize<I82cps8ng2jtug>, FixedSizeBinary<32>, false, never>;
        /**
        
         */
        WhitelistedCreators: StorageDescriptor<[], Anonymize<I4gqmlq9k6jlk3>, false, never>;
        /**
        
         */
        DisableWhitelistCheck: StorageDescriptor<[], boolean, false, never>;
    };
    EVMChainId: {
        /**
         * The EVM chain ID.
         */
        ChainId: StorageDescriptor<[], bigint, false, never>;
    };
    BaseFee: {
        /**
        
         */
        BaseFeePerGas: StorageDescriptor<[], Anonymize<I4totqt881mlti>, false, never>;
        /**
        
         */
        Elasticity: StorageDescriptor<[], number, false, never>;
    };
    Drand: {
        /**
         * the drand beacon configuration
         */
        BeaconConfig: StorageDescriptor<[], Anonymize<I494mq1ertfc9k>, false, never>;
        /**
         * Storage for migration run status
         */
        HasMigrationRun: StorageDescriptor<[Key: Binary], boolean, false, never>;
        /**
         * map round number to pulse
         */
        Pulses: StorageDescriptor<[Key: bigint], Anonymize<Ialchst9lgd11u>, true, never>;
        /**
        
         */
        LastStoredRound: StorageDescriptor<[], bigint, false, never>;
        /**
         * oldest stored round
         */
        OldestStoredRound: StorageDescriptor<[], bigint, false, never>;
        /**
         * Defines the block when next unsigned transaction will be accepted.
         *
         * To prevent spam of unsigned (and unpaid!) transactions on the network,
         * we only allow one transaction per block.
         * This storage entry defines when new transaction is going to be accepted.
         */
        NextUnsignedAt: StorageDescriptor<[], number, false, never>;
    };
    Crowdloan: {
        /**
         * A map of crowdloan ids to their information.
         */
        Crowdloans: StorageDescriptor<[Key: number], Anonymize<If0p9hvn3kegj1>, true, never>;
        /**
         * The next incrementing crowdloan id.
         */
        NextCrowdloanId: StorageDescriptor<[], number, false, never>;
        /**
         * A map of crowdloan ids to their contributors and their contributions.
         */
        Contributions: StorageDescriptor<Anonymize<I7svnfko10tq2e>, bigint, true, never>;
        /**
         * The current crowdloan id that will be set during the finalize call, making it
         * temporarily accessible to the dispatched call.
         */
        CurrentCrowdloanId: StorageDescriptor<[], number, true, never>;
        /**
         * Storage for the migration run status.
         */
        HasMigrationRun: StorageDescriptor<[Key: Binary], boolean, false, never>;
    };
    Swap: {
        /**
         * The fee rate applied to swaps per subnet, normalized value between 0 and u16::MAX
         */
        FeeRate: StorageDescriptor<[Key: number], number, false, never>;
        /**
        
         */
        FeeGlobalTao: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
        
         */
        FeeGlobalAlpha: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * Storage for all ticks, using subnet ID as the primary key and tick index as the secondary key
         */
        Ticks: StorageDescriptor<Anonymize<I5g2vv0ckl2m8b>, Anonymize<I8ac0r18acljm6>, true, never>;
        /**
         * Storage to determine whether swap V3 was initialized for a specific subnet.
         */
        SwapV3Initialized: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * Storage for the square root price of Alpha token for each subnet.
         */
        AlphaSqrtPrice: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * Storage for the current price tick.
         */
        CurrentTick: StorageDescriptor<[Key: number], number, false, never>;
        /**
         * Storage for the current liquidity amount for each subnet.
         */
        CurrentLiquidity: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * Indicates whether a subnet has been switched to V3 swap from V2.
         * If `true`, the subnet is permanently on V3 swap mode allowing add/remove liquidity
         * operations. Once set to `true` for a subnet, it cannot be changed back to `false`.
         */
        EnabledUserLiquidity: StorageDescriptor<[Key: number], boolean, false, never>;
        /**
         * Storage for user positions, using subnet ID and account ID as keys
         * The value is a bounded vector of Position structs with details about the liquidity positions
         */
        Positions: StorageDescriptor<Anonymize<Icsknfl0f6r973>, Anonymize<I5mi4kb05lrsa9>, true, never>;
        /**
         * Position ID counter.
         */
        LastPositionId: StorageDescriptor<[], bigint, false, never>;
        /**
         * Tick index bitmap words storage
         */
        TickIndexBitmapWords: StorageDescriptor<Anonymize<I1ptic1rnhda0n>, bigint, false, never>;
        /**
         * TAO reservoir for scraps of protocol claimed fees.
         */
        ScrapReservoirTao: StorageDescriptor<[Key: number], bigint, false, never>;
        /**
         * Alpha reservoir for scraps of protocol claimed fees.
         */
        ScrapReservoirAlpha: StorageDescriptor<[Key: number], bigint, false, never>;
    };
    Contracts: {
        /**
         * A mapping from a contract's code hash to its code.
         */
        PristineCode: StorageDescriptor<[Key: FixedSizeBinary<32>], Binary, true, never>;
        /**
         * A mapping from a contract's code hash to its code info.
         */
        CodeInfoOf: StorageDescriptor<[Key: FixedSizeBinary<32>], Anonymize<I5kulbesqc1h1t>, true, never>;
        /**
         * This is a **monotonic** counter incremented on contract instantiation.
         *
         * This is used in order to generate unique trie ids for contracts.
         * The trie id of a new contract is calculated from hash(account_id, nonce).
         * The nonce is required because otherwise the following sequence would lead to
         * a possible collision of storage:
         *
         * 1. Create a new contract.
         * 2. Terminate the contract.
         * 3. Immediately recreate the contract with the same account_id.
         *
         * This is bad because the contents of a trie are deleted lazily and there might be
         * storage of the old instantiation still in it when the new contract is created. Please
         * note that we can't replace the counter by the block number because the sequence above
         * can happen in the same block. We also can't keep the account counter in memory only
         * because storage is the only way to communicate across different extrinsics in the
         * same block.
         *
         * # Note
         *
         * Do not use it to determine the number of contracts. It won't be decremented if
         * a contract is destroyed.
         */
        Nonce: StorageDescriptor<[], bigint, false, never>;
        /**
         * The code associated with a given account.
         *
         * TWOX-NOTE: SAFE since `AccountId` is a secure hash.
         */
        ContractInfoOf: StorageDescriptor<[Key: SS58String], Anonymize<I36dvimehsh2tm>, true, never>;
        /**
         * Evicted contracts that await child trie deletion.
         *
         * Child trie deletion is a heavy operation depending on the amount of storage items
         * stored in said trie. Therefore this operation is performed lazily in `on_idle`.
         */
        DeletionQueue: StorageDescriptor<[Key: number], Binary, true, never>;
        /**
         * A pair of monotonic counters used to track the latest contract marked for deletion
         * and the latest deleted contract in queue.
         */
        DeletionQueueCounter: StorageDescriptor<[], Anonymize<I8t4pajubp34g3>, false, never>;
        /**
         * A migration can span across multiple blocks. This storage defines a cursor to track the
         * progress of the migration, enabling us to resume from the last completed position.
         */
        MigrationInProgress: StorageDescriptor<[], Binary, true, never>;
    };
    MevShield: {
        /**
         * Current ML‑KEM‑768 public key bytes (encoded form).
         */
        CurrentKey: StorageDescriptor<[], Binary, true, never>;
        /**
         * Next ML‑KEM‑768 public key bytes, announced by the block author.
         */
        NextKey: StorageDescriptor<[], Binary, true, never>;
        /**
         * Buffered encrypted submissions, indexed by wrapper id.
         */
        Submissions: StorageDescriptor<[Key: FixedSizeBinary<32>], Anonymize<Ifdiflqufkknl8>, true, never>;
        /**
         * Hash(CurrentKey) per block, used to bind `key_hash` to the epoch at submit time.
         */
        KeyHashByBlock: StorageDescriptor<[Key: number], FixedSizeBinary<32>, true, never>;
    };
};
type ICalls = {
    System: {
        /**
         * Make some on-chain remark.
         *
         * Can be executed by every `origin`.
         */
        remark: TxDescriptor<Anonymize<I8ofcg5rbj0g2c>>;
        /**
         * Set the number of pages in the WebAssembly environment's heap.
         */
        set_heap_pages: TxDescriptor<Anonymize<I4adgbll7gku4i>>;
        /**
         * Set the new runtime code.
         */
        set_code: TxDescriptor<Anonymize<I6pjjpfvhvcfru>>;
        /**
         * Set the new runtime code without doing any checks of the given `code`.
         *
         * Note that runtime upgrades will not run if this is called with a not-increasing spec
         * version!
         */
        set_code_without_checks: TxDescriptor<Anonymize<I6pjjpfvhvcfru>>;
        /**
         * Set some items of storage.
         */
        set_storage: TxDescriptor<Anonymize<I9pj91mj79qekl>>;
        /**
         * Kill some items from storage.
         */
        kill_storage: TxDescriptor<Anonymize<I39uah9nss64h9>>;
        /**
         * Kill all storage items with a key that starts with the given prefix.
         *
         * **NOTE:** We rely on the Root origin to provide us the number of subkeys under
         * the prefix we are removing to accurately calculate the weight of this function.
         */
        kill_prefix: TxDescriptor<Anonymize<Ik64dknsq7k08>>;
        /**
         * Make some on-chain remark and emit event.
         */
        remark_with_event: TxDescriptor<Anonymize<I8ofcg5rbj0g2c>>;
        /**
         * Authorize an upgrade to a given `code_hash` for the runtime. The runtime can be supplied
         * later.
         *
         * This call requires Root origin.
         */
        authorize_upgrade: TxDescriptor<Anonymize<Ib51vk42m1po4n>>;
        /**
         * Authorize an upgrade to a given `code_hash` for the runtime. The runtime can be supplied
         * later.
         *
         * WARNING: This authorizes an upgrade that will take place without any safety checks, for
         * example that the spec name remains the same and that the version number increases. Not
         * recommended for normal use. Use `authorize_upgrade` instead.
         *
         * This call requires Root origin.
         */
        authorize_upgrade_without_checks: TxDescriptor<Anonymize<Ib51vk42m1po4n>>;
        /**
         * Provide the preimage (runtime binary) `code` for an upgrade that has been authorized.
         *
         * If the authorization required a version check, this call will ensure the spec name
         * remains unchanged and that the spec version has increased.
         *
         * Depending on the runtime's `OnSetCode` configuration, this function may directly apply
         * the new `code` in the same block or attempt to schedule the upgrade.
         *
         * All origins are allowed.
         */
        apply_authorized_upgrade: TxDescriptor<Anonymize<I6pjjpfvhvcfru>>;
    };
    Timestamp: {
        /**
         * Set the current time.
         *
         * This call should be invoked exactly once per block. It will panic at the finalization
         * phase, if this call hasn't been invoked by that time.
         *
         * The timestamp should be greater than the previous one by the amount specified by
         * [`Config::MinimumPeriod`].
         *
         * The dispatch origin for this call must be _None_.
         *
         * This dispatch class is _Mandatory_ to ensure it gets executed in the block. Be aware
         * that changing the complexity of this call could result exhausting the resources in a
         * block to execute any other calls.
         *
         * ## Complexity
         * - `O(1)` (Note that implementations of `OnTimestampSet` must also be `O(1)`)
         * - 1 storage read and 1 storage mutation (codec `O(1)` because of `DidUpdate::take` in
         * `on_finalize`)
         * - 1 event handler `on_timestamp_set`. Must be `O(1)`.
         */
        set: TxDescriptor<Anonymize<Idcr6u6361oad9>>;
    };
    Grandpa: {
        /**
         * Report voter equivocation/misbehavior. This method will verify the
         * equivocation proof and validate the given key ownership proof
         * against the extracted offender. If both are valid, the offence
         * will be reported.
         */
        report_equivocation: TxDescriptor<Anonymize<I3a5kuu5t5jj3g>>;
        /**
         * Report voter equivocation/misbehavior. This method will verify the
         * equivocation proof and validate the given key ownership proof
         * against the extracted offender. If both are valid, the offence
         * will be reported.
         *
         * This extrinsic must be called unsigned and it is expected that only
         * block authors will call it (validated in `ValidateUnsigned`), as such
         * if the block author is defined it will be defined as the equivocation
         * reporter.
         */
        report_equivocation_unsigned: TxDescriptor<Anonymize<I3a5kuu5t5jj3g>>;
        /**
         * Note that the current authority set of the GRANDPA finality gadget has stalled.
         *
         * This will trigger a forced authority set change at the beginning of the next session, to
         * be enacted `delay` blocks after that. The `delay` should be high enough to safely assume
         * that the block signalling the forced change will not be re-orged e.g. 1000 blocks.
         * The block production rate (which may be slowed down because of finality lagging) should
         * be taken into account when choosing the `delay`. The GRANDPA voters based on the new
         * authority will start voting on top of `best_finalized_block_number` for new finalized
         * blocks. `best_finalized_block_number` should be the highest of the latest finalized
         * block of all validators of the new authority set.
         *
         * Only callable by root.
         */
        note_stalled: TxDescriptor<Anonymize<I2hviml3snvhhn>>;
    };
    Balances: {
        /**
         * Transfer some liquid free balance to another account.
         *
         * `transfer_allow_death` will set the `FreeBalance` of the sender and receiver.
         * If the sender's account is below the existential deposit as a result
         * of the transfer, the account will be reaped.
         *
         * The dispatch origin for this call must be `Signed` by the transactor.
         */
        transfer_allow_death: TxDescriptor<Anonymize<I4ktuaksf5i1gk>>;
        /**
         * Exactly as `transfer_allow_death`, except the origin must be root and the source account
         * may be specified.
         */
        force_transfer: TxDescriptor<Anonymize<I9bqtpv2ii35mp>>;
        /**
         * Same as the [`transfer_allow_death`] call, but with a check that the transfer will not
         * kill the origin account.
         *
         * 99% of the time you want [`transfer_allow_death`] instead.
         *
         * [`transfer_allow_death`]: struct.Pallet.html#method.transfer
         */
        transfer_keep_alive: TxDescriptor<Anonymize<I4ktuaksf5i1gk>>;
        /**
         * Transfer the entire transferable balance from the caller account.
         *
         * NOTE: This function only attempts to transfer _transferable_ balances. This means that
         * any locked, reserved, or existential deposits (when `keep_alive` is `true`), will not be
         * transferred by this function. To ensure that this function results in a killed account,
         * you might need to prepare the account by removing any reference counters, storage
         * deposits, etc...
         *
         * The dispatch origin of this call must be Signed.
         *
         * - `dest`: The recipient of the transfer.
         * - `keep_alive`: A boolean to determine if the `transfer_all` operation should send all
         * of the funds the account has, causing the sender account to be killed (false), or
         * transfer everything except at least the existential deposit, which will guarantee to
         * keep the sender account alive (true).
         */
        transfer_all: TxDescriptor<Anonymize<I9j7pagd6d4bda>>;
        /**
         * Unreserve some balance from a user by force.
         *
         * Can only be called by ROOT.
         */
        force_unreserve: TxDescriptor<Anonymize<I2h9pmio37r7fb>>;
        /**
         * Upgrade a specified account.
         *
         * - `origin`: Must be `Signed`.
         * - `who`: The account to be upgraded.
         *
         * This will waive the transaction fee if at least all but 10% of the accounts needed to
         * be upgraded. (We let some not have to be upgraded just in order to allow for the
         * possibility of churn).
         */
        upgrade_accounts: TxDescriptor<Anonymize<Ibmr18suc9ikh9>>;
        /**
         * Set the regular balance of a given account.
         *
         * The dispatch origin for this call is `root`.
         */
        force_set_balance: TxDescriptor<Anonymize<I9iq22t0burs89>>;
        /**
         * Adjust the total issuance in a saturating way.
         *
         * Can only be called by root and always needs a positive `delta`.
         *
         * # Example
         */
        force_adjust_total_issuance: TxDescriptor<Anonymize<I5u8olqbbvfnvf>>;
        /**
         * Burn the specified liquid free balance from the origin account.
         *
         * If the origin's account ends up below the existential deposit as a result
         * of the burn and `keep_alive` is false, the account will be reaped.
         *
         * Unlike sending funds to a _burn_ address, which merely makes the funds inaccessible,
         * this `burn` operation will reduce total issuance by the amount _burned_.
         */
        burn: TxDescriptor<Anonymize<I5utcetro501ir>>;
    };
    SubtensorModule: {
        /**
         * --- Sets the caller weights for the incentive mechanism. The call can be
         * made from the hotkey account so is potentially insecure, however, the damage
         * of changing weights is minimal if caught early. This function includes all the
         * checks that the passed weights meet the requirements. Stored as u16s they represent
         * rational values in the range [0,1] which sum to 1 and can be interpreted as
         * probabilities. The specific weights determine how inflation propagates outward
         * from this peer.
         *
         * Note: The 16 bit integers weights should represent 1.0 as the max u16.
         * However, the function normalizes all integers to u16_max anyway. This means that if the sum of all
         * elements is larger or smaller than the amount of elements * u16_max, all elements
         * will be corrected for this deviation.
         *
         * # Args:
         * * `origin`: (<T as frame_system::Config>Origin):
         * - The caller, a hotkey who wishes to set their weights.
         *
         * * `netuid` (u16):
         * - The network uid we are setting these weights on.
         *
         * * `dests` (Vec<u16>):
         * - The edge endpoint for the weight, i.e. j for w_ij.
         *
         * * 'weights' (Vec<u16>):
         * - The u16 integer encoded weights. Interpreted as rational
         * values in the range [0,1]. They must sum to in32::MAX.
         *
         * * 'version_key' ( u64 ):
         * - The network version key to check if the validator is up to date.
         *
         * # Event:
         * * WeightsSet;
         * - On successfully setting the weights on chain.
         *
         * # Raises:
         * * 'MechanismDoesNotExist':
         * - Attempting to set weights on a non-existent network.
         *
         * * 'NotRegistered':
         * - Attempting to set weights from a non registered account.
         *
         * * 'WeightVecNotEqualSize':
         * - Attempting to set weights with uids not of same length.
         *
         * * 'DuplicateUids':
         * - Attempting to set weights with duplicate uids.
         *
         * * 'UidsLengthExceedUidsInSubNet':
         * - Attempting to set weights above the max allowed uids.
         *
         * * 'UidVecContainInvalidOne':
         * - Attempting to set weights with invalid uids.
         *
         * * 'WeightVecLengthIsLow':
         * - Attempting to set weights with fewer weights than min.
         *
         * * 'MaxWeightExceeded':
         * - Attempting to set weights with max value exceeding limit.
         */
        set_weights: TxDescriptor<Anonymize<Icv6ofu4lqekr4>>;
        /**
         * --- Sets the caller weights for the incentive mechanism for mechanisms. The call
         * can be made from the hotkey account so is potentially insecure, however, the damage
         * of changing weights is minimal if caught early. This function includes all the
         * checks that the passed weights meet the requirements. Stored as u16s they represent
         * rational values in the range [0,1] which sum to 1 and can be interpreted as
         * probabilities. The specific weights determine how inflation propagates outward
         * from this peer.
         *
         * Note: The 16 bit integers weights should represent 1.0 as the max u16.
         * However, the function normalizes all integers to u16_max anyway. This means that if the sum of all
         * elements is larger or smaller than the amount of elements * u16_max, all elements
         * will be corrected for this deviation.
         *
         * # Args:
         * * `origin`: (<T as frame_system::Config>Origin):
         * - The caller, a hotkey who wishes to set their weights.
         *
         * * `netuid` (u16):
         * - The network uid we are setting these weights on.
         *
         * * `mecid` (`u8`):
         * - The u8 mechnism identifier.
         *
         * * `dests` (Vec<u16>):
         * - The edge endpoint for the weight, i.e. j for w_ij.
         *
         * * 'weights' (Vec<u16>):
         * - The u16 integer encoded weights. Interpreted as rational
         * values in the range [0,1]. They must sum to in32::MAX.
         *
         * * 'version_key' ( u64 ):
         * - The network version key to check if the validator is up to date.
         *
         * # Event:
         * * WeightsSet;
         * - On successfully setting the weights on chain.
         *
         * # Raises:
         * * 'MechanismDoesNotExist':
         * - Attempting to set weights on a non-existent network.
         *
         * * 'NotRegistered':
         * - Attempting to set weights from a non registered account.
         *
         * * 'WeightVecNotEqualSize':
         * - Attempting to set weights with uids not of same length.
         *
         * * 'DuplicateUids':
         * - Attempting to set weights with duplicate uids.
         *
         * * 'UidsLengthExceedUidsInSubNet':
         * - Attempting to set weights above the max allowed uids.
         *
         * * 'UidVecContainInvalidOne':
         * - Attempting to set weights with invalid uids.
         *
         * * 'WeightVecLengthIsLow':
         * - Attempting to set weights with fewer weights than min.
         *
         * * 'MaxWeightExceeded':
         * - Attempting to set weights with max value exceeding limit.
         */
        set_mechanism_weights: TxDescriptor<Anonymize<I48embv0n659kj>>;
        /**
         * --- Allows a hotkey to set weights for multiple netuids as a batch.
         *
         * # Args:
         * * `origin`: (<T as frame_system::Config>Origin):
         * - The caller, a hotkey who wishes to set their weights.
         *
         * * `netuids` (Vec<Compact<u16>>):
         * - The network uids we are setting these weights on.
         *
         * * `weights` (Vec<Vec<(Compact<u16>, Compact<u16>)>):
         * - The weights to set for each network. [(uid, weight), ...]
         *
         * * `version_keys` (Vec<Compact<u64>>):
         * - The network version keys to check if the validator is up to date.
         *
         * # Event:
         * * WeightsSet;
         * - On successfully setting the weights on chain.
         * * BatchWeightsCompleted;
         * - On success of the batch.
         * * BatchCompletedWithErrors;
         * - On failure of any of the weights in the batch.
         * * BatchWeightItemFailed;
         * - On failure for each failed item in the batch.
         *
         */
        batch_set_weights: TxDescriptor<Anonymize<I8l6dbd18t5aja>>;
        /**
         * ---- Used to commit a hash of your weight values to later be revealed.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The signature of the committing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `commit_hash` (`H256`):
         * - The hash representing the committed weights.
         *
         * # Raises:
         * * `CommitRevealDisabled`:
         * - Attempting to commit when the commit-reveal mechanism is disabled.
         *
         * * `TooManyUnrevealedCommits`:
         * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
         *
         */
        commit_weights: TxDescriptor<Anonymize<I513du23unvan>>;
        /**
         * ---- Used to commit a hash of your weight values to later be revealed for mechanisms.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The signature of the committing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `mecid` (`u8`):
         * - The u8 mechanism identifier.
         *
         * * `commit_hash` (`H256`):
         * - The hash representing the committed weights.
         *
         * # Raises:
         * * `CommitRevealDisabled`:
         * - Attempting to commit when the commit-reveal mechanism is disabled.
         *
         * * `TooManyUnrevealedCommits`:
         * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
         *
         */
        commit_mechanism_weights: TxDescriptor<Anonymize<I36o6oho99gjm8>>;
        /**
         * --- Allows a hotkey to commit weight hashes for multiple netuids as a batch.
         *
         * # Args:
         * * `origin`: (<T as frame_system::Config>Origin):
         * - The caller, a hotkey who wishes to set their weights.
         *
         * * `netuids` (Vec<Compact<u16>>):
         * - The network uids we are setting these weights on.
         *
         * * `commit_hashes` (Vec<H256>):
         * - The commit hashes to commit.
         *
         * # Event:
         * * WeightsSet;
         * - On successfully setting the weights on chain.
         * * BatchWeightsCompleted;
         * - On success of the batch.
         * * BatchCompletedWithErrors;
         * - On failure of any of the weights in the batch.
         * * BatchWeightItemFailed;
         * - On failure for each failed item in the batch.
         *
         */
        batch_commit_weights: TxDescriptor<Anonymize<If3mvus4cmnb7l>>;
        /**
         * ---- Used to reveal the weights for a previously committed hash.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The signature of the revealing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `uids` (`Vec<u16>`):
         * - The uids for the weights being revealed.
         *
         * * `values` (`Vec<u16>`):
         * - The values of the weights being revealed.
         *
         * * `salt` (`Vec<u16>`):
         * - The salt used to generate the commit hash.
         *
         * * `version_key` (`u64`):
         * - The network version key.
         *
         * # Raises:
         * * `CommitRevealDisabled`:
         * - Attempting to reveal weights when the commit-reveal mechanism is disabled.
         *
         * * `NoWeightsCommitFound`:
         * - Attempting to reveal weights without an existing commit.
         *
         * * `ExpiredWeightCommit`:
         * - Attempting to reveal a weight commit that has expired.
         *
         * * `RevealTooEarly`:
         * - Attempting to reveal weights outside the valid reveal period.
         *
         * * `InvalidRevealCommitHashNotMatch`:
         * - The revealed hash does not match any committed hash.
         *
         */
        reveal_weights: TxDescriptor<Anonymize<I3qrhi1ua10nnf>>;
        /**
         * ---- Used to reveal the weights for a previously committed hash for mechanisms.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The signature of the revealing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `mecid` (`u8`):
         * - The u8 mechanism identifier.
         *
         * * `uids` (`Vec<u16>`):
         * - The uids for the weights being revealed.
         *
         * * `values` (`Vec<u16>`):
         * - The values of the weights being revealed.
         *
         * * `salt` (`Vec<u16>`):
         * - The salt used to generate the commit hash.
         *
         * * `version_key` (`u64`):
         * - The network version key.
         *
         * # Raises:
         * * `CommitRevealDisabled`:
         * - Attempting to reveal weights when the commit-reveal mechanism is disabled.
         *
         * * `NoWeightsCommitFound`:
         * - Attempting to reveal weights without an existing commit.
         *
         * * `ExpiredWeightCommit`:
         * - Attempting to reveal a weight commit that has expired.
         *
         * * `RevealTooEarly`:
         * - Attempting to reveal weights outside the valid reveal period.
         *
         * * `InvalidRevealCommitHashNotMatch`:
         * - The revealed hash does not match any committed hash.
         *
         */
        reveal_mechanism_weights: TxDescriptor<Anonymize<I2hpc4ev2drsf2>>;
        /**
         * ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The committing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `commit` (`Vec<u8>`):
         * - The encrypted compressed commit.
         * The steps for this are:
         * 1. Instantiate [`WeightsTlockPayload`]
         * 2. Serialize it using the `parity_scale_codec::Encode` trait
         * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
         * to produce a [`TLECiphertext<TinyBLS381>`] type.
         * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
         *
         * * reveal_round (`u64`):
         * - The drand reveal round which will be avaliable during epoch `n+1` from the current
         * epoch.
         *
         * # Raises:
         * * `CommitRevealV3Disabled`:
         * - Attempting to commit when the commit-reveal mechanism is disabled.
         *
         * * `TooManyUnrevealedCommits`:
         * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
         *
         * ---- Used to commit encrypted commit-reveal v3 weight values to later be revealed for mechanisms.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The committing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `mecid` (`u8`):
         * - The u8 mechanism identifier.
         *
         * * `commit` (`Vec<u8>`):
         * - The encrypted compressed commit.
         * The steps for this are:
         * 1. Instantiate [`WeightsTlockPayload`]
         * 2. Serialize it using the `parity_scale_codec::Encode` trait
         * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
         * to produce a [`TLECiphertext<TinyBLS381>`] type.
         * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
         *
         * * reveal_round (`u64`):
         * - The drand reveal round which will be avaliable during epoch `n+1` from the current
         * epoch.
         *
         * # Raises:
         * * `CommitRevealV3Disabled`:
         * - Attempting to commit when the commit-reveal mechanism is disabled.
         *
         * * `TooManyUnrevealedCommits`:
         * - Attempting to commit when the user has more than the allowed limit of unrevealed commits.
         *
         */
        commit_crv3_mechanism_weights: TxDescriptor<Anonymize<I73q6qh9ckhm04>>;
        /**
         * ---- The implementation for batch revealing committed weights.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The signature of the revealing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `uids_list` (`Vec<Vec<u16>>`):
         * - A list of uids for each set of weights being revealed.
         *
         * * `values_list` (`Vec<Vec<u16>>`):
         * - A list of values for each set of weights being revealed.
         *
         * * `salts_list` (`Vec<Vec<u16>>`):
         * - A list of salts used to generate the commit hashes.
         *
         * * `version_keys` (`Vec<u64>`):
         * - A list of network version keys.
         *
         * # Raises:
         * * `CommitRevealDisabled`:
         * - Attempting to reveal weights when the commit-reveal mechanism is disabled.
         *
         * * `NoWeightsCommitFound`:
         * - Attempting to reveal weights without an existing commit.
         *
         * * `ExpiredWeightCommit`:
         * - Attempting to reveal a weight commit that has expired.
         *
         * * `RevealTooEarly`:
         * - Attempting to reveal weights outside the valid reveal period.
         *
         * * `InvalidRevealCommitHashNotMatch`:
         * - The revealed hash does not match any committed hash.
         *
         * * `InvalidInputLengths`:
         * - The input vectors are of mismatched lengths.
         */
        batch_reveal_weights: TxDescriptor<Anonymize<Idia8cmqvul6et>>;
        /**
         * --- Allows delegates to decrease its take value.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * 'hotkey' (T::AccountId):
         * - The hotkey we are delegating (must be owned by the coldkey.)
         *
         * * 'netuid' (u16):
         * - Subnet ID to decrease take for
         *
         * * 'take' (u16):
         * - The new stake proportion that this hotkey takes from delegations.
         * The new value can be between 0 and 11_796 and should be strictly
         * lower than the previous value. It T is the new value (rational number),
         * the the parameter is calculated as [65535 * T]. For example, 1% would be
         * [0.01 * 65535] = [655.35] = 655
         *
         * # Event:
         * * TakeDecreased;
         * - On successfully setting a decreased take for this hotkey.
         *
         * # Raises:
         * * 'NotRegistered':
         * - The hotkey we are delegating is not registered on the network.
         *
         * * 'NonAssociatedColdKey':
         * - The hotkey we are delegating is not owned by the calling coldkey.
         *
         * * 'DelegateTakeTooLow':
         * - The delegate is setting a take which is not lower than the previous.
         *
         */
        decrease_take: TxDescriptor<Anonymize<Idardmhchnv8aa>>;
        /**
         * --- Allows delegates to increase its take value. This call is rate-limited.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * 'hotkey' (T::AccountId):
         * - The hotkey we are delegating (must be owned by the coldkey.)
         *
         * * 'take' (u16):
         * - The new stake proportion that this hotkey takes from delegations.
         * The new value can be between 0 and 11_796 and should be strictly
         * greater than the previous value. T is the new value (rational number),
         * the the parameter is calculated as [65535 * T]. For example, 1% would be
         * [0.01 * 65535] = [655.35] = 655
         *
         * # Event:
         * * TakeIncreased;
         * - On successfully setting a increased take for this hotkey.
         *
         * # Raises:
         * * 'NotRegistered':
         * - The hotkey we are delegating is not registered on the network.
         *
         * * 'NonAssociatedColdKey':
         * - The hotkey we are delegating is not owned by the calling coldkey.
         *
         * * 'DelegateTakeTooHigh':
         * - The delegate is setting a take which is not greater than the previous.
         *
         */
        increase_take: TxDescriptor<Anonymize<Idardmhchnv8aa>>;
        /**
         * --- Adds stake to a hotkey. The call is made from a coldkey account.
         * This delegates stake to the hotkey.
         *
         * Note: the coldkey account may own the hotkey, in which case they are
         * delegating to themselves.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller's coldkey.
         *
         * * 'hotkey' (T::AccountId):
         * - The associated hotkey account.
         *
         * * 'netuid' (u16):
         * - Subnetwork UID
         *
         * * 'amount_staked' (u64):
         * - The amount of stake to be added to the hotkey staking account.
         *
         * # Event:
         * * StakeAdded;
         * - On the successfully adding stake to a global account.
         *
         * # Raises:
         * * 'NotEnoughBalanceToStake':
         * - Not enough balance on the coldkey to add onto the global account.
         *
         * * 'NonAssociatedColdKey':
         * - The calling coldkey is not associated with this hotkey.
         *
         * * 'BalanceWithdrawalError':
         * - Errors stemming from transaction pallet.
         *
         */
        add_stake: TxDescriptor<Anonymize<Icud5m8j0nlgtj>>;
        /**
         * Remove stake from the staking account. The call must be made
         * from the coldkey account attached to the neuron metadata. Only this key
         * has permission to make staking and unstaking requests.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller's coldkey.
         *
         * * 'hotkey' (T::AccountId):
         * - The associated hotkey account.
         *
         * * 'netuid' (u16):
         * - Subnetwork UID
         *
         * * 'amount_unstaked' (u64):
         * - The amount of stake to be added to the hotkey staking account.
         *
         * # Event:
         * * StakeRemoved;
         * - On the successfully removing stake from the hotkey account.
         *
         * # Raises:
         * * 'NotRegistered':
         * - Thrown if the account we are attempting to unstake from is non existent.
         *
         * * 'NonAssociatedColdKey':
         * - Thrown if the coldkey does not own the hotkey we are unstaking from.
         *
         * * 'NotEnoughStakeToWithdraw':
         * - Thrown if there is not enough stake on the hotkey to withdwraw this amount.
         *
         */
        remove_stake: TxDescriptor<Anonymize<I850u7ir5o34um>>;
        /**
         * Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is
         * already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller.
         *
         * * 'netuid' (u16):
         * - The u16 network identifier.
         *
         * * 'version' (u64):
         * - The bittensor version identifier.
         *
         * * 'ip' (u64):
         * - The endpoint ip information as a u128 encoded integer.
         *
         * * 'port' (u16):
         * - The endpoint port information as a u16 encoded integer.
         *
         * * 'ip_type' (u8):
         * - The endpoint ip version as a u8, 4 or 6.
         *
         * * 'protocol' (u8):
         * - UDP:1 or TCP:0
         *
         * * 'placeholder1' (u8):
         * - Placeholder for further extra params.
         *
         * * 'placeholder2' (u8):
         * - Placeholder for further extra params.
         *
         * # Event:
         * * AxonServed;
         * - On successfully serving the axon info.
         *
         * # Raises:
         * * 'MechanismDoesNotExist':
         * - Attempting to set weights on a non-existent network.
         *
         * * 'NotRegistered':
         * - Attempting to set weights from a non registered account.
         *
         * * 'InvalidIpType':
         * - The ip type is not 4 or 6.
         *
         * * 'InvalidIpAddress':
         * - The numerically encoded ip address does not resolve to a proper ip.
         *
         * * 'ServingRateLimitExceeded':
         * - Attempting to set prometheus information withing the rate limit min.
         *
         */
        serve_axon: TxDescriptor<Anonymize<Ica88a899k1afk>>;
        /**
         * Same as `serve_axon` but takes a certificate as an extra optional argument.
         * Serves or updates axon /prometheus information for the neuron associated with the caller. If the caller is
         * already registered the metadata is updated. If the caller is not registered this call throws NotRegistered.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller.
         *
         * * 'netuid' (u16):
         * - The u16 network identifier.
         *
         * * 'version' (u64):
         * - The bittensor version identifier.
         *
         * * 'ip' (u64):
         * - The endpoint ip information as a u128 encoded integer.
         *
         * * 'port' (u16):
         * - The endpoint port information as a u16 encoded integer.
         *
         * * 'ip_type' (u8):
         * - The endpoint ip version as a u8, 4 or 6.
         *
         * * 'protocol' (u8):
         * - UDP:1 or TCP:0
         *
         * * 'placeholder1' (u8):
         * - Placeholder for further extra params.
         *
         * * 'placeholder2' (u8):
         * - Placeholder for further extra params.
         *
         * * 'certificate' (Vec<u8>):
         * - TLS certificate for inter neuron communitation.
         *
         * # Event:
         * * AxonServed;
         * - On successfully serving the axon info.
         *
         * # Raises:
         * * 'MechanismDoesNotExist':
         * - Attempting to set weights on a non-existent network.
         *
         * * 'NotRegistered':
         * - Attempting to set weights from a non registered account.
         *
         * * 'InvalidIpType':
         * - The ip type is not 4 or 6.
         *
         * * 'InvalidIpAddress':
         * - The numerically encoded ip address does not resolve to a proper ip.
         *
         * * 'ServingRateLimitExceeded':
         * - Attempting to set prometheus information withing the rate limit min.
         *
         */
        serve_axon_tls: TxDescriptor<Anonymize<I4tfn6eb3ekqt2>>;
        /**
         * ---- Set prometheus information for the neuron.
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the calling hotkey.
         *
         * * 'netuid' (u16):
         * - The u16 network identifier.
         *
         * * 'version' (u16):
         * -  The bittensor version identifier.
         *
         * * 'ip' (u128):
         * - The prometheus ip information as a u128 encoded integer.
         *
         * * 'port' (u16):
         * - The prometheus port information as a u16 encoded integer.
         *
         * * 'ip_type' (u8):
         * - The ip type v4 or v6.
         *
         */
        serve_prometheus: TxDescriptor<Anonymize<Ia5r6mm7trbg6a>>;
        /**
         * ---- Registers a new neuron to the subnetwork.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the calling hotkey.
         *
         * * 'netuid' (u16):
         * - The u16 network identifier.
         *
         * * 'block_number' ( u64 ):
         * - Block hash used to prove work done.
         *
         * * 'nonce' ( u64 ):
         * - Positive integer nonce used in POW.
         *
         * * 'work' ( Vec<u8> ):
         * - Vector encoded bytes representing work done.
         *
         * * 'hotkey' ( T::AccountId ):
         * - Hotkey to be registered to the network.
         *
         * * 'coldkey' ( T::AccountId ):
         * - Associated coldkey account.
         *
         * # Event:
         * * NeuronRegistered;
         * - On successfully registering a uid to a neuron slot on a subnetwork.
         *
         * # Raises:
         * * 'MechanismDoesNotExist':
         * - Attempting to register to a non existent network.
         *
         * * 'TooManyRegistrationsThisBlock':
         * - This registration exceeds the total allowed on this network this block.
         *
         * * 'HotKeyAlreadyRegisteredInSubNet':
         * - The hotkey is already registered on this network.
         *
         * * 'InvalidWorkBlock':
         * - The work has been performed on a stale, future, or non existent block.
         *
         * * 'InvalidDifficulty':
         * - The work does not match the difficulty.
         *
         * * 'InvalidSeal':
         * - The seal is incorrect.
         *
         */
        register: TxDescriptor<Anonymize<I27gr0ss2ikvqh>>;
        /**
         * Register the hotkey to root network
         */
        root_register: TxDescriptor<Anonymize<Ie7hipi75c7vn0>>;
        /**
         * User register a new subnetwork via burning token
         */
        burned_register: TxDescriptor<Anonymize<I7f38r2vt6r9k1>>;
        /**
         * The extrinsic for user to change its hotkey in subnet or all subnets.
         */
        swap_hotkey: TxDescriptor<Anonymize<I6b53cjq4m9nsr>>;
        /**
         * Performs an arbitrary coldkey swap for any coldkey.
         *
         * Only callable by root as it doesn't require an announcement and can be used to swap any coldkey.
         */
        swap_coldkey: TxDescriptor<Anonymize<I216fvnrl9nq6l>>;
        /**
         * Sets the childkey take for a given hotkey.
         *
         * This function allows a coldkey to set the childkey take for a given hotkey.
         * The childkey take determines the proportion of stake that the hotkey keeps for itself
         * when distributing stake to its children.
         *
         * # Arguments:
         * * `origin` (<T as frame_system::Config>::RuntimeOrigin):
         * - The signature of the calling coldkey. Setting childkey take can only be done by the coldkey.
         *
         * * `hotkey` (T::AccountId):
         * - The hotkey for which the childkey take will be set.
         *
         * * `take` (u16):
         * - The new childkey take value. This is a percentage represented as a value between 0 and 10000,
         * where 10000 represents 100%.
         *
         * # Events:
         * * `ChildkeyTakeSet`:
         * - On successfully setting the childkey take for a hotkey.
         *
         * # Errors:
         * * `NonAssociatedColdKey`:
         * - The coldkey does not own the hotkey.
         * * `InvalidChildkeyTake`:
         * - The provided take value is invalid (greater than the maximum allowed take).
         * * `TxChildkeyTakeRateLimitExceeded`:
         * - The rate limit for changing childkey take has been exceeded.
         *
         */
        set_childkey_take: TxDescriptor<Anonymize<I9n4d52k0luroe>>;
        /**
         * Sets the transaction rate limit for changing childkey take.
         *
         * This function can only be called by the root origin.
         *
         * # Arguments:
         * * `origin` - The origin of the call, must be root.
         * * `tx_rate_limit` - The new rate limit in blocks.
         *
         * # Errors:
         * * `BadOrigin` - If the origin is not root.
         *
         */
        sudo_set_tx_childkey_take_rate_limit: TxDescriptor<Anonymize<I3gk6eeddm0hsd>>;
        /**
         * Sets the minimum allowed childkey take.
         *
         * This function can only be called by the root origin.
         *
         * # Arguments:
         * * `origin` - The origin of the call, must be root.
         * * `take` - The new minimum childkey take value.
         *
         * # Errors:
         * * `BadOrigin` - If the origin is not root.
         *
         */
        sudo_set_min_childkey_take: TxDescriptor<Anonymize<I6ue7qc27uhiev>>;
        /**
         * Sets the maximum allowed childkey take.
         *
         * This function can only be called by the root origin.
         *
         * # Arguments:
         * * `origin` - The origin of the call, must be root.
         * * `take` - The new maximum childkey take value.
         *
         * # Errors:
         * * `BadOrigin` - If the origin is not root.
         *
         */
        sudo_set_max_childkey_take: TxDescriptor<Anonymize<I6ue7qc27uhiev>>;
        /**
         * User register a new subnetwork
         */
        register_network: TxDescriptor<Anonymize<Ie7hipi75c7vn0>>;
        /**
         * Facility extrinsic for user to get taken from faucet
         * It is only available when pow-faucet feature enabled
         * Just deployed in testnet and devnet for testing purpose
         */
        faucet: TxDescriptor<Anonymize<Ifp8lgrkla2dig>>;
        /**
         * Remove a user's subnetwork
         * The caller must be the owner of the network
         */
        dissolve_network: TxDescriptor<Anonymize<I30l38oi9ed9dj>>;
        /**
         * Set a single child for a given hotkey on a specified network.
         *
         * This function allows a coldkey to set a single child for a given hotkey on a specified network.
         * The proportion of the hotkey's stake to be allocated to the child is also specified.
         *
         * # Arguments:
         * * `origin` (<T as frame_system::Config>::RuntimeOrigin):
         * - The signature of the calling coldkey. Setting a hotkey child can only be done by the coldkey.
         *
         * * `hotkey` (T::AccountId):
         * - The hotkey which will be assigned the child.
         *
         * * `child` (T::AccountId):
         * - The child which will be assigned to the hotkey.
         *
         * * `netuid` (u16):
         * - The u16 network identifier where the childkey will exist.
         *
         * * `proportion` (u64):
         * - Proportion of the hotkey's stake to be given to the child, the value must be u64 normalized.
         *
         * # Events:
         * * `ChildAddedSingular`:
         * - On successfully registering a child to a hotkey.
         *
         * # Errors:
         * * `MechanismDoesNotExist`:
         * - Attempting to register to a non-existent network.
         * * `RegistrationNotPermittedOnRootSubnet`:
         * - Attempting to register a child on the root network.
         * * `NonAssociatedColdKey`:
         * - The coldkey does not own the hotkey or the child is the same as the hotkey.
         * * `HotKeyAccountNotExists`:
         * - The hotkey account does not exist.
         *
         * # Detailed Explanation of Checks:
         * 1. **Signature Verification**: Ensures that the caller has signed the transaction, verifying the coldkey.
         * 2. **Root Network Check**: Ensures that the delegation is not on the root network, as child hotkeys are not valid on the root.
         * 3. **Network Existence Check**: Ensures that the specified network exists.
         * 4. **Ownership Verification**: Ensures that the coldkey owns the hotkey.
         * 5. **Hotkey Account Existence Check**: Ensures that the hotkey account already exists.
         * 6. **Child-Hotkey Distinction**: Ensures that the child is not the same as the hotkey.
         * 7. **Old Children Cleanup**: Removes the hotkey from the parent list of its old children.
         * 8. **New Children Assignment**: Assigns the new child to the hotkey and updates the parent list for the new child.
         */
        set_children: TxDescriptor<Anonymize<Ifj9gf4ekq9snm>>;
        /**
         * Schedules a coldkey swap operation to be executed at a future block.
         *
         * WARNING: This function is deprecated, please migrate to `announce_coldkey_swap`/`coldkey_swap`
         */
        schedule_swap_coldkey: TxDescriptor<Anonymize<If2k69ql8jgivj>>;
        /**
         * ---- Set prometheus information for the neuron.
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the calling hotkey.
         *
         * * 'netuid' (u16):
         * - The u16 network identifier.
         *
         * * 'version' (u16):
         * -  The bittensor version identifier.
         *
         * * 'ip' (u128):
         * - The prometheus ip information as a u128 encoded integer.
         *
         * * 'port' (u16):
         * - The prometheus port information as a u16 encoded integer.
         *
         * * 'ip_type' (u8):
         * - The ip type v4 or v6.
         *
         */
        set_identity: TxDescriptor<Anonymize<Ifjlj958aeheic>>;
        /**
         * ---- Set the identity information for a subnet.
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the calling coldkey, which must be the owner of the subnet.
         *
         * * `netuid` (u16):
         * - The unique network identifier of the subnet.
         *
         * * `subnet_name` (Vec<u8>):
         * - The name of the subnet.
         *
         * * `github_repo` (Vec<u8>):
         * - The GitHub repository associated with the subnet identity.
         *
         * * `subnet_contact` (Vec<u8>):
         * - The contact information for the subnet.
         */
        set_subnet_identity: TxDescriptor<Anonymize<I4378ieh1uba9u>>;
        /**
         * User register a new subnetwork
         */
        register_network_with_identity: TxDescriptor<Anonymize<I8e6f7r9dtk9c1>>;
        /**
         * ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
         *
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * `hotkey` (T::AccountId):
         * - The associated hotkey account.
         *
         * # Event:
         * * StakeRemoved;
         * - On the successfully removing stake from the hotkey account.
         *
         * # Raises:
         * * `NotRegistered`:
         * - Thrown if the account we are attempting to unstake from is non existent.
         *
         * * `NonAssociatedColdKey`:
         * - Thrown if the coldkey does not own the hotkey we are unstaking from.
         *
         * * `NotEnoughStakeToWithdraw`:
         * - Thrown if there is not enough stake on the hotkey to withdraw this amount.
         *
         * * `TxRateLimitExceeded`:
         * - Thrown if key has hit transaction rate limit
         */
        unstake_all: TxDescriptor<Anonymize<Ie7hipi75c7vn0>>;
        /**
         * ---- The implementation for the extrinsic unstake_all: Removes all stake from a hotkey account across all subnets and adds it onto a coldkey.
         *
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * `hotkey` (T::AccountId):
         * - The associated hotkey account.
         *
         * # Event:
         * * StakeRemoved;
         * - On the successfully removing stake from the hotkey account.
         *
         * # Raises:
         * * `NotRegistered`:
         * - Thrown if the account we are attempting to unstake from is non existent.
         *
         * * `NonAssociatedColdKey`:
         * - Thrown if the coldkey does not own the hotkey we are unstaking from.
         *
         * * `NotEnoughStakeToWithdraw`:
         * - Thrown if there is not enough stake on the hotkey to withdraw this amount.
         *
         * * `TxRateLimitExceeded`:
         * - Thrown if key has hit transaction rate limit
         */
        unstake_all_alpha: TxDescriptor<Anonymize<Ie7hipi75c7vn0>>;
        /**
         * ---- The implementation for the extrinsic move_stake: Moves specified amount of stake from a hotkey to another across subnets.
         *
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * `origin_hotkey` (T::AccountId):
         * - The hotkey account to move stake from.
         *
         * * `destination_hotkey` (T::AccountId):
         * - The hotkey account to move stake to.
         *
         * * `origin_netuid` (T::AccountId):
         * - The subnet ID to move stake from.
         *
         * * `destination_netuid` (T::AccountId):
         * - The subnet ID to move stake to.
         *
         * * `alpha_amount` (T::AccountId):
         * - The alpha stake amount to move.
         *
         */
        move_stake: TxDescriptor<Anonymize<I9d117ni3tprb>>;
        /**
         * Transfers a specified amount of stake from one coldkey to another, optionally across subnets,
         * while keeping the same hotkey.
         *
         * # Arguments
         * * `origin` - The origin of the transaction, which must be signed by the `origin_coldkey`.
         * * `destination_coldkey` - The coldkey to which the stake is transferred.
         * * `hotkey` - The hotkey associated with the stake.
         * * `origin_netuid` - The network/subnet ID to move stake from.
         * * `destination_netuid` - The network/subnet ID to move stake to (for cross-subnet transfer).
         * * `alpha_amount` - The amount of stake to transfer.
         *
         * # Errors
         * Returns an error if:
         * * The origin is not signed by the correct coldkey.
         * * Either subnet does not exist.
         * * The hotkey does not exist.
         * * There is insufficient stake on `(origin_coldkey, hotkey, origin_netuid)`.
         * * The transfer amount is below the minimum stake requirement.
         *
         * # Events
         * May emit a `StakeTransferred` event on success.
         */
        transfer_stake: TxDescriptor<Anonymize<I340k0hbj1hc6r>>;
        /**
         * Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.
         *
         * # Arguments
         * * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
         * * `hotkey` - The hotkey whose stake is being swapped.
         * * `origin_netuid` - The network/subnet ID from which stake is removed.
         * * `destination_netuid` - The network/subnet ID to which stake is added.
         * * `alpha_amount` - The amount of stake to swap.
         *
         * # Errors
         * Returns an error if:
         * * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails).
         * * Either `origin_netuid` or `destination_netuid` does not exist.
         * * The hotkey does not exist.
         * * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`.
         * * The swap amount is below the minimum stake requirement.
         *
         * # Events
         * May emit a `StakeSwapped` event on success.
         */
        swap_stake: TxDescriptor<Anonymize<Ibapoov2fa817a>>;
        /**
         * --- Adds stake to a hotkey on a subnet with a price limit.
         * This extrinsic allows to specify the limit price for alpha token
         * at which or better (lower) the staking should execute.
         *
         * In case if slippage occurs and the price shall move beyond the limit
         * price, the staking order may execute only partially or not execute
         * at all.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller's coldkey.
         *
         * * 'hotkey' (T::AccountId):
         * - The associated hotkey account.
         *
         * * 'netuid' (u16):
         * - Subnetwork UID
         *
         * * 'amount_staked' (u64):
         * - The amount of stake to be added to the hotkey staking account.
         *
         * * 'limit_price' (u64):
         * - The limit price expressed in units of RAO per one Alpha.
         *
         * * 'allow_partial' (bool):
         * - Allows partial execution of the amount. If set to false, this becomes
         * fill or kill type or order.
         *
         * # Event:
         * * StakeAdded;
         * - On the successfully adding stake to a global account.
         *
         * # Raises:
         * * 'NotEnoughBalanceToStake':
         * - Not enough balance on the coldkey to add onto the global account.
         *
         * * 'NonAssociatedColdKey':
         * - The calling coldkey is not associated with this hotkey.
         *
         * * 'BalanceWithdrawalError':
         * - Errors stemming from transaction pallet.
         *
         */
        add_stake_limit: TxDescriptor<Anonymize<I2eon60c4gde7f>>;
        /**
         * --- Removes stake from a hotkey on a subnet with a price limit.
         * This extrinsic allows to specify the limit price for alpha token
         * at which or better (higher) the staking should execute.
         *
         * In case if slippage occurs and the price shall move beyond the limit
         * price, the staking order may execute only partially or not execute
         * at all.
         *
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller's coldkey.
         *
         * * 'hotkey' (T::AccountId):
         * - The associated hotkey account.
         *
         * * 'netuid' (u16):
         * - Subnetwork UID
         *
         * * 'amount_unstaked' (u64):
         * - The amount of stake to be added to the hotkey staking account.
         *
         * * 'limit_price' (u64):
         * - The limit price expressed in units of RAO per one Alpha.
         *
         * * 'allow_partial' (bool):
         * - Allows partial execution of the amount. If set to false, this becomes
         * fill or kill type or order.
         *
         * # Event:
         * * StakeRemoved;
         * - On the successfully removing stake from the hotkey account.
         *
         * # Raises:
         * * 'NotRegistered':
         * - Thrown if the account we are attempting to unstake from is non existent.
         *
         * * 'NonAssociatedColdKey':
         * - Thrown if the coldkey does not own the hotkey we are unstaking from.
         *
         * * 'NotEnoughStakeToWithdraw':
         * - Thrown if there is not enough stake on the hotkey to withdwraw this amount.
         *
         */
        remove_stake_limit: TxDescriptor<Anonymize<I7egr0053sjpci>>;
        /**
         * Swaps a specified amount of stake from one subnet to another, while keeping the same coldkey and hotkey.
         *
         * # Arguments
         * * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
         * * `hotkey` - The hotkey whose stake is being swapped.
         * * `origin_netuid` - The network/subnet ID from which stake is removed.
         * * `destination_netuid` - The network/subnet ID to which stake is added.
         * * `alpha_amount` - The amount of stake to swap.
         * * `limit_price` - The limit price expressed in units of RAO per one Alpha.
         * * `allow_partial` - Allows partial execution of the amount. If set to false, this becomes fill or kill type or order.
         *
         * # Errors
         * Returns an error if:
         * * The transaction is not signed by the correct coldkey (i.e., `coldkey_owns_hotkey` fails).
         * * Either `origin_netuid` or `destination_netuid` does not exist.
         * * The hotkey does not exist.
         * * There is insufficient stake on `(coldkey, hotkey, origin_netuid)`.
         * * The swap amount is below the minimum stake requirement.
         *
         * # Events
         * May emit a `StakeSwapped` event on success.
         */
        swap_stake_limit: TxDescriptor<Anonymize<I6r22p9usi2mkl>>;
        /**
         * Attempts to associate a hotkey with a coldkey.
         *
         * # Arguments
         * * `origin` - The origin of the transaction, which must be signed by the coldkey that owns the `hotkey`.
         * * `hotkey` - The hotkey to associate with the coldkey.
         *
         * # Note
         * Will charge based on the weight even if the hotkey is already associated with a coldkey.
         */
        try_associate_hotkey: TxDescriptor<Anonymize<Ie7hipi75c7vn0>>;
        /**
         * Initiates a call on a subnet.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be signed by the subnet owner.
         * * `netuid` - The unique identifier of the subnet on which the call is being initiated.
         *
         * # Events
         * Emits a `FirstEmissionBlockNumberSet` event on success.
         */
        start_call: TxDescriptor<Anonymize<I6cm4c5a1euio9>>;
        /**
         * Attempts to associate a hotkey with an EVM key.
         *
         * The signature will be checked to see if the recovered public key matches the `evm_key` provided.
         *
         * The EVM key is expected to sign the message according to this formula to produce the signature:
         * ```text
         * keccak_256(hotkey ++ keccak_256(block_number))
         * ```
         *
         * # Arguments
         * * `origin` - The origin of the transaction, which must be signed by the `hotkey`.
         * * `netuid` - The netuid that the `hotkey` belongs to.
         * * `evm_key` - The EVM key to associate with the `hotkey`.
         * * `block_number` - The block number used in the `signature`.
         * * `signature` - A signed message by the `evm_key` containing the `hotkey` and the hashed `block_number`.
         *
         * # Errors
         * Returns an error if:
         * * The transaction is not signed.
         * * The hotkey does not belong to the subnet identified by the netuid.
         * * The EVM key cannot be recovered from the signature.
         * * The EVM key recovered from the signature does not match the given EVM key.
         *
         * # Events
         * May emit a `EvmKeyAssociated` event on success
         */
        associate_evm_key: TxDescriptor<Anonymize<I96k3nrdjfd63k>>;
        /**
         * Recycles alpha from a cold/hot key pair, reducing AlphaOut on a subnet
         *
         * # Arguments
         * * `origin` - The origin of the call (must be signed by the coldkey)
         * * `hotkey` - The hotkey account
         * * `amount` - The amount of alpha to recycle
         * * `netuid` - The subnet ID
         *
         * # Events
         * Emits a `TokensRecycled` event on success.
         */
        recycle_alpha: TxDescriptor<Anonymize<Ibg3cp8vjl5u55>>;
        /**
         * Burns alpha from a cold/hot key pair without reducing `AlphaOut`
         *
         * # Arguments
         * * `origin` - The origin of the call (must be signed by the coldkey)
         * * `hotkey` - The hotkey account
         * * `amount` - The amount of alpha to burn
         * * `netuid` - The subnet ID
         *
         * # Events
         * Emits a `TokensBurned` event on success.
         */
        burn_alpha: TxDescriptor<Anonymize<Ibg3cp8vjl5u55>>;
        /**
         * Sets the pending childkey cooldown (in blocks). Root only.
         */
        set_pending_childkey_cooldown: TxDescriptor<Anonymize<Ibtu1gfmdnou5k>>;
        /**
         * Removes all stake from a hotkey on a subnet with a price limit.
         * This extrinsic allows to specify the limit price for alpha token
         * at which or better (higher) the staking should execute.
         * Without limit_price it remove all the stake similar to `remove_stake` extrinsic
         */
        remove_stake_full_limit: TxDescriptor<Anonymize<Iaoomvri5btde>>;
        /**
         * Register a new leased network.
         *
         * The crowdloan's contributions are used to compute the share of the emissions that the contributors
         * will receive as dividends.
         *
         * The leftover cap is refunded to the contributors and the beneficiary.
         *
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * `emissions_share` (Percent):
         * - The share of the emissions that the contributors will receive as dividends.
         *
         * * `end_block` (Option<BlockNumberFor<T>>):
         * - The block at which the lease will end. If not defined, the lease is perpetual.
         */
        register_leased_network: TxDescriptor<Anonymize<Ic80igo4eds6rq>>;
        /**
         * Terminate a lease.
         *
         * The beneficiary can terminate the lease after the end block has passed and get the subnet ownership.
         * The subnet is transferred to the beneficiary and the lease is removed from storage.
         *
         * **The hotkey must be owned by the beneficiary coldkey.**
         *
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * `lease_id` (LeaseId):
         * - The ID of the lease to terminate.
         *
         * * `hotkey` (T::AccountId):
         * - The hotkey of the beneficiary to mark as subnet owner hotkey.
         */
        terminate_lease: TxDescriptor<Anonymize<Iflrm8un6aibtn>>;
        /**
         * Updates the symbol for a subnet.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the subnet owner or root.
         * * `netuid` - The unique identifier of the subnet on which the symbol is being set.
         * * `symbol` - The symbol to set for the subnet.
         *
         * # Errors
         * Returns an error if:
         * * The transaction is not signed by the subnet owner.
         * * The symbol does not exist.
         * * The symbol is already in use by another subnet.
         *
         * # Events
         * Emits a `SymbolUpdated` event on success.
         */
        update_symbol: TxDescriptor<Anonymize<I62rrikn5vj0p5>>;
        /**
         * ---- Used to commit timelock encrypted commit-reveal weight values to later be revealed.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The committing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `commit` (`Vec<u8>`):
         * - The encrypted compressed commit.
         * The steps for this are:
         * 1. Instantiate [`WeightsTlockPayload`]
         * 2. Serialize it using the `parity_scale_codec::Encode` trait
         * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
         * to produce a [`TLECiphertext<TinyBLS381>`] type.
         * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
         *
         * * reveal_round (`u64`):
         * - The drand reveal round which will be avaliable during epoch `n+1` from the current
         * epoch.
         *
         * * commit_reveal_version (`u16`):
         * - The client (bittensor-drand) version
         */
        commit_timelocked_weights: TxDescriptor<Anonymize<Ietm4rjshhu7sf>>;
        /**
         * Set the autostake destination hotkey for a coldkey.
         *
         * The caller selects a hotkey where all future rewards
         * will be automatically staked.
         *
         * # Args:
         * * `origin` - (<T as frame_system::Config>::Origin):
         * - The signature of the caller's coldkey.
         *
         * * `hotkey` (T::AccountId):
         * - The hotkey account to designate as the autostake destination.
         */
        set_coldkey_auto_stake_hotkey: TxDescriptor<Anonymize<I7f38r2vt6r9k1>>;
        /**
         * ---- Used to commit timelock encrypted commit-reveal weight values to later be revealed for
         * a mechanism.
         *
         * # Args:
         * * `origin`: (`<T as frame_system::Config>::RuntimeOrigin`):
         * - The committing hotkey.
         *
         * * `netuid` (`u16`):
         * - The u16 network identifier.
         *
         * * `mecid` (`u8`):
         * - The u8 mechanism identifier.
         *
         * * `commit` (`Vec<u8>`):
         * - The encrypted compressed commit.
         * The steps for this are:
         * 1. Instantiate [`WeightsTlockPayload`]
         * 2. Serialize it using the `parity_scale_codec::Encode` trait
         * 3. Encrypt it following the steps (here)[https://github.com/ideal-lab5/tle/blob/f8e6019f0fb02c380ebfa6b30efb61786dede07b/timelock/src/tlock.rs#L283-L336]
         * to produce a [`TLECiphertext<TinyBLS381>`] type.
         * 4. Serialize and compress using the `ark-serialize` `CanonicalSerialize` trait.
         *
         * * reveal_round (`u64`):
         * - The drand reveal round which will be avaliable during epoch `n+1` from the current
         * epoch.
         *
         * * commit_reveal_version (`u16`):
         * - The client (bittensor-drand) version
         */
        commit_timelocked_mechanism_weights: TxDescriptor<Anonymize<I1v9m3ms1elitm>>;
        /**
         * Remove a subnetwork
         * The caller must be root
         */
        root_dissolve_network: TxDescriptor<Anonymize<I6cm4c5a1euio9>>;
        /**
         * --- Claims the root emissions for a coldkey.
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller's coldkey.
         *
         * # Event:
         * * RootClaimed;
         * - On the successfully claiming the root emissions for a coldkey.
         *
         * # Raises:
         *
         */
        claim_root: TxDescriptor<Anonymize<I2t4b7068rtebl>>;
        /**
         * --- Sets the root claim type for the coldkey.
         * # Args:
         * * 'origin': (<T as frame_system::Config>Origin):
         * - The signature of the caller's coldkey.
         *
         * # Event:
         * * RootClaimTypeSet;
         * - On the successfully setting the root claim type for the coldkey.
         *
         */
        set_root_claim_type: TxDescriptor<Anonymize<I7a99hd3nbic2l>>;
        /**
         * --- Sets root claim number (sudo extrinsic). Zero disables auto-claim.
         */
        sudo_set_num_root_claims: TxDescriptor<Anonymize<Ie8hpsm3jhsvo3>>;
        /**
         * --- Sets root claim threshold for subnet (sudo or owner origin).
         */
        sudo_set_root_claim_threshold: TxDescriptor<Anonymize<Ifcj247vgfdg56>>;
        /**
         * Announces a coldkey swap using BlakeTwo256 hash of the new coldkey.
         *
         * This is required before the coldkey swap can be performed
         * after the delay period.
         *
         * It can be reannounced after a delay of `ColdkeySwapReannouncementDelay` following
         * the first valid execution block of the original announcement.
         *
         * The dispatch origin of this call must be the original coldkey that made the announcement.
         *
         * - `new_coldkey_hash`: The hash of the new coldkey using BlakeTwo256.
         *
         * The `ColdkeySwapAnnounced` event is emitted on successful announcement.
         *
         */
        announce_coldkey_swap: TxDescriptor<Anonymize<Ic21uicfit5vcu>>;
        /**
         * Performs a coldkey swap if an announcement has been made.
         *
         * The dispatch origin of this call must be the original coldkey that made the announcement.
         *
         * - `new_coldkey`: The new coldkey to swap to. The BlakeTwo256 hash of the new coldkey must be
         * the same as the announced coldkey hash.
         *
         * The `ColdkeySwapped` event is emitted on successful swap.
         */
        swap_coldkey_announced: TxDescriptor<Anonymize<If2k69ql8jgivj>>;
        /**
         * Dispute a coldkey swap.
         *
         * This will prevent any further actions on the coldkey swap
         * until triumvirate step in to resolve the issue.
         *
         * - `coldkey`: The coldkey to dispute the swap for.
         *
         */
        dispute_coldkey_swap: TxDescriptor<undefined>;
        /**
         * Reset a coldkey swap by clearing the announcement and dispute status.
         *
         * The dispatch origin of this call must be root.
         *
         * - `coldkey`: The coldkey to reset the swap for.
         *
         */
        reset_coldkey_swap: TxDescriptor<Anonymize<I375tmdui1ejfc>>;
        /**
         * Enables voting power tracking for a subnet.
         *
         * This function can be called by the subnet owner or root.
         * When enabled, voting power EMA is updated every epoch for all validators.
         * Voting power starts at 0 and increases over epochs.
         *
         * # Arguments:
         * * `origin` - The origin of the call, must be subnet owner or root.
         * * `netuid` - The subnet to enable voting power tracking for.
         *
         * # Errors:
         * * `SubnetNotExist` - If the subnet does not exist.
         * * `NotSubnetOwner` - If the caller is not the subnet owner or root.
         */
        enable_voting_power_tracking: TxDescriptor<Anonymize<I6cm4c5a1euio9>>;
        /**
         * Schedules disabling of voting power tracking for a subnet.
         *
         * This function can be called by the subnet owner or root.
         * Voting power tracking will continue for 14 days (grace period) after this call,
         * then automatically disable and clear all VotingPower entries for the subnet.
         *
         * # Arguments:
         * * `origin` - The origin of the call, must be subnet owner or root.
         * * `netuid` - The subnet to schedule disabling voting power tracking for.
         *
         * # Errors:
         * * `SubnetNotExist` - If the subnet does not exist.
         * * `NotSubnetOwner` - If the caller is not the subnet owner or root.
         * * `VotingPowerTrackingNotEnabled` - If voting power tracking is not enabled.
         */
        disable_voting_power_tracking: TxDescriptor<Anonymize<I6cm4c5a1euio9>>;
        /**
         * Sets the EMA alpha value for voting power calculation on a subnet.
         *
         * This function can only be called by root (sudo).
         * Higher alpha = faster response to stake changes.
         * Alpha is stored as u64 with 18 decimal precision (1.0 = 10^18).
         *
         * # Arguments:
         * * `origin` - The origin of the call, must be root.
         * * `netuid` - The subnet to set the alpha for.
         * * `alpha` - The new alpha value (u64 with 18 decimal precision).
         *
         * # Errors:
         * * `BadOrigin` - If the origin is not root.
         * * `SubnetNotExist` - If the subnet does not exist.
         * * `InvalidVotingPowerEmaAlpha` - If alpha is greater than 10^18 (1.0).
         */
        sudo_set_voting_power_ema_alpha: TxDescriptor<Anonymize<I4guv8rii4s6je>>;
        /**
         * --- The extrinsic is a combination of add_stake(add_stake_limit) and burn_alpha. We buy
         * alpha token first and immediately burn the acquired amount of alpha (aka Subnet buyback).
         */
        add_stake_burn: TxDescriptor<Anonymize<I2t2h3sjr2mdj0>>;
    };
    Utility: {
        /**
         * Send a batch of dispatch calls.
         *
         * May be called from any origin except `None`.
         *
         * - `calls`: The calls to be dispatched from the same origin. The number of call must not
         * exceed the constant: `batched_calls_limit` (available in constant metadata).
         *
         * If origin is root then the calls are dispatched without checking origin filter. (This
         * includes bypassing `frame_system::Config::BaseCallFilter`).
         *
         * ## Complexity
         * - O(C) where C is the number of calls to be batched.
         *
         * This will return `Ok` in all circumstances. To determine the success of the batch, an
         * event is deposited. If a call failed and the batch was interrupted, then the
         * `BatchInterrupted` event is deposited, along with the number of successful calls made
         * and the error of the failed call. If all were successful, then the `BatchCompleted`
         * event is deposited.
         */
        batch: TxDescriptor<Anonymize<I80tnmsfsu19sl>>;
        /**
         * Send a call through an indexed pseudonym of the sender.
         *
         * Filter from origin are passed along. The call will be dispatched with an origin which
         * use the same filter as the origin of this call.
         *
         * NOTE: If you need to ensure that any account-based filtering is not honored (i.e.
         * because you expect `proxy` to have been used prior in the call stack and you do not want
         * the call restrictions to apply to any sub-accounts), then use `as_multi_threshold_1`
         * in the Multisig pallet instead.
         *
         * NOTE: Prior to version *12, this was called `as_limited_sub`.
         *
         * The dispatch origin for this call must be _Signed_.
         */
        as_derivative: TxDescriptor<Anonymize<Ib7nn1mns0usdp>>;
        /**
         * Send a batch of dispatch calls and atomically execute them.
         * The whole transaction will rollback and fail if any of the calls failed.
         *
         * May be called from any origin except `None`.
         *
         * - `calls`: The calls to be dispatched from the same origin. The number of call must not
         * exceed the constant: `batched_calls_limit` (available in constant metadata).
         *
         * If origin is root then the calls are dispatched without checking origin filter. (This
         * includes bypassing `frame_system::Config::BaseCallFilter`).
         *
         * ## Complexity
         * - O(C) where C is the number of calls to be batched.
         */
        batch_all: TxDescriptor<Anonymize<I80tnmsfsu19sl>>;
        /**
         * Dispatches a function call with a provided origin.
         *
         * The dispatch origin for this call must be _Root_.
         *
         * ## Complexity
         * - O(1).
         */
        dispatch_as: TxDescriptor<Anonymize<I4fivl1mrn0hhc>>;
        /**
         * Send a batch of dispatch calls.
         * Unlike `batch`, it allows errors and won't interrupt.
         *
         * May be called from any origin except `None`.
         *
         * - `calls`: The calls to be dispatched from the same origin. The number of call must not
         * exceed the constant: `batched_calls_limit` (available in constant metadata).
         *
         * If origin is root then the calls are dispatch without checking origin filter. (This
         * includes bypassing `frame_system::Config::BaseCallFilter`).
         *
         * ## Complexity
         * - O(C) where C is the number of calls to be batched.
         */
        force_batch: TxDescriptor<Anonymize<I80tnmsfsu19sl>>;
        /**
         * Dispatch a function call with a specified weight.
         *
         * This function does not check the weight of the call, and instead allows the
         * Root origin to specify the weight of the call.
         *
         * The dispatch origin for this call must be _Root_.
         */
        with_weight: TxDescriptor<Anonymize<I2ead8rm0h16hm>>;
        /**
         * Dispatch a fallback call in the event the main call fails to execute.
         * May be called from any origin except `None`.
         *
         * This function first attempts to dispatch the `main` call.
         * If the `main` call fails, the `fallback` is attemted.
         * if the fallback is successfully dispatched, the weights of both calls
         * are accumulated and an event containing the main call error is deposited.
         *
         * In the event of a fallback failure the whole call fails
         * with the weights returned.
         *
         * - `main`: The main call to be dispatched. This is the primary action to execute.
         * - `fallback`: The fallback call to be dispatched in case the `main` call fails.
         *
         * ## Dispatch Logic
         * - If the origin is `root`, both the main and fallback calls are executed without
         * applying any origin filters.
         * - If the origin is not `root`, the origin filter is applied to both the `main` and
         * `fallback` calls.
         *
         * ## Use Case
         * - Some use cases might involve submitting a `batch` type call in either main, fallback
         * or both.
         */
        if_else: TxDescriptor<Anonymize<I25l72483lbgf9>>;
        /**
         * Dispatches a function call with a provided origin.
         *
         * Almost the same as [`Pallet::dispatch_as`] but forwards any error of the inner call.
         *
         * The dispatch origin for this call must be _Root_.
         */
        dispatch_as_fallible: TxDescriptor<Anonymize<I4fivl1mrn0hhc>>;
    };
    Sudo: {
        /**
         * Authenticates the sudo key and dispatches a function call with `Root` origin.
         */
        sudo: TxDescriptor<Anonymize<I9okvr56cd7277>>;
        /**
         * Authenticates the sudo key and dispatches a function call with `Root` origin.
         * This function does not check the weight of the call, and instead allows the
         * Sudo user to specify the weight of the call.
         *
         * The dispatch origin for this call must be _Signed_.
         */
        sudo_unchecked_weight: TxDescriptor<Anonymize<I2ead8rm0h16hm>>;
        /**
         * Authenticates the current sudo key and sets the given AccountId (`new`) as the new sudo
         * key.
         */
        set_key: TxDescriptor<Anonymize<I8k3rnvpeeh4hv>>;
        /**
         * Authenticates the sudo key and dispatches a function call with `Signed` origin from
         * a given account.
         *
         * The dispatch origin for this call must be _Signed_.
         */
        sudo_as: TxDescriptor<Anonymize<I56sht7incdimf>>;
        /**
         * Permanently removes the sudo key.
         *
         * **This cannot be un-done.**
         */
        remove_key: TxDescriptor<undefined>;
    };
    Multisig: {
        /**
         * Immediately dispatch a multi-signature call using a single approval from the caller.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * - `other_signatories`: The accounts (other than the sender) who are part of the
         * multi-signature, but do not participate in the approval process.
         * - `call`: The call to be executed.
         *
         * Result is equivalent to the dispatched result.
         *
         * ## Complexity
         * O(Z + C) where Z is the length of the call and C its execution weight.
         */
        as_multi_threshold_1: TxDescriptor<Anonymize<I8hge8nrufr05f>>;
        /**
         * Register approval for a dispatch to be made from a deterministic composite account if
         * approved by a total of `threshold - 1` of `other_signatories`.
         *
         * If there are enough, then dispatch the call.
         *
         * Payment: `DepositBase` will be reserved if this is the first approval, plus
         * `threshold` times `DepositFactor`. It is returned once this dispatch happens or
         * is cancelled.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * - `threshold`: The total number of approvals for this dispatch before it is executed.
         * - `other_signatories`: The accounts (other than the sender) who can approve this
         * dispatch. May not be empty.
         * - `maybe_timepoint`: If this is the first approval, then this must be `None`. If it is
         * not the first approval, then it must be `Some`, with the timepoint (block number and
         * transaction index) of the first approval transaction.
         * - `call`: The call to be executed.
         *
         * NOTE: Unless this is the final approval, you will generally want to use
         * `approve_as_multi` instead, since it only requires a hash of the call.
         *
         * Result is equivalent to the dispatched result if `threshold` is exactly `1`. Otherwise
         * on success, result is `Ok` and the result from the interior call, if it was executed,
         * may be found in the deposited `MultisigExecuted` event.
         *
         * ## Complexity
         * - `O(S + Z + Call)`.
         * - Up to one balance-reserve or unreserve operation.
         * - One passthrough operation, one insert, both `O(S)` where `S` is the number of
         * signatories. `S` is capped by `MaxSignatories`, with weight being proportional.
         * - One call encode & hash, both of complexity `O(Z)` where `Z` is tx-len.
         * - One encode & hash, both of complexity `O(S)`.
         * - Up to one binary search and insert (`O(logS + S)`).
         * - I/O: 1 read `O(S)`, up to 1 mutate `O(S)`. Up to one remove.
         * - One event.
         * - The weight of the `call`.
         * - Storage: inserts one item, value size bounded by `MaxSignatories`, with a deposit
         * taken for its lifetime of `DepositBase + threshold * DepositFactor`.
         */
        as_multi: TxDescriptor<Anonymize<I5v0mk7rggegmh>>;
        /**
         * Register approval for a dispatch to be made from a deterministic composite account if
         * approved by a total of `threshold - 1` of `other_signatories`.
         *
         * Payment: `DepositBase` will be reserved if this is the first approval, plus
         * `threshold` times `DepositFactor`. It is returned once this dispatch happens or
         * is cancelled.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * - `threshold`: The total number of approvals for this dispatch before it is executed.
         * - `other_signatories`: The accounts (other than the sender) who can approve this
         * dispatch. May not be empty.
         * - `maybe_timepoint`: If this is the first approval, then this must be `None`. If it is
         * not the first approval, then it must be `Some`, with the timepoint (block number and
         * transaction index) of the first approval transaction.
         * - `call_hash`: The hash of the call to be executed.
         *
         * NOTE: If this is the final approval, you will want to use `as_multi` instead.
         *
         * ## Complexity
         * - `O(S)`.
         * - Up to one balance-reserve or unreserve operation.
         * - One passthrough operation, one insert, both `O(S)` where `S` is the number of
         * signatories. `S` is capped by `MaxSignatories`, with weight being proportional.
         * - One encode & hash, both of complexity `O(S)`.
         * - Up to one binary search and insert (`O(logS + S)`).
         * - I/O: 1 read `O(S)`, up to 1 mutate `O(S)`. Up to one remove.
         * - One event.
         * - Storage: inserts one item, value size bounded by `MaxSignatories`, with a deposit
         * taken for its lifetime of `DepositBase + threshold * DepositFactor`.
         */
        approve_as_multi: TxDescriptor<Anonymize<Ideaemvoneh309>>;
        /**
         * Cancel a pre-existing, on-going multisig transaction. Any deposit reserved previously
         * for this operation will be unreserved on success.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * - `threshold`: The total number of approvals for this dispatch before it is executed.
         * - `other_signatories`: The accounts (other than the sender) who can approve this
         * dispatch. May not be empty.
         * - `timepoint`: The timepoint (block number and transaction index) of the first approval
         * transaction for this dispatch.
         * - `call_hash`: The hash of the call to be executed.
         *
         * ## Complexity
         * - `O(S)`.
         * - Up to one balance-reserve or unreserve operation.
         * - One passthrough operation, one insert, both `O(S)` where `S` is the number of
         * signatories. `S` is capped by `MaxSignatories`, with weight being proportional.
         * - One encode & hash, both of complexity `O(S)`.
         * - One event.
         * - I/O: 1 read `O(S)`, one remove.
         * - Storage: removes one item.
         */
        cancel_as_multi: TxDescriptor<Anonymize<I3d9o9d7epp66v>>;
        /**
         * Poke the deposit reserved for an existing multisig operation.
         *
         * The dispatch origin for this call must be _Signed_ and must be the original depositor of
         * the multisig operation.
         *
         * The transaction fee is waived if the deposit amount has changed.
         *
         * - `threshold`: The total number of approvals needed for this multisig.
         * - `other_signatories`: The accounts (other than the sender) who are part of the
         * multisig.
         * - `call_hash`: The hash of the call this deposit is reserved for.
         *
         * Emits `DepositPoked` if successful.
         */
        poke_deposit: TxDescriptor<Anonymize<I6lqh1vgb4mcja>>;
    };
    Preimage: {
        /**
         * Register a preimage on-chain.
         *
         * If the preimage was previously requested, no fees or deposits are taken for providing
         * the preimage. Otherwise, a deposit is taken proportional to the size of the preimage.
         */
        note_preimage: TxDescriptor<Anonymize<I82nfqfkd48n10>>;
        /**
         * Clear an unrequested preimage from the runtime storage.
         *
         * If `len` is provided, then it will be a much cheaper operation.
         *
         * - `hash`: The hash of the preimage to be removed from the store.
         * - `len`: The length of the preimage of `hash`.
         */
        unnote_preimage: TxDescriptor<Anonymize<I1jm8m1rh9e20v>>;
        /**
         * Request a preimage be uploaded to the chain without paying any fees or deposits.
         *
         * If the preimage requests has already been provided on-chain, we unreserve any deposit
         * a user may have paid, and take the control of the preimage out of their hands.
         */
        request_preimage: TxDescriptor<Anonymize<I1jm8m1rh9e20v>>;
        /**
         * Clear a previously made request for a preimage.
         *
         * NOTE: THIS MUST NOT BE CALLED ON `hash` MORE TIMES THAN `request_preimage`.
         */
        unrequest_preimage: TxDescriptor<Anonymize<I1jm8m1rh9e20v>>;
        /**
         * Ensure that the bulk of pre-images is upgraded.
         *
         * The caller pays no fee if at least 90% of pre-images were successfully updated.
         */
        ensure_updated: TxDescriptor<Anonymize<I3o5j3bli1pd8e>>;
    };
    Scheduler: {
        /**
         * Anonymously schedule a task.
         */
        schedule: TxDescriptor<Anonymize<Ivqkjqsbgj1dj>>;
        /**
         * Cancel an anonymously scheduled task.
         */
        cancel: TxDescriptor<Anonymize<I5n4sebgkfr760>>;
        /**
         * Schedule a named task.
         */
        schedule_named: TxDescriptor<Anonymize<Ib6bm2ug64rldc>>;
        /**
         * Cancel a named scheduled task.
         */
        cancel_named: TxDescriptor<Anonymize<Ifs1i5fk9cqvr6>>;
        /**
         * Anonymously schedule a task after a delay.
         */
        schedule_after: TxDescriptor<Anonymize<I5q3t0hm83a58h>>;
        /**
         * Schedule a named task after a delay.
         */
        schedule_named_after: TxDescriptor<Anonymize<I2gnaqoj2eimi0>>;
        /**
         * Set a retry configuration for a task so that, in case its scheduled run fails, it will
         * be retried after `period` blocks, for a total amount of `retries` retries or until it
         * succeeds.
         *
         * Tasks which need to be scheduled for a retry are still subject to weight metering and
         * agenda space, same as a regular task. If a periodic task fails, it will be scheduled
         * normally while the task is retrying.
         *
         * Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic
         * clones of the original task. Their retry configuration will be derived from the
         * original task's configuration, but will have a lower value for `remaining` than the
         * original `total_retries`.
         */
        set_retry: TxDescriptor<Anonymize<Ieg3fd8p4pkt10>>;
        /**
         * Set a retry configuration for a named task so that, in case its scheduled run fails, it
         * will be retried after `period` blocks, for a total amount of `retries` retries or until
         * it succeeds.
         *
         * Tasks which need to be scheduled for a retry are still subject to weight metering and
         * agenda space, same as a regular task. If a periodic task fails, it will be scheduled
         * normally while the task is retrying.
         *
         * Tasks scheduled as a result of a retry for a periodic task are unnamed, non-periodic
         * clones of the original task. Their retry configuration will be derived from the
         * original task's configuration, but will have a lower value for `remaining` than the
         * original `total_retries`.
         */
        set_retry_named: TxDescriptor<Anonymize<I8kg5ll427kfqq>>;
        /**
         * Removes the retry configuration of a task.
         */
        cancel_retry: TxDescriptor<Anonymize<I467333262q1l9>>;
        /**
         * Cancel the retry configuration of a named task.
         */
        cancel_retry_named: TxDescriptor<Anonymize<Ifs1i5fk9cqvr6>>;
    };
    Proxy: {
        /**
         * Dispatch the given `call` from an account that the sender is authorised for through
         * `add_proxy`.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `real`: The account that the proxy will make a call on behalf of.
         * - `force_proxy_type`: Specify the exact proxy type to be used and checked for this call.
         * - `call`: The call to be made by the `real` account.
         */
        proxy: TxDescriptor<Anonymize<Idlqs144rc48hk>>;
        /**
         * Register a proxy account for the sender that is able to make calls on its behalf.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `proxy`: The account that the `caller` would like to make a proxy.
         * - `proxy_type`: The permissions allowed for this proxy account.
         * - `delay`: The announcement period required of the initial proxy. Will generally be
         * zero.
         */
        add_proxy: TxDescriptor<Anonymize<It11trpppbc3l>>;
        /**
         * Unregister a proxy account for the sender.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `proxy`: The account that the `caller` would like to remove as a proxy.
         * - `proxy_type`: The permissions currently enabled for the removed proxy account.
         */
        remove_proxy: TxDescriptor<Anonymize<It11trpppbc3l>>;
        /**
         * Unregister all proxy accounts for the sender.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * WARNING: This may be called on accounts created by `create_pure`, however if done, then
         * the unreserved fees will be inaccessible. **All access to this account will be lost.**
         */
        remove_proxies: TxDescriptor<undefined>;
        /**
         * Spawn a fresh new account that is guaranteed to be otherwise inaccessible, and
         * initialize it with a proxy of `proxy_type` for `origin` sender.
         *
         * Requires a `Signed` origin.
         *
         * - `proxy_type`: The type of the proxy that the sender will be registered as over the
         * new account. This will almost always be the most permissive `ProxyType` possible to
         * allow for maximum flexibility.
         * - `index`: A disambiguation index, in case this is called multiple times in the same
         * transaction (e.g. with `utility::batch`). Unless you're using `batch` you probably just
         * want to use `0`.
         * - `delay`: The announcement period required of the initial proxy. Will generally be
         * zero.
         *
         * Fails with `Duplicate` if this has already been called in this transaction, from the
         * same sender, with the same parameters.
         *
         * Fails if there are insufficient funds to pay for deposit.
         */
        create_pure: TxDescriptor<Anonymize<Ietml13sclqs1q>>;
        /**
         * Removes a previously spawned pure proxy.
         *
         * WARNING: **All access to this account will be lost.** Any funds held in it will be
         * inaccessible.
         *
         * Requires a `Signed` origin, and the sender account must have been created by a call to
         * `create_pure` with corresponding parameters.
         *
         * - `spawner`: The account that originally called `create_pure` to create this account.
         * - `index`: The disambiguation index originally passed to `create_pure`. Probably `0`.
         * - `proxy_type`: The proxy type originally passed to `create_pure`.
         * - `height`: The height of the chain when the call to `create_pure` was processed.
         * - `ext_index`: The extrinsic index in which the call to `create_pure` was processed.
         *
         * Fails with `NoPermission` in case the caller is not a previously created pure
         * account whose `create_pure` call has corresponding parameters.
         */
        kill_pure: TxDescriptor<Anonymize<Iftfic7p3uban2>>;
        /**
         * Publish the hash of a proxy-call that will be made in the future.
         *
         * This must be called some number of blocks before the corresponding `proxy` is attempted
         * if the delay associated with the proxy relationship is greater than zero.
         *
         * No more than `MaxPending` announcements may be made at any one time.
         *
         * This will take a deposit of `AnnouncementDepositFactor` as well as
         * `AnnouncementDepositBase` if there are no other pending announcements.
         *
         * The dispatch origin for this call must be _Signed_ and a proxy of `real`.
         *
         * Parameters:
         * - `real`: The account that the proxy will make a call on behalf of.
         * - `call_hash`: The hash of the call to be made by the `real` account.
         */
        announce: TxDescriptor<Anonymize<I2eb501t8s6hsq>>;
        /**
         * Remove a given announcement.
         *
         * May be called by a proxy account to remove a call they previously announced and return
         * the deposit.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `real`: The account that the proxy will make a call on behalf of.
         * - `call_hash`: The hash of the call to be made by the `real` account.
         */
        remove_announcement: TxDescriptor<Anonymize<I2eb501t8s6hsq>>;
        /**
         * Remove the given announcement of a delegate.
         *
         * May be called by a target (proxied) account to remove a call that one of their delegates
         * (`delegate`) has announced they want to execute. The deposit is returned.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `delegate`: The account that previously announced the call.
         * - `call_hash`: The hash of the call to be made.
         */
        reject_announcement: TxDescriptor<Anonymize<Ianmuoljk2sk1u>>;
        /**
         * Dispatch the given `call` from an account that the sender is authorized for through
         * `add_proxy`.
         *
         * Removes any corresponding announcement(s).
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `real`: The account that the proxy will make a call on behalf of.
         * - `force_proxy_type`: Specify the exact proxy type to be used and checked for this call.
         * - `call`: The call to be made by the `real` account.
         */
        proxy_announced: TxDescriptor<Anonymize<I7hgtlnpelk0fc>>;
        /**
         * Poke / Adjust deposits made for proxies and announcements based on current values.
         * This can be used by accounts to possibly lower their locked amount.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * The transaction fee is waived if the deposit amount has changed.
         *
         * Emits `DepositPoked` if successful.
         */
        poke_deposit: TxDescriptor<undefined>;
    };
    Registry: {
        /**
         * Register an identity for an account. This will overwrite any existing identity.
         */
        set_identity: TxDescriptor<Anonymize<I3p6khp3nv37cu>>;
        /**
         * Clear the identity of an account.
         */
        clear_identity: TxDescriptor<Anonymize<I6pnnj50tnq448>>;
    };
    Commitments: {
        /**
         * Set the commitment for a given netuid
         */
        set_commitment: TxDescriptor<Anonymize<I57v1t6776pl3a>>;
        /**
         * Sudo-set MaxSpace
         */
        set_max_space: TxDescriptor<Anonymize<I1il5mj68vvsms>>;
    };
    AdminUtils: {
        /**
         * The extrinsic sets the new authorities for Aura consensus.
         * It is only callable by the root account.
         * The extrinsic will call the Aura pallet to change the authorities.
         */
        swap_authorities: TxDescriptor<Anonymize<I42mob3hqe6j7h>>;
        /**
         * The extrinsic sets the default take for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the default take.
         */
        sudo_set_default_take: TxDescriptor<Anonymize<Icdbq0j31b3g9c>>;
        /**
         * The extrinsic sets the transaction rate limit for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the transaction rate limit.
         */
        sudo_set_tx_rate_limit: TxDescriptor<Anonymize<I3gk6eeddm0hsd>>;
        /**
         * The extrinsic sets the serving rate limit for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the serving rate limit.
         */
        sudo_set_serving_rate_limit: TxDescriptor<Anonymize<I2t2rlclb0ce3e>>;
        /**
         * The extrinsic sets the minimum difficulty for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the minimum difficulty.
         */
        sudo_set_min_difficulty: TxDescriptor<Anonymize<Iar87gdqmug5o7>>;
        /**
         * The extrinsic sets the maximum difficulty for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the maximum difficulty.
         */
        sudo_set_max_difficulty: TxDescriptor<Anonymize<I3oullii9p80a1>>;
        /**
         * The extrinsic sets the weights version key for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the weights version key.
         */
        sudo_set_weights_version_key: TxDescriptor<Anonymize<I8t8ta6lfbia9e>>;
        /**
         * The extrinsic sets the weights set rate limit for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the weights set rate limit.
         */
        sudo_set_weights_set_rate_limit: TxDescriptor<Anonymize<I3akfmjle982qg>>;
        /**
         * The extrinsic sets the adjustment interval for a subnet.
         * It is only callable by the root account, not changeable by the subnet owner.
         * The extrinsic will call the Subtensor pallet to set the adjustment interval.
         */
        sudo_set_adjustment_interval: TxDescriptor<Anonymize<Ibaje86kdit7s6>>;
        /**
         * The extrinsic sets the adjustment alpha for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the adjustment alpha.
         */
        sudo_set_adjustment_alpha: TxDescriptor<Anonymize<I90lra4vl5j4db>>;
        /**
         * The extrinsic sets the immunity period for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the immunity period.
         */
        sudo_set_immunity_period: TxDescriptor<Anonymize<I1q480m57ftcms>>;
        /**
         * The extrinsic sets the minimum allowed weights for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the minimum allowed weights.
         */
        sudo_set_min_allowed_weights: TxDescriptor<Anonymize<Ie2bjglo51atf6>>;
        /**
         * The extrinsic sets the maximum allowed UIDs for a subnet.
         * It is only callable by the root account and subnet owner.
         * The extrinsic will call the Subtensor pallet to set the maximum allowed UIDs for a subnet.
         */
        sudo_set_max_allowed_uids: TxDescriptor<Anonymize<Ievma38tc25kil>>;
        /**
         * The extrinsic sets the kappa for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the kappa.
         */
        sudo_set_kappa: TxDescriptor<Anonymize<I2er75v4akf5cc>>;
        /**
         * The extrinsic sets the rho for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the rho.
         */
        sudo_set_rho: TxDescriptor<Anonymize<I5pldh0j0v0u4l>>;
        /**
         * The extrinsic sets the activity cutoff for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the activity cutoff.
         */
        sudo_set_activity_cutoff: TxDescriptor<Anonymize<Ifhou5p0slv68r>>;
        /**
         * The extrinsic sets the network registration allowed for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the network registration allowed.
         */
        sudo_set_network_registration_allowed: TxDescriptor<Anonymize<I9m89dnau2i4tt>>;
        /**
         * The extrinsic sets the network PoW registration allowed for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the network PoW registration allowed.
         */
        sudo_set_network_pow_registration_allowed: TxDescriptor<Anonymize<I9m89dnau2i4tt>>;
        /**
         * The extrinsic sets the target registrations per interval for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the target registrations per interval.
         */
        sudo_set_target_registrations_per_interval: TxDescriptor<Anonymize<Ifunpjbsc4jrrr>>;
        /**
         * The extrinsic sets the minimum burn for a subnet.
         * It is only callable by root and subnet owner.
         * The extrinsic will call the Subtensor pallet to set the minimum burn.
         */
        sudo_set_min_burn: TxDescriptor<Anonymize<I85uujfpnu8gum>>;
        /**
         * The extrinsic sets the maximum burn for a subnet.
         * It is only callable by root and subnet owner.
         * The extrinsic will call the Subtensor pallet to set the maximum burn.
         */
        sudo_set_max_burn: TxDescriptor<Anonymize<I7bl5t0it6ck2m>>;
        /**
         * The extrinsic sets the difficulty for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the difficulty.
         */
        sudo_set_difficulty: TxDescriptor<Anonymize<I4iope0tjiqgu4>>;
        /**
         * The extrinsic sets the maximum allowed validators for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the maximum allowed validators.
         */
        sudo_set_max_allowed_validators: TxDescriptor<Anonymize<Iptqa236frcvo>>;
        /**
         * The extrinsic sets the bonds moving average for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the bonds moving average.
         */
        sudo_set_bonds_moving_average: TxDescriptor<Anonymize<I8hbi1vrve1i2>>;
        /**
         * The extrinsic sets the bonds penalty for a subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the bonds penalty.
         */
        sudo_set_bonds_penalty: TxDescriptor<Anonymize<I1v9a50gjqk26k>>;
        /**
         * The extrinsic sets the maximum registrations per block for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the maximum registrations per block.
         */
        sudo_set_max_registrations_per_block: TxDescriptor<Anonymize<Idv4d3rktbigfh>>;
        /**
         * The extrinsic sets the subnet owner cut for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the subnet owner cut.
         */
        sudo_set_subnet_owner_cut: TxDescriptor<Anonymize<I56j1e9gqlq602>>;
        /**
         * The extrinsic sets the network rate limit for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the network rate limit.
         */
        sudo_set_network_rate_limit: TxDescriptor<Anonymize<Ib6k4vik9ruq8h>>;
        /**
         * The extrinsic sets the tempo for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the tempo.
         */
        sudo_set_tempo: TxDescriptor<Anonymize<I9u9gu9aa92l5m>>;
        /**
         * The extrinsic sets the total issuance for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the issuance for the network.
         */
        sudo_set_total_issuance: TxDescriptor<Anonymize<Idmd4tos09qd68>>;
        /**
         * The extrinsic sets the immunity period for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the immunity period for the network.
         */
        sudo_set_network_immunity_period: TxDescriptor<Anonymize<Ia0sp2p68e9k16>>;
        /**
         * The extrinsic sets the min lock cost for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the min lock cost for the network.
         */
        sudo_set_network_min_lock_cost: TxDescriptor<Anonymize<Ie318529rgoagk>>;
        /**
         * The extrinsic sets the subnet limit for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the subnet limit.
         */
        sudo_set_subnet_limit: TxDescriptor<Anonymize<Iam4iou8r3isc1>>;
        /**
         * The extrinsic sets the lock reduction interval for the network.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the lock reduction interval.
         */
        sudo_set_lock_reduction_interval: TxDescriptor<Anonymize<I21ajnsdtbutjh>>;
        /**
         * The extrinsic sets the recycled RAO for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the recycled RAO.
         */
        sudo_set_rao_recycled: TxDescriptor<Anonymize<I203rofi4rpmo4>>;
        /**
         * The extrinsic sets the weights min stake.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the weights min stake.
         */
        sudo_set_stake_threshold: TxDescriptor<Anonymize<I1e290fmo892vi>>;
        /**
         * The extrinsic sets the minimum stake required for nominators.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the minimum stake required for nominators.
         */
        sudo_set_nominator_min_required_stake: TxDescriptor<Anonymize<I1e290fmo892vi>>;
        /**
         * The extrinsic sets the rate limit for delegate take transactions.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the rate limit for delegate take transactions.
         */
        sudo_set_tx_delegate_take_rate_limit: TxDescriptor<Anonymize<I3gk6eeddm0hsd>>;
        /**
         * The extrinsic sets the minimum delegate take.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the minimum delegate take.
         */
        sudo_set_min_delegate_take: TxDescriptor<Anonymize<I6ue7qc27uhiev>>;
        /**
         * The extrinsic enabled/disables commit/reaveal for a given subnet.
         * It is only callable by the root account or subnet owner.
         * The extrinsic will call the Subtensor pallet to set the value.
         */
        sudo_set_commit_reveal_weights_enabled: TxDescriptor<Anonymize<Ie31ro5s5e089f>>;
        /**
         * Enables or disables Liquid Alpha for a given subnet.
         *
         * # Parameters
         * - `origin`: The origin of the call, which must be the root account or subnet owner.
         * - `netuid`: The unique identifier for the subnet.
         * - `enabled`: A boolean flag to enable or disable Liquid Alpha.
         *
         * # Weight
         * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
         */
        sudo_set_liquid_alpha_enabled: TxDescriptor<Anonymize<Ie31ro5s5e089f>>;
        /**
         * Sets values for liquid alpha
         */
        sudo_set_alpha_values: TxDescriptor<Anonymize<I71lu4gpn88cf0>>;
        /**
         * Sets the duration of the dissolve network schedule.
         *
         * This extrinsic allows the root account to set the duration for the dissolve network schedule.
         * The dissolve network schedule determines how long it takes for a network dissolution operation to complete.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the root account.
         * * `duration` - The new duration for the dissolve network schedule, in number of blocks.
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_dissolve_network_schedule_duration: TxDescriptor<Anonymize<I98iornf3ajrp9>>;
        /**
         * Sets the commit-reveal weights periods for a specific subnet.
         *
         * This extrinsic allows the subnet owner or root account to set the duration (in epochs) during which committed weights must be revealed.
         * The commit-reveal mechanism ensures that users commit weights in advance and reveal them only within a specified period.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the subnet owner or the root account.
         * * `netuid` - The unique identifier of the subnet for which the periods are being set.
         * * `periods` - The number of epochs that define the commit-reveal period.
         *
         * # Errors
         * * `BadOrigin` - If the caller is neither the subnet owner nor the root account.
         * * `SubnetDoesNotExist` - If the specified subnet does not exist.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_commit_reveal_weights_interval: TxDescriptor<Anonymize<I9893mbk9nh201>>;
        /**
         * Sets the EVM ChainID.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the subnet owner or the root account.
         * * `chainId` - The u64 chain ID
         *
         * # Errors
         * * `BadOrigin` - If the caller is neither the subnet owner nor the root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_evm_chain_id: TxDescriptor<Anonymize<I623eo8t3jrbeo>>;
        /**
         * A public interface for `pallet_grandpa::Pallet::schedule_grandpa_change`.
         *
         * Schedule a change in the authorities.
         *
         * The change will be applied at the end of execution of the block `in_blocks` after the
         * current block. This value may be 0, in which case the change is applied at the end of
         * the current block.
         *
         * If the `forced` parameter is defined, this indicates that the current set has been
         * synchronously determined to be offline and that after `in_blocks` the given change
         * should be applied. The given block number indicates the median last finalized block
         * number and it should be used as the canon block when starting the new grandpa voter.
         *
         * No change should be signaled while any change is pending. Returns an error if a change
         * is already pending.
         */
        schedule_grandpa_change: TxDescriptor<Anonymize<Ieo8qamskgm4dk>>;
        /**
         * Enable or disable atomic alpha transfers for a given subnet.
         *
         * # Parameters
         * - `origin`: The origin of the call, which must be the root account or subnet owner.
         * - `netuid`: The unique identifier for the subnet.
         * - `enabled`: A boolean flag to enable or disable Liquid Alpha.
         *
         * # Weight
         * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
         */
        sudo_set_toggle_transfer: TxDescriptor<Anonymize<Ift1efpssa32g2>>;
        /**
         * Set the behaviour of the "burn" UID(s) for a given subnet.
         * If set to `Burn`, the miner emission sent to the burn UID(s) will be burned.
         * If set to `Recycle`, the miner emission sent to the burn UID(s) will be recycled.
         *
         * # Parameters
         * - `origin`: The origin of the call, which must be the root account or subnet owner.
         * - `netuid`: The unique identifier for the subnet.
         * - `recycle_or_burn`: The desired behaviour of the "burn" UID(s) for the subnet.
         *
         */
        sudo_set_recycle_or_burn: TxDescriptor<Anonymize<Ibk3v0rrpo1bio>>;
        /**
         * Toggles the enablement of an EVM precompile.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the root account.
         * * `precompile_id` - The identifier of the EVM precompile to toggle.
         * * `enabled` - The new enablement state of the precompile.
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_toggle_evm_precompile: TxDescriptor<Anonymize<I1sj8huj7of8mb>>;
        /**
         *
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the root account.
         * * `alpha` - The new moving alpha value for the SubnetMovingAlpha.
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_subnet_moving_alpha: TxDescriptor<Anonymize<I6av3sq9jkhmm3>>;
        /**
         * Change the SubnetOwnerHotkey for a given subnet.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the subnet owner.
         * * `netuid` - The unique identifier for the subnet.
         * * `hotkey` - The new hotkey for the subnet owner.
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the subnet owner or root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_subnet_owner_hotkey: TxDescriptor<Anonymize<I7f38r2vt6r9k1>>;
        /**
         *
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the root account.
         * * `ema_alpha_period` - Number of blocks for EMA price to halve
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_ema_price_halving_period: TxDescriptor<Anonymize<I70cd7doki8rme>>;
        /**
         *
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the root account.
         * * `netuid` - The unique identifier for the subnet.
         * * `steepness` - The Steepness for the alpha sigmoid function. (range is 0-int16::MAX,
         * negative values are reserved for future use)
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the root account.
         * * `SubnetDoesNotExist` - If the specified subnet does not exist.
         * * `NegativeSigmoidSteepness` - If the steepness is negative and the caller is
         * root.
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_alpha_sigmoid_steepness: TxDescriptor<Anonymize<Iam7j42j9f1go6>>;
        /**
         * Enables or disables Yuma3 for a given subnet.
         *
         * # Parameters
         * - `origin`: The origin of the call, which must be the root account or subnet owner.
         * - `netuid`: The unique identifier for the subnet.
         * - `enabled`: A boolean flag to enable or disable Yuma3.
         *
         * # Weight
         * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
         */
        sudo_set_yuma3_enabled: TxDescriptor<Anonymize<Ie31ro5s5e089f>>;
        /**
         * Enables or disables Bonds Reset for a given subnet.
         *
         * # Parameters
         * - `origin`: The origin of the call, which must be the root account or subnet owner.
         * - `netuid`: The unique identifier for the subnet.
         * - `enabled`: A boolean flag to enable or disable Bonds Reset.
         *
         * # Weight
         * This function has a fixed weight of 0 and is classified as an operational transaction that does not incur any fees.
         */
        sudo_set_bonds_reset_enabled: TxDescriptor<Anonymize<Ie31ro5s5e089f>>;
        /**
         * Sets or updates the hotkey account associated with the owner of a specific subnet.
         *
         * This function allows either the root origin or the current subnet owner to set or update
         * the hotkey for a given subnet. The subnet must already exist. To prevent abuse, the call is
         * rate-limited to once per configured interval (default: one week) per subnet.
         *
         * # Parameters
         * - `origin`: The dispatch origin of the call. Must be either root or the current owner of the subnet.
         * - `netuid`: The unique identifier of the subnet whose owner hotkey is being set.
         * - `hotkey`: The new hotkey account to associate with the subnet owner.
         *
         * # Returns
         * - `DispatchResult`: Returns `Ok(())` if the hotkey was successfully set, or an appropriate error otherwise.
         *
         * # Errors
         * - `Error::SubnetNotExists`: If the specified subnet does not exist.
         * - `Error::TxRateLimitExceeded`: If the function is called more frequently than the allowed rate limit.
         *
         * # Access Control
         * Only callable by:
         * - Root origin, or
         * - The coldkey account that owns the subnet.
         *
         * # Storage
         * - Updates [`SubnetOwnerHotkey`] for the given `netuid`.
         * - Reads and updates [`LastRateLimitedBlock`] for rate-limiting.
         * - Reads [`DefaultSetSNOwnerHotkeyRateLimit`] to determine the interval between allowed updates.
         *
         * # Rate Limiting
         * This function is rate-limited to one call per subnet per interval (e.g., one week).
         */
        sudo_set_sn_owner_hotkey: TxDescriptor<Anonymize<I7f38r2vt6r9k1>>;
        /**
         * Enables or disables subtoken trading for a given subnet.
         *
         * # Arguments
         * * `origin` - The origin of the call, which must be the root account.
         * * `netuid` - The unique identifier of the subnet.
         * * `subtoken_enabled` - A boolean indicating whether subtoken trading should be enabled or disabled.
         *
         * # Errors
         * * `BadOrigin` - If the caller is not the root account.
         *
         * # Weight
         * Weight is handled by the `#[pallet::weight]` attribute.
         */
        sudo_set_subtoken_enabled: TxDescriptor<Anonymize<Idco9ambhipg4i>>;
        /**
         * Sets the commit-reveal weights version for all subnets
         */
        sudo_set_commit_reveal_version: TxDescriptor<Anonymize<I6s1nbislhk619>>;
        /**
         * Sets the number of immune owner neurons
         */
        sudo_set_owner_immune_neuron_limit: TxDescriptor<Anonymize<I9jtu7slb30qvs>>;
        /**
         * Sets the childkey burn for a subnet.
         * It is only callable by the root account.
         * The extrinsic will call the Subtensor pallet to set the childkey burn.
         */
        sudo_set_ck_burn: TxDescriptor<Anonymize<Idv3j6a15pjc16>>;
        /**
         * Sets the admin freeze window length (in blocks) at the end of a tempo.
         * Only callable by root.
         */
        sudo_set_admin_freeze_window: TxDescriptor<Anonymize<I206qvjkjun95i>>;
        /**
         * Sets the owner hyperparameter rate limit in epochs (global multiplier).
         * Only callable by root.
         */
        sudo_set_owner_hparam_rate_limit: TxDescriptor<Anonymize<I4qhb3plq4ifmq>>;
        /**
         * Sets the desired number of mechanisms in a subnet
         */
        sudo_set_mechanism_count: TxDescriptor<Anonymize<Ic58lhlh1ocpm1>>;
        /**
         * Sets the emission split between mechanisms in a subnet
         */
        sudo_set_mechanism_emission_split: TxDescriptor<Anonymize<I6uopd4b2os90n>>;
        /**
         * Trims the maximum number of UIDs for a subnet.
         *
         * The trimming is done by sorting the UIDs by emission descending and then trimming
         * the lowest emitters while preserving temporally and owner immune UIDs. The UIDs are
         * then compressed to the left and storage is migrated to the new compressed UIDs.
         */
        sudo_trim_to_max_allowed_uids: TxDescriptor<Anonymize<I6idbvi8v00o5j>>;
        /**
         * The extrinsic sets the minimum allowed UIDs for a subnet.
         * It is only callable by the root account.
         */
        sudo_set_min_allowed_uids: TxDescriptor<Anonymize<Ifbgbhkj74b35k>>;
        /**
         * Sets TAO flow cutoff value (A)
         */
        sudo_set_tao_flow_cutoff: TxDescriptor<Anonymize<Ibt4a800kb7frq>>;
        /**
         * Sets TAO flow normalization exponent (p)
         */
        sudo_set_tao_flow_normalization_exponent: TxDescriptor<Anonymize<Icb4un8h4cokoo>>;
        /**
         * Sets TAO flow smoothing factor (alpha)
         */
        sudo_set_tao_flow_smoothing_factor: TxDescriptor<Anonymize<I1up607q6ce947>>;
        /**
         * Sets the global maximum number of mechanisms in a subnet
         */
        sudo_set_max_mechanism_count: TxDescriptor<Anonymize<I7hktg5sccf8op>>;
        /**
         * Sets the minimum number of non-immortal & non-immune UIDs that must remain in a subnet
         */
        sudo_set_min_non_immune_uids: TxDescriptor<Anonymize<Ib1d0bomkbrqv1>>;
        /**
         * Sets the delay before a subnet can call start
         */
        sudo_set_start_call_delay: TxDescriptor<Anonymize<Iaflrold1ds0nq>>;
        /**
         * Sets the announcement delay for coldkey swap.
         */
        sudo_set_coldkey_swap_announcement_delay: TxDescriptor<Anonymize<I98iornf3ajrp9>>;
        /**
         * Sets the coldkey swap reannouncement delay.
         */
        sudo_set_coldkey_swap_reannouncement_delay: TxDescriptor<Anonymize<I98iornf3ajrp9>>;
    };
    SafeMode: {
        /**
         * Enter safe-mode permissionlessly for [`Config::EnterDuration`] blocks.
         *
         * Reserves [`Config::EnterDepositAmount`] from the caller's account.
         * Emits an [`Event::Entered`] event on success.
         * Errors with [`Error::Entered`] if the safe-mode is already entered.
         * Errors with [`Error::NotConfigured`] if the deposit amount is `None`.
         */
        enter: TxDescriptor<undefined>;
        /**
         * Enter safe-mode by force for a per-origin configured number of blocks.
         *
         * Emits an [`Event::Entered`] event on success.
         * Errors with [`Error::Entered`] if the safe-mode is already entered.
         *
         * Can only be called by the [`Config::ForceEnterOrigin`] origin.
         */
        force_enter: TxDescriptor<undefined>;
        /**
         * Extend the safe-mode permissionlessly for [`Config::ExtendDuration`] blocks.
         *
         * This accumulates on top of the current remaining duration.
         * Reserves [`Config::ExtendDepositAmount`] from the caller's account.
         * Emits an [`Event::Extended`] event on success.
         * Errors with [`Error::Exited`] if the safe-mode is entered.
         * Errors with [`Error::NotConfigured`] if the deposit amount is `None`.
         *
         * This may be called by any signed origin with [`Config::ExtendDepositAmount`] free
         * currency to reserve. This call can be disabled for all origins by configuring
         * [`Config::ExtendDepositAmount`] to `None`.
         */
        extend: TxDescriptor<undefined>;
        /**
         * Extend the safe-mode by force for a per-origin configured number of blocks.
         *
         * Emits an [`Event::Extended`] event on success.
         * Errors with [`Error::Exited`] if the safe-mode is inactive.
         *
         * Can only be called by the [`Config::ForceExtendOrigin`] origin.
         */
        force_extend: TxDescriptor<undefined>;
        /**
         * Exit safe-mode by force.
         *
         * Emits an [`Event::Exited`] with [`ExitReason::Force`] event on success.
         * Errors with [`Error::Exited`] if the safe-mode is inactive.
         *
         * Note: `safe-mode` will be automatically deactivated by [`Pallet::on_initialize`] hook
         * after the block height is greater than the [`EnteredUntil`] storage item.
         * Emits an [`Event::Exited`] with [`ExitReason::Timeout`] event when deactivated in the
         * hook.
         */
        force_exit: TxDescriptor<undefined>;
        /**
         * Slash a deposit for an account that entered or extended safe-mode at a given
         * historical block.
         *
         * This can only be called while safe-mode is entered.
         *
         * Emits a [`Event::DepositSlashed`] event on success.
         * Errors with [`Error::Entered`] if safe-mode is entered.
         *
         * Can only be called by the [`Config::ForceDepositOrigin`] origin.
         */
        force_slash_deposit: TxDescriptor<Anonymize<I1ssp78ejl639m>>;
        /**
         * Permissionlessly release a deposit for an account that entered safe-mode at a
         * given historical block.
         *
         * The call can be completely disabled by setting [`Config::ReleaseDelay`] to `None`.
         * This cannot be called while safe-mode is entered and not until
         * [`Config::ReleaseDelay`] blocks have passed since safe-mode was entered.
         *
         * Emits a [`Event::DepositReleased`] event on success.
         * Errors with [`Error::Entered`] if the safe-mode is entered.
         * Errors with [`Error::CannotReleaseYet`] if [`Config::ReleaseDelay`] block have not
         * passed since safe-mode was entered. Errors with [`Error::NoDeposit`] if the payee has no
         * reserved currency at the block specified.
         */
        release_deposit: TxDescriptor<Anonymize<I1ssp78ejl639m>>;
        /**
         * Force to release a deposit for an account that entered safe-mode at a given
         * historical block.
         *
         * This can be called while safe-mode is still entered.
         *
         * Emits a [`Event::DepositReleased`] event on success.
         * Errors with [`Error::Entered`] if safe-mode is entered.
         * Errors with [`Error::NoDeposit`] if the payee has no reserved currency at the
         * specified block.
         *
         * Can only be called by the [`Config::ForceDepositOrigin`] origin.
         */
        force_release_deposit: TxDescriptor<Anonymize<I1ssp78ejl639m>>;
    };
    Ethereum: {
        /**
         * Transact an Ethereum transaction.
         */
        transact: TxDescriptor<Anonymize<I13qib3vtm9cs3>>;
    };
    EVM: {
        /**
         * Withdraw balance from EVM into currency/balances pallet.
         */
        withdraw: TxDescriptor<Anonymize<Idcabvplu05lea>>;
        /**
         * Issue an EVM call operation. This is similar to a message call transaction in Ethereum.
         */
        call: TxDescriptor<Anonymize<Id38gdpcotl637>>;
        /**
         * Issue an EVM create operation. This is similar to a contract creation transaction in
         * Ethereum.
         */
        create: TxDescriptor<Anonymize<I73q3qf5u7nnqg>>;
        /**
         * Issue an EVM create2 operation.
         */
        create2: TxDescriptor<Anonymize<Idpm1bc2cr6dgj>>;
        /**
        
         */
        set_whitelist: TxDescriptor<Anonymize<I837c61fc07ine>>;
        /**
        
         */
        disable_whitelist: TxDescriptor<Anonymize<I6m0oguilvhn8>>;
    };
    BaseFee: {
        /**
        
         */
        set_base_fee_per_gas: TxDescriptor<Anonymize<I7vi74gbubc8u5>>;
        /**
        
         */
        set_elasticity: TxDescriptor<Anonymize<I3u0knmtb1ueq7>>;
    };
    Drand: {
        /**
         * Verify and write a pulse from the beacon into the runtime
         */
        write_pulse: TxDescriptor<Anonymize<I87tlou92i0bot>>;
        /**
         * allows the root user to set the beacon configuration
         * generally this would be called from an offchain worker context.
         * there is no verification of configurations, so be careful with this.
         *
         * * `origin`: the root user
         * * `config`: the beacon configuration
         */
        set_beacon_config: TxDescriptor<Anonymize<Ifd3mkud9g8rb1>>;
        /**
         * allows the root user to set the oldest stored round
         */
        set_oldest_stored_round: TxDescriptor<Anonymize<Iakvbbhvger3oa>>;
    };
    Crowdloan: {
        /**
         * Create a crowdloan that will raise funds up to a maximum cap and if successful,
         * will transfer funds to the target address if provided and dispatch the call
         * (using creator origin).
         *
         * The initial deposit will be transfered to the crowdloan account and will be refunded
         * in case the crowdloan fails to raise the cap. Additionally, the creator will pay for
         * the execution of the call.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `deposit`: The initial deposit from the creator.
         * - `min_contribution`: The minimum contribution required to contribute to the crowdloan.
         * - `cap`: The maximum amount of funds that can be raised.
         * - `end`: The block number at which the crowdloan will end.
         * - `call`: The call to dispatch when the crowdloan is finalized.
         * - `target_address`: The address to transfer the raised funds to if provided.
         */
        create: TxDescriptor<Anonymize<I92t98snpjjcts>>;
        /**
         * Contribute to an active crowdloan.
         *
         * The contribution will be transfered to the crowdloan account and will be refunded
         * if the crowdloan fails to raise the cap. If the contribution would raise the amount above the cap,
         * the contribution will be set to the amount that is left to be raised.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to contribute to.
         * - `amount`: The amount to contribute.
         */
        contribute: TxDescriptor<Anonymize<Iet4pe2le7ku09>>;
        /**
         * Withdraw a contribution from an active (not yet finalized or dissolved) crowdloan.
         *
         * Only contributions over the deposit can be withdrawn by the creator.
         *
         * The dispatch origin for this call must be _Signed_.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to withdraw from.
         */
        withdraw: TxDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * Finalize crowdloan that has reached the cap.
         *
         * The call will transfer the raised amount to the target address if it was provided when the crowdloan was created
         * and dispatch the call that was provided using the creator origin. The CurrentCrowdloanId will be set to the
         * crowdloan id being finalized so the dispatched call can access it temporarily by accessing
         * the `CurrentCrowdloanId` storage item.
         *
         * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to finalize.
         */
        finalize: TxDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * Refund contributors of a non-finalized crowdloan.
         *
         * The call will try to refund all contributors (excluding the creator) up to the limit defined by the `RefundContributorsLimit`.
         * If the limit is reached, the call will stop and the crowdloan will be marked as partially refunded.
         * It may be needed to dispatch this call multiple times to refund all contributors.
         *
         * The dispatch origin for this call must be _Signed_ and doesn't need to be the creator of the crowdloan.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to refund.
         */
        refund: TxDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * Dissolve a crowdloan.
         *
         * The crowdloan will be removed from the storage.
         * All contributions must have been refunded before the crowdloan can be dissolved (except the creator's one).
         *
         * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to dissolve.
         */
        dissolve: TxDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * Update the minimum contribution of a non-finalized crowdloan.
         *
         * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to update the minimum contribution of.
         * - `new_min_contribution`: The new minimum contribution.
         */
        update_min_contribution: TxDescriptor<Anonymize<I64ev05f6q10es>>;
        /**
         * Update the end block of a non-finalized crowdloan.
         *
         * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to update the end block of.
         * - `new_end`: The new end block.
         */
        update_end: TxDescriptor<Anonymize<Ikc5h15joooak>>;
        /**
         * Update the cap of a non-finalized crowdloan.
         *
         * The dispatch origin for this call must be _Signed_ and must be the creator of the crowdloan.
         *
         * Parameters:
         * - `crowdloan_id`: The id of the crowdloan to update the cap of.
         * - `new_cap`: The new cap.
         */
        update_cap: TxDescriptor<Anonymize<Ie8f436ua5fs59>>;
    };
    Swap: {
        /**
         * Set the fee rate for swaps on a specific subnet (normalized value).
         * For example, 0.3% is approximately 196.
         *
         * Only callable by the admin origin
         */
        set_fee_rate: TxDescriptor<Anonymize<I3mkis681qg30e>>;
        /**
         * Enable user liquidity operations for a specific subnet. This switches the
         * subnet from V2 to V3 swap mode. Thereafter, adding new user liquidity can be disabled
         * by toggling this flag to false, but the swap mode will remain V3 because of existing
         * user liquidity until all users withdraw their liquidity.
         *
         * Only sudo or subnet owner can enable user liquidity.
         * Only sudo can disable user liquidity.
         */
        toggle_user_liquidity: TxDescriptor<Anonymize<I2foqo7cbqf35v>>;
        /**
         * Add liquidity to a specific price range for a subnet.
         *
         * Parameters:
         * - origin: The origin of the transaction
         * - netuid: Subnet ID
         * - tick_low: Lower bound of the price range
         * - tick_high: Upper bound of the price range
         * - liquidity: Amount of liquidity to add
         *
         * Emits `Event::LiquidityAdded` on success
         */
        add_liquidity: TxDescriptor<Anonymize<I3mcu79ge1e54v>>;
        /**
         * Remove liquidity from a specific position.
         *
         * Parameters:
         * - origin: The origin of the transaction
         * - netuid: Subnet ID
         * - position_id: ID of the position to remove
         *
         * Emits `Event::LiquidityRemoved` on success
         */
        remove_liquidity: TxDescriptor<Anonymize<Icf66vuktncksu>>;
        /**
         * Modify a liquidity position.
         *
         * Parameters:
         * - origin: The origin of the transaction
         * - netuid: Subnet ID
         * - position_id: ID of the position to remove
         * - liquidity_delta: Liquidity to add (if positive) or remove (if negative)
         *
         * Emits `Event::LiquidityRemoved` on success
         */
        modify_position: TxDescriptor<Anonymize<Id69glo8rcjef>>;
        /**
         * Disable user liquidity in all subnets.
         *
         * Emits `Event::UserLiquidityToggled` on success
         */
        disable_lp: TxDescriptor<undefined>;
    };
    Contracts: {
        /**
         * Deprecated version if [`Self::call`] for use in an in-storage `Call`.
         */
        call_old_weight: TxDescriptor<Anonymize<Ia2rnh5pfua40a>>;
        /**
         * Deprecated version if [`Self::instantiate_with_code`] for use in an in-storage `Call`.
         */
        instantiate_with_code_old_weight: TxDescriptor<Anonymize<I3otc7e9a35k1k>>;
        /**
         * Deprecated version if [`Self::instantiate`] for use in an in-storage `Call`.
         */
        instantiate_old_weight: TxDescriptor<Anonymize<I89ier5tb9ne0s>>;
        /**
         * Upload new `code` without instantiating a contract from it.
         *
         * If the code does not already exist a deposit is reserved from the caller
         * and unreserved only when [`Self::remove_code`] is called. The size of the reserve
         * depends on the size of the supplied `code`.
         *
         * If the code already exists in storage it will still return `Ok` and upgrades
         * the in storage version to the current
         * [`InstructionWeights::version`](InstructionWeights).
         *
         * - `determinism`: If this is set to any other value but [`Determinism::Enforced`] then
         * the only way to use this code is to delegate call into it from an offchain execution.
         * Set to [`Determinism::Enforced`] if in doubt.
         *
         * # Note
         *
         * Anyone can instantiate a contract from any uploaded code and thus prevent its removal.
         * To avoid this situation a constructor could employ access control so that it can
         * only be instantiated by permissioned entities. The same is true when uploading
         * through [`Self::instantiate_with_code`].
         *
         * Use [`Determinism::Relaxed`] exclusively for non-deterministic code. If the uploaded
         * code is deterministic, specifying [`Determinism::Relaxed`] will be disregarded and
         * result in higher gas costs.
         */
        upload_code: TxDescriptor<Anonymize<Im2f0numhevg3>>;
        /**
         * Remove the code stored under `code_hash` and refund the deposit to its owner.
         *
         * A code can only be removed by its original uploader (its owner) and only if it is
         * not used by any contract.
         */
        remove_code: TxDescriptor<Anonymize<Ib51vk42m1po4n>>;
        /**
         * Privileged function that changes the code of an existing contract.
         *
         * This takes care of updating refcounts and all other necessary operations. Returns
         * an error if either the `code_hash` or `dest` do not exist.
         *
         * # Note
         *
         * This does **not** change the address of the contract in question. This means
         * that the contract address is no longer derived from its code hash after calling
         * this dispatchable.
         */
        set_code: TxDescriptor<Anonymize<I2agkcpojhkk43>>;
        /**
         * Makes a call to an account, optionally transferring some balance.
         *
         * # Parameters
         *
         * * `dest`: Address of the contract to call.
         * * `value`: The balance to transfer from the `origin` to `dest`.
         * * `gas_limit`: The gas limit enforced when executing the constructor.
         * * `storage_deposit_limit`: The maximum amount of balance that can be charged from the
         * caller to pay for the storage consumed.
         * * `data`: The input data to pass to the contract.
         *
         * * If the account is a smart-contract account, the associated code will be
         * executed and any value will be transferred.
         * * If the account is a regular account, any value will be transferred.
         * * If no account exists and the call value is not less than `existential_deposit`,
         * a regular account will be created and any value will be transferred.
         */
        call: TxDescriptor<Anonymize<I32rvg545edabm>>;
        /**
         * Instantiates a new contract from the supplied `code` optionally transferring
         * some balance.
         *
         * This dispatchable has the same effect as calling [`Self::upload_code`] +
         * [`Self::instantiate`]. Bundling them together provides efficiency gains. Please
         * also check the documentation of [`Self::upload_code`].
         *
         * # Parameters
         *
         * * `value`: The balance to transfer from the `origin` to the newly created contract.
         * * `gas_limit`: The gas limit enforced when executing the constructor.
         * * `storage_deposit_limit`: The maximum amount of balance that can be charged/reserved
         * from the caller to pay for the storage consumed.
         * * `code`: The contract code to deploy in raw bytes.
         * * `data`: The input data to pass to the contract constructor.
         * * `salt`: Used for the address derivation. See [`Pallet::contract_address`].
         *
         * Instantiation is executed as follows:
         *
         * - The supplied `code` is deployed, and a `code_hash` is created for that code.
         * - If the `code_hash` already exists on the chain the underlying `code` will be shared.
         * - The destination address is computed based on the sender, code_hash and the salt.
         * - The smart-contract account is created at the computed address.
         * - The `value` is transferred to the new account.
         * - The `deploy` function is executed in the context of the newly-created account.
         */
        instantiate_with_code: TxDescriptor<Anonymize<I83fv0vi59md7i>>;
        /**
         * Instantiates a contract from a previously deployed wasm binary.
         *
         * This function is identical to [`Self::instantiate_with_code`] but without the
         * code deployment step. Instead, the `code_hash` of an on-chain deployed wasm binary
         * must be supplied.
         */
        instantiate: TxDescriptor<Anonymize<I5tjjqcdd4tae0>>;
        /**
         * When a migration is in progress, this dispatchable can be used to run migration steps.
         * Calls that contribute to advancing the migration have their fees waived, as it's helpful
         * for the chain. Note that while the migration is in progress, the pallet will also
         * leverage the `on_idle` hooks to run migration steps.
         */
        migrate: TxDescriptor<Anonymize<I1894dm1lf1ae7>>;
    };
    MevShield: {
        /**
         * Announce the ML‑KEM public key that will become `CurrentKey` in
         * the following block.
         */
        announce_next_key: TxDescriptor<Anonymize<Idkfsqnep2hpeb>>;
        /**
         * Users submit an encrypted wrapper.
         *
         * Client‑side:
         *
         * 1. Read `NextKey` (ML‑KEM public key bytes) from storage.
         * 2. Sign your extrinsic so that it can be executed when added to the pool,
         * i.e. you may need to increment the nonce if you submit using the same account.
         * 3. `commitment = Hashing::hash(signed_extrinsic)`.
         * 4. Encrypt:
         *
         * plaintext = signed_extrinsic
         *
         * with ML‑KEM‑768 + XChaCha20‑Poly1305, producing
         *
         * ciphertext = [u16 kem_len] || kem_ct || nonce24 || aead_ct
         *
         */
        submit_encrypted: TxDescriptor<Anonymize<I2u5b4034ft9hp>>;
        /**
         * Marks a submission as failed to decrypt and removes it from storage.
         *
         * Called by the block author when decryption fails at any stage (e.g., ML-KEM decapsulate
         * failed, AEAD decrypt failed, invalid ciphertext format, etc.). This allows clients to be
         * notified of decryption failures through on-chain events.
         *
         * # Arguments
         *
         * * `id` - The wrapper id (hash of (author, commitment, ciphertext))
         * * `reason` - Human-readable reason for the decryption failure (e.g., "ML-KEM decapsulate failed")
         */
        mark_decryption_failed: TxDescriptor<Anonymize<I602p6mm30elei>>;
    };
};
type IEvent = {
    System: {
        /**
         * An extrinsic completed successfully.
         */
        ExtrinsicSuccess: PlainDescriptor<Anonymize<Ia82mnkmeo2rhc>>;
        /**
         * An extrinsic failed.
         */
        ExtrinsicFailed: PlainDescriptor<Anonymize<I6u3ru0d29kkj0>>;
        /**
         * `:code` was updated.
         */
        CodeUpdated: PlainDescriptor<undefined>;
        /**
         * A new account was created.
         */
        NewAccount: PlainDescriptor<Anonymize<Icbccs0ug47ilf>>;
        /**
         * An account was reaped.
         */
        KilledAccount: PlainDescriptor<Anonymize<Icbccs0ug47ilf>>;
        /**
         * On on-chain remark happened.
         */
        Remarked: PlainDescriptor<Anonymize<I855j4i3kr8ko1>>;
        /**
         * An upgrade was authorized.
         */
        UpgradeAuthorized: PlainDescriptor<Anonymize<Ibgl04rn6nbfm6>>;
        /**
         * An invalid authorized upgrade was rejected while trying to apply it.
         */
        RejectedInvalidAuthorizedUpgrade: PlainDescriptor<Anonymize<Ibk0nulspilods>>;
    };
    Grandpa: {
        /**
         * New authority set has been applied.
         */
        NewAuthorities: PlainDescriptor<Anonymize<I5768ac424h061>>;
        /**
         * Current authority set has been paused.
         */
        Paused: PlainDescriptor<undefined>;
        /**
         * Current authority set has been resumed.
         */
        Resumed: PlainDescriptor<undefined>;
    };
    Balances: {
        /**
         * An account was created with some free balance.
         */
        Endowed: PlainDescriptor<Anonymize<Icv68aq8841478>>;
        /**
         * An account was removed whose balance was non-zero but below ExistentialDeposit,
         * resulting in an outright loss.
         */
        DustLost: PlainDescriptor<Anonymize<Ic262ibdoec56a>>;
        /**
         * Transfer succeeded.
         */
        Transfer: PlainDescriptor<Anonymize<Iflcfm9b6nlmdd>>;
        /**
         * A balance was set by root.
         */
        BalanceSet: PlainDescriptor<Anonymize<Ijrsf4mnp3eka>>;
        /**
         * Some balance was reserved (moved from free to reserved).
         */
        Reserved: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some balance was unreserved (moved from reserved to free).
         */
        Unreserved: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some balance was moved from the reserve of the first account to the second account.
         * Final argument indicates the destination balance type.
         */
        ReserveRepatriated: PlainDescriptor<Anonymize<I8tjvj9uq4b7hi>>;
        /**
         * Some amount was deposited (e.g. for transaction fees).
         */
        Deposit: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some amount was withdrawn from the account (e.g. for transaction fees).
         */
        Withdraw: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some amount was removed from the account (e.g. for misbehavior).
         */
        Slashed: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some amount was minted into an account.
         */
        Minted: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some amount was burned from an account.
         */
        Burned: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some amount was suspended from an account (it can be restored later).
         */
        Suspended: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some amount was restored into an account.
         */
        Restored: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * An account was upgraded.
         */
        Upgraded: PlainDescriptor<Anonymize<I4cbvqmqadhrea>>;
        /**
         * Total issuance was increased by `amount`, creating a credit to be balanced.
         */
        Issued: PlainDescriptor<Anonymize<I3qt1hgg4djhgb>>;
        /**
         * Total issuance was decreased by `amount`, creating a debt to be balanced.
         */
        Rescinded: PlainDescriptor<Anonymize<I3qt1hgg4djhgb>>;
        /**
         * Some balance was locked.
         */
        Locked: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some balance was unlocked.
         */
        Unlocked: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some balance was frozen.
         */
        Frozen: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * Some balance was thawed.
         */
        Thawed: PlainDescriptor<Anonymize<Id5fm4p8lj5qgi>>;
        /**
         * The `TotalIssuance` was forcefully changed.
         */
        TotalIssuanceForced: PlainDescriptor<Anonymize<I4fooe9dun9o0t>>;
    };
    TransactionPayment: {
        /**
         * A transaction fee `actual_fee`, of which `tip` was added to the minimum inclusion fee,
         * has been paid by `who`.
         */
        TransactionFeePaid: PlainDescriptor<Anonymize<Ier2cke86dqbr2>>;
    };
    SubtensorModule: {
        /**
         * a new network is added.
         */
        NetworkAdded: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * a network is removed.
         */
        NetworkRemoved: PlainDescriptor<number>;
        /**
         * stake has been transferred from the a coldkey account onto the hotkey staking account.
         */
        StakeAdded: PlainDescriptor<Anonymize<Io45lnue7n40k>>;
        /**
         * stake has been removed from the hotkey staking account onto the coldkey account.
         */
        StakeRemoved: PlainDescriptor<Anonymize<Io45lnue7n40k>>;
        /**
         * stake has been moved from origin (hotkey, subnet ID) to destination (hotkey, subnet ID) of this amount (in TAO).
         */
        StakeMoved: PlainDescriptor<Anonymize<I83e4tgdv5ohg1>>;
        /**
         * a caller successfully sets their weights on a subnetwork.
         */
        WeightsSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * a new neuron account has been registered to the chain.
         */
        NeuronRegistered: PlainDescriptor<Anonymize<I6o6dmud53u1fj>>;
        /**
         * multiple uids have been concurrently registered.
         */
        BulkNeuronsRegistered: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * FIXME: Not used yet
         */
        BulkBalancesSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * max allowed uids has been set for a subnetwork.
         */
        MaxAllowedUidsSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * DEPRECATED: max weight limit updates are no longer supported.
         */
        MaxWeightLimitSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * the difficulty has been set for a subnet.
         */
        DifficultySet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * the adjustment interval is set for a subnet.
         */
        AdjustmentIntervalSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * registration per interval is set for a subnet.
         */
        RegistrationPerIntervalSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * we set max registrations per block.
         */
        MaxRegistrationsPerBlockSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * an activity cutoff is set for a subnet.
         */
        ActivityCutoffSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * Rho value is set.
         */
        RhoSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * steepness of the sigmoid used to compute alpha values.
         */
        AlphaSigmoidSteepnessSet: PlainDescriptor<Anonymize<I5g2vv0ckl2m8b>>;
        /**
         * Kappa is set for a subnet.
         */
        KappaSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * minimum allowed weight is set for a subnet.
         */
        MinAllowedWeightSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * the validator pruning length has been set.
         */
        ValidatorPruneLenSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * the scaling law power has been set for a subnet.
         */
        ScalingLawPowerSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * weights set rate limit has been set for a subnet.
         */
        WeightsSetRateLimitSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * immunity period is set for a subnet.
         */
        ImmunityPeriodSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * bonds moving average is set for a subnet.
         */
        BondsMovingAverageSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * bonds penalty is set for a subnet.
         */
        BondsPenaltySet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * bonds reset is set for a subnet.
         */
        BondsResetOnSet: PlainDescriptor<Anonymize<I39p6ln31i4n46>>;
        /**
         * setting the max number of allowed validators on a subnet.
         */
        MaxAllowedValidatorsSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * the axon server information is added to the network.
         */
        AxonServed: PlainDescriptor<Anonymize<I7svnfko10tq2e>>;
        /**
         * the prometheus server information is added to the network.
         */
        PrometheusServed: PlainDescriptor<Anonymize<I7svnfko10tq2e>>;
        /**
         * a hotkey has become a delegate.
         */
        DelegateAdded: PlainDescriptor<Anonymize<I7svrbkiu01iec>>;
        /**
         * the default take is set.
         */
        DefaultTakeSet: PlainDescriptor<number>;
        /**
         * weights version key is set for a network.
         */
        WeightsVersionKeySet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting min difficulty on a network.
         */
        MinDifficultySet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting max difficulty on a network.
         */
        MaxDifficultySet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting the prometheus serving rate limit.
         */
        ServingRateLimitSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting burn on a network.
         */
        BurnSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting max burn on a network.
         */
        MaxBurnSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting min burn on a network.
         */
        MinBurnSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * setting the transaction rate limit.
         */
        TxRateLimitSet: PlainDescriptor<bigint>;
        /**
         * setting the delegate take transaction rate limit.
         */
        TxDelegateTakeRateLimitSet: PlainDescriptor<bigint>;
        /**
         * setting the childkey take transaction rate limit.
         */
        TxChildKeyTakeRateLimitSet: PlainDescriptor<bigint>;
        /**
         * setting the admin freeze window length (last N blocks of tempo)
         */
        AdminFreezeWindowSet: PlainDescriptor<number>;
        /**
         * setting the owner hyperparameter rate limit in epochs
         */
        OwnerHyperparamRateLimitSet: PlainDescriptor<number>;
        /**
         * minimum childkey take set
         */
        MinChildKeyTakeSet: PlainDescriptor<number>;
        /**
         * maximum childkey take set
         */
        MaxChildKeyTakeSet: PlainDescriptor<number>;
        /**
         * childkey take set
         */
        ChildKeyTakeSet: PlainDescriptor<Anonymize<I6ouflveob4eli>>;
        /**
         * a sudo call is done.
         */
        Sudid: PlainDescriptor<Anonymize<Ibq6c27da62s2q>>;
        /**
         * registration is allowed/disallowed for a subnet.
         */
        RegistrationAllowed: PlainDescriptor<Anonymize<I39p6ln31i4n46>>;
        /**
         * POW registration is allowed/disallowed for a subnet.
         */
        PowRegistrationAllowed: PlainDescriptor<Anonymize<I39p6ln31i4n46>>;
        /**
         * setting tempo on a network
         */
        TempoSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * setting the RAO recycled for registration.
         */
        RAORecycledForRegistrationSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * min stake is set for validators to set weights.
         */
        StakeThresholdSet: PlainDescriptor<bigint>;
        /**
         * setting the adjustment alpha on a subnet.
         */
        AdjustmentAlphaSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * the faucet it called on the test net.
         */
        Faucet: PlainDescriptor<Anonymize<I95l2k9b1re95f>>;
        /**
         * the subnet owner cut is set.
         */
        SubnetOwnerCutSet: PlainDescriptor<number>;
        /**
         * the network creation rate limit is set.
         */
        NetworkRateLimitSet: PlainDescriptor<bigint>;
        /**
         * the network immunity period is set.
         */
        NetworkImmunityPeriodSet: PlainDescriptor<bigint>;
        /**
         * the start call delay is set.
         */
        StartCallDelaySet: PlainDescriptor<bigint>;
        /**
         * the network minimum locking cost is set.
         */
        NetworkMinLockCostSet: PlainDescriptor<bigint>;
        /**
         * the maximum number of subnets is set
         */
        SubnetLimitSet: PlainDescriptor<number>;
        /**
         * the lock cost reduction is set
         */
        NetworkLockCostReductionIntervalSet: PlainDescriptor<bigint>;
        /**
         * the take for a delegate is decreased.
         */
        TakeDecreased: PlainDescriptor<Anonymize<I7svrbkiu01iec>>;
        /**
         * the take for a delegate is increased.
         */
        TakeIncreased: PlainDescriptor<Anonymize<I7svrbkiu01iec>>;
        /**
         * the hotkey is swapped
         */
        HotkeySwapped: PlainDescriptor<Anonymize<Ifkgc6cte1k96e>>;
        /**
         * maximum delegate take is set by sudo/admin transaction
         */
        MaxDelegateTakeSet: PlainDescriptor<number>;
        /**
         * minimum delegate take is set by sudo/admin transaction
         */
        MinDelegateTakeSet: PlainDescriptor<number>;
        /**
         * A coldkey swap announcement has been made.
         */
        ColdkeySwapAnnounced: PlainDescriptor<Anonymize<I6kvs2mb8unk0t>>;
        /**
         * A coldkey swap has been reset.
         */
        ColdkeySwapReset: PlainDescriptor<Anonymize<I4cbvqmqadhrea>>;
        /**
         * A coldkey has been swapped.
         */
        ColdkeySwapped: PlainDescriptor<Anonymize<Idbuci3sr3i1f7>>;
        /**
         * A coldkey swap has been disputed.
         */
        ColdkeySwapDisputed: PlainDescriptor<Anonymize<I375tmdui1ejfc>>;
        /**
         * All balance of a hotkey has been unstaked and transferred to a new coldkey
         */
        AllBalanceUnstakedAndTransferredToNewColdkey: PlainDescriptor<Anonymize<I73drt1hl9e70v>>;
        /**
         * The arbitration period has been extended
         */
        ArbitrationPeriodExtended: PlainDescriptor<Anonymize<I375tmdui1ejfc>>;
        /**
         * Setting of children of a hotkey have been scheduled
         */
        SetChildrenScheduled: PlainDescriptor<Anonymize<I1dm4sip108q0g>>;
        /**
         * The children of a hotkey have been set
         */
        SetChildren: PlainDescriptor<Anonymize<Iajgphfb1fka7l>>;
        /**
         * The identity of a coldkey has been set
         */
        ChainIdentitySet: PlainDescriptor<SS58String>;
        /**
         * The identity of a subnet has been set
         */
        SubnetIdentitySet: PlainDescriptor<number>;
        /**
         * The identity of a subnet has been removed
         */
        SubnetIdentityRemoved: PlainDescriptor<number>;
        /**
         * A dissolve network extrinsic scheduled.
         */
        DissolveNetworkScheduled: PlainDescriptor<Anonymize<I4hnmf90qkrer9>>;
        /**
         * The coldkey swap announcement delay has been set.
         */
        ColdkeySwapAnnouncementDelaySet: PlainDescriptor<number>;
        /**
         * The coldkey swap reannouncement delay has been set.
         */
        ColdkeySwapReannouncementDelaySet: PlainDescriptor<number>;
        /**
         * The duration of dissolve network has been set
         */
        DissolveNetworkScheduleDurationSet: PlainDescriptor<number>;
        /**
         * Commit-reveal v3 weights have been successfully committed.
         *
         * - **who**: The account ID of the user committing the weights.
         * - **netuid**: The network identifier.
         * - **commit_hash**: The hash representing the committed weights.
         */
        CRV3WeightsCommitted: PlainDescriptor<Anonymize<Ijsohbv0raf36>>;
        /**
         * Weights have been successfully committed.
         *
         * - **who**: The account ID of the user committing the weights.
         * - **netuid**: The network identifier.
         * - **commit_hash**: The hash representing the committed weights.
         */
        WeightsCommitted: PlainDescriptor<Anonymize<Ijsohbv0raf36>>;
        /**
         * Weights have been successfully revealed.
         *
         * - **who**: The account ID of the user revealing the weights.
         * - **netuid**: The network identifier.
         * - **commit_hash**: The hash of the revealed weights.
         */
        WeightsRevealed: PlainDescriptor<Anonymize<Ijsohbv0raf36>>;
        /**
         * Weights have been successfully batch revealed.
         *
         * - **who**: The account ID of the user revealing the weights.
         * - **netuid**: The network identifier.
         * - **revealed_hashes**: A vector of hashes representing each revealed weight set.
         */
        WeightsBatchRevealed: PlainDescriptor<Anonymize<I4ga01hppthoe1>>;
        /**
         * A batch of weights (or commits) have been force-set.
         *
         * - **netuids**: The netuids these weights were successfully set/committed for.
         * - **who**: The hotkey that set this batch.
         */
        BatchWeightsCompleted: PlainDescriptor<Anonymize<I4hckkcv10tcue>>;
        /**
         * A batch extrinsic completed but with some errors.
         */
        BatchCompletedWithErrors: PlainDescriptor<undefined>;
        /**
         * A weight set among a batch of weights failed.
         *
         * - **error**: The dispatch error emitted by the failed item.
         */
        BatchWeightItemFailed: PlainDescriptor<Anonymize<Ic871mj76419vm>>;
        /**
         * Stake has been transferred from one coldkey to another on the same subnet.
         * Parameters:
         * (origin_coldkey, destination_coldkey, hotkey, origin_netuid, destination_netuid, amount)
         */
        StakeTransferred: PlainDescriptor<Anonymize<If2ieedn10ujdv>>;
        /**
         * Stake has been swapped from one subnet to another for the same coldkey-hotkey pair.
         *
         * Parameters:
         * (coldkey, hotkey, origin_netuid, destination_netuid, amount)
         */
        StakeSwapped: PlainDescriptor<Anonymize<Iaseh340tnovdh>>;
        /**
         * Event called when transfer is toggled on a subnet.
         *
         * Parameters:
         * (netuid, bool)
         */
        TransferToggle: PlainDescriptor<Anonymize<I39p6ln31i4n46>>;
        /**
         * The owner hotkey for a subnet has been set.
         *
         * Parameters:
         * (netuid, new_hotkey)
         */
        SubnetOwnerHotkeySet: PlainDescriptor<Anonymize<I7svnfko10tq2e>>;
        /**
         * FirstEmissionBlockNumber is set via start call extrinsic
         *
         * Parameters:
         * netuid
         * block number
         */
        FirstEmissionBlockNumberSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * Alpha has been recycled, reducing AlphaOut on a subnet.
         *
         * Parameters:
         * (coldkey, hotkey, amount, subnet_id)
         */
        AlphaRecycled: PlainDescriptor<Anonymize<I8m5umt6snnmlj>>;
        /**
         * Alpha have been burned without reducing AlphaOut.
         *
         * Parameters:
         * (coldkey, hotkey, amount, subnet_id)
         */
        AlphaBurned: PlainDescriptor<Anonymize<I8m5umt6snnmlj>>;
        /**
         * An EVM key has been associated with a hotkey.
         */
        EvmKeyAssociated: PlainDescriptor<Anonymize<I5aeg4u9kpsp8o>>;
        /**
         * CRV3 Weights have been successfully revealed.
         *
         * - **netuid**: The network identifier.
         * - **who**: The account ID of the user revealing the weights.
         */
        CRV3WeightsRevealed: PlainDescriptor<Anonymize<I7svnfko10tq2e>>;
        /**
         * Commit-Reveal periods has been successfully set.
         *
         * - **netuid**: The network identifier.
         * - **periods**: The number of epochs before the reveal.
         */
        CommitRevealPeriodsSet: PlainDescriptor<Anonymize<I4ojmnsk1dchql>>;
        /**
         * Commit-Reveal has been successfully toggled.
         *
         * - **netuid**: The network identifier.
         * - **Enabled**: Is Commit-Reveal enabled.
         */
        CommitRevealEnabled: PlainDescriptor<Anonymize<I39p6ln31i4n46>>;
        /**
         * the hotkey is swapped
         */
        HotkeySwappedOnSubnet: PlainDescriptor<Anonymize<I3fsv5f1boeqf3>>;
        /**
         * A subnet lease has been created.
         */
        SubnetLeaseCreated: PlainDescriptor<Anonymize<Ifoov68qt28nbm>>;
        /**
         * A subnet lease has been terminated.
         */
        SubnetLeaseTerminated: PlainDescriptor<Anonymize<Ib937mhlbop6j7>>;
        /**
         * The symbol for a subnet has been updated.
         */
        SymbolUpdated: PlainDescriptor<Anonymize<I62rrikn5vj0p5>>;
        /**
         * Commit Reveal Weights version has been updated.
         *
         * - **version**: The required version.
         */
        CommitRevealVersionSet: PlainDescriptor<number>;
        /**
         * Timelocked weights have been successfully committed.
         *
         * - **who**: The account ID of the user committing the weights.
         * - **netuid**: The network identifier.
         * - **commit_hash**: The hash representing the committed weights.
         * - **reveal_round**: The round at which weights can be revealed.
         */
        TimelockedWeightsCommitted: PlainDescriptor<Anonymize<I838gqvljm75tj>>;
        /**
         * Timelocked Weights have been successfully revealed.
         *
         * - **netuid**: The network identifier.
         * - **who**: The account ID of the user revealing the weights.
         */
        TimelockedWeightsRevealed: PlainDescriptor<Anonymize<I7svnfko10tq2e>>;
        /**
         * Auto-staking hotkey received stake
         */
        AutoStakeAdded: PlainDescriptor<Anonymize<I1cu36qostj5d8>>;
        /**
         * End-of-epoch miner incentive alpha by UID
         */
        IncentiveAlphaEmittedToMiners: PlainDescriptor<Anonymize<I4r2ptfsrl017r>>;
        /**
         * The minimum allowed UIDs for a subnet have been set.
         */
        MinAllowedUidsSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * The auto stake destination has been set.
         *
         * - **coldkey**: The account ID of the coldkey.
         * - **netuid**: The network identifier.
         * - **hotkey**: The account ID of the hotkey.
         */
        AutoStakeDestinationSet: PlainDescriptor<Anonymize<Ielglukq9ekcit>>;
        /**
         * The minimum allowed non-Immune UIDs has been set.
         */
        MinNonImmuneUidsSet: PlainDescriptor<Anonymize<I9jd27rnpm8ttv>>;
        /**
         * Root emissions have been claimed for a coldkey on all subnets and hotkeys.
         * Parameters:
         * (coldkey)
         */
        RootClaimed: PlainDescriptor<Anonymize<I375tmdui1ejfc>>;
        /**
         * Root claim type for a coldkey has been set.
         * Parameters:
         * (coldkey, u8)
         */
        RootClaimTypeSet: PlainDescriptor<Anonymize<I1clsdhcok4nle>>;
        /**
         * Voting power tracking has been enabled for a subnet.
         */
        VotingPowerTrackingEnabled: PlainDescriptor<Anonymize<I6cm4c5a1euio9>>;
        /**
         * Voting power tracking has been scheduled for disabling.
         * Tracking will continue until disable_at_block, then stop and clear entries.
         */
        VotingPowerTrackingDisableScheduled: PlainDescriptor<Anonymize<Iemddv6u2buvfn>>;
        /**
         * Voting power tracking has been fully disabled and entries cleared.
         */
        VotingPowerTrackingDisabled: PlainDescriptor<Anonymize<I6cm4c5a1euio9>>;
        /**
         * Voting power EMA alpha has been set for a subnet.
         */
        VotingPowerEmaAlphaSet: PlainDescriptor<Anonymize<I4guv8rii4s6je>>;
        /**
         * Subnet lease dividends have been distributed.
         */
        SubnetLeaseDividendsDistributed: PlainDescriptor<Anonymize<Ic149bnrif7lpr>>;
        /**
         * "Add stake and burn" event: alpha token was purchased and burned.
         */
        AddStakeBurn: PlainDescriptor<Anonymize<I89dsvf7sdo4ko>>;
    };
    Utility: {
        /**
         * Batch of dispatches did not complete fully. Index of first failing dispatch given, as
         * well as the error.
         */
        BatchInterrupted: PlainDescriptor<Anonymize<I804q3c12638a0>>;
        /**
         * Batch of dispatches completed fully with no error.
         */
        BatchCompleted: PlainDescriptor<undefined>;
        /**
         * Batch of dispatches completed but has errors.
         */
        BatchCompletedWithErrors: PlainDescriptor<undefined>;
        /**
         * A single item within a Batch of dispatches has completed with no error.
         */
        ItemCompleted: PlainDescriptor<undefined>;
        /**
         * A single item within a Batch of dispatches has completed with error.
         */
        ItemFailed: PlainDescriptor<Anonymize<Idguve298jnare>>;
        /**
         * A call was dispatched.
         */
        DispatchedAs: PlainDescriptor<Anonymize<Idi3fb8585u2lp>>;
        /**
         * Main call was dispatched.
         */
        IfElseMainSuccess: PlainDescriptor<undefined>;
        /**
         * The fallback call was dispatched.
         */
        IfElseFallbackCalled: PlainDescriptor<Anonymize<I1327b77famnt3>>;
    };
    Sudo: {
        /**
         * A sudo call just took place.
         */
        Sudid: PlainDescriptor<Anonymize<If58ibsptjm2at>>;
        /**
         * The sudo key has been updated.
         */
        KeyChanged: PlainDescriptor<Anonymize<I5rtkmhm2dng4u>>;
        /**
         * The key was permanently removed.
         */
        KeyRemoved: PlainDescriptor<undefined>;
        /**
         * A [sudo_as](Pallet::sudo_as) call just took place.
         */
        SudoAsDone: PlainDescriptor<Anonymize<If58ibsptjm2at>>;
    };
    Multisig: {
        /**
         * A new multisig operation has begun.
         */
        NewMultisig: PlainDescriptor<Anonymize<Iep27ialq4a7o7>>;
        /**
         * A multisig operation has been approved by someone.
         */
        MultisigApproval: PlainDescriptor<Anonymize<Iasu5jvoqr43mv>>;
        /**
         * A multisig operation has been executed.
         */
        MultisigExecuted: PlainDescriptor<Anonymize<I88p4dmln8611r>>;
        /**
         * A multisig operation has been cancelled.
         */
        MultisigCancelled: PlainDescriptor<Anonymize<I5qolde99acmd1>>;
        /**
         * The deposit for a multisig operation has been updated/poked.
         */
        DepositPoked: PlainDescriptor<Anonymize<I8gtde5abn1g9a>>;
    };
    Preimage: {
        /**
         * A preimage has been noted.
         */
        Noted: PlainDescriptor<Anonymize<I1jm8m1rh9e20v>>;
        /**
         * A preimage has been requested.
         */
        Requested: PlainDescriptor<Anonymize<I1jm8m1rh9e20v>>;
        /**
         * A preimage has ben cleared.
         */
        Cleared: PlainDescriptor<Anonymize<I1jm8m1rh9e20v>>;
    };
    Scheduler: {
        /**
         * Scheduled some task.
         */
        Scheduled: PlainDescriptor<Anonymize<I5n4sebgkfr760>>;
        /**
         * Canceled some task.
         */
        Canceled: PlainDescriptor<Anonymize<I5n4sebgkfr760>>;
        /**
         * Dispatched some task.
         */
        Dispatched: PlainDescriptor<Anonymize<I3dvon8akhmsut>>;
        /**
         * Set a retry configuration for some task.
         */
        RetrySet: PlainDescriptor<Anonymize<Ia3c82eadg79bj>>;
        /**
         * Cancel a retry configuration for some task.
         */
        RetryCancelled: PlainDescriptor<Anonymize<Ienusoeb625ftq>>;
        /**
         * The call for the provided hash was not found so the task has been aborted.
         */
        CallUnavailable: PlainDescriptor<Anonymize<Ienusoeb625ftq>>;
        /**
         * The given task was unable to be renewed since the agenda is full at that block.
         */
        PeriodicFailed: PlainDescriptor<Anonymize<Ienusoeb625ftq>>;
        /**
         * The given task was unable to be retried since the agenda is full at that block or there
         * was not enough weight to reschedule it.
         */
        RetryFailed: PlainDescriptor<Anonymize<Ienusoeb625ftq>>;
        /**
         * The given task can never be executed since it is overweight.
         */
        PermanentlyOverweight: PlainDescriptor<Anonymize<Ienusoeb625ftq>>;
        /**
         * Agenda is incomplete from `when`.
         */
        AgendaIncomplete: PlainDescriptor<Anonymize<Ibtsa3docbr9el>>;
    };
    Proxy: {
        /**
         * A proxy was executed correctly, with the given.
         */
        ProxyExecuted: PlainDescriptor<Anonymize<Idi3fb8585u2lp>>;
        /**
         * A pure account has been created by new proxy with given
         * disambiguation index and proxy type.
         */
        PureCreated: PlainDescriptor<Anonymize<Iek6442ldi23n3>>;
        /**
         * A pure proxy was killed by its spawner.
         */
        PureKilled: PlainDescriptor<Anonymize<Idpdo54rotesu2>>;
        /**
         * An announcement was placed to make a call in the future.
         */
        Announced: PlainDescriptor<Anonymize<I2ur0oeqg495j8>>;
        /**
         * A proxy was added.
         */
        ProxyAdded: PlainDescriptor<Anonymize<Ibco2bqthggul0>>;
        /**
         * A proxy was removed.
         */
        ProxyRemoved: PlainDescriptor<Anonymize<Ibco2bqthggul0>>;
        /**
         * A deposit stored for proxies or announcements was poked / updated.
         */
        DepositPoked: PlainDescriptor<Anonymize<I1bhd210c3phjj>>;
    };
    Registry: {
        /**
         * Emitted when a user registers an identity
         */
        IdentitySet: PlainDescriptor<Anonymize<I4cbvqmqadhrea>>;
        /**
         * Emitted when a user dissolves an identity
         */
        IdentityDissolved: PlainDescriptor<Anonymize<I4cbvqmqadhrea>>;
    };
    Commitments: {
        /**
         * A commitment was set
         */
        Commitment: PlainDescriptor<Anonymize<Idcqgi2844k5he>>;
        /**
         * A timelock-encrypted commitment was set
         */
        TimelockCommitment: PlainDescriptor<Anonymize<Iej2173ou338sm>>;
        /**
         * A timelock-encrypted commitment was auto-revealed
         */
        CommitmentRevealed: PlainDescriptor<Anonymize<Idcqgi2844k5he>>;
    };
    AdminUtils: {
        /**
         * Event emitted when a precompile operation is updated.
         */
        PrecompileUpdated: PlainDescriptor<Anonymize<I1sj8huj7of8mb>>;
        /**
         * Event emitted when the Yuma3 enable is toggled.
         */
        Yuma3EnableToggled: PlainDescriptor<Anonymize<Ie31ro5s5e089f>>;
        /**
         * Event emitted when Bonds Reset is toggled.
         */
        BondsResetToggled: PlainDescriptor<Anonymize<Ie31ro5s5e089f>>;
    };
    SafeMode: {
        /**
         * The safe-mode was entered until inclusively this block.
         */
        Entered: PlainDescriptor<Anonymize<I20e9ph536u7ti>>;
        /**
         * The safe-mode was extended until inclusively this block.
         */
        Extended: PlainDescriptor<Anonymize<I20e9ph536u7ti>>;
        /**
         * Exited the safe-mode for a specific reason.
         */
        Exited: PlainDescriptor<Anonymize<I8kcpmsh450rp>>;
        /**
         * An account reserved funds for either entering or extending the safe-mode.
         */
        DepositPlaced: PlainDescriptor<Anonymize<Ic262ibdoec56a>>;
        /**
         * An account had a reserve released that was reserved.
         */
        DepositReleased: PlainDescriptor<Anonymize<Ic262ibdoec56a>>;
        /**
         * An account had reserve slashed that was reserved.
         */
        DepositSlashed: PlainDescriptor<Anonymize<Ic262ibdoec56a>>;
        /**
         * Could not hold funds for entering or extending the safe-mode.
         *
         * This error comes from the underlying `Currency`.
         */
        CannotDeposit: PlainDescriptor<undefined>;
        /**
         * Could not release funds for entering or extending the safe-mode.
         *
         * This error comes from the underlying `Currency`.
         */
        CannotRelease: PlainDescriptor<undefined>;
    };
    Ethereum: {
        /**
         * An ethereum transaction was successfully executed.
         */
        Executed: PlainDescriptor<Anonymize<Iea4g5ovhnolus>>;
    };
    EVM: {
        /**
         * Ethereum events from contracts.
         */
        Log: PlainDescriptor<Anonymize<Ifmc9boeeia623>>;
        /**
         * A contract has been created at given address.
         */
        Created: PlainDescriptor<Anonymize<Itmchvgqfl28g>>;
        /**
         * A contract was attempted to be created, but the execution failed.
         */
        CreatedFailed: PlainDescriptor<Anonymize<Itmchvgqfl28g>>;
        /**
         * A contract has been executed successfully with states applied.
         */
        Executed: PlainDescriptor<Anonymize<Itmchvgqfl28g>>;
        /**
         * A contract has been executed with errors. States are reverted with only gas fees applied.
         */
        ExecutedFailed: PlainDescriptor<Anonymize<Itmchvgqfl28g>>;
    };
    BaseFee: {
        /**
        
         */
        NewBaseFeePerGas: PlainDescriptor<Anonymize<I7vi74gbubc8u5>>;
        /**
        
         */
        BaseFeeOverflow: PlainDescriptor<undefined>;
        /**
        
         */
        NewElasticity: PlainDescriptor<Anonymize<I3u0knmtb1ueq7>>;
    };
    Drand: {
        /**
         * Beacon Configuration has changed.
         */
        BeaconConfigChanged: PlainDescriptor<undefined>;
        /**
         * Successfully set a new pulse(s).
         */
        NewPulse: PlainDescriptor<Anonymize<I5tf7b5o64mfpl>>;
        /**
         * Oldest Stored Round has been set.
         */
        SetOldestStoredRound: PlainDescriptor<bigint>;
    };
    Crowdloan: {
        /**
         * A crowdloan was created.
         */
        Created: PlainDescriptor<Anonymize<If71d2q730qf6n>>;
        /**
         * A contribution was made to an active crowdloan.
         */
        Contributed: PlainDescriptor<Anonymize<If0sk51c1n7ri8>>;
        /**
         * A contribution was withdrawn from a failed crowdloan.
         */
        Withdrew: PlainDescriptor<Anonymize<If0sk51c1n7ri8>>;
        /**
         * A refund was partially processed for a failed crowdloan.
         */
        PartiallyRefunded: PlainDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * A refund was fully processed for a failed crowdloan.
         */
        AllRefunded: PlainDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * A crowdloan was finalized, funds were transferred and the call was dispatched.
         */
        Finalized: PlainDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * A crowdloan was dissolved.
         */
        Dissolved: PlainDescriptor<Anonymize<I5dueehi6i2dg9>>;
        /**
         * The minimum contribution was updated.
         */
        MinContributionUpdated: PlainDescriptor<Anonymize<I64ev05f6q10es>>;
        /**
         * The end was updated.
         */
        EndUpdated: PlainDescriptor<Anonymize<Ikc5h15joooak>>;
        /**
         * The cap was updated.
         */
        CapUpdated: PlainDescriptor<Anonymize<Ie8f436ua5fs59>>;
    };
    Swap: {
        /**
         * Event emitted when the fee rate has been updated for a subnet
         */
        FeeRateSet: PlainDescriptor<Anonymize<I3mkis681qg30e>>;
        /**
         * Event emitted when user liquidity operations are enabled for a subnet.
         * First enable even indicates a switch from V2 to V3 swap.
         */
        UserLiquidityToggled: PlainDescriptor<Anonymize<I2foqo7cbqf35v>>;
        /**
         * Event emitted when a liquidity position is added to a subnet's liquidity pool.
         */
        LiquidityAdded: PlainDescriptor<Anonymize<I4b2eh3b1oi815>>;
        /**
         * Event emitted when a liquidity position is removed from a subnet's liquidity pool.
         */
        LiquidityRemoved: PlainDescriptor<Anonymize<I57q620f4fu1bl>>;
        /**
         * Event emitted when a liquidity position is modified in a subnet's liquidity pool.
         * Modifying causes the fees to be claimed.
         */
        LiquidityModified: PlainDescriptor<Anonymize<I57q620f4fu1bl>>;
    };
    Contracts: {
        /**
         * Contract deployed by address at the specified address.
         */
        Instantiated: PlainDescriptor<Anonymize<Ie5222qfrr24ek>>;
        /**
         * Contract has been removed.
         *
         * # Note
         *
         * The only way for a contract to be removed and emitting this event is by calling
         * `seal_terminate`.
         */
        Terminated: PlainDescriptor<Anonymize<I28g8sphdu312k>>;
        /**
         * Code with the specified hash has been stored.
         */
        CodeStored: PlainDescriptor<Anonymize<Idqbjt2c6r46t6>>;
        /**
         * A custom event emitted by the contract.
         */
        ContractEmitted: PlainDescriptor<Anonymize<I853aigjva3f0t>>;
        /**
         * A code with the specified hash was removed.
         */
        CodeRemoved: PlainDescriptor<Anonymize<I9uehhems5hkqm>>;
        /**
         * A contract's code was updated.
         */
        ContractCodeUpdated: PlainDescriptor<Anonymize<I7q5qk4uoanhof>>;
        /**
         * A contract was called either by a plain account or another contract.
         *
         * # Note
         *
         * Please keep in mind that like all events this is only emitted for successful
         * calls. This is because on failure all storage changes including events are
         * rolled back.
         */
        Called: PlainDescriptor<Anonymize<Iehpbs40l3jkit>>;
        /**
         * A contract delegate called a code hash.
         *
         * # Note
         *
         * Please keep in mind that like all events this is only emitted for successful
         * calls. This is because on failure all storage changes including events are
         * rolled back.
         */
        DelegateCalled: PlainDescriptor<Anonymize<Idht9upmipvd4j>>;
        /**
         * Some funds have been transferred and held as storage deposit.
         */
        StorageDepositTransferredAndHeld: PlainDescriptor<Anonymize<Iflcfm9b6nlmdd>>;
        /**
         * Some storage deposit funds have been transferred and released.
         */
        StorageDepositTransferredAndReleased: PlainDescriptor<Anonymize<Iflcfm9b6nlmdd>>;
    };
    MevShield: {
        /**
         * Encrypted wrapper accepted.
         */
        EncryptedSubmitted: PlainDescriptor<Anonymize<Icns2sqr5hp8s3>>;
        /**
         * Decrypted call executed.
         */
        DecryptedExecuted: PlainDescriptor<Anonymize<I9n4hs8p3rlkag>>;
        /**
         * Decrypted execution rejected.
         */
        DecryptedRejected: PlainDescriptor<Anonymize<I6a8j73186lfdf>>;
        /**
         * Decryption failed - validator could not decrypt the submission.
         */
        DecryptionFailed: PlainDescriptor<Anonymize<I602p6mm30elei>>;
    };
};
type IError = {
    System: {
        /**
         * The name of specification does not match between the current runtime
         * and the new runtime.
         */
        InvalidSpecName: PlainDescriptor<undefined>;
        /**
         * The specification version is not allowed to decrease between the current runtime
         * and the new runtime.
         */
        SpecVersionNeedsToIncrease: PlainDescriptor<undefined>;
        /**
         * Failed to extract the runtime version from the new runtime.
         *
         * Either calling `Core_version` or decoding `RuntimeVersion` failed.
         */
        FailedToExtractRuntimeVersion: PlainDescriptor<undefined>;
        /**
         * Suicide called when the account has non-default composite data.
         */
        NonDefaultComposite: PlainDescriptor<undefined>;
        /**
         * There is a non-zero reference count preventing the account from being purged.
         */
        NonZeroRefCount: PlainDescriptor<undefined>;
        /**
         * The origin filter prevent the call to be dispatched.
         */
        CallFiltered: PlainDescriptor<undefined>;
        /**
         * A multi-block migration is ongoing and prevents the current code from being replaced.
         */
        MultiBlockMigrationsOngoing: PlainDescriptor<undefined>;
        /**
         * No upgrade authorized.
         */
        NothingAuthorized: PlainDescriptor<undefined>;
        /**
         * The submitted code is not authorized.
         */
        Unauthorized: PlainDescriptor<undefined>;
    };
    Grandpa: {
        /**
         * Attempt to signal GRANDPA pause when the authority set isn't live
         * (either paused or already pending pause).
         */
        PauseFailed: PlainDescriptor<undefined>;
        /**
         * Attempt to signal GRANDPA resume when the authority set isn't paused
         * (either live or already pending resume).
         */
        ResumeFailed: PlainDescriptor<undefined>;
        /**
         * Attempt to signal GRANDPA change with one already pending.
         */
        ChangePending: PlainDescriptor<undefined>;
        /**
         * Cannot signal forced change so soon after last.
         */
        TooSoon: PlainDescriptor<undefined>;
        /**
         * A key ownership proof provided as part of an equivocation report is invalid.
         */
        InvalidKeyOwnershipProof: PlainDescriptor<undefined>;
        /**
         * An equivocation proof provided as part of an equivocation report is invalid.
         */
        InvalidEquivocationProof: PlainDescriptor<undefined>;
        /**
         * A given equivocation report is valid but already previously reported.
         */
        DuplicateOffenceReport: PlainDescriptor<undefined>;
    };
    Balances: {
        /**
         * Vesting balance too high to send value.
         */
        VestingBalance: PlainDescriptor<undefined>;
        /**
         * Account liquidity restrictions prevent withdrawal.
         */
        LiquidityRestrictions: PlainDescriptor<undefined>;
        /**
         * Balance too low to send value.
         */
        InsufficientBalance: PlainDescriptor<undefined>;
        /**
         * Value too low to create account due to existential deposit.
         */
        ExistentialDeposit: PlainDescriptor<undefined>;
        /**
         * Transfer/payment would kill account.
         */
        Expendability: PlainDescriptor<undefined>;
        /**
         * A vesting schedule already exists for this account.
         */
        ExistingVestingSchedule: PlainDescriptor<undefined>;
        /**
         * Beneficiary account must pre-exist.
         */
        DeadAccount: PlainDescriptor<undefined>;
        /**
         * Number of named reserves exceed `MaxReserves`.
         */
        TooManyReserves: PlainDescriptor<undefined>;
        /**
         * Number of holds exceed `VariantCountOf<T::RuntimeHoldReason>`.
         */
        TooManyHolds: PlainDescriptor<undefined>;
        /**
         * Number of freezes exceed `MaxFreezes`.
         */
        TooManyFreezes: PlainDescriptor<undefined>;
        /**
         * The issuance cannot be modified since it is already deactivated.
         */
        IssuanceDeactivated: PlainDescriptor<undefined>;
        /**
         * The delta cannot be zero.
         */
        DeltaZero: PlainDescriptor<undefined>;
    };
    SubtensorModule: {
        /**
         * The root network does not exist.
         */
        RootNetworkDoesNotExist: PlainDescriptor<undefined>;
        /**
         * The user is trying to serve an axon which is not of type 4 (IPv4) or 6 (IPv6).
         */
        InvalidIpType: PlainDescriptor<undefined>;
        /**
         * An invalid IP address is passed to the serve function.
         */
        InvalidIpAddress: PlainDescriptor<undefined>;
        /**
         * An invalid port is passed to the serve function.
         */
        InvalidPort: PlainDescriptor<undefined>;
        /**
         * The hotkey is not registered in subnet
         */
        HotKeyNotRegisteredInSubNet: PlainDescriptor<undefined>;
        /**
         * The hotkey does not exists
         */
        HotKeyAccountNotExists: PlainDescriptor<undefined>;
        /**
         * The hotkey is not registered in any subnet.
         */
        HotKeyNotRegisteredInNetwork: PlainDescriptor<undefined>;
        /**
         * Request to stake, unstake or subscribe is made by a coldkey that is not associated with
         * the hotkey account.
         */
        NonAssociatedColdKey: PlainDescriptor<undefined>;
        /**
         * DEPRECATED: Stake amount to withdraw is zero.
         * The caller does not have enought stake to perform this action.
         */
        NotEnoughStake: PlainDescriptor<undefined>;
        /**
         * The caller is requesting removing more stake than there exists in the staking account.
         * See: "[remove_stake()]".
         */
        NotEnoughStakeToWithdraw: PlainDescriptor<undefined>;
        /**
         * The caller is requesting to set weights but the caller has less than minimum stake
         * required to set weights (less than WeightsMinStake).
         */
        NotEnoughStakeToSetWeights: PlainDescriptor<undefined>;
        /**
         * The parent hotkey doesn't have enough own stake to set childkeys.
         */
        NotEnoughStakeToSetChildkeys: PlainDescriptor<undefined>;
        /**
         * The caller is requesting adding more stake than there exists in the coldkey account.
         * See: "[add_stake()]"
         */
        NotEnoughBalanceToStake: PlainDescriptor<undefined>;
        /**
         * The caller is trying to add stake, but for some reason the requested amount could not be
         * withdrawn from the coldkey account.
         */
        BalanceWithdrawalError: PlainDescriptor<undefined>;
        /**
         * Unsuccessfully withdraw, balance could be zero (can not make account exist) after
         * withdrawal.
         */
        ZeroBalanceAfterWithdrawn: PlainDescriptor<undefined>;
        /**
         * The caller is attempting to set non-self weights without being a permitted validator.
         */
        NeuronNoValidatorPermit: PlainDescriptor<undefined>;
        /**
         * The caller is attempting to set the weight keys and values but these vectors have
         * different size.
         */
        WeightVecNotEqualSize: PlainDescriptor<undefined>;
        /**
         * The caller is attempting to set weights with duplicate UIDs in the weight matrix.
         */
        DuplicateUids: PlainDescriptor<undefined>;
        /**
         * The caller is attempting to set weight to at least one UID that does not exist in the
         * metagraph.
         */
        UidVecContainInvalidOne: PlainDescriptor<undefined>;
        /**
         * The dispatch is attempting to set weights on chain with fewer elements than are allowed.
         */
        WeightVecLengthIsLow: PlainDescriptor<undefined>;
        /**
         * Number of registrations in this block exceeds the allowed number (i.e., exceeds the
         * subnet hyperparameter "max_regs_per_block").
         */
        TooManyRegistrationsThisBlock: PlainDescriptor<undefined>;
        /**
         * The caller is requesting registering a neuron which already exists in the active set.
         */
        HotKeyAlreadyRegisteredInSubNet: PlainDescriptor<undefined>;
        /**
         * The new hotkey is the same as old one
         */
        NewHotKeyIsSameWithOld: PlainDescriptor<undefined>;
        /**
         * The supplied PoW hash block is in the future or negative.
         */
        InvalidWorkBlock: PlainDescriptor<undefined>;
        /**
         * The supplied PoW hash block does not meet the network difficulty.
         */
        InvalidDifficulty: PlainDescriptor<undefined>;
        /**
         * The supplied PoW hash seal does not match the supplied work.
         */
        InvalidSeal: PlainDescriptor<undefined>;
        /**
         * The dispatch is attempting to set weights on chain with weight value exceeding the
         * configured max weight limit (currently `u16::MAX`).
         */
        MaxWeightExceeded: PlainDescriptor<undefined>;
        /**
         * The hotkey is attempting to become a delegate when the hotkey is already a delegate.
         */
        HotKeyAlreadyDelegate: PlainDescriptor<undefined>;
        /**
         * A transactor exceeded the rate limit for setting weights.
         */
        SettingWeightsTooFast: PlainDescriptor<undefined>;
        /**
         * A validator is attempting to set weights from a validator with incorrect weight version.
         */
        IncorrectWeightVersionKey: PlainDescriptor<undefined>;
        /**
         * An axon or prometheus serving exceeded the rate limit for a registered neuron.
         */
        ServingRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * The caller is attempting to set weights with more UIDs than allowed.
         */
        UidsLengthExceedUidsInSubNet: PlainDescriptor<undefined>;
        /**
         * A transactor exceeded the rate limit for add network transaction.
         */
        NetworkTxRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * A transactor exceeded the rate limit for delegate transaction.
         */
        DelegateTxRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * A transactor exceeded the rate limit for setting or swapping hotkey.
         */
        HotKeySetTxRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * A transactor exceeded the rate limit for staking.
         */
        StakingRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * Registration is disabled.
         */
        SubNetRegistrationDisabled: PlainDescriptor<undefined>;
        /**
         * The number of registration attempts exceeded the allowed number in the interval.
         */
        TooManyRegistrationsThisInterval: PlainDescriptor<undefined>;
        /**
         * The hotkey is required to be the origin.
         */
        TransactorAccountShouldBeHotKey: PlainDescriptor<undefined>;
        /**
         * Faucet is disabled.
         */
        FaucetDisabled: PlainDescriptor<undefined>;
        /**
         * Not a subnet owner.
         */
        NotSubnetOwner: PlainDescriptor<undefined>;
        /**
         * Operation is not permitted on the root subnet.
         */
        RegistrationNotPermittedOnRootSubnet: PlainDescriptor<undefined>;
        /**
         * A hotkey with too little stake is attempting to join the root subnet.
         */
        StakeTooLowForRoot: PlainDescriptor<undefined>;
        /**
         * All subnets are in the immunity period.
         */
        AllNetworksInImmunity: PlainDescriptor<undefined>;
        /**
         * Not enough balance to pay swapping hotkey.
         */
        NotEnoughBalanceToPaySwapHotKey: PlainDescriptor<undefined>;
        /**
         * Netuid does not match for setting root network weights.
         */
        NotRootSubnet: PlainDescriptor<undefined>;
        /**
         * Can not set weights for the root network.
         */
        CanNotSetRootNetworkWeights: PlainDescriptor<undefined>;
        /**
         * No neuron ID is available.
         */
        NoNeuronIdAvailable: PlainDescriptor<undefined>;
        /**
         * Delegate take is too low.
         */
        DelegateTakeTooLow: PlainDescriptor<undefined>;
        /**
         * Delegate take is too high.
         */
        DelegateTakeTooHigh: PlainDescriptor<undefined>;
        /**
         * No commit found for the provided hotkey+netuid combination when attempting to reveal the
         * weights.
         */
        NoWeightsCommitFound: PlainDescriptor<undefined>;
        /**
         * Committed hash does not equal the hashed reveal data.
         */
        InvalidRevealCommitHashNotMatch: PlainDescriptor<undefined>;
        /**
         * Attempting to call set_weights when commit/reveal is enabled
         */
        CommitRevealEnabled: PlainDescriptor<undefined>;
        /**
         * Attemtping to commit/reveal weights when disabled.
         */
        CommitRevealDisabled: PlainDescriptor<undefined>;
        /**
         * Attempting to set alpha high/low while disabled
         */
        LiquidAlphaDisabled: PlainDescriptor<undefined>;
        /**
         * Alpha high is too low: alpha_high > 0.8
         */
        AlphaHighTooLow: PlainDescriptor<undefined>;
        /**
         * Alpha low is out of range: alpha_low > 0 && alpha_low < 0.8
         */
        AlphaLowOutOfRange: PlainDescriptor<undefined>;
        /**
         * The coldkey has already been swapped
         */
        ColdKeyAlreadyAssociated: PlainDescriptor<undefined>;
        /**
         * The coldkey balance is not enough to pay for the swap
         */
        NotEnoughBalanceToPaySwapColdKey: PlainDescriptor<undefined>;
        /**
         * Attempting to set an invalid child for a hotkey on a network.
         */
        InvalidChild: PlainDescriptor<undefined>;
        /**
         * Duplicate child when setting children.
         */
        DuplicateChild: PlainDescriptor<undefined>;
        /**
         * Proportion overflow when setting children.
         */
        ProportionOverflow: PlainDescriptor<undefined>;
        /**
         * Too many children MAX 5.
         */
        TooManyChildren: PlainDescriptor<undefined>;
        /**
         * Default transaction rate limit exceeded.
         */
        TxRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * Coldkey swap announcement not found
         */
        ColdkeySwapAnnouncementNotFound: PlainDescriptor<undefined>;
        /**
         * Coldkey swap too early.
         */
        ColdkeySwapTooEarly: PlainDescriptor<undefined>;
        /**
         * Coldkey swap reannounced too early.
         */
        ColdkeySwapReannouncedTooEarly: PlainDescriptor<undefined>;
        /**
         * The announced coldkey hash does not match the new coldkey hash.
         */
        AnnouncedColdkeyHashDoesNotMatch: PlainDescriptor<undefined>;
        /**
         * Coldkey swap already disputed
         */
        ColdkeySwapAlreadyDisputed: PlainDescriptor<undefined>;
        /**
         * New coldkey is hotkey
         */
        NewColdKeyIsHotkey: PlainDescriptor<undefined>;
        /**
         * Childkey take is invalid.
         */
        InvalidChildkeyTake: PlainDescriptor<undefined>;
        /**
         * Childkey take rate limit exceeded.
         */
        TxChildkeyTakeRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * Invalid identity.
         */
        InvalidIdentity: PlainDescriptor<undefined>;
        /**
         * Subnet mechanism does not exist.
         */
        MechanismDoesNotExist: PlainDescriptor<undefined>;
        /**
         * Trying to unstake your lock amount.
         */
        CannotUnstakeLock: PlainDescriptor<undefined>;
        /**
         * Trying to perform action on non-existent subnet.
         */
        SubnetNotExists: PlainDescriptor<undefined>;
        /**
         * Maximum commit limit reached
         */
        TooManyUnrevealedCommits: PlainDescriptor<undefined>;
        /**
         * Attempted to reveal weights that are expired.
         */
        ExpiredWeightCommit: PlainDescriptor<undefined>;
        /**
         * Attempted to reveal weights too early.
         */
        RevealTooEarly: PlainDescriptor<undefined>;
        /**
         * Attempted to batch reveal weights with mismatched vector input lenghts.
         */
        InputLengthsUnequal: PlainDescriptor<undefined>;
        /**
         * A transactor exceeded the rate limit for setting weights.
         */
        CommittingWeightsTooFast: PlainDescriptor<undefined>;
        /**
         * Stake amount is too low.
         */
        AmountTooLow: PlainDescriptor<undefined>;
        /**
         * Not enough liquidity.
         */
        InsufficientLiquidity: PlainDescriptor<undefined>;
        /**
         * Slippage is too high for the transaction.
         */
        SlippageTooHigh: PlainDescriptor<undefined>;
        /**
         * Subnet disallows transfer.
         */
        TransferDisallowed: PlainDescriptor<undefined>;
        /**
         * Activity cutoff is being set too low.
         */
        ActivityCutoffTooLow: PlainDescriptor<undefined>;
        /**
         * Call is disabled
         */
        CallDisabled: PlainDescriptor<undefined>;
        /**
         * FirstEmissionBlockNumber is already set.
         */
        FirstEmissionBlockNumberAlreadySet: PlainDescriptor<undefined>;
        /**
         * need wait for more blocks to accept the start call extrinsic.
         */
        NeedWaitingMoreBlocksToStarCall: PlainDescriptor<undefined>;
        /**
         * Not enough AlphaOut on the subnet to recycle
         */
        NotEnoughAlphaOutToRecycle: PlainDescriptor<undefined>;
        /**
         * Cannot burn or recycle TAO from root subnet
         */
        CannotBurnOrRecycleOnRootSubnet: PlainDescriptor<undefined>;
        /**
         * Public key cannot be recovered.
         */
        UnableToRecoverPublicKey: PlainDescriptor<undefined>;
        /**
         * Recovered public key is invalid.
         */
        InvalidRecoveredPublicKey: PlainDescriptor<undefined>;
        /**
         * SubToken disabled now
         */
        SubtokenDisabled: PlainDescriptor<undefined>;
        /**
         * Too frequent hotkey swap on subnet
         */
        HotKeySwapOnSubnetIntervalNotPassed: PlainDescriptor<undefined>;
        /**
         * Zero max stake amount
         */
        ZeroMaxStakeAmount: PlainDescriptor<undefined>;
        /**
         * Invalid netuid duplication
         */
        SameNetuid: PlainDescriptor<undefined>;
        /**
         * The caller does not have enough balance for the operation.
         */
        InsufficientBalance: PlainDescriptor<undefined>;
        /**
         * Too frequent staking operations
         */
        StakingOperationRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * Invalid lease beneficiary to register the leased network.
         */
        InvalidLeaseBeneficiary: PlainDescriptor<undefined>;
        /**
         * Lease cannot end in the past.
         */
        LeaseCannotEndInThePast: PlainDescriptor<undefined>;
        /**
         * Couldn't find the lease netuid.
         */
        LeaseNetuidNotFound: PlainDescriptor<undefined>;
        /**
         * Lease does not exist.
         */
        LeaseDoesNotExist: PlainDescriptor<undefined>;
        /**
         * Lease has no end block.
         */
        LeaseHasNoEndBlock: PlainDescriptor<undefined>;
        /**
         * Lease has not ended.
         */
        LeaseHasNotEnded: PlainDescriptor<undefined>;
        /**
         * An overflow occurred.
         */
        Overflow: PlainDescriptor<undefined>;
        /**
         * Beneficiary does not own hotkey.
         */
        BeneficiaryDoesNotOwnHotkey: PlainDescriptor<undefined>;
        /**
         * Expected beneficiary origin.
         */
        ExpectedBeneficiaryOrigin: PlainDescriptor<undefined>;
        /**
         * Admin operation is prohibited during the protected weights window
         */
        AdminActionProhibitedDuringWeightsWindow: PlainDescriptor<undefined>;
        /**
         * Symbol does not exist.
         */
        SymbolDoesNotExist: PlainDescriptor<undefined>;
        /**
         * Symbol already in use.
         */
        SymbolAlreadyInUse: PlainDescriptor<undefined>;
        /**
         * Incorrect commit-reveal version.
         */
        IncorrectCommitRevealVersion: PlainDescriptor<undefined>;
        /**
         * Reveal period is too large.
         */
        RevealPeriodTooLarge: PlainDescriptor<undefined>;
        /**
         * Reveal period is too small.
         */
        RevealPeriodTooSmall: PlainDescriptor<undefined>;
        /**
         * Generic error for out-of-range parameter value
         */
        InvalidValue: PlainDescriptor<undefined>;
        /**
         * Subnet limit reached & there is no eligible subnet to prune
         */
        SubnetLimitReached: PlainDescriptor<undefined>;
        /**
         * Insufficient funds to meet the subnet lock cost
         */
        CannotAffordLockCost: PlainDescriptor<undefined>;
        /**
         * exceeded the rate limit for associating an EVM key.
         */
        EvmKeyAssociateRateLimitExceeded: PlainDescriptor<undefined>;
        /**
         * Same auto stake hotkey already set
         */
        SameAutoStakeHotkeyAlreadySet: PlainDescriptor<undefined>;
        /**
         * The UID map for the subnet could not be cleared
         */
        UidMapCouldNotBeCleared: PlainDescriptor<undefined>;
        /**
         * Trimming would exceed the max immune neurons percentage
         */
        TrimmingWouldExceedMaxImmunePercentage: PlainDescriptor<undefined>;
        /**
         * Violating the rules of Childkey-Parentkey consistency
         */
        ChildParentInconsistency: PlainDescriptor<undefined>;
        /**
         * Invalid number of root claims
         */
        InvalidNumRootClaim: PlainDescriptor<undefined>;
        /**
         * Invalid value of root claim threshold
         */
        InvalidRootClaimThreshold: PlainDescriptor<undefined>;
        /**
         * Exceeded subnet limit number or zero.
         */
        InvalidSubnetNumber: PlainDescriptor<undefined>;
        /**
         * The maximum allowed UIDs times mechanism count should not exceed 256.
         */
        TooManyUIDsPerMechanism: PlainDescriptor<undefined>;
        /**
         * Voting power tracking is not enabled for this subnet.
         */
        VotingPowerTrackingNotEnabled: PlainDescriptor<undefined>;
        /**
         * Invalid voting power EMA alpha value (must be <= 10^18).
         */
        InvalidVotingPowerEmaAlpha: PlainDescriptor<undefined>;
        /**
         * Unintended precision loss when unstaking alpha
         */
        PrecisionLoss: PlainDescriptor<undefined>;
        /**
         * Deprecated call.
         */
        Deprecated: PlainDescriptor<undefined>;
        /**
         * "Add stake and burn" exceeded the operation rate limit
         */
        AddStakeBurnRateLimitExceeded: PlainDescriptor<undefined>;
    };
    Utility: {
        /**
         * Too many calls batched.
         */
        TooManyCalls: PlainDescriptor<undefined>;
        /**
         * Bad input data for derived account ID
         */
        InvalidDerivedAccount: PlainDescriptor<undefined>;
    };
    Sudo: {
        /**
         * Sender must be the Sudo account.
         */
        RequireSudo: PlainDescriptor<undefined>;
    };
    Multisig: {
        /**
         * Threshold must be 2 or greater.
         */
        MinimumThreshold: PlainDescriptor<undefined>;
        /**
         * Call is already approved by this signatory.
         */
        AlreadyApproved: PlainDescriptor<undefined>;
        /**
         * Call doesn't need any (more) approvals.
         */
        NoApprovalsNeeded: PlainDescriptor<undefined>;
        /**
         * There are too few signatories in the list.
         */
        TooFewSignatories: PlainDescriptor<undefined>;
        /**
         * There are too many signatories in the list.
         */
        TooManySignatories: PlainDescriptor<undefined>;
        /**
         * The signatories were provided out of order; they should be ordered.
         */
        SignatoriesOutOfOrder: PlainDescriptor<undefined>;
        /**
         * The sender was contained in the other signatories; it shouldn't be.
         */
        SenderInSignatories: PlainDescriptor<undefined>;
        /**
         * Multisig operation not found in storage.
         */
        NotFound: PlainDescriptor<undefined>;
        /**
         * Only the account that originally created the multisig is able to cancel it or update
         * its deposits.
         */
        NotOwner: PlainDescriptor<undefined>;
        /**
         * No timepoint was given, yet the multisig operation is already underway.
         */
        NoTimepoint: PlainDescriptor<undefined>;
        /**
         * A different timepoint was given to the multisig operation that is underway.
         */
        WrongTimepoint: PlainDescriptor<undefined>;
        /**
         * A timepoint was given, yet no multisig operation is underway.
         */
        UnexpectedTimepoint: PlainDescriptor<undefined>;
        /**
         * The maximum weight information provided was too low.
         */
        MaxWeightTooLow: PlainDescriptor<undefined>;
        /**
         * The data to be stored is already stored.
         */
        AlreadyStored: PlainDescriptor<undefined>;
    };
    Preimage: {
        /**
         * Preimage is too large to store on-chain.
         */
        TooBig: PlainDescriptor<undefined>;
        /**
         * Preimage has already been noted on-chain.
         */
        AlreadyNoted: PlainDescriptor<undefined>;
        /**
         * The user is not authorized to perform this action.
         */
        NotAuthorized: PlainDescriptor<undefined>;
        /**
         * The preimage cannot be removed since it has not yet been noted.
         */
        NotNoted: PlainDescriptor<undefined>;
        /**
         * A preimage may not be removed when there are outstanding requests.
         */
        Requested: PlainDescriptor<undefined>;
        /**
         * The preimage request cannot be removed since no outstanding requests exist.
         */
        NotRequested: PlainDescriptor<undefined>;
        /**
         * More than `MAX_HASH_UPGRADE_BULK_COUNT` hashes were requested to be upgraded at once.
         */
        TooMany: PlainDescriptor<undefined>;
        /**
         * Too few hashes were requested to be upgraded (i.e. zero).
         */
        TooFew: PlainDescriptor<undefined>;
    };
    Scheduler: {
        /**
         * Failed to schedule a call
         */
        FailedToSchedule: PlainDescriptor<undefined>;
        /**
         * Cannot find the scheduled call.
         */
        NotFound: PlainDescriptor<undefined>;
        /**
         * Given target block number is in the past.
         */
        TargetBlockNumberInPast: PlainDescriptor<undefined>;
        /**
         * Reschedule failed because it does not change scheduled time.
         */
        RescheduleNoChange: PlainDescriptor<undefined>;
        /**
         * Attempt to use a non-named function on a named task.
         */
        Named: PlainDescriptor<undefined>;
    };
    Proxy: {
        /**
         * There are too many proxies registered or too many announcements pending.
         */
        TooMany: PlainDescriptor<undefined>;
        /**
         * Proxy registration not found.
         */
        NotFound: PlainDescriptor<undefined>;
        /**
         * Sender is not a proxy of the account to be proxied.
         */
        NotProxy: PlainDescriptor<undefined>;
        /**
         * A call which is incompatible with the proxy type's filter was attempted.
         */
        Unproxyable: PlainDescriptor<undefined>;
        /**
         * Account is already a proxy.
         */
        Duplicate: PlainDescriptor<undefined>;
        /**
         * Call may not be made by proxy because it may escalate its privileges.
         */
        NoPermission: PlainDescriptor<undefined>;
        /**
         * Announcement, if made at all, was made too recently.
         */
        Unannounced: PlainDescriptor<undefined>;
        /**
         * Cannot add self as proxy.
         */
        NoSelfProxy: PlainDescriptor<undefined>;
        /**
         * Invariant violated: deposit recomputation returned None after updating announcements.
         */
        AnnouncementDepositInvariantViolated: PlainDescriptor<undefined>;
        /**
         * Failed to derive a valid account id from the provided entropy.
         */
        InvalidDerivedAccountId: PlainDescriptor<undefined>;
    };
    Registry: {
        /**
         * Account attempted to register an identity but does not meet the requirements.
         */
        CannotRegister: PlainDescriptor<undefined>;
        /**
         * Account passed too many additional fields to their identity
         */
        TooManyFieldsInIdentityInfo: PlainDescriptor<undefined>;
        /**
         * Account doesn't have a registered identity
         */
        NotRegistered: PlainDescriptor<undefined>;
    };
    Commitments: {
        /**
         * Account passed too many additional fields to their commitment
         */
        TooManyFieldsInCommitmentInfo: PlainDescriptor<undefined>;
        /**
         * Account is not allowed to make commitments to the chain
         */
        AccountNotAllowedCommit: PlainDescriptor<undefined>;
        /**
         * Space Limit Exceeded for the current interval
         */
        SpaceLimitExceeded: PlainDescriptor<undefined>;
        /**
         * Indicates that unreserve returned a leftover, which is unexpected.
         */
        UnexpectedUnreserveLeftover: PlainDescriptor<undefined>;
    };
    AdminUtils: {
        /**
         * The subnet does not exist, check the netuid parameter
         */
        SubnetDoesNotExist: PlainDescriptor<undefined>;
        /**
         * The maximum number of subnet validators must be less than the maximum number of allowed UIDs in the subnet.
         */
        MaxValidatorsLargerThanMaxUIds: PlainDescriptor<undefined>;
        /**
         * The maximum number of subnet validators must be more than the current number of UIDs already in the subnet.
         */
        MaxAllowedUIdsLessThanCurrentUIds: PlainDescriptor<undefined>;
        /**
         * The maximum value for bonds moving average is reached
         */
        BondsMovingAverageMaxReached: PlainDescriptor<undefined>;
        /**
         * Only root can set negative sigmoid steepness values
         */
        NegativeSigmoidSteepness: PlainDescriptor<undefined>;
        /**
         * Value not in allowed bounds.
         */
        ValueNotInBounds: PlainDescriptor<undefined>;
        /**
         * The minimum allowed UIDs must be less than the current number of UIDs in the subnet.
         */
        MinAllowedUidsGreaterThanCurrentUids: PlainDescriptor<undefined>;
        /**
         * The minimum allowed UIDs must be less than the maximum allowed UIDs.
         */
        MinAllowedUidsGreaterThanMaxAllowedUids: PlainDescriptor<undefined>;
        /**
         * The maximum allowed UIDs must be greater than the minimum allowed UIDs.
         */
        MaxAllowedUidsLessThanMinAllowedUids: PlainDescriptor<undefined>;
        /**
         * The maximum allowed UIDs must be less than the default maximum allowed UIDs.
         */
        MaxAllowedUidsGreaterThanDefaultMaxAllowedUids: PlainDescriptor<undefined>;
        /**
         * Bad parameter value
         */
        InvalidValue: PlainDescriptor<undefined>;
    };
    SafeMode: {
        /**
         * The safe-mode is (already or still) entered.
         */
        Entered: PlainDescriptor<undefined>;
        /**
         * The safe-mode is (already or still) exited.
         */
        Exited: PlainDescriptor<undefined>;
        /**
         * This functionality of the pallet is disabled by the configuration.
         */
        NotConfigured: PlainDescriptor<undefined>;
        /**
         * There is no balance reserved.
         */
        NoDeposit: PlainDescriptor<undefined>;
        /**
         * The account already has a deposit reserved and can therefore not enter or extend again.
         */
        AlreadyDeposited: PlainDescriptor<undefined>;
        /**
         * This deposit cannot be released yet.
         */
        CannotReleaseYet: PlainDescriptor<undefined>;
        /**
         * An error from the underlying `Currency`.
         */
        CurrencyError: PlainDescriptor<undefined>;
    };
    Ethereum: {
        /**
         * Signature is invalid.
         */
        InvalidSignature: PlainDescriptor<undefined>;
        /**
         * Pre-log is present, therefore transact is not allowed.
         */
        PreLogExists: PlainDescriptor<undefined>;
    };
    EVM: {
        /**
         * Not enough balance to perform action
         */
        BalanceLow: PlainDescriptor<undefined>;
        /**
         * Calculating total fee overflowed
         */
        FeeOverflow: PlainDescriptor<undefined>;
        /**
         * Calculating total payment overflowed
         */
        PaymentOverflow: PlainDescriptor<undefined>;
        /**
         * Withdraw fee failed
         */
        WithdrawFailed: PlainDescriptor<undefined>;
        /**
         * Gas price is too low.
         */
        GasPriceTooLow: PlainDescriptor<undefined>;
        /**
         * Nonce is invalid
         */
        InvalidNonce: PlainDescriptor<undefined>;
        /**
         * Gas limit is too low.
         */
        GasLimitTooLow: PlainDescriptor<undefined>;
        /**
         * Gas limit is too high.
         */
        GasLimitTooHigh: PlainDescriptor<undefined>;
        /**
         * The chain id is invalid.
         */
        InvalidChainId: PlainDescriptor<undefined>;
        /**
         * the signature is invalid.
         */
        InvalidSignature: PlainDescriptor<undefined>;
        /**
         * EVM reentrancy
         */
        Reentrancy: PlainDescriptor<undefined>;
        /**
         * EIP-3607,
         */
        TransactionMustComeFromEOA: PlainDescriptor<undefined>;
        /**
         * Undefined error.
         */
        Undefined: PlainDescriptor<undefined>;
        /**
         * Origin is not allowed to perform the operation.
         */
        NotAllowed: PlainDescriptor<undefined>;
        /**
         * Address not allowed to deploy contracts either via CREATE or CALL(CREATE).
         */
        CreateOriginNotAllowed: PlainDescriptor<undefined>;
    };
    Drand: {
        /**
         * The value retrieved was `None` as no value was previously set.
         */
        NoneValue: PlainDescriptor<undefined>;
        /**
         * There was an attempt to increment the value in storage over `u32::MAX`.
         */
        StorageOverflow: PlainDescriptor<undefined>;
        /**
         * failed to connect to the
         */
        DrandConnectionFailure: PlainDescriptor<undefined>;
        /**
         * the pulse is invalid
         */
        UnverifiedPulse: PlainDescriptor<undefined>;
        /**
         * the round number did not increment
         */
        InvalidRoundNumber: PlainDescriptor<undefined>;
        /**
         * the pulse could not be verified
         */
        PulseVerificationError: PlainDescriptor<undefined>;
    };
    Crowdloan: {
        /**
         * The crowdloan initial deposit is too low.
         */
        DepositTooLow: PlainDescriptor<undefined>;
        /**
         * The crowdloan cap is too low.
         */
        CapTooLow: PlainDescriptor<undefined>;
        /**
         * The minimum contribution is too low.
         */
        MinimumContributionTooLow: PlainDescriptor<undefined>;
        /**
         * The crowdloan cannot end in the past.
         */
        CannotEndInPast: PlainDescriptor<undefined>;
        /**
         * The crowdloan block duration is too short.
         */
        BlockDurationTooShort: PlainDescriptor<undefined>;
        /**
         * The block duration is too long.
         */
        BlockDurationTooLong: PlainDescriptor<undefined>;
        /**
         * The account does not have enough balance to pay for the initial deposit/contribution.
         */
        InsufficientBalance: PlainDescriptor<undefined>;
        /**
         * An overflow occurred.
         */
        Overflow: PlainDescriptor<undefined>;
        /**
         * The crowdloan id is invalid.
         */
        InvalidCrowdloanId: PlainDescriptor<undefined>;
        /**
         * The crowdloan cap has been fully raised.
         */
        CapRaised: PlainDescriptor<undefined>;
        /**
         * The contribution period has ended.
         */
        ContributionPeriodEnded: PlainDescriptor<undefined>;
        /**
         * The contribution is too low.
         */
        ContributionTooLow: PlainDescriptor<undefined>;
        /**
         * The origin of this call is invalid.
         */
        InvalidOrigin: PlainDescriptor<undefined>;
        /**
         * The crowdloan has already been finalized.
         */
        AlreadyFinalized: PlainDescriptor<undefined>;
        /**
         * The crowdloan contribution period has not ended yet.
         */
        ContributionPeriodNotEnded: PlainDescriptor<undefined>;
        /**
         * The contributor has no contribution for this crowdloan.
         */
        NoContribution: PlainDescriptor<undefined>;
        /**
         * The crowdloan cap has not been raised.
         */
        CapNotRaised: PlainDescriptor<undefined>;
        /**
         * An underflow occurred.
         */
        Underflow: PlainDescriptor<undefined>;
        /**
         * Call to dispatch was not found in the preimage storage.
         */
        CallUnavailable: PlainDescriptor<undefined>;
        /**
         * The crowdloan is not ready to be dissolved, it still has contributions.
         */
        NotReadyToDissolve: PlainDescriptor<undefined>;
        /**
         * The deposit cannot be withdrawn from the crowdloan.
         */
        DepositCannotBeWithdrawn: PlainDescriptor<undefined>;
        /**
         * The maximum number of contributors has been reached.
         */
        MaxContributorsReached: PlainDescriptor<undefined>;
    };
    Swap: {
        /**
         * The fee rate is too high
         */
        FeeRateTooHigh: PlainDescriptor<undefined>;
        /**
         * The provided amount is insufficient for the swap.
         */
        InsufficientInputAmount: PlainDescriptor<undefined>;
        /**
         * The provided liquidity is insufficient for the operation.
         */
        InsufficientLiquidity: PlainDescriptor<undefined>;
        /**
         * The operation would exceed the price limit.
         */
        PriceLimitExceeded: PlainDescriptor<undefined>;
        /**
         * The caller does not have enough balance for the operation.
         */
        InsufficientBalance: PlainDescriptor<undefined>;
        /**
         * Attempted to remove liquidity that does not exist.
         */
        LiquidityNotFound: PlainDescriptor<undefined>;
        /**
         * The provided tick range is invalid.
         */
        InvalidTickRange: PlainDescriptor<undefined>;
        /**
         * Maximum user positions exceeded
         */
        MaxPositionsExceeded: PlainDescriptor<undefined>;
        /**
         * Too many swap steps
         */
        TooManySwapSteps: PlainDescriptor<undefined>;
        /**
         * Provided liquidity parameter is invalid (likely too small)
         */
        InvalidLiquidityValue: PlainDescriptor<undefined>;
        /**
         * Reserves too low for operation.
         */
        ReservesTooLow: PlainDescriptor<undefined>;
        /**
         * The subnet does not exist.
         */
        MechanismDoesNotExist: PlainDescriptor<undefined>;
        /**
         * User liquidity operations are disabled for this subnet
         */
        UserLiquidityDisabled: PlainDescriptor<undefined>;
        /**
         * The subnet does not have subtoken enabled
         */
        SubtokenDisabled: PlainDescriptor<undefined>;
    };
    Contracts: {
        /**
         * Invalid schedule supplied, e.g. with zero weight of a basic operation.
         */
        InvalidSchedule: PlainDescriptor<undefined>;
        /**
         * Invalid combination of flags supplied to `seal_call` or `seal_delegate_call`.
         */
        InvalidCallFlags: PlainDescriptor<undefined>;
        /**
         * The executed contract exhausted its gas limit.
         */
        OutOfGas: PlainDescriptor<undefined>;
        /**
         * The output buffer supplied to a contract API call was too small.
         */
        OutputBufferTooSmall: PlainDescriptor<undefined>;
        /**
         * Performing the requested transfer failed. Probably because there isn't enough
         * free balance in the sender's account.
         */
        TransferFailed: PlainDescriptor<undefined>;
        /**
         * Performing a call was denied because the calling depth reached the limit
         * of what is specified in the schedule.
         */
        MaxCallDepthReached: PlainDescriptor<undefined>;
        /**
         * No contract was found at the specified address.
         */
        ContractNotFound: PlainDescriptor<undefined>;
        /**
         * The code supplied to `instantiate_with_code` exceeds the limit specified in the
         * current schedule.
         */
        CodeTooLarge: PlainDescriptor<undefined>;
        /**
         * No code could be found at the supplied code hash.
         */
        CodeNotFound: PlainDescriptor<undefined>;
        /**
         * No code info could be found at the supplied code hash.
         */
        CodeInfoNotFound: PlainDescriptor<undefined>;
        /**
         * A buffer outside of sandbox memory was passed to a contract API function.
         */
        OutOfBounds: PlainDescriptor<undefined>;
        /**
         * Input passed to a contract API function failed to decode as expected type.
         */
        DecodingFailed: PlainDescriptor<undefined>;
        /**
         * Contract trapped during execution.
         */
        ContractTrapped: PlainDescriptor<undefined>;
        /**
         * The size defined in `T::MaxValueSize` was exceeded.
         */
        ValueTooLarge: PlainDescriptor<undefined>;
        /**
         * Termination of a contract is not allowed while the contract is already
         * on the call stack. Can be triggered by `seal_terminate`.
         */
        TerminatedWhileReentrant: PlainDescriptor<undefined>;
        /**
         * `seal_call` forwarded this contracts input. It therefore is no longer available.
         */
        InputForwarded: PlainDescriptor<undefined>;
        /**
         * The subject passed to `seal_random` exceeds the limit.
         */
        RandomSubjectTooLong: PlainDescriptor<undefined>;
        /**
         * The amount of topics passed to `seal_deposit_events` exceeds the limit.
         */
        TooManyTopics: PlainDescriptor<undefined>;
        /**
         * The chain does not provide a chain extension. Calling the chain extension results
         * in this error. Note that this usually  shouldn't happen as deploying such contracts
         * is rejected.
         */
        NoChainExtension: PlainDescriptor<undefined>;
        /**
         * Failed to decode the XCM program.
         */
        XCMDecodeFailed: PlainDescriptor<undefined>;
        /**
         * A contract with the same AccountId already exists.
         */
        DuplicateContract: PlainDescriptor<undefined>;
        /**
         * A contract self destructed in its constructor.
         *
         * This can be triggered by a call to `seal_terminate`.
         */
        TerminatedInConstructor: PlainDescriptor<undefined>;
        /**
         * A call tried to invoke a contract that is flagged as non-reentrant.
         * The only other cause is that a call from a contract into the runtime tried to call back
         * into `pallet-contracts`. This would make the whole pallet reentrant with regard to
         * contract code execution which is not supported.
         */
        ReentranceDenied: PlainDescriptor<undefined>;
        /**
         * A contract attempted to invoke a state modifying API while being in read-only mode.
         */
        StateChangeDenied: PlainDescriptor<undefined>;
        /**
         * Origin doesn't have enough balance to pay the required storage deposits.
         */
        StorageDepositNotEnoughFunds: PlainDescriptor<undefined>;
        /**
         * More storage was created than allowed by the storage deposit limit.
         */
        StorageDepositLimitExhausted: PlainDescriptor<undefined>;
        /**
         * Code removal was denied because the code is still in use by at least one contract.
         */
        CodeInUse: PlainDescriptor<undefined>;
        /**
         * The contract ran to completion but decided to revert its storage changes.
         * Please note that this error is only returned from extrinsics. When called directly
         * or via RPC an `Ok` will be returned. In this case the caller needs to inspect the flags
         * to determine whether a reversion has taken place.
         */
        ContractReverted: PlainDescriptor<undefined>;
        /**
         * The contract's code was found to be invalid during validation.
         *
         * The most likely cause of this is that an API was used which is not supported by the
         * node. This happens if an older node is used with a new version of ink!. Try updating
         * your node to the newest available version.
         *
         * A more detailed error can be found on the node console if debug messages are enabled
         * by supplying `-lruntime::contracts=debug`.
         */
        CodeRejected: PlainDescriptor<undefined>;
        /**
         * An indeterministic code was used in a context where this is not permitted.
         */
        Indeterministic: PlainDescriptor<undefined>;
        /**
         * A pending migration needs to complete before the extrinsic can be called.
         */
        MigrationInProgress: PlainDescriptor<undefined>;
        /**
         * Migrate dispatch call was attempted but no migration was performed.
         */
        NoMigrationPerformed: PlainDescriptor<undefined>;
        /**
         * The contract has reached its maximum number of delegate dependencies.
         */
        MaxDelegateDependenciesReached: PlainDescriptor<undefined>;
        /**
         * The dependency was not found in the contract's delegate dependencies.
         */
        DelegateDependencyNotFound: PlainDescriptor<undefined>;
        /**
         * The contract already depends on the given delegate dependency.
         */
        DelegateDependencyAlreadyExists: PlainDescriptor<undefined>;
        /**
         * Can not add a delegate dependency to the code hash of the contract itself.
         */
        CannotAddSelfAsDelegateDependency: PlainDescriptor<undefined>;
        /**
         * Can not add more data to transient storage.
         */
        OutOfTransientStorage: PlainDescriptor<undefined>;
    };
    MevShield: {
        /**
         * A submission with the same id already exists in `Submissions`.
         */
        SubmissionAlreadyExists: PlainDescriptor<undefined>;
        /**
         * The referenced submission id does not exist in `Submissions`.
         */
        MissingSubmission: PlainDescriptor<undefined>;
        /**
         * The recomputed commitment does not match the stored commitment.
         */
        CommitmentMismatch: PlainDescriptor<undefined>;
        /**
         * The provided signature over the payload is invalid.
         */
        SignatureInvalid: PlainDescriptor<undefined>;
        /**
         * The announced ML‑KEM public key length is invalid.
         */
        BadPublicKeyLen: PlainDescriptor<undefined>;
        /**
         * The MEV‑Shield key epoch for this submission has expired and is no longer accepted.
         */
        KeyExpired: PlainDescriptor<undefined>;
        /**
         * The provided `key_hash` does not match the expected epoch key hash.
         */
        KeyHashMismatch: PlainDescriptor<undefined>;
    };
};
type IConstants = {
    System: {
        /**
         * Block & extrinsics weights: base values and limits.
         */
        BlockWeights: PlainDescriptor<Anonymize<In7a38730s6qs>>;
        /**
         * The maximum length of a block (in bytes).
         */
        BlockLength: PlainDescriptor<Anonymize<If15el53dd76v9>>;
        /**
         * Maximum number of block number to block hash mappings to keep (oldest pruned first).
         */
        BlockHashCount: PlainDescriptor<number>;
        /**
         * The weight of runtime database operations the runtime can invoke.
         */
        DbWeight: PlainDescriptor<Anonymize<I9s0ave7t0vnrk>>;
        /**
         * Get the chain's in-code version.
         */
        Version: PlainDescriptor<Anonymize<I4fo08joqmcqnm>>;
        /**
         * The designated SS58 prefix of this chain.
         *
         * This replaces the "ss58Format" property declared in the chain spec. Reason is
         * that the runtime should know about the prefix in order to make use of it as
         * an identifier of the chain.
         */
        SS58Prefix: PlainDescriptor<number>;
    };
    Timestamp: {
        /**
         * The minimum period between blocks.
         *
         * Be aware that this is different to the *expected* period that the block production
         * apparatus provides. Your chosen consensus system will generally work with this to
         * determine a sensible block time. For example, in the Aura pallet it will be double this
         * period on default settings.
         */
        MinimumPeriod: PlainDescriptor<bigint>;
    };
    Aura: {
        /**
         * The slot duration Aura should run with, expressed in milliseconds.
         * The effective value of this type should not change while the chain is running.
         *
         * For backwards compatibility either use [`MinimumPeriodTimesTwo`] or a const.
         */
        SlotDuration: PlainDescriptor<bigint>;
    };
    Grandpa: {
        /**
         * Max Authorities in use
         */
        MaxAuthorities: PlainDescriptor<number>;
        /**
         * The maximum number of nominators for each validator.
         */
        MaxNominators: PlainDescriptor<number>;
        /**
         * The maximum number of entries to keep in the set id to session index mapping.
         *
         * Since the `SetIdSession` map is only used for validating equivocations this
         * value should relate to the bonding duration of whatever staking system is
         * being used (if any). If equivocation handling is not enabled then this value
         * can be zero.
         */
        MaxSetIdSessionEntries: PlainDescriptor<bigint>;
    };
    Balances: {
        /**
         * The minimum amount required to keep an account open. MUST BE GREATER THAN ZERO!
         *
         * If you *really* need it to be zero, you can enable the feature `insecure_zero_ed` for
         * this pallet. However, you do so at your own risk: this will open up a major DoS vector.
         * In case you have multiple sources of provider references, you may also get unexpected
         * behaviour if you set this to zero.
         *
         * Bottom line: Do yourself a favour and make it at least one!
         */
        ExistentialDeposit: PlainDescriptor<bigint>;
        /**
         * The maximum number of locks that should exist on an account.
         * Not strictly enforced, but used for weight estimation.
         *
         * Use of locks is deprecated in favour of freezes. See `https://github.com/paritytech/substrate/pull/12951/`
         */
        MaxLocks: PlainDescriptor<number>;
        /**
         * The maximum number of named reserves that can exist on an account.
         *
         * Use of reserves is deprecated in favour of holds. See `https://github.com/paritytech/substrate/pull/12951/`
         */
        MaxReserves: PlainDescriptor<number>;
        /**
         * The maximum number of individual freeze locks that can exist on an account at any time.
         */
        MaxFreezes: PlainDescriptor<number>;
    };
    TransactionPayment: {
        /**
         * A fee multiplier for `Operational` extrinsics to compute "virtual tip" to boost their
         * `priority`
         *
         * This value is multiplied by the `final_fee` to obtain a "virtual tip" that is later
         * added to a tip component in regular `priority` calculations.
         * It means that a `Normal` transaction can front-run a similarly-sized `Operational`
         * extrinsic (with no tip), by including a tip value greater than the virtual tip.
         *
         * ```rust,ignore
         * // For `Normal`
         * let priority = priority_calc(tip);
         *
         * // For `Operational`
         * let virtual_tip = (inclusion_fee + tip) * OperationalFeeMultiplier;
         * let priority = priority_calc(tip + virtual_tip);
         * ```
         *
         * Note that since we use `final_fee` the multiplier applies also to the regular `tip`
         * sent with the transaction. So, not only does the transaction get a priority bump based
         * on the `inclusion_fee`, but we also amplify the impact of tips applied to `Operational`
         * transactions.
         */
        OperationalFeeMultiplier: PlainDescriptor<number>;
    };
    SubtensorModule: {
        /**
         * =================================
         * ==== Initial Value Constants ====
         * =================================
         * Initial currency issuance.
         */
        InitialIssuance: PlainDescriptor<bigint>;
        /**
         * Initial min allowed weights setting.
         */
        InitialMinAllowedWeights: PlainDescriptor<number>;
        /**
         * Initial Emission Ratio.
         */
        InitialEmissionValue: PlainDescriptor<number>;
        /**
         * Tempo for each network.
         */
        InitialTempo: PlainDescriptor<number>;
        /**
         * Initial Difficulty.
         */
        InitialDifficulty: PlainDescriptor<bigint>;
        /**
         * Initial Max Difficulty.
         */
        InitialMaxDifficulty: PlainDescriptor<bigint>;
        /**
         * Initial Min Difficulty.
         */
        InitialMinDifficulty: PlainDescriptor<bigint>;
        /**
         * Initial RAO Recycled.
         */
        InitialRAORecycledForRegistration: PlainDescriptor<bigint>;
        /**
         * Initial Burn.
         */
        InitialBurn: PlainDescriptor<bigint>;
        /**
         * Initial Max Burn.
         */
        InitialMaxBurn: PlainDescriptor<bigint>;
        /**
         * Initial Min Burn.
         */
        InitialMinBurn: PlainDescriptor<bigint>;
        /**
         * Min  burn upper bound.
         */
        MinBurnUpperBound: PlainDescriptor<bigint>;
        /**
         * Max burn lower bound.
         */
        MaxBurnLowerBound: PlainDescriptor<bigint>;
        /**
         * Initial adjustment interval.
         */
        InitialAdjustmentInterval: PlainDescriptor<number>;
        /**
         * Initial bonds moving average.
         */
        InitialBondsMovingAverage: PlainDescriptor<bigint>;
        /**
         * Initial bonds penalty.
         */
        InitialBondsPenalty: PlainDescriptor<number>;
        /**
         * Initial bonds reset.
         */
        InitialBondsResetOn: PlainDescriptor<boolean>;
        /**
         * Initial target registrations per interval.
         */
        InitialTargetRegistrationsPerInterval: PlainDescriptor<number>;
        /**
         * Rho constant.
         */
        InitialRho: PlainDescriptor<number>;
        /**
         * AlphaSigmoidSteepness constant.
         */
        InitialAlphaSigmoidSteepness: PlainDescriptor<number>;
        /**
         * Kappa constant.
         */
        InitialKappa: PlainDescriptor<number>;
        /**
         * Initial minimum allowed network UIDs
         */
        InitialMinAllowedUids: PlainDescriptor<number>;
        /**
         * Initial maximum allowed network UIDs
         */
        InitialMaxAllowedUids: PlainDescriptor<number>;
        /**
         * Initial validator context pruning length.
         */
        InitialValidatorPruneLen: PlainDescriptor<bigint>;
        /**
         * Initial scaling law power.
         */
        InitialScalingLawPower: PlainDescriptor<number>;
        /**
         * Immunity Period Constant.
         */
        InitialImmunityPeriod: PlainDescriptor<number>;
        /**
         * Activity constant.
         */
        InitialActivityCutoff: PlainDescriptor<number>;
        /**
         * Initial max registrations per block.
         */
        InitialMaxRegistrationsPerBlock: PlainDescriptor<number>;
        /**
         * Initial pruning score for each neuron.
         */
        InitialPruningScore: PlainDescriptor<number>;
        /**
         * Initial maximum allowed validators per network.
         */
        InitialMaxAllowedValidators: PlainDescriptor<number>;
        /**
         * Initial default delegation take.
         */
        InitialDefaultDelegateTake: PlainDescriptor<number>;
        /**
         * Initial minimum delegation take.
         */
        InitialMinDelegateTake: PlainDescriptor<number>;
        /**
         * Initial default childkey take.
         */
        InitialDefaultChildKeyTake: PlainDescriptor<number>;
        /**
         * Initial minimum childkey take.
         */
        InitialMinChildKeyTake: PlainDescriptor<number>;
        /**
         * Initial maximum childkey take.
         */
        InitialMaxChildKeyTake: PlainDescriptor<number>;
        /**
         * Initial weights version key.
         */
        InitialWeightsVersionKey: PlainDescriptor<bigint>;
        /**
         * Initial serving rate limit.
         */
        InitialServingRateLimit: PlainDescriptor<bigint>;
        /**
         * Initial transaction rate limit.
         */
        InitialTxRateLimit: PlainDescriptor<bigint>;
        /**
         * Initial delegate take transaction rate limit.
         */
        InitialTxDelegateTakeRateLimit: PlainDescriptor<bigint>;
        /**
         * Initial childkey take transaction rate limit.
         */
        InitialTxChildKeyTakeRateLimit: PlainDescriptor<bigint>;
        /**
         * Initial adjustment alpha on burn and pow.
         */
        InitialAdjustmentAlpha: PlainDescriptor<bigint>;
        /**
         * Initial network immunity period
         */
        InitialNetworkImmunityPeriod: PlainDescriptor<bigint>;
        /**
         * Initial network minimum burn cost
         */
        InitialNetworkMinLockCost: PlainDescriptor<bigint>;
        /**
         * Initial network subnet cut.
         */
        InitialSubnetOwnerCut: PlainDescriptor<number>;
        /**
         * Initial lock reduction interval.
         */
        InitialNetworkLockReductionInterval: PlainDescriptor<bigint>;
        /**
         * Initial network creation rate limit
         */
        InitialNetworkRateLimit: PlainDescriptor<bigint>;
        /**
         * Cost of swapping a hotkey.
         */
        KeySwapCost: PlainDescriptor<bigint>;
        /**
         * The upper bound for the alpha parameter. Used for Liquid Alpha.
         */
        AlphaHigh: PlainDescriptor<number>;
        /**
         * The lower bound for the alpha parameter. Used for Liquid Alpha.
         */
        AlphaLow: PlainDescriptor<number>;
        /**
         * A flag to indicate if Liquid Alpha is enabled.
         */
        LiquidAlphaOn: PlainDescriptor<boolean>;
        /**
         * A flag to indicate if Yuma3 is enabled.
         */
        Yuma3On: PlainDescriptor<boolean>;
        /**
         * Coldkey swap announcement delay.
         */
        InitialColdkeySwapAnnouncementDelay: PlainDescriptor<number>;
        /**
         * Coldkey swap reannouncement delay.
         */
        InitialColdkeySwapReannouncementDelay: PlainDescriptor<number>;
        /**
         * Dissolve network schedule duration
         */
        InitialDissolveNetworkScheduleDuration: PlainDescriptor<number>;
        /**
         * Initial TAO weight.
         */
        InitialTaoWeight: PlainDescriptor<bigint>;
        /**
         * Initial EMA price halving period
         */
        InitialEmaPriceHalvingPeriod: PlainDescriptor<bigint>;
        /**
         * Delay after which a new subnet can dispatch start call extrinsic.
         */
        InitialStartCallDelay: PlainDescriptor<bigint>;
        /**
         * Cost of swapping a hotkey in a subnet.
         */
        KeySwapOnSubnetCost: PlainDescriptor<bigint>;
        /**
         * Block number for a coldkey swap the hotkey in specific subnet.
         */
        HotkeySwapOnSubnetInterval: PlainDescriptor<bigint>;
        /**
         * Number of blocks between dividends distribution.
         */
        LeaseDividendsDistributionInterval: PlainDescriptor<number>;
        /**
         * Maximum percentage of immune UIDs.
         */
        MaxImmuneUidsPercentage: PlainDescriptor<number>;
    };
    Utility: {
        /**
         * The limit on the number of batched calls.
         */
        batched_calls_limit: PlainDescriptor<number>;
    };
    Multisig: {
        /**
         * The base amount of currency needed to reserve for creating a multisig execution or to
         * store a dispatch call for later.
         *
         * This is held for an additional storage item whose value size is
         * `4 + sizeof((BlockNumber, Balance, AccountId))` bytes and whose key size is
         * `32 + sizeof(AccountId)` bytes.
         */
        DepositBase: PlainDescriptor<bigint>;
        /**
         * The amount of currency needed per unit threshold when creating a multisig execution.
         *
         * This is held for adding 32 bytes more into a pre-existing storage value.
         */
        DepositFactor: PlainDescriptor<bigint>;
        /**
         * The maximum amount of signatories allowed in the multisig.
         */
        MaxSignatories: PlainDescriptor<number>;
    };
    Scheduler: {
        /**
         * The maximum weight that may be scheduled per block for any dispatchables.
         */
        MaximumWeight: PlainDescriptor<Anonymize<I4q39t5hn830vp>>;
        /**
         * The maximum number of scheduled calls in the queue for a single block.
         *
         * NOTE:
         * + Dependent pallets' benchmarks might require a higher limit for the setting. Set a
         * higher limit under `runtime-benchmarks` feature.
         */
        MaxScheduledPerBlock: PlainDescriptor<number>;
    };
    Proxy: {
        /**
         * The base amount of currency needed to reserve for creating a proxy.
         *
         * This is held for an additional storage item whose value size is
         * `sizeof(Balance)` bytes and whose key size is `sizeof(AccountId)` bytes.
         */
        ProxyDepositBase: PlainDescriptor<bigint>;
        /**
         * The amount of currency needed per proxy added.
         *
         * This is held for adding 32 bytes plus an instance of `ProxyType` more into a
         * pre-existing storage value. Thus, when configuring `ProxyDepositFactor` one should take
         * into account `32 + proxy_type.encode().len()` bytes of data.
         */
        ProxyDepositFactor: PlainDescriptor<bigint>;
        /**
         * The maximum amount of proxies allowed for a single account.
         */
        MaxProxies: PlainDescriptor<number>;
        /**
         * The maximum amount of time-delayed announcements that are allowed to be pending.
         */
        MaxPending: PlainDescriptor<number>;
        /**
         * The base amount of currency needed to reserve for creating an announcement.
         *
         * This is held when a new storage item holding a `Balance` is created (typically 16
         * bytes).
         */
        AnnouncementDepositBase: PlainDescriptor<bigint>;
        /**
         * The amount of currency needed per announcement made.
         *
         * This is held for adding an `AccountId`, `Hash` and `BlockNumber` (typically 68 bytes)
         * into a pre-existing storage value.
         */
        AnnouncementDepositFactor: PlainDescriptor<bigint>;
    };
    Registry: {
        /**
         * Configuration fields
         * Maximum user-configured additional fields
         */
        MaxAdditionalFields: PlainDescriptor<number>;
        /**
         * The amount held on deposit for a registered identity
         */
        InitialDeposit: PlainDescriptor<bigint>;
        /**
         * The amount held on deposit per additional field for a registered identity.
         */
        FieldDeposit: PlainDescriptor<bigint>;
    };
    Commitments: {
        /**
         * The maximum number of additional fields that can be added to a commitment
         */
        MaxFields: PlainDescriptor<number>;
        /**
         * The amount held on deposit for a registered identity
         */
        InitialDeposit: PlainDescriptor<bigint>;
        /**
         * The amount held on deposit per additional field for a registered identity.
         */
        FieldDeposit: PlainDescriptor<bigint>;
    };
    SafeMode: {
        /**
         * For how many blocks the safe-mode will be entered by [`Pallet::enter`].
         */
        EnterDuration: PlainDescriptor<number>;
        /**
         * For how many blocks the safe-mode can be extended by each [`Pallet::extend`] call.
         *
         * This does not impose a hard limit as the safe-mode can be extended multiple times.
         */
        ExtendDuration: PlainDescriptor<number>;
        /**
         * The amount that will be reserved upon calling [`Pallet::enter`].
         *
         * `None` disallows permissionlessly enabling the safe-mode and is a sane default.
         */
        EnterDepositAmount: PlainDescriptor<Anonymize<I35p85j063s0il>>;
        /**
         * The amount that will be reserved upon calling [`Pallet::extend`].
         *
         * `None` disallows permissionlessly extending the safe-mode and is a sane default.
         */
        ExtendDepositAmount: PlainDescriptor<Anonymize<I35p85j063s0il>>;
        /**
         * The minimal duration a deposit will remain reserved after safe-mode is entered or
         * extended, unless [`Pallet::force_release_deposit`] is successfully called sooner.
         *
         * Every deposit is tied to a specific activation or extension, thus each deposit can be
         * released independently after the delay for it has passed.
         *
         * `None` disallows permissionlessly releasing the safe-mode deposits and is a sane
         * default.
         */
        ReleaseDelay: PlainDescriptor<Anonymize<I4arjljr6dpflb>>;
    };
    Drand: {
        /**
         * A configuration for base priority of unsigned transactions.
         *
         * This is exposed so that it can be tuned for particular runtime, when
         * multiple pallets send unsigned transactions.
         */
        UnsignedPriority: PlainDescriptor<bigint>;
        /**
         * The maximum number of milliseconds we are willing to wait for the HTTP request to
         * complete.
         */
        HttpFetchTimeout: PlainDescriptor<bigint>;
    };
    Crowdloan: {
        /**
         * The pallet id that will be used to derive crowdloan account ids.
         */
        PalletId: PlainDescriptor<FixedSizeBinary<8>>;
        /**
         * The minimum deposit required to create a crowdloan.
         */
        MinimumDeposit: PlainDescriptor<bigint>;
        /**
         * The absolute minimum contribution required to contribute to a crowdloan.
         */
        AbsoluteMinimumContribution: PlainDescriptor<bigint>;
        /**
         * The minimum block duration for a crowdloan.
         */
        MinimumBlockDuration: PlainDescriptor<number>;
        /**
         * The maximum block duration for a crowdloan.
         */
        MaximumBlockDuration: PlainDescriptor<number>;
        /**
         * The maximum number of contributors that can be refunded in a single refund.
         */
        RefundContributorsLimit: PlainDescriptor<number>;
        /**
        
         */
        MaxContributors: PlainDescriptor<number>;
    };
    Swap: {
        /**
         * This type is used to derive protocol accoun ID.
         */
        ProtocolId: PlainDescriptor<FixedSizeBinary<8>>;
        /**
         * The maximum fee rate that can be set
         */
        MaxFeeRate: PlainDescriptor<number>;
        /**
         * The maximum number of positions a user can have
         */
        MaxPositions: PlainDescriptor<number>;
        /**
         * Minimum liquidity that is safe for rounding and integer math.
         */
        MinimumLiquidity: PlainDescriptor<bigint>;
        /**
         * Minimum reserve for tao and alpha
         */
        MinimumReserve: PlainDescriptor<bigint>;
    };
    Contracts: {
        /**
         * Cost schedule and limits.
         */
        Schedule: PlainDescriptor<Anonymize<Ijc5n210o8bbf>>;
        /**
         * The amount of balance a caller has to pay for each byte of storage.
         *
         * # Note
         *
         * Changing this value for an existing chain might need a storage migration.
         */
        DepositPerByte: PlainDescriptor<bigint>;
        /**
         * Fallback value to limit the storage deposit if it's not being set by the caller.
         */
        DefaultDepositLimit: PlainDescriptor<bigint>;
        /**
         * The amount of balance a caller has to pay for each storage item.
         *
         * # Note
         *
         * Changing this value for an existing chain might need a storage migration.
         */
        DepositPerItem: PlainDescriptor<bigint>;
        /**
         * The percentage of the storage deposit that should be held for using a code hash.
         * Instantiating a contract, or calling [`chain_extension::Ext::lock_delegate_dependency`]
         * protects the code from being removed. In order to prevent abuse these actions are
         * protected with a percentage of the code deposit.
         */
        CodeHashLockupDepositPercent: PlainDescriptor<number>;
        /**
         * The maximum length of a contract code in bytes.
         *
         * The value should be chosen carefully taking into the account the overall memory limit
         * your runtime has, as well as the [maximum allowed callstack
         * depth](#associatedtype.CallStack). Look into the `integrity_test()` for some insights.
         */
        MaxCodeLen: PlainDescriptor<number>;
        /**
         * The maximum allowable length in bytes for storage keys.
         */
        MaxStorageKeyLen: PlainDescriptor<number>;
        /**
         * The maximum size of the transient storage in bytes.
         * This includes keys, values, and previous entries used for storage rollback.
         */
        MaxTransientStorageSize: PlainDescriptor<number>;
        /**
         * The maximum number of delegate_dependencies that a contract can lock with
         * [`chain_extension::Ext::lock_delegate_dependency`].
         */
        MaxDelegateDependencies: PlainDescriptor<number>;
        /**
         * Make contract callable functions marked as `#[unstable]` available.
         *
         * Contracts that use `#[unstable]` functions won't be able to be uploaded unless
         * this is set to `true`. This is only meant for testnets and dev nodes in order to
         * experiment with new features.
         *
         * # Warning
         *
         * Do **not** set to `true` on productions chains.
         */
        UnsafeUnstableInterface: PlainDescriptor<boolean>;
        /**
         * The maximum length of the debug buffer in bytes.
         */
        MaxDebugBufferLen: PlainDescriptor<number>;
        /**
         * Type that bundles together all the runtime configurable interface types.
         *
         * This is not a real config. We just mention the type here as constant so that
         * its type appears in the metadata. Only valid value is `()`.
         */
        Environment: PlainDescriptor<Anonymize<I3m5sq54sjdlso>>;
        /**
         * The version of the HostFn APIs that are available in the runtime.
         *
         * Only valid value is `()`.
         */
        ApiVersion: PlainDescriptor<number>;
    };
};
type IViewFns = {
    Proxy: {
        /**
         * Check if a `RuntimeCall` is allowed for a given `ProxyType`.
         */
        check_permissions: RuntimeDescriptor<[call: Anonymize<I8vbtb6bd00lm0>, proxy_type: Anonymize<I8v1041j74kmaj>], boolean>;
        /**
         * Check if one `ProxyType` is a subset of another `ProxyType`.
         */
        is_superset: RuntimeDescriptor<[to_check: Anonymize<I8v1041j74kmaj>, against: Anonymize<I8v1041j74kmaj>], boolean>;
    };
};
type IRuntimeCalls = {
    /**
     * The `Core` runtime api that every Substrate runtime needs to implement.
     */
    Core: {
        /**
         * Returns the version of the runtime.
         */
        version: RuntimeDescriptor<[], Anonymize<I4fo08joqmcqnm>>;
        /**
         * Execute the given block.
         */
        execute_block: RuntimeDescriptor<[block: Anonymize<Iaqet9jc3ihboe>], undefined>;
        /**
         * Initialize a block with the given header and return the runtime executive mode.
         */
        initialize_block: RuntimeDescriptor<[header: Anonymize<Ic952bubvq4k7d>], Anonymize<I2v50gu3s1aqk6>>;
    };
    /**
     * The `Metadata` api trait that returns metadata for the runtime.
     */
    Metadata: {
        /**
         * Returns the metadata of a runtime.
         */
        metadata: RuntimeDescriptor<[], Binary>;
        /**
         * Returns the metadata at a given version.
         *
         * If the given `version` isn't supported, this will return `None`.
         * Use [`Self::metadata_versions`] to find out about supported metadata version of the runtime.
         */
        metadata_at_version: RuntimeDescriptor<[version: number], Anonymize<Iabpgqcjikia83>>;
        /**
         * Returns the supported metadata versions.
         *
         * This can be used to call `metadata_at_version`.
         */
        metadata_versions: RuntimeDescriptor<[], Anonymize<Icgljjb6j82uhn>>;
    };
    /**
     * The `BlockBuilder` api trait that provides the required functionality for building a block.
     */
    BlockBuilder: {
        /**
         * Apply the given extrinsic.
         *
         * Returns an inclusion outcome which specifies if this extrinsic is included in
         * this block or not.
         */
        apply_extrinsic: RuntimeDescriptor<[extrinsic: Binary], Anonymize<Ibmofsd95figtn>>;
        /**
         * Finish the current block.
         */
        finalize_block: RuntimeDescriptor<[], Anonymize<Ic952bubvq4k7d>>;
        /**
         * Generate inherent extrinsics. The inherent data will vary from chain to chain.
         */
        inherent_extrinsics: RuntimeDescriptor<[inherent: Anonymize<If7uv525tdvv7a>], Anonymize<Itom7fk49o0c9>>;
        /**
         * Check that the inherents are valid. The inherent data will vary from chain to chain.
         */
        check_inherents: RuntimeDescriptor<[block: Anonymize<Iaqet9jc3ihboe>, data: Anonymize<If7uv525tdvv7a>], Anonymize<I2an1fs2eiebjp>>;
    };
    /**
     * API to interact with `RuntimeGenesisConfig` for the runtime
     */
    GenesisBuilder: {
        /**
         * Build `RuntimeGenesisConfig` from a JSON blob not using any defaults and store it in the
         * storage.
         *
         * In the case of a FRAME-based runtime, this function deserializes the full
         * `RuntimeGenesisConfig` from the given JSON blob and puts it into the storage. If the
         * provided JSON blob is incorrect or incomplete or the deserialization fails, an error
         * is returned.
         *
         * Please note that provided JSON blob must contain all `RuntimeGenesisConfig` fields, no
         * defaults will be used.
         */
        build_state: RuntimeDescriptor<[json: Binary], Anonymize<Ie9sr1iqcg3cgm>>;
        /**
         * Returns a JSON blob representation of the built-in `RuntimeGenesisConfig` identified by
         * `id`.
         *
         * If `id` is `None` the function should return JSON blob representation of the default
         * `RuntimeGenesisConfig` struct of the runtime. Implementation must provide default
         * `RuntimeGenesisConfig`.
         *
         * Otherwise function returns a JSON representation of the built-in, named
         * `RuntimeGenesisConfig` preset identified by `id`, or `None` if such preset does not
         * exist. Returned `Vec<u8>` contains bytes of JSON blob (patch) which comprises a list of
         * (potentially nested) key-value pairs that are intended for customizing the default
         * runtime genesis config. The patch shall be merged (rfc7386) with the JSON representation
         * of the default `RuntimeGenesisConfig` to create a comprehensive genesis config that can
         * be used in `build_state` method.
         */
        get_preset: RuntimeDescriptor<[id: Anonymize<I1mqgk2tmnn9i2>], Anonymize<Iabpgqcjikia83>>;
        /**
         * Returns a list of identifiers for available builtin `RuntimeGenesisConfig` presets.
         *
         * The presets from the list can be queried with [`GenesisBuilder::get_preset`] method. If
         * no named presets are provided by the runtime the list is empty.
         */
        preset_names: RuntimeDescriptor<[], Anonymize<I6lr8sctk0bi4e>>;
    };
    /**
     * The `TaggedTransactionQueue` api trait for interfering with the transaction queue.
     */
    TaggedTransactionQueue: {
        /**
         * Validate the transaction.
         *
         * This method is invoked by the transaction pool to learn details about given transaction.
         * The implementation should make sure to verify the correctness of the transaction
         * against current state. The given `block_hash` corresponds to the hash of the block
         * that is used as current state.
         *
         * Note that this call may be performed by the pool multiple times and transactions
         * might be verified in any possible order.
         */
        validate_transaction: RuntimeDescriptor<[source: TransactionValidityTransactionSource, tx: Binary, block_hash: FixedSizeBinary<32>], Anonymize<I9ask1o4tfvcvs>>;
    };
    /**
     * The offchain worker api.
     */
    OffchainWorkerApi: {
        /**
         * Starts the off-chain task for given block header.
         */
        offchain_worker: RuntimeDescriptor<[header: Anonymize<Ic952bubvq4k7d>], undefined>;
    };
    /**
     * API necessary for block authorship with aura.
     */
    AuraApi: {
        /**
         * Returns the slot duration for Aura.
         *
         * Currently, only the value provided by this type at genesis will be used.
         */
        slot_duration: RuntimeDescriptor<[], bigint>;
        /**
         * Return the current set of authorities.
         */
        authorities: RuntimeDescriptor<[], Anonymize<Ic5m5lp1oioo8r>>;
    };
    /**
     * Session keys runtime api.
     */
    SessionKeys: {
        /**
         * Generate a set of session keys with optionally using the given seed.
         * The keys should be stored within the keystore exposed via runtime
         * externalities.
         *
         * The seed needs to be a valid `utf8` string.
         *
         * Returns the concatenated SCALE encoded public keys.
         */
        generate_session_keys: RuntimeDescriptor<[seed: Anonymize<Iabpgqcjikia83>], Binary>;
        /**
         * Decode the given public session keys.
         *
         * Returns the list of public raw public keys + key type.
         */
        decode_session_keys: RuntimeDescriptor<[encoded: Binary], Anonymize<Icerf8h8pdu8ss>>;
    };
    /**
     * APIs for integrating the GRANDPA finality gadget into runtimes.
     * This should be implemented on the runtime side.
     *
     * This is primarily used for negotiating authority-set changes for the
     * gadget. GRANDPA uses a signaling model of changing authority sets:
     * changes should be signaled with a delay of N blocks, and then automatically
     * applied in the runtime after those N blocks have passed.
     *
     * The consensus protocol will coordinate the handoff externally.
     */
    GrandpaApi: {
        /**
         * Get the current GRANDPA authorities and weights. This should not change except
         * for when changes are scheduled and the corresponding delay has passed.
         *
         * When called at block B, it will return the set of authorities that should be
         * used to finalize descendants of this block (B+1, B+2, ...). The block B itself
         * is finalized by the authorities from block B-1.
         */
        grandpa_authorities: RuntimeDescriptor<[], Anonymize<I3geksg000c171>>;
        /**
         * Submits an unsigned extrinsic to report an equivocation. The caller
         * must provide the equivocation proof and a key ownership proof
         * (should be obtained using `generate_key_ownership_proof`). The
         * extrinsic will be unsigned and should only be accepted for local
         * authorship (not to be broadcast to the network). This method returns
         * `None` when creation of the extrinsic fails, e.g. if equivocation
         * reporting is disabled for the given runtime (i.e. this method is
         * hardcoded to return `None`). Only useful in an offchain context.
         */
        submit_report_equivocation_unsigned_extrinsic: RuntimeDescriptor<[equivocation_proof: Anonymize<I9puqgoda8ofk4>, key_owner_proof: Binary], boolean>;
        /**
         * Generates a proof of key ownership for the given authority in the
         * given set. An example usage of this module is coupled with the
         * session historical module to prove that a given authority key is
         * tied to a given staking identity during a specific session. Proofs
         * of key ownership are necessary for submitting equivocation reports.
         * NOTE: even though the API takes a `set_id` as parameter the current
         * implementations ignore this parameter and instead rely on this
         * method being called at the correct block height, i.e. any point at
         * which the given set id is live on-chain. Future implementations will
         * instead use indexed data through an offchain worker, not requiring
         * older states to be available.
         */
        generate_key_ownership_proof: RuntimeDescriptor<[set_id: bigint, authority_id: FixedSizeBinary<32>], Anonymize<Iabpgqcjikia83>>;
        /**
         * Get current GRANDPA authority set id.
         */
        current_set_id: RuntimeDescriptor<[], bigint>;
    };
    /**
     * The API to query account nonce.
     */
    AccountNonceApi: {
        /**
         * Get current account nonce of given `AccountId`.
         */
        account_nonce: RuntimeDescriptor<[account: SS58String], number>;
    };
    /**
    
     */
    TransactionPaymentApi: {
        /**
        
         */
        query_info: RuntimeDescriptor<[uxt: Binary, len: number], Anonymize<I6spmpef2c7svf>>;
        /**
        
         */
        query_fee_details: RuntimeDescriptor<[uxt: Binary, len: number], Anonymize<Iei2mvq0mjvt81>>;
        /**
        
         */
        query_weight_to_fee: RuntimeDescriptor<[weight: Anonymize<I4q39t5hn830vp>], bigint>;
        /**
        
         */
        query_length_to_fee: RuntimeDescriptor<[length: number], bigint>;
    };
    /**
    
     */
    TransactionPaymentCallApi: {
        /**
         * Query information of a dispatch class, weight, and fee of a given encoded `Call`.
         */
        query_call_info: RuntimeDescriptor<[call: Anonymize<I8vbtb6bd00lm0>, len: number], Anonymize<I6spmpef2c7svf>>;
        /**
         * Query fee details of a given encoded `Call`.
         */
        query_call_fee_details: RuntimeDescriptor<[call: Anonymize<I8vbtb6bd00lm0>, len: number], Anonymize<Iei2mvq0mjvt81>>;
        /**
         * Query the output of the current `WeightToFee` given some input.
         */
        query_weight_to_fee: RuntimeDescriptor<[weight: Anonymize<I4q39t5hn830vp>], bigint>;
        /**
         * Query the output of the current `LengthToFee` given some input.
         */
        query_length_to_fee: RuntimeDescriptor<[length: number], bigint>;
    };
    /**
     * API necessary for Ethereum-compatibility layer.
     */
    EthereumRuntimeRPCApi: {
        /**
         * Returns runtime defined pallet_evm::ChainId.
         */
        chain_id: RuntimeDescriptor<[], bigint>;
        /**
         * Returns pallet_evm::Accounts by address.
         */
        account_basic: RuntimeDescriptor<[address: FixedSizeBinary<20>], Anonymize<If08sfhqn8ujfr>>;
        /**
         * Returns FixedGasPrice::min_gas_price
         */
        gas_price: RuntimeDescriptor<[], Anonymize<I4totqt881mlti>>;
        /**
         * For a given account address, returns pallet_evm::AccountCodes.
         */
        account_code_at: RuntimeDescriptor<[address: FixedSizeBinary<20>], Binary>;
        /**
         * Returns the converted FindAuthor::find_author authority id.
         */
        author: RuntimeDescriptor<[], FixedSizeBinary<20>>;
        /**
         * For a given account address and index, returns pallet_evm::AccountStorages.
         */
        storage_at: RuntimeDescriptor<[address: FixedSizeBinary<20>, index: Anonymize<I4totqt881mlti>], FixedSizeBinary<32>>;
        /**
        
         */
        call: RuntimeDescriptor<[from: FixedSizeBinary<20>, to: FixedSizeBinary<20>, data: Binary, value: Anonymize<I4totqt881mlti>, gas_limit: Anonymize<I4totqt881mlti>, max_fee_per_gas: Anonymize<Ic4rgfgksgmm3e>, max_priority_fee_per_gas: Anonymize<Ic4rgfgksgmm3e>, nonce: Anonymize<Ic4rgfgksgmm3e>, estimate: boolean, access_list: Anonymize<I3dj14b7k3rkm5>, authorization_list: Anonymize<Ic5egmm215ml6k>], Anonymize<Ibg4am9lqg35ku>>;
        /**
        
         */
        create: RuntimeDescriptor<[from: FixedSizeBinary<20>, data: Binary, value: Anonymize<I4totqt881mlti>, gas_limit: Anonymize<I4totqt881mlti>, max_fee_per_gas: Anonymize<Ic4rgfgksgmm3e>, max_priority_fee_per_gas: Anonymize<Ic4rgfgksgmm3e>, nonce: Anonymize<Ic4rgfgksgmm3e>, estimate: boolean, access_list: Anonymize<I3dj14b7k3rkm5>, authorization_list: Anonymize<Ic5egmm215ml6k>], Anonymize<I7efspe2svrt0g>>;
        /**
         * Return the current block.
         */
        current_block: RuntimeDescriptor<[], Anonymize<I5fvdd841odbi3>>;
        /**
         * Return the current receipt.
         */
        current_receipts: RuntimeDescriptor<[], Anonymize<I35vouom6s9r2>>;
        /**
         * Return the current transaction status.
         */
        current_transaction_statuses: RuntimeDescriptor<[], Anonymize<Ie6kgk6f04rsvk>>;
        /**
        
         */
        current_all: RuntimeDescriptor<[], Anonymize<Ifgqf2rskq94om>>;
        /**
         * Receives a `Vec<OpaqueExtrinsic>` and filters all the ethereum transactions.
         */
        extrinsic_filter: RuntimeDescriptor<[xts: Anonymize<Itom7fk49o0c9>], Anonymize<Ie30stbbeaul1o>>;
        /**
         * Return the elasticity multiplier.
         */
        elasticity: RuntimeDescriptor<[], Anonymize<I4arjljr6dpflb>>;
        /**
         * Used to determine if gas limit multiplier for non-transactional calls (eth_call/estimateGas)
         * is supported.
         */
        gas_limit_multiplier_support: RuntimeDescriptor<[], undefined>;
        /**
         * Return the pending block.
         */
        pending_block: RuntimeDescriptor<[xts: Anonymize<Itom7fk49o0c9>], Anonymize<I7aold6s47n103>>;
        /**
         * Initialize the pending block.
         * The behavior should be the same as the runtime api Core_initialize_block but
         * for a "pending" block.
         * If your project don't need to have a different behavior to initialize "pending" blocks,
         * you can copy your Core_initialize_block implementation.
         */
        initialize_pending_block: RuntimeDescriptor<[header: Anonymize<Ic952bubvq4k7d>], undefined>;
    };
    /**
    
     */
    ConvertTransactionRuntimeApi: {
        /**
        
         */
        convert_transaction: RuntimeDescriptor<[transaction: Anonymize<Ibjuap2vk03rp6>], Binary>;
    };
    /**
     * The API used to dry-run contract interactions.
     */
    ContractsApi: {
        /**
         * Perform a call from a specified account to a given contract.
         *
         * See [`crate::Pallet::bare_call`].
         */
        call: RuntimeDescriptor<[origin: SS58String, dest: SS58String, value: bigint, gas_limit: Anonymize<Iasb8k6ash5mjn>, storage_deposit_limit: Anonymize<I35p85j063s0il>, input_data: Binary], Anonymize<Ifla7g8u5j9k68>>;
        /**
         * Instantiate a new contract.
         *
         * See `[crate::Pallet::bare_instantiate]`.
         */
        instantiate: RuntimeDescriptor<[origin: SS58String, value: bigint, gas_limit: Anonymize<Iasb8k6ash5mjn>, storage_deposit_limit: Anonymize<I35p85j063s0il>, code: Anonymize<I9sijb8gfrns29>, data: Binary, salt: Binary], Anonymize<I17s97pb2d5tj3>>;
        /**
         * Upload new code without instantiating a contract from it.
         *
         * See [`crate::Pallet::bare_upload_code`].
         */
        upload_code: RuntimeDescriptor<[origin: SS58String, code: Binary, storage_deposit_limit: Anonymize<I35p85j063s0il>, determinism: Anonymize<I2dfliekq1ed7e>], Anonymize<I4gah17u2nc33h>>;
        /**
         * Query a given storage key in a given contract.
         *
         * Returns `Ok(Some(Vec<u8>))` if the storage value exists under the given key in the
         * specified account and `Ok(None)` if it doesn't. If the account specified by the address
         * doesn't exist, or doesn't have a contract then `Err` is returned.
         */
        get_storage: RuntimeDescriptor<[address: SS58String, key: Binary], Anonymize<I9u22scd4ksrjm>>;
    };
    /**
    
     */
    DelegateInfoRuntimeApi: {
        /**
        
         */
        get_delegates: RuntimeDescriptor<[], Anonymize<Ibil6rvg3saeb3>>;
        /**
        
         */
        get_delegate: RuntimeDescriptor<[delegate_account: SS58String], Anonymize<I97cs1i8k87lnm>>;
        /**
        
         */
        get_delegated: RuntimeDescriptor<[delegatee_account: SS58String], Anonymize<I874e758ge6pa9>>;
    };
    /**
    
     */
    NeuronInfoRuntimeApi: {
        /**
        
         */
        get_neurons: RuntimeDescriptor<[netuid: number], Anonymize<I86tq0h1o8f1g5>>;
        /**
        
         */
        get_neuron: RuntimeDescriptor<[netuid: number, uid: number], Anonymize<I78cq8c9mego2f>>;
        /**
        
         */
        get_neurons_lite: RuntimeDescriptor<[netuid: number], Anonymize<I64hm01ml98m4p>>;
        /**
        
         */
        get_neuron_lite: RuntimeDescriptor<[netuid: number, uid: number], Anonymize<I3gjbugrk45her>>;
    };
    /**
    
     */
    SubnetInfoRuntimeApi: {
        /**
        
         */
        get_subnet_info: RuntimeDescriptor<[netuid: number], Anonymize<I9nvi04b7jiso4>>;
        /**
        
         */
        get_subnets_info: RuntimeDescriptor<[], Anonymize<I6s1052v0hl6mr>>;
        /**
        
         */
        get_subnet_info_v2: RuntimeDescriptor<[netuid: number], Anonymize<I31p8sd8onusg0>>;
        /**
        
         */
        get_subnets_info_v2: RuntimeDescriptor<[], Anonymize<I2vgg418k9gfnm>>;
        /**
        
         */
        get_subnet_hyperparams: RuntimeDescriptor<[netuid: number], Anonymize<I7dp6t7k7a8r36>>;
        /**
        
         */
        get_subnet_hyperparams_v2: RuntimeDescriptor<[netuid: number], Anonymize<Ibtpedbm9ai3hp>>;
        /**
        
         */
        get_all_dynamic_info: RuntimeDescriptor<[], Anonymize<I8ivaf995pho4u>>;
        /**
        
         */
        get_all_metagraphs: RuntimeDescriptor<[], Anonymize<Icr6rj04unermu>>;
        /**
        
         */
        get_metagraph: RuntimeDescriptor<[netuid: number], Anonymize<I5gfdo8kg6rloq>>;
        /**
        
         */
        get_all_mechagraphs: RuntimeDescriptor<[], Anonymize<Icr6rj04unermu>>;
        /**
        
         */
        get_mechagraph: RuntimeDescriptor<[netuid: number, mecid: number], Anonymize<I5gfdo8kg6rloq>>;
        /**
        
         */
        get_dynamic_info: RuntimeDescriptor<[netuid: number], Anonymize<Ibjoh8vk2j7bqd>>;
        /**
        
         */
        get_subnet_state: RuntimeDescriptor<[netuid: number], Anonymize<I2u4s5o1c0r3fu>>;
        /**
        
         */
        get_selective_metagraph: RuntimeDescriptor<[netuid: number, metagraph_indexes: Anonymize<Icgljjb6j82uhn>], Anonymize<Ic0g2vnp5r296p>>;
        /**
        
         */
        get_coldkey_auto_stake_hotkey: RuntimeDescriptor<[coldkey: SS58String, netuid: number], Anonymize<Ihfphjolmsqq1>>;
        /**
        
         */
        get_selective_mechagraph: RuntimeDescriptor<[netuid: number, subid: number, metagraph_indexes: Anonymize<Icgljjb6j82uhn>], Anonymize<Ic0g2vnp5r296p>>;
        /**
        
         */
        get_subnet_to_prune: RuntimeDescriptor<[], Anonymize<I4arjljr6dpflb>>;
    };
    /**
    
     */
    StakeInfoRuntimeApi: {
        /**
        
         */
        get_stake_info_for_coldkey: RuntimeDescriptor<[coldkey_account: SS58String], Anonymize<Ic9fkrj2ggjleq>>;
        /**
        
         */
        get_stake_info_for_coldkeys: RuntimeDescriptor<[coldkey_accounts: Anonymize<Ia2lhg7l2hilo3>], Anonymize<Ifi9cmevnosufh>>;
        /**
        
         */
        get_stake_info_for_hotkey_coldkey_netuid: RuntimeDescriptor<[hotkey_account: SS58String, coldkey_account: SS58String, netuid: number], Anonymize<I1i5jfmqcsjper>>;
        /**
        
         */
        get_stake_fee: RuntimeDescriptor<[origin: Anonymize<I3pbrjdm4vnbsa>, origin_coldkey_account: SS58String, destination: Anonymize<I3pbrjdm4vnbsa>, destination_coldkey_account: SS58String, amount: bigint], bigint>;
    };
    /**
    
     */
    SubnetRegistrationRuntimeApi: {
        /**
        
         */
        get_network_registration_cost: RuntimeDescriptor<[], bigint>;
    };
    /**
     * API necessary for block authorship with BABE.
     */
    BabeApi: {
        /**
         * Return the configuration for BABE.
         */
        configuration: RuntimeDescriptor<[], Anonymize<Iems84l8lk2v0c>>;
        /**
         * Returns the slot that started the current epoch.
         */
        current_epoch_start: RuntimeDescriptor<[], bigint>;
        /**
         * Returns information regarding the current epoch.
         */
        current_epoch: RuntimeDescriptor<[], Anonymize<I1r5ke30ueqo0r>>;
        /**
         * Returns information regarding the next epoch (which was already
         * previously announced).
         */
        next_epoch: RuntimeDescriptor<[], Anonymize<I1r5ke30ueqo0r>>;
        /**
         * Generates a proof of key ownership for the given authority in the
         * current epoch. An example usage of this module is coupled with the
         * session historical module to prove that a given authority key is
         * tied to a given staking identity during a specific session. Proofs
         * of key ownership are necessary for submitting equivocation reports.
         * NOTE: even though the API takes a `slot` as parameter the current
         * implementations ignores this parameter and instead relies on this
         * method being called at the correct block height, i.e. any point at
         * which the epoch for the given slot is live on-chain. Future
         * implementations will instead use indexed data through an offchain
         * worker, not requiring older states to be available.
         */
        generate_key_ownership_proof: RuntimeDescriptor<[slot: bigint, authority_id: FixedSizeBinary<32>], Anonymize<Iabpgqcjikia83>>;
        /**
         * Submits an unsigned extrinsic to report an equivocation. The caller
         * must provide the equivocation proof and a key ownership proof
         * (should be obtained using `generate_key_ownership_proof`). The
         * extrinsic will be unsigned and should only be accepted for local
         * authorship (not to be broadcast to the network). This method returns
         * `None` when creation of the extrinsic fails, e.g. if equivocation
         * reporting is disabled for the given runtime (i.e. this method is
         * hardcoded to return `None`). Only useful in an offchain context.
         */
        submit_report_equivocation_unsigned_extrinsic: RuntimeDescriptor<[equivocation_proof: Anonymize<I68ii5ik8avr9o>, key_owner_proof: Binary], boolean>;
    };
    /**
    
     */
    SwapRuntimeApi: {
        /**
        
         */
        current_alpha_price: RuntimeDescriptor<[netuid: number], bigint>;
        /**
        
         */
        current_alpha_price_all: RuntimeDescriptor<[], Anonymize<I8slfm2rri67ri>>;
        /**
        
         */
        sim_swap_tao_for_alpha: RuntimeDescriptor<[netuid: number, tao: bigint], Anonymize<I34n2itmpoq7on>>;
        /**
        
         */
        sim_swap_alpha_for_tao: RuntimeDescriptor<[netuid: number, alpha: bigint], Anonymize<I34n2itmpoq7on>>;
    };
};
export type DevnetDispatchError = Anonymize<Ic871mj76419vm>;
type IAsset = PlainDescriptor<void>;
export type DevnetExtensions = {};
type PalletsTypedef = {
    __storage: IStorage;
    __tx: ICalls;
    __event: IEvent;
    __error: IError;
    __const: IConstants;
    __view: IViewFns;
};
export type Devnet = {
    descriptors: {
        pallets: PalletsTypedef;
        apis: IRuntimeCalls;
    } & Promise<any>;
    metadataTypes: Promise<Uint8Array>;
    asset: IAsset;
    extensions: DevnetExtensions;
    getMetadata: () => Promise<Uint8Array>;
    genesis: string | undefined;
};
declare const _allDescriptors: Devnet;
export default _allDescriptors;
export type DevnetApis = ApisFromDef<IRuntimeCalls>;
export type DevnetQueries = QueryFromPalletsDef<PalletsTypedef>;
export type DevnetCalls = TxFromPalletsDef<PalletsTypedef>;
export type DevnetEvents = EventsFromPalletsDef<PalletsTypedef>;
export type DevnetErrors = ErrorsFromPalletsDef<PalletsTypedef>;
export type DevnetConstants = ConstFromPalletsDef<PalletsTypedef>;
export type DevnetViewFns = ViewFnsFromPalletsDef<PalletsTypedef>;
export type DevnetCallData = Anonymize<I8vbtb6bd00lm0> & {
    value: {
        type: string;
    };
};
export type DevnetWhitelistEntry = PalletKey | ApiKey<IRuntimeCalls> | `query.${NestedKey<PalletsTypedef['__storage']>}` | `tx.${NestedKey<PalletsTypedef['__tx']>}` | `event.${NestedKey<PalletsTypedef['__event']>}` | `error.${NestedKey<PalletsTypedef['__error']>}` | `const.${NestedKey<PalletsTypedef['__const']>}` | `view.${NestedKey<PalletsTypedef['__view']>}`;
type PalletKey = `*.${keyof (IStorage & ICalls & IEvent & IError & IConstants & IRuntimeCalls & IViewFns)}`;
type NestedKey<D extends Record<string, Record<string, any>>> = "*" | {
    [P in keyof D & string]: `${P}.*` | {
        [N in keyof D[P] & string]: `${P}.${N}`;
    }[keyof D[P] & string];
}[keyof D & string];
type ApiKey<D extends Record<string, Record<string, any>>> = "api.*" | {
    [P in keyof D & string]: `api.${P}.*` | {
        [N in keyof D[P] & string]: `api.${P}.${N}`;
    }[keyof D[P] & string];
}[keyof D & string];
