//! Fee baseline guard.
//!
//! `fee_baseline/pins.tsv` pins, for every dispatchable, the fee (rao) a 100-byte
//! extrinsic with one unit of every argument is quoted on spec 467. This test fails when
//! any current fee is above its pin, so a benchmark regen or a new declared bound that
//! raises what a user pays cannot land silently; someone must raise the pin on purpose.
//!
//! Regenerate the table after a deliberate fee change:
//! `FEE_BASELINE_PRINT=1 cargo test -p node-subtensor-runtime --test fee_baseline -- --nocapture`
//! and paste the printed lines into `fee_baseline/pins.tsv`.

#![allow(clippy::expect_used)]

use codec::{Decode, Encode};
use frame_support::dispatch::GetDispatchInfo;
use node_subtensor_runtime::transaction_payment_wrapper::{FeeWeightDiscount, fee_dispatch_info};
use node_subtensor_runtime::{
    BuildStorage, Runtime, RuntimeCall, RuntimeGenesisConfig, System, TransactionPayment,
};
use subtensor_runtime_common::{TaoBalance, Token};

const PINS: &str = include_str!("fee_baseline/pins.tsv");
/// Encoded extrinsic length every pin is quoted at.
const LEN: u32 = 100;
/// Finney has more than `MAX_UNSTAKE_ALL_LEGS` subnets; price bulk unstakes at the cap.
const NETWORKS: u16 = 128;

fn new_test_ext() -> sp_io::TestExternalities {
    let mut ext: sp_io::TestExternalities = RuntimeGenesisConfig::default()
        .build_storage()
        .expect("runtime genesis storage builds")
        .into();
    ext.execute_with(|| {
        System::set_block_number(1);
        pallet_subtensor::TotalNetworks::<Runtime>::put(NETWORKS);
    });
    ext
}

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = core::str::from_utf8(pair).expect("ascii hex");
            u8::from_str_radix(pair, 16).expect("hex digit pair")
        })
        .collect()
}

struct Pin {
    name: &'static str,
    call: RuntimeCall,
    fee_rao: u64,
}

fn pins() -> Vec<Pin> {
    PINS.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().expect("name");
            let hex = fields.next().expect("call hex");
            let fee_rao = fields
                .next()
                .expect("fee")
                .parse::<u64>()
                .expect("fee in rao");
            let call = RuntimeCall::decode(&mut unhex(hex).as_slice())
                .unwrap_or_else(|error| panic!("{name}: call bytes no longer decode: {error:?}"));
            Pin {
                name,
                call,
                fee_rao,
            }
        })
        .collect()
}

/// What the fee wrapper bills for `call`: declared weight minus every discount, then the
/// same `compute_fee` the node's `payment_queryInfo` uses.
fn quoted_fee_rao(call: &RuntimeCall) -> u64 {
    let info = call.get_dispatch_info();
    let fee_info = fee_dispatch_info(&info, Runtime::fee_weight_discount(call, &info));
    TransactionPayment::compute_fee(LEN, &fee_info, TaoBalance::new(0)).to_u64()
}

#[test]
fn every_dispatchable_fee_is_at_or_below_its_467_pin() {
    new_test_ext().execute_with(|| {
        let pins = pins();
        assert!(
            pins.len() > 250,
            "pin table lost most of its rows: {}",
            pins.len()
        );
        let print = std::env::var_os("FEE_BASELINE_PRINT").is_some();
        let mut above = Vec::new();
        for pin in &pins {
            let fee = quoted_fee_rao(&pin.call);
            if print {
                println!("{}\t{}\t{fee}", pin.name, hex_of(&pin.call));
            }
            if fee > pin.fee_rao {
                above.push(format!(
                    "{} quotes {fee} rao, pinned {}",
                    pin.name, pin.fee_rao
                ));
            }
        }
        assert!(
            above.is_empty(),
            "fees rose above their 467 pins; lower the fee or raise the pin on purpose:\n{}",
            above.join("\n")
        );
    });
}

fn hex_of(call: &RuntimeCall) -> String {
    call.encode()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
