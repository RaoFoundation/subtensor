extern crate test;

const NO_OF_MULTI_SIG_SIGNERS : usize = 10000;
    use test::{Bencher, black_box};

    // #[bench]
    // fn only_generate_key_pairs(b: &mut Bencher) {
    //     b.iter(|| {

    //         let mut keypairs = generate_many_keypairs(NO_OF_MULTI_SIG_SIGNERS);
    //     });
    // }
    
    #[bench]
    fn test_many_tiny_aggregate_and_verify_in_g2(b: &mut Bencher) {
        let message = Message::new(b"ctx",b"test message");
        let mut keypairs = generate_many_keypairs(NO_OF_MULTI_SIG_SIGNERS);
	let mut pub_keys_in_sig_grp : Vec<PublicKeyInSignatureGroup<TinyBLS377>> = keypairs.iter().map(|k| k.into_public_key_in_signature_group()).collect();

	let mut aggregated_public_key = PublicKey::<TinyBLS377>(<TinyBLS377 as EngineBLS>::PublicKeyGroup::zero());
	let mut aggregator = MultiMessageSignatureAggregatorAssumingPoP::<TinyBLS377>::new();

        for k in &mut keypairs {
	    aggregator.aggregate(&k.signed_message(message));
	    aggregated_public_key.0 += k.public.0;
        }
	
        b.iter(|| {
	    let mut verifier_aggregator = MultiMessageSignatureAggregatorAssumingPoP::<TinyBLS377>::new();
	    let mut verifier_aggregated_public_key = PublicKey::<TinyBLS377>(<TinyBLS377 as EngineBLS>::PublicKeyGroup::zero());

	    verifier_aggregator.add_signature(&aggregator.signature);

	    for k in &mut keypairs {
		verifier_aggregated_public_key.0 += k.public.0;
            }

	    verifier_aggregator.add_message_n_publickey(&message, &verifier_aggregated_public_key);
	    
            assert!(verifier_aggregator.verify());
        });
    }

    // #[bench]
    // fn test_many_tiny_aggregate_only_no_verify(b: &mut Bencher) {
    //     let mut keypairs = generate_many_keypairs(NO_OF_MULTI_SIG_SIGNERS);
    // 	let mut pub_keys_in_sig_grp : Vec<PublicKeyInSignatureGroup<TinyBLS377>> = keypairs.iter().map(|k| k.into_public_key_in_signature_group()).collect();
    //     let message = Message::new(b"ctx",b"test message");

    //     b.iter(|| {
    //         let mut aggregator = MultiMessageSignatureAggregatorAssumingPoP::<TinyBLS377>::new();
    // 	    let mut aggregated_public_key = PublicKey::<TinyBLS377>(<TinyBLS377 as EngineBLS>::PublicKeyGroup::zero());

    //         for k in &mut keypairs {
    //             aggregator.aggregate(&k.signed_message(message));
    // 		aggregated_public_key.0 += k.public.0;
		
		
    //         }
    //     });
    // }

    #[bench]
    fn test_many_tiny_aggregate_and_verify_in_g1(b: &mut Bencher) {
        let message = Message::new(b"ctx",b"test message");
        let mut keypairs = generate_many_keypairs(NO_OF_MULTI_SIG_SIGNERS);
	let mut pub_keys_in_sig_grp : Vec<PublicKeyInSignatureGroup<TinyBLS377>> = keypairs.iter().map(|k| k.into_public_key_in_signature_group()).collect();
 
        let mut aggregator = MultiMessageSignatureAggregatorAssumingPoP::<TinyBLS377>::new();
	let mut aggregated_public_key = PublicKey::<TinyBLS377>(<TinyBLS377 as EngineBLS>::PublicKeyGroup::zero());

        for k in &mut keypairs {
	    aggregator.aggregate(&k.signed_message(message));
	    aggregated_public_key.0 += k.public.0;
        }

        b.iter(|| {
	    let mut verifier_aggregator = MultiMessageSignatureAggregatorAssumingPoP::<TinyBLS377>::new();

	    verifier_aggregator.add_signature(&aggregator.signature);
	    verifier_aggregator.add_message_n_publickey(&message, &aggregated_public_key);
	    
            for k in &pub_keys_in_sig_grp {
		verifier_aggregator.add_auxiliary_public_key(k);
	    }

            assert!(verifier_aggregator.verify_using_aggregated_auxiliary_public_keys());

        });
                                                 
    }

    const NO_OF_MULTI_SIG_SIGNERS : usize = 1000;
    use test::{Bencher, black_box};
    //#[bench]
    fn test_bls_verify_many_signatures_simple(b: &mut Bencher) {
        let good = Message::new(b"ctx",b"test message");

        let mut keypair = Keypair::<TinyBLS377>::generate(thread_rng());
        let message = Message::new(b"ctx",b"test message");

        let sig = keypair.signed_message(message);

	b.iter(||
        for i in 1..NO_OF_MULTI_SIG_SIGNERS {
            sig.verify();
        });                                            
    }

    //#[bench]
    fn test_bls_verify_many_signatures_chaum_pedersen(b: &mut Bencher) {
        let mut keypair = Keypair::<TinyBLS377>::generate(thread_rng());
        let message = Message::new(b"ctx",b"test message");

        let sig = <Keypair<TinyBLS377> as ChaumPedersenSigner<TinyBLS377, Sha256>>::generate_cp_signature(&mut keypair, message);
        let public_key_in_sig_group = keypair.into_public_key_in_signature_group();

	b.iter(||
               for i in 1..NO_OF_MULTI_SIG_SIGNERS {
		   assert!(<PublicKeyInSignatureGroup<TinyBLS377> as ChaumPedersenVerifier<TinyBLS377, Sha256>>::verify_cp_signature(&public_key_in_sig_group, message,sig));
        });
    }

    //#[bench]
    fn test_pairing(b: &mut Bencher) {
        let mut keypair1 = Keypair::<TinyBLS377>::generate(thread_rng());

	let point_1 = keypair1.into_public_key_in_signature_group().0;
	let point_2 = keypair1.public.0;

	b.iter(||
	for i in 0..NO_OF_MULTI_SIG_SIGNERS {
            TinyBLS377::pairing(point_2, point_1);
        });

    }
    //#[bench]
    fn test_scalar_multiplication(b: &mut Bencher) {
        let mut keypair1 = Keypair::<TinyBLS377>::generate(thread_rng());

	let point_1 = keypair1.into_public_key_in_signature_group().0;
	let point_2 = keypair1.public.0;
	let scalar = keypair1.secret.into_vartime().0;

	b.iter(||
	       for i in 0..NO_OF_MULTI_SIG_SIGNERS {
		   point_1 * scalar;
               });

    }
