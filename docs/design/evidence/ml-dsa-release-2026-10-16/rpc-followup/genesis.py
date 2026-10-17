import json,sys,hashlib
from pathlib import Path
p=Path(sys.argv[1]); spec=json.loads(p.read_text())
k="0x26aa394eea5630e07c48ae0c9558cef7b99d880ec681799c0cf30e8886371da9de1e86a9a8c739864cf3cc5ec2bea59fd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d"
v=bytearray.fromhex(spec["genesis"]["raw"]["top"][k][2:]); assert v[:4]==bytes(4); v[:4]=(7).to_bytes(4,"little")
spec["genesis"]["raw"]["top"][k]="0x"+v.hex()
spec["protocolId"]="ms-validation-rpc-followup"; spec["forkId"]="ms-validation-rpc-followup"
p.write_text(json.dumps(spec))
Path("/tmp/ms-release-validation/rpc-followup/isolation.json").write_text(json.dumps({"genesis_patch":"Alice System.Account nonce=7; unique protocolId/forkId","spec_sha256":hashlib.sha256(p.read_bytes()).hexdigest(),"timing":"normal 12 seconds","pruning":"archive"},indent=2)+"\n")
