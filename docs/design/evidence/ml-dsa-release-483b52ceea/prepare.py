import json, pathlib, shutil, subprocess
R=pathlib.Path('/home/johnreed/Desktop/work/subtensor'); O=pathlib.Path('/tmp/ms-head-validation-20261017'); N=pathlib.Path(__file__).parent
chains=json.loads((O/'chains.json').read_text())[1:]+json.loads((O/'chains-extra.json').read_text())
for i,name in enumerate(['sponsorship','nested','validator-proxy']):
 chains.append(dict(name=name,directory=str(O/name),rpc_port=22144+i*100,p2p_port=33134+i*100,normal_block_seconds=12,archive=True))
for i,c in enumerate(chains):
 d=N/c['name'];d.mkdir(exist_ok=True);c['directory']=str(d)
 template=O/c['name']/'localnet.sh'
 if not template.exists(): template=O/'release/localnet.sh'
 s=template.read_text();oldname=template.parent.name
 s=s.replace(str(O/oldname),str(d)).replace(str(O),str(N))
 # set all ports from actual template
 import re
 oldrpc=int(re.search(r'--rpc-port (\d+)',s)[1]); oldp=int(re.search(r'--port (\d+)',s)[1])
 for j in range(3): s=s.replace(f'--rpc-port {oldrpc+j}',f'--rpc-port {c["rpc_port"]+j}').replace(f'--port {oldp+j}',f'--port {c["p2p_port"]+j}')
 (d/'localnet.sh').write_text(s)
 genesis=(O/'release/genesis.py').read_text().replace(str(O),str(N)).replace('test-runtime.wasm','runtime.wasm').replace('(21).to_bytes',f'({100+i}).to_bytes').replace('ms-head-20261017-release',f'ms-release-483b52ceea-{c["name"]}')
 (d/'genesis.py').write_text(genesis)
(N/'chains.json').write_text(json.dumps(chains,indent=2))
s=(O/'run-live-extra.py').read_text().replace('chains-extra.json','chains.json').replace('live-results-extra.json','live-results.json').replace('shutdown-extra.json','shutdown.json').replace('launchers-extra.json','launchers.json')
s=s.replace('suites = {','suites = {\n    "sponsorship": ("test_mldsa_sponsorship_live.py", 4),\n    "nested": ("test_mldsa_nested_authority.py", 0),\n    "validator-proxy": ("test_mldsa_validator_proxy.py", 2),')
# Keep chains alive for canonical receipt / decoder verification; signal terminates finally.
s=s.replace('finally:\n    shutdown()', '    print("ALL_SUITES_FINISHED", flush=True)\n    while not (RUN / "stop").exists():\n        time.sleep(2)\nfinally:\n    shutdown()')
(N/'run-live.py').write_text(s)
a=(O/'activate.py').read_text().replace('("production-runtime.wasm" if name == "production" else "test-runtime.wasm")','"runtime.wasm"')
a=a.replace('        for flag in ("MlDsaRegistrationEnabled", "MlDsaAuthorizationEnabled"):\n            assert await c.query(("HashedAccounts", flag)) is False\n','').replace('            bt.calls.HashedAccounts.set_mldsa_enabled(registration=True, authorization=True),\n','')
(N/'activate.py').write_text(a)
(N/'initial-manifest.json').write_text(json.dumps({'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'runtime_overlay':False,'blocks_seconds':12,'chains':len(chains),'csv_line_endings_only':True},indent=2))
