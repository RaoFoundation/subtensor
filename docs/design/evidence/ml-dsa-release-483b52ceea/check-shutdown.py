from pathlib import Path
from datetime import datetime,timezone
import json,socket,subprocess
P=Path(__file__).parent
ports=[]
for c in json.loads((P/'chains.json').read_text()):
 for base in [c['rpc_port'],c['p2p_port']]:
  for port in range(base,base+3):
   with socket.socket() as s:s.settimeout(.2);closed=s.connect_ex(('127.0.0.1',port))!=0
   ports.append({'chain':c['name'],'port':port,'closed':closed})
with socket.socket() as s:s.settimeout(.2);qr_closed=s.connect_ex(('127.0.0.1',9977))!=0
processes=[]
for d in Path('/proc').iterdir():
 if not d.name.isdigit():continue
 try:
  parts=(d/'cmdline').read_bytes().split(b'\0');args=[s.decode(errors='replace') for s in parts if s]
 except (OSError,PermissionError):continue
 if not args:continue
 owned_node=args[0]==str(P/'node-subtensor')
 owned_pytest='pytest' in args and any(str(P) in s for s in args)
 owned_launcher=any(s==str(P/c['name']/'localnet.sh') for c in json.loads((P/'chains.json').read_text()) for s in args)
 if owned_node or owned_pytest or owned_launcher:processes.append({'pid':int(d.name),'args':args})
r={'checked_utc':datetime.now(timezone.utc).isoformat(),'ports':ports,'all_closed':all(x['closed'] for x in ports) and qr_closed,'qr_port_9977_closed':qr_closed,'owned_processes':processes,'owned_processes_remaining':len(processes)}
assert r['all_closed'] and not processes,r
(P/'shutdown-final.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'all_closed':r['all_closed'],'checked_chain_ports':len(ports),'owned_processes_remaining':len(processes),'qr_port_9977_closed':qr_closed}))
