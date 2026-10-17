from pathlib import Path
import concurrent.futures,os,subprocess,sys,time,xml.etree.ElementTree as E
P=Path(__file__).parent
def finish(name,count):
 xml=P/name/('rerun/results.xml' if name=='operations' else 'results.xml')
 deadline=time.monotonic()+7200
 while time.monotonic()<deadline:
  try:
   cases=list(E.parse(xml).iter('testcase'))
   if len(cases)==count:
    assert all(not any(c.find(x) is not None for x in ('failure','error','skipped')) for c in cases),name
    break
  except (FileNotFoundError,E.ParseError):pass
  time.sleep(5)
 else:raise TimeoutError(name)
 return name,subprocess.run([sys.executable,str(P/'finish-chain.py'),name],check=True).returncode
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
 for result in pool.map(lambda args:finish(*args),[('release',17),('operations',8),('lifecycle',6)]):print(result,flush=True)
