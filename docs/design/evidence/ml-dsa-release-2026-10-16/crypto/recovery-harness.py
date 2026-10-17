import json
from bittensor.sp_core import WrongPasswordError, KeyfileError, Keypair, serialized_keypair_to_keyfile_data, deserialize_keypair_from_keyfile_data, encrypt_keyfile_data, decrypt_keyfile_data, verify_mldsa_authorization
phrase=' '.join(['abandon']*23+['art'])
results=[]
for crypto in (7,8):
 original=Keypair.create_from_mnemonic(phrase,crypto,password='public-test-bip39-passphrase')
 plain=bytes(serialized_keypair_to_keyfile_data(original)); fields=json.loads(plain)
 assert 'secretPhrase' not in fields
 assert fields['walletVersion']==2
 ciphertext=bytes(encrypt_keyfile_data(plain,'public-test-encryption-password'))
 restored=deserialize_keypair_from_keyfile_data(bytes(decrypt_keyfile_data(ciphertext,'public-test-encryption-password')))
 assert restored.ss58_address==original.ss58_address
 for generation in (0,1,63,64,255,256,9001,2**32,2**64-2):
  active=restored.at_generation(generation); expected=original.at_generation(generation)
  assert bytes(active.hashed_public_key)==bytes(expected.hashed_public_key)
  assert bytes(active.hashed_next_commitment)==bytes(expected.hashed_next_commitment)
  proof=bytes(active.sign_mldsa(b'restored full call and extensions'))
  assert verify_mldsa_authorization(proof,b'restored full call and extensions',bytes(restored.hashed_descriptor),generation,bytes(expected.hashed_current_commitment))
  assert not verify_mldsa_authorization(proof,b'restored full call and extensions',bytes(restored.hashed_descriptor),generation+1,bytes(expected.hashed_current_commitment))
 for wrong in ('incorrect-password',):
  try: decrypt_keyfile_data(ciphertext,wrong)
  except (ValueError, WrongPasswordError, KeyfileError): pass
  else: raise AssertionError('wrong password accepted')
 damaged=ciphertext[:-1]+bytes([ciphertext[-1]^1])
 try: decrypt_keyfile_data(damaged,'public-test-encryption-password')
 except (ValueError, WrongPasswordError, KeyfileError): pass
 else: raise AssertionError('corrupted ciphertext accepted')
 for field,value in [('walletVersion',1),('cryptoType',15-crypto),('accountId','0x'+'00'*32),('publicKey','0x'+'00'*32),('accountType','invalid'),('signingScheme','sr25519'),('secretSeed','0x'+'00'*32),('mldsaDescriptor','0x'+'00'*66)]:
  altered=fields|{field:value}
  try: deserialize_keypair_from_keyfile_data(json.dumps(altered).encode())
  except (ValueError, WrongPasswordError, KeyfileError): pass
  else: raise AssertionError('mismatched '+field+' accepted')
 results.append({'crypto_type':crypto,'encrypted_original_backup_restore_generations':9,'password_and_ciphertext_rejection':2,'keyfile_mismatch_rejections':8,'status':'PASS'})
print(json.dumps({'status':'PASS','cases':results},indent=2))
