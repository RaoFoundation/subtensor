from pathlib import Path
import tempfile
from unittest.mock import patch
from bittensor.keyfiles import Keyfile
import os, json
with tempfile.TemporaryDirectory(prefix='ms-keyfile-short-write-') as directory:
 p=Path(directory)/'coldkey'; p.write_bytes(b'previous complete recoverable wallet')
 original_write=os.write
 def short_write(fd,data):
  return original_write(fd,data[:len(data)//2])
 with patch('bittensor.keyfiles.os.write',side_effect=short_write):
  Keyfile(p)._write_data(b'new complete recoverable wallet',overwrite=True)
 result={'returned_success':True,'old_wallet_preserved':p.read_bytes()==b'previous complete recoverable wallet','new_wallet_complete':p.read_bytes()==b'new complete recoverable wallet','saved_bytes':len(p.read_bytes()),'expected_bytes':len(b'new complete recoverable wallet')}
 print(json.dumps(result,indent=2))
 assert result['old_wallet_preserved'] or result['new_wallet_complete'],'Short write silently destroys both complete backups'
