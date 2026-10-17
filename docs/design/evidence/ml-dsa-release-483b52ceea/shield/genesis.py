import hashlib,json,sys
from pathlib import Path
p=Path(sys.argv[1]);s=json.loads(p.read_text());top=s['genesis']['raw']['top']
top['0x3a636f6465']='0x'+Path('/tmp/ms-release-483b52ceea/runtime.wasm').read_bytes().hex()
key="0x26aa394eea5630e07c48ae0c9558cef7b99d880ec681799c0cf30e8886371da9de1e86a9a8c739864cf3cc5ec2bea59fd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d"
assert top[key].startswith('0x00000000')
top[key]='0x'+(109).to_bytes(4,'little').hex()+top[key][10:]
s['protocolId']=s['forkId']='ms-release-483b52ceea-shield'
p.write_text(json.dumps(s))
