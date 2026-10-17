#![allow(deprecated)]
use fips_local::{ml_dsa_65 as local, traits::{KeyGen as _, SerDes as _, Signer as _, Verifier as _}};
use fips_ref::{ml_dsa_65 as reference, traits::{KeyGen as _, SerDes as _, Signer as _, Verifier as _}};
use serde_json::{Value, json};
use sha3::{Shake256, digest::{Update, ExtendableOutput, XofReader}};
use std::{fs, path::Path};
fn hx<const N:usize>(v: &Value)->[u8;N] {hex::decode(v.as_str().unwrap()).unwrap().try_into().unwrap()}
fn shake(parts:&[&[u8]])->[u8;64] {let mut h=Shake256::default();for p in parts {h.update(p)}let mut out=[0;64];h.finalize_xof().read(&mut out);out}
fn main(){
 let base=Path::new("/home/johnreed/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/fips204-0.4.6/tests/nist_vectors");
 let mut counts=std::collections::BTreeMap::<&str,usize>::new();
 for mode in ["keyGen","sigGen","sigVer"] {
  let file=base.join(format!("ML-DSA-{mode}-FIPS204/internalProjection.json"));
  let j:Value=serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
  for group in j["testGroups"].as_array().unwrap().iter().filter(|g| g["parameterSet"]=="ML-DSA-65") {
   for t in group["tests"].as_array().unwrap(){
    match mode {
     "keyGen"=>{let (p,s)=local::KG::keygen_from_seed(&hx(&t["seed"]));assert_eq!(p.into_bytes(),hx::<1952>(&t["pk"]));assert_eq!(s.into_bytes(),hx::<4032>(&t["sk"]));*counts.entry("nist_keygen").or_default()+=1},
     "sigGen"=>{let s=local::PrivateKey::try_from_bytes(hx(&t["sk"])).unwrap();let m=hex::decode(t["message"].as_str().unwrap()).unwrap();let rnd=if group["deterministic"]==true{[0;32]}else{hx(&t["rnd"])};assert_eq!(local::_internal_sign(&s,&m,&[],rnd).unwrap(),hx::<3309>(&t["signature"]));*counts.entry("nist_siggen").or_default()+=1},
     "sigVer"=>{let p=local::PublicKey::try_from_bytes(hx(&group["pk"])).unwrap();let m=hex::decode(t["message"].as_str().unwrap()).unwrap();assert_eq!(local::_internal_verify(&p,&m,&hx(&t["signature"]),&[]),t["testPassed"].as_bool().unwrap());*counts.entry("nist_sigver").or_default()+=1},_=>unreachable!()
    }
   }
  }
 }
 for n in 0u16..128 {
  let seed:[u8;32]=std::array::from_fn(|i|(n as u8).wrapping_add((i*7)as u8));
  let (lp,ls)=local::KG::keygen_from_seed(&seed);let(rp,rs)=reference::KG::keygen_from_seed(&seed);
  let lbytes=lp.clone().into_bytes();assert_eq!(lbytes,rp.clone().into_bytes());assert_eq!(ls.clone().into_bytes(),rs.clone().into_bytes());
  let mlen=[0,1,255,256,257,8192,16384,65536][n as usize%8];
  let m:Vec<u8>=(0..mlen).map(|i| (i as u8).wrapping_add(n as u8)).collect();
  let ctx=vec![(n as u8)^0x55;[0,1,18,255][n as usize%4]];let rnd=if n%2==0{[0;32]}else{[n as u8;32]};
  let sig=ls.try_sign_with_seed(&rnd,&m,&ctx).unwrap();assert_eq!(sig,rs.try_sign_with_seed(&rnd,&m,&ctx).unwrap());
  assert!(lp.verify(&m,&sig,&ctx));assert!(rp.verify(&m,&sig,&ctx));
  let tr=shake(&[&lbytes]);let mu=shake(&[&tr,&[0,ctx.len()as u8],&ctx,&m]);assert!(lp.verify_mu(&mu,&sig));
  let mut badmu=mu;badmu[0]^=1;assert!(!lp.verify_mu(&badmu,&sig));
  let mut badctx=ctx.clone();badctx.push(1);assert!(!lp.verify(&m,&sig,&badctx));
  assert!(!lp.verify(&m,&sig,&vec![0;256]));
  *counts.entry("upstream_key_sign_verify_mu_differential").or_default()+=1;
  for off in [0,47,48,100,3247,3248,3250,3302,3303,3308]{let mut bad=sig;bad[off]^=1;let l=lp.verify(&m,&bad,&ctx);assert_eq!(l,rp.verify(&m,&bad,&ctx));assert!(!l);*counts.entry("mutated_signature_differential").or_default()+=1;}
  if n==0 {
   for off in 0..3309 {let mut bad=sig;bad[off]^=0x80;let l=lp.verify(&m,&bad,&ctx);assert_eq!(l,rp.verify(&m,&bad,&ctx));assert!(!l);*counts.entry("all_signature_byte_mutations").or_default()+=1;}
   for off in 3248..3309 {let mut bad=sig;bad[off]=255;let l=lp.verify(&m,&bad,&ctx);assert_eq!(l,rp.verify(&m,&bad,&ctx));if bad!=sig{assert!(!l)}*counts.entry("malformed_hint_differential").or_default()+=1;}
  }
 }
 println!("{}",serde_json::to_string_pretty(&json!({"status":"PASS","upstream_commit":"8d456d989dce681233ef963d3c3306da8f76d7c7","acvp_revision":"65370b861b96efd30dfe0daae607bde26a78a5c8","counts":counts})).unwrap());
}
