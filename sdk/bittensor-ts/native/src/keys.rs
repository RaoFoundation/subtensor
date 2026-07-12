use bittensor_core::keyfiles;
use bittensor_core::keys::{self, Keypair, CRYPTO_ED25519, CRYPTO_SR25519, DEFAULT_SS58_FORMAT};
use napi::bindgen_prelude::{AsyncTask, Buffer};
use napi::{Env, Task};
use napi_derive::napi;
use std::path::PathBuf;

use crate::errors::{invalid_arg, CoreResultExt, NapiResult};

#[napi]
pub struct NativeKeypair {
    pub(crate) inner: Keypair,
}

impl NativeKeypair {
    fn new(inner: Keypair) -> Self {
        Self { inner }
    }
}

pub struct KeypairFromEncryptedJsonTask {
    json_data: String,
    passphrase: String,
}

impl Task for KeypairFromEncryptedJsonTask {
    type Output = Keypair;
    type JsValue = NativeKeypair;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        Keypair::from_encrypted_json(&self.json_data, &self.passphrase).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeKeypair::new(output))
    }
}

pub struct KeypairToKeyfileDataTask {
    keypair: Keypair,
    password: Option<String>,
}

impl Task for KeypairToKeyfileDataTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::keypair_to_keyfile_data(&self.keypair, self.password.as_deref()).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

pub struct DeserializeKeypairFromKeyfileTask {
    keyfile_data: Vec<u8>,
    password: Option<String>,
}

impl Task for DeserializeKeypairFromKeyfileTask {
    type Output = Keypair;
    type JsValue = NativeKeypair;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::deserialize_keypair_from_keyfile(&self.keyfile_data, self.password.as_deref())
            .napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeKeypair::new(output))
    }
}

pub struct ReadKeypairKeyfileTask {
    path: PathBuf,
    password: Option<String>,
}

impl Task for ReadKeypairKeyfileTask {
    type Output = Keypair;
    type JsValue = NativeKeypair;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::read_keypair_from_keyfile(&self.path, self.password.as_deref()).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(NativeKeypair::new(output))
    }
}

pub struct WriteKeypairKeyfileTask {
    keypair: Keypair,
    path: PathBuf,
    password: Option<String>,
    overwrite: bool,
    allow_plaintext: bool,
}

impl Task for WriteKeypairKeyfileTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::save_keypair_to_keyfile(
            &self.keypair,
            &self.path,
            self.password.as_deref(),
            self.overwrite,
            self.allow_plaintext,
        )
        .napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct WriteKeypairPairKeyfileTask {
    private_keypair: Keypair,
    private_path: PathBuf,
    private_password: Option<String>,
    public_keypair: Keypair,
    public_path: PathBuf,
    overwrite: bool,
    allow_plaintext: bool,
}

impl Task for WriteKeypairPairKeyfileTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::save_keypair_pair_to_keyfiles(
            &self.private_keypair,
            &self.private_path,
            self.private_password.as_deref(),
            &self.public_keypair,
            &self.public_path,
            self.overwrite,
            self.allow_plaintext,
        )
        .napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct EncryptKeyfileDataTask {
    keyfile_data: Vec<u8>,
    password: String,
}

impl Task for EncryptKeyfileDataTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::encrypt_keyfile_data(&self.keyfile_data, &self.password).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

pub struct DecryptKeyfileDataTask {
    keyfile_data: Vec<u8>,
    password: Option<String>,
}

impl Task for DecryptKeyfileDataTask {
    type Output = Vec<u8>;
    type JsValue = Buffer;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        keyfiles::decrypt_keyfile_data(&self.keyfile_data, self.password.as_deref()).napi()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

#[napi]
impl NativeKeypair {
    #[napi(getter)]
    pub fn crypto_type(&self) -> u8 {
        self.inner.crypto_type()
    }

    #[napi(getter)]
    pub fn kind(&self) -> String {
        if !self.inner.has_private_key() {
            return "PublicOnly".to_owned();
        }
        match self.inner.crypto_type() {
            CRYPTO_ED25519 => "Ed25519".to_owned(),
            CRYPTO_SR25519 => "Sr25519".to_owned(),
            _ => "PublicOnly".to_owned(),
        }
    }

    #[napi(getter)]
    pub fn public_key(&self) -> Buffer {
        self.inner.public_key_bytes().to_vec().into()
    }

    #[napi(getter)]
    pub fn ss58_address(&self) -> String {
        self.inner.ss58_address()
    }

    #[napi(getter)]
    pub fn ss58_format(&self) -> u16 {
        self.inner.ss58_format()
    }

    #[napi]
    pub fn derive(&self, path: String) -> NapiResult<NativeKeypair> {
        self.inner.derive(&path).napi().map(NativeKeypair::new)
    }

    #[napi]
    pub fn sign(&self, message: Buffer) -> NapiResult<Buffer> {
        self.inner.sign(message.as_ref()).napi().map(Into::into)
    }

    #[napi]
    pub fn verify(&self, message: Buffer, signature: Buffer) -> NapiResult<bool> {
        self.inner
            .verify(message.as_ref(), signature.as_ref())
            .napi()
    }

    #[napi]
    pub fn encrypt(&self, message: Buffer) -> NapiResult<Buffer> {
        self.inner.encrypt(message.as_ref()).napi().map(Into::into)
    }

    #[napi]
    pub fn decrypt(&self, ciphertext: Buffer) -> NapiResult<Buffer> {
        self.inner
            .decrypt(ciphertext.as_ref())
            .napi()
            .map(Into::into)
    }
}

#[napi(js_name = "keypairNew")]
pub fn keypair_new(
    ss58_address: Option<String>,
    public_key: Option<Buffer>,
    crypto_type: u8,
    ss58_format: u16,
) -> NapiResult<NativeKeypair> {
    Keypair::new(
        ss58_address.as_deref(),
        public_key.as_ref().map(|value| value.as_ref()),
        crypto_type,
        ss58_format,
    )
    .napi()
    .map(NativeKeypair::new)
}

#[napi(js_name = "keypairFromMnemonic")]
pub fn keypair_from_mnemonic(
    mnemonic: String,
    crypto_type: u8,
    password: Option<String>,
) -> NapiResult<NativeKeypair> {
    Keypair::from_mnemonic(&mnemonic, crypto_type, password.as_deref())
        .napi()
        .map(NativeKeypair::new)
}

#[napi(js_name = "keypairFromSeed")]
pub fn keypair_from_seed(seed: Buffer, crypto_type: u8) -> NapiResult<NativeKeypair> {
    Keypair::from_seed(seed.as_ref(), crypto_type)
        .napi()
        .map(NativeKeypair::new)
}

#[napi(js_name = "keypairFromUri")]
pub fn keypair_from_uri(uri: String, crypto_type: u8) -> NapiResult<NativeKeypair> {
    Keypair::from_uri(&uri, crypto_type)
        .napi()
        .map(NativeKeypair::new)
}

#[napi(js_name = "keypairFromPrivateKey")]
pub fn keypair_from_private_key(private_key: String, crypto_type: u8) -> NapiResult<NativeKeypair> {
    Keypair::from_private_key(&private_key, crypto_type)
        .napi()
        .map(NativeKeypair::new)
}

#[napi(js_name = "keypairFromEncryptedJson")]
pub fn keypair_from_encrypted_json(
    json_data: String,
    passphrase: String,
) -> AsyncTask<KeypairFromEncryptedJsonTask> {
    AsyncTask::new(KeypairFromEncryptedJsonTask {
        json_data,
        passphrase,
    })
}

#[napi(js_name = "generateMnemonic")]
pub fn generate_mnemonic(n_words: u32) -> NapiResult<String> {
    let n_words = usize::try_from(n_words)
        .map_err(|_| invalid_arg("mnemonic word count does not fit usize"))?;
    Keypair::generate_mnemonic(n_words).napi()
}

#[napi(js_name = "encryptFor")]
pub fn encrypt_for(ss58_address: String, message: Buffer, crypto_type: u8) -> NapiResult<Buffer> {
    Keypair::encrypt_for(&ss58_address, message.as_ref(), crypto_type)
        .napi()
        .map(Into::into)
}

#[napi(js_name = "verifySignature")]
pub fn verify_signature(
    message: Buffer,
    signature: Buffer,
    ss58_address: String,
    crypto_type: u8,
) -> NapiResult<bool> {
    keys::verify(
        message.as_ref(),
        signature.as_ref(),
        &ss58_address,
        crypto_type,
    )
    .napi()
}

#[napi(js_name = "publicKeyFromSs58")]
pub fn public_key_from_ss58(ss58_address: String) -> NapiResult<Buffer> {
    keys::public_key_from_ss58(&ss58_address)
        .napi()
        .map(|value| value.to_vec().into())
}

#[napi(js_name = "ss58FromPublic")]
pub fn ss58_from_public(public_key: Buffer, ss58_format: u16) -> NapiResult<String> {
    let public_key: [u8; 32] = public_key
        .as_ref()
        .try_into()
        .map_err(|_| invalid_arg("public key must be exactly 32 bytes"))?;
    Ok(keys::ss58_from_public(public_key, ss58_format))
}

#[napi(js_name = "serializeKeypair")]
pub fn serialize_keypair(keypair: &NativeKeypair) -> NapiResult<Buffer> {
    keyfiles::keypair_to_keyfile_data(&keypair.inner, None)
        .napi()
        .map(Into::into)
}

#[napi(js_name = "keypairToKeyfileData")]
pub fn keypair_to_keyfile_data(
    keypair: &NativeKeypair,
    password: Option<String>,
) -> AsyncTask<KeypairToKeyfileDataTask> {
    AsyncTask::new(KeypairToKeyfileDataTask {
        keypair: keypair.inner.clone(),
        password,
    })
}

#[napi(js_name = "deserializeKeypair")]
pub fn deserialize_keypair(keyfile_data: Buffer) -> NapiResult<NativeKeypair> {
    keyfiles::deserialize_keypair_from_keyfile_data(keyfile_data.as_ref())
        .napi()
        .map(NativeKeypair::new)
}

#[napi(js_name = "deserializeKeypairFromKeyfile")]
pub fn deserialize_keypair_from_keyfile(
    keyfile_data: Buffer,
    password: Option<String>,
) -> AsyncTask<DeserializeKeypairFromKeyfileTask> {
    AsyncTask::new(DeserializeKeypairFromKeyfileTask {
        keyfile_data: keyfile_data.to_vec(),
        password,
    })
}

#[napi(js_name = "readKeypairKeyfile")]
pub fn read_keypair_keyfile(
    path: String,
    password: Option<String>,
) -> AsyncTask<ReadKeypairKeyfileTask> {
    AsyncTask::new(ReadKeypairKeyfileTask {
        path: PathBuf::from(path),
        password,
    })
}

#[napi(js_name = "writeKeypairKeyfile")]
pub fn write_keypair_keyfile(
    keypair: &NativeKeypair,
    path: String,
    password: Option<String>,
    overwrite: bool,
    allow_plaintext: bool,
) -> AsyncTask<WriteKeypairKeyfileTask> {
    AsyncTask::new(WriteKeypairKeyfileTask {
        keypair: keypair.inner.clone(),
        path: PathBuf::from(path),
        password,
        overwrite,
        allow_plaintext,
    })
}

#[napi(js_name = "writeKeypairPairKeyfile")]
pub fn write_keypair_pair_keyfile(
    private_keypair: &NativeKeypair,
    private_path: String,
    private_password: Option<String>,
    public_keypair: &NativeKeypair,
    public_path: String,
    overwrite: bool,
    allow_plaintext: bool,
) -> AsyncTask<WriteKeypairPairKeyfileTask> {
    AsyncTask::new(WriteKeypairPairKeyfileTask {
        private_keypair: private_keypair.inner.clone(),
        private_path: PathBuf::from(private_path),
        private_password,
        public_keypair: public_keypair.inner.clone(),
        public_path: PathBuf::from(public_path),
        overwrite,
        allow_plaintext,
    })
}

#[napi(js_name = "encryptKeyfileData")]
pub fn encrypt_keyfile_data(
    keyfile_data: Buffer,
    password: String,
) -> AsyncTask<EncryptKeyfileDataTask> {
    AsyncTask::new(EncryptKeyfileDataTask {
        keyfile_data: keyfile_data.to_vec(),
        password,
    })
}

#[napi(js_name = "decryptKeyfileData")]
pub fn decrypt_keyfile_data(
    keyfile_data: Buffer,
    password: Option<String>,
) -> AsyncTask<DecryptKeyfileDataTask> {
    AsyncTask::new(DecryptKeyfileDataTask {
        keyfile_data: keyfile_data.to_vec(),
        password,
    })
}

#[napi(js_name = "keyfileDataIsEncrypted")]
pub fn keyfile_data_is_encrypted(keyfile_data: Buffer) -> bool {
    keyfiles::keyfile_data_is_encrypted(keyfile_data.as_ref())
}

#[napi(js_name = "keyfileDataIsEncryptedNacl")]
pub fn keyfile_data_is_encrypted_nacl(keyfile_data: Buffer) -> bool {
    keyfiles::keyfile_data_is_encrypted_nacl(keyfile_data.as_ref())
}

#[napi(js_name = "keyfileDataIsEncryptedAnsible")]
pub fn keyfile_data_is_encrypted_ansible(keyfile_data: Buffer) -> bool {
    keyfiles::keyfile_data_is_encrypted_ansible(keyfile_data.as_ref())
}

#[napi(js_name = "keyfileDataIsEncryptedLegacy")]
pub fn keyfile_data_is_encrypted_legacy(keyfile_data: Buffer) -> bool {
    keyfiles::keyfile_data_is_encrypted_legacy(keyfile_data.as_ref())
}

#[napi(js_name = "keyfileDataEncryptionMethod")]
pub fn keyfile_data_encryption_method(keyfile_data: Buffer) -> String {
    keyfiles::keyfile_data_encryption_method(keyfile_data.as_ref()).to_owned()
}

#[napi(js_name = "getPasswordFromEnvironment")]
pub fn get_password_from_environment(env_var_name: String) -> NapiResult<Option<String>> {
    keyfiles::get_password_from_environment(&env_var_name).napi()
}

#[napi(js_name = "savePasswordToEnvironment")]
pub fn save_password_to_environment(env_var_name: String, password: String) -> NapiResult<String> {
    keyfiles::save_password_to_environment(&env_var_name, &password).napi()
}

#[napi(js_name = "cryptoEd25519")]
pub fn crypto_ed25519() -> u8 {
    CRYPTO_ED25519
}

#[napi(js_name = "cryptoSr25519")]
pub fn crypto_sr25519() -> u8 {
    CRYPTO_SR25519
}

#[napi(js_name = "defaultSs58Format")]
pub fn default_ss58_format() -> u16 {
    DEFAULT_SS58_FORMAT
}
