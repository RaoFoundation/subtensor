from pathlib import Path
import json,os,signal,subprocess,sys,xml.etree.ElementTree as E
P=Path(__file__).parent;name=sys.argv[1]
assert name in {'authority','release','operations','lifecycle'}
xml=P/name/('rerun/results.xml' if name=='operations' else 'results.xml')
cases=list(E.parse(xml).iter('testcase'));assert cases and all(not any(c.find(s) is not None for s in ('failure','error','skipped')) for c in cases)
with (P/f'receipt-verification/completed-{name}.log').open('w') as out:
 subprocess.run([sys.executable,str(P/'receipt-verification/verify.py'),'--label',f'completed-{name}'],env={**os.environ,'VERIFY_CHAINS':name},stdout=out,stderr=subprocess.STDOUT,check=True)
r=next(x for x in json.loads((P/'launchers.json').read_text()) if x['name']==name)
assert str(P/name/'localnet.sh').encode() in Path(f'/proc/{r["pid"]}/cmdline').read_bytes()
os.kill(r['pid'],signal.SIGTERM)
f=P/'early-shutdown.json';rows=json.loads(f.read_text());rows.append(r);f.write_text(json.dumps(rows,indent=2));print('Verified receipts and shut down',name)
