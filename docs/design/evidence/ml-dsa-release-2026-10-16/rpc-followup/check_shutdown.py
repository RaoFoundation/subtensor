import datetime,json,socket
from pathlib import Path
root=Path('/tmp/ms-release-validation/rpc-followup')
prior=json.loads(Path('docs/design/evidence/ml-dsa-release-2026-10-16/local/all-localnets-shutdown.json').read_text())
ports=sorted(set(map(int,prior['ports_closed']))|{20544,20545,20546,20550,31534,31535,31536,9615})
nodes=[]; runners=[]
for p in Path('/proc').iterdir():
 if not p.name.isdigit(): continue
 try:
  cmd=p.joinpath('cmdline').read_bytes().split(b'\0')
  args=[x.decode(errors='replace') for x in cmd if x]
  if not args: continue
  if Path(args[0]).name=='node-subtensor': nodes.append({'pid':int(p.name),'argv':args})
  elif len(args)>1 and (args[1].endswith('/localnet.sh') or args[1].endswith('/run-fixed.sh') or args[1].endswith('/run-resumed.sh') or args[1].endswith('/run-baseline-corrected.sh')): runners.append({'pid':int(p.name),'argv':args})
 except (FileNotFoundError,ProcessLookupError,PermissionError): pass
closed={}
for port in ports:
 with socket.socket() as sock:
  sock.settimeout(.2)
  closed[str(port)]=sock.connect_ex(('127.0.0.1',port))!=0
out={'observed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'node_processes_remaining':nodes,'localnet_launchers_remaining':runners,'ports_closed':closed,'task_pid_file_absent':not(root/'localnet.pids').exists(),'scope':'Process-wide node-subtensor scan and all original/follow-up RPC/P2P, preflight, and metrics ports. Original five localnets plus sequential baseline/repaired follow-up launches and archive resumption.','passed':not nodes and not runners and all(closed.values()) and not(root/'localnet.pids').exists()}
(root/'shutdown.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps(out,indent=2))
assert out['passed']
