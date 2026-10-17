from pathlib import Path
import json
N=Path(__file__).parent;O=Path('/tmp/ms-head-validation-20261017');d=N/'rpc';d.mkdir(exist_ok=True)
chains=json.loads((N/'chains.json').read_text());chains.append(dict(name='rpc',directory=str(d),rpc_port=21044,p2p_port=32034,normal_block_seconds=12,archive=True));(N/'chains.json').write_text(json.dumps(chains,indent=2))
s=(N/'release/localnet.sh').read_text().replace(str(N/'release'),str(d))
for i in range(3):s=s.replace(str(21144+i),str(21044+i)).replace(str(32134+i),str(32034+i))
(d/'localnet.sh').write_text(s)
(d/'genesis.py').write_text((N/'release/genesis.py').read_text().replace('(100).to_bytes','(150).to_bytes').replace('483b52ceea-release','483b52ceea-rpc'))
for f in ['live_rpc.py','ethereum_pending_transfers.py','pending_contract_semantics.py']:
 (d/f).write_text((O/'rpc'/f).read_text().replace(str(O),str(N)))
s=(N/'run-live.py').read_text();needle='    with (directory / "activation.log").open("w") as output:'
s=s.replace(needle,'''    if name == "rpc":
        with (directory / "runner.log").open("w") as output:
            process = subprocess.Popen([str(PYTHON), str(directory / "run.py")], cwd=ROOT / "sdk/python", env=env, stdout=output, stderr=subprocess.STDOUT)
            children.append(process)
            return {"rpc_exit": process.wait(timeout=7200)}
'''+needle)
(N/'run-live.py').write_text(s)
s=(N/'verify-chains.py').read_text().replace('==13','==14').replace("'distinct_chains':13","'distinct_chains':14").replace('All13','All14');(N/'verify-chains.py').write_text(s)
s=(N/'receipt-verification/verify.py').read_text().replace('437053f80d5ebebce950602b168901bfe6591483','483b52ceea5cf08a0fb492ca2915b7efa6a8690e');(N/'receipt-verification/verify.py').write_text(s)
