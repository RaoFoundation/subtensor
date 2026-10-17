from pathlib import Path
from collections import Counter
from datetime import datetime,timezone
import hashlib,json
P=Path(__file__).parent;D=P/'receipt-verification'
names=['completed-first','completed-extended','completed-parity','completed-authority','completed-release','completed-operations','completed-lifecycle']
all_rows=[];parts={};seen=set();files={}
for name in names:
 path=D/(name+'.json');r=json.loads(path.read_text());assert not r['errors'],name
 s=json.loads((D/(name+'-summary.json')).read_text());assert s['all_recorded_receipts_verified'],name
 parts[name]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),**s}
 for f in r['files']:
  src=P/f['path'];assert hashlib.sha256(src.read_bytes()).hexdigest()==f['sha256'],f['path'];files[f['path']]=f['sha256']
 for row in r['receipts']:
  key=(row['chain'],row['block_hash'],row['extrinsic_index']);assert key not in seen,key;seen.add(key)
  assert row['canonical_finalized'],key;all_rows.append(row)
summary={'candidate':(P/'head.txt').read_text().strip(),'finished_utc':datetime.now(timezone.utc).isoformat(),'unique_receipts':len(all_rows),'status_counts':dict(Counter(r['status'] for r in all_rows)),'all_recorded_receipts_verified':True,'chains':sorted({r['chain'] for r in all_rows}),'parts':parts,'source_receipt_files_sha256':files,'scope':'Canonical finalized inclusion and independently classified System/wrapper events for persisted receipts on 11 suites. Includes recorded receipts from the superseded operation attempt. Does not claim every submitted extrinsic was inventoried. Mixed CLI, decoder/Shield, and RPC evidence are separate.'}
(D/'combined-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:summary[k] for k in ['unique_receipts','status_counts','all_recorded_receipts_verified','chains']}))
