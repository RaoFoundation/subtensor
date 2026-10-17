from pathlib import Path
import hashlib,json,subprocess,os
root=Path('/home/johnreed/Desktop/work/subtensor');out=Path('/tmp/ms-release-validation/pending-helper');source=root/'vendor/frontier/client/rpc/src/eth/pending.rs';original=source.read_text()
production=original[original.index('/// Only a pool transaction'):original.index('impl<B, C, P, CT, BE, CIDP, EC> Eth')]
tests=original[original.index('#[cfg(test)]\nmod tests {'):original.index('/// Consensus data provider')]
extracted=out/'pending_helper_tests.rs';extracted.write_text('use sp_runtime::TransactionOutcome;\nuse sp_blockchain::ApplyExtrinsicFailed;\n'+production+tests)
release=root/'target/release';fingerprints=release/'.fingerprint'
fc=max([p for p in fingerprints.glob('fc-rpc-*/lib-fc_rpc.json') if (release/'deps'/f'libfc_rpc-{p.parent.name.rsplit("-",1)[1]}.rlib').exists()],key=lambda p:p.stat().st_mtime_ns)
index={}
for p in fingerprints.glob('*/lib-*'):
 if p.suffix:continue
 try:digest=int.from_bytes(bytes.fromhex(p.read_text()),'little')
 except ValueError:continue
 index[digest]=p.with_suffix('.json')
deps={name:index[digest] for _,name,_,digest in json.loads(fc.read_text())['deps'] if name in ('sp_api','sp_runtime','sp_blockchain','sp_inherents','thiserror','fp_rpc')}
assert len(deps)==6
cmd=['/home/johnreed/.cargo/bin/rustc','--edition=2021','--test','-O',str(extracted),'-o',str(out/'pending-helper-tests'),'-L',f'dependency={release/"deps"}']
evidence={'scope':'Exact source-extracted production outcome helper, complete production Error enum, and all three source tests; full pending_runtime_api module is compiled separately by production node build.','baseline':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'source':str(source),'source_sha256':hashlib.sha256(original.encode()).hexdigest(),'harness_sha256':hashlib.sha256(extracted.read_bytes()).hexdigest(),'fc_rpc_fingerprint':str(fc),'dependencies':{}}
for name,p in deps.items():
 crate=p.stem.removeprefix('lib-');tag=p.parent.name.rsplit('-',1)[1];library=release/'deps'/f'lib{crate}-{tag}.rlib';assert library.exists(),library
 cmd+=['--extern',f'{name}={library}'];evidence['dependencies'][name]={'path':str(library),'sha256':hashlib.sha256(library.read_bytes()).hexdigest()}
for p in sorted((release/'build').glob('*/out')):cmd+=['-L',f'native={p}']
subprocess.run(cmd,cwd=root,check=True)
res=subprocess.run([str(out/'pending-helper-tests'),'--nocapture'],cwd=root,check=True,text=True,capture_output=True)
(out/'tests.log').write_text(res.stdout+res.stderr);evidence['status']='PASS';evidence['test_count']=3;evidence['rustc']=subprocess.check_output(['/home/johnreed/.cargo/bin/rustc','--version','--verbose'],text=True)
(out/'evidence.json').write_text(json.dumps(evidence,indent=2)+'\n');print(res.stdout)
