from pathlib import Path
import shutil
N=Path(__file__).parent;O=Path('/tmp/ms-head-validation-20261017');R=Path('/home/johnreed/Desktop/work/subtensor')
shutil.copy('/tmp/ms-single-activation/verify-weight-selection.py',N/'verify-weight-selection.py')
(N/'crypto').mkdir(exist_ok=True)
for f in ['build_bridge.py','primitive_bridge.rs','crosscheck.py','reference.json']:
 if (O/'crypto'/f).exists():shutil.copy(O/'crypto'/f,N/'crypto'/f)
(N/'crypto/build').symlink_to(O/'crypto/build',target_is_directory=True)
s=(O/'receipt-verification/verify.py').read_text().replace(str(O),str(N)).replace("'nested': 'extended'","'nested': 'nested'").replace("'sponsorship': 'extended'","'sponsorship': 'sponsorship'").replace("'validator-proxy': 'validators'","'validator-proxy': 'validator-proxy'")
(N/'receipt-verification').mkdir(exist_ok=True);(N/'receipt-verification/verify.py').write_text(s)
s=(O/'verify-chains.py').read_text().replace("+json.loads((RUN/'chains-extra.json').read_text())",'').replace('==11','==13').replace("'distinct_chains':11","'distinct_chains':13").replace('All11','All13')
(N/'verify-chains.py').write_text(s)
(N/'codecs').mkdir(exist_ok=True)
s=(R/'docs/design/evidence/ml-dsa-localnet-codecs.cjs').read_text().replace('path.resolve(__dirname, "../../..")',repr(str(R))).replace('Local functional runtime uses test weights','Current HEAD runtime uses committed reference weights')
(N/'codecs/check.cjs').write_text(s)
