from pathlib import Path
import concurrent.futures,os,subprocess,sys
P=Path(__file__).parent;R=Path('/home/johnreed/Desktop/work/subtensor')
for prefix in ['sdk-final','sdk-types-final','preflight-final']:
 for path in list(P.glob(prefix+'.*')):path.rename(path.with_name(path.name.replace(prefix,prefix+'-before-payload-snapshot',1)))
env={**os.environ,'PYTHONPATH':str(P/'native')+':'+str(R/'sdk/python'),'PATH':'/tmp/ms-reference-39060946210/bin:'+os.environ['PATH'],'PREFLIGHT_PUSH_ACTOR':'UnArbosSix'}
def run(name,cmd,cwd):
 with (P/(name+'.log')).open('w') as f:rc=subprocess.call(cmd,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT)
 (P/(name+'.exit')).write_text(str(rc)+'\n');print(name,rc,flush=True);return rc
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
 futures=[pool.submit(run,'sdk-final',[sys.executable,'-m','pytest','tests/unit','-q','--junitxml='+str(P/'sdk-final.xml')],R/'sdk/python'),pool.submit(run,'sdk-types-final',['uv','run','--no-sync','ty','check','--exit-zero-on-warning','bittensor'],R/'sdk/python'),pool.submit(run,'preflight-final',['bash','scripts/preflight.sh'],R)]
 assert all(f.result()==0 for f in futures)
