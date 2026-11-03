use crate::types::Ph;
#[cfg(feature = "default-rng")]
use rand_core::OsRng;
use rand_core::{CryptoRng, CryptoRngCore, RngCore};

/// The `KeyGen` trait is defined to allow trait objects for keygen.
pub trait KeyGen {
    /// An expanded public key containing precomputed elements to increase (repeated)
    /// verify performance. Derived from the public key.
    type PublicKey;

    /// An expanded private key containing precomputed elements to increase (repeated)
    /// signing performance. Derived from the private key.
    type PrivateKey;

    /// Generates a public and private key pair specific to this security parameter set.
    /// This function utilizes the **OS default** random number generator. This function operates
    /// in constant-time relative to secret data (which specifically excludes the OS random
    /// number generator internals, the `rho` value stored in the public key, and the hash-derived
    /// `rho_prime` values that are rejection-sampled/expanded into the internal `s_1` and `s_2` values).
    ///
    /// # Errors
    /// Returns an error when the random number generator fails.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(all(feature = "ml-dsa-44", feature = "default-rng"))] {
    /// use fips204::ml_dsa_44; // Could also be ml_dsa_65 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_44::KG::try_keygen()?; // Generate both public and secret keys
    /// let sig = sk.try_sign(&message, &[0])?; // Use the secret key to generate a message signature
    /// # }
    /// # Ok(())}
    /// ```
    #[cfg(feature = "default-rng")]
    fn try_keygen() -> Result<(Self::PublicKey, Self::PrivateKey), &'static str> {
        Self::try_keygen_with_rng(&mut OsRng)
    }

    /// Generates a public and private key pair specific to this security parameter set.
    /// This function utilizes the **provided** random number generator. This function operates
    /// in constant-time relative to secret data (which specifically excludes the provided random
    /// number generator internals, the `rho` value stored in the public key, and the hash-derived
    /// `rho_prime` values that are rejection-sampled/expanded into the internal `s_1` and `s_2` values).
    ///
    /// # Errors
    /// Returns an error when the random number generator fails.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(all(feature = "ml-dsa-44", feature = "default-rng"))] {
    /// use fips204::ml_dsa_44; // Could also be ml_dsa_65 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    /// use rand_chacha::rand_core::SeedableRng;
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_44::KG::try_keygen_with_rng(&mut rng)?;  // Generate both public and secret keys
    /// let sig = sk.try_sign(&message, &[0])?;  // Use the secret key to generate a message signature
    /// }
    /// # Ok(())}
    /// ```
    fn try_keygen_with_rng(
        rng: &mut impl CryptoRngCore,
    ) -> Result<(Self::PublicKey, Self::PrivateKey), &'static str>;

    /// Generates an public and private key key pair specific to this security parameter set
    /// based on a provided seed. <br>
    /// This function operates in constant-time relative to secret data (which specifically excludes
    /// the the `rho` value stored in the public key and the hash-derived `rho_prime` values that are
    /// rejection-sampled/expanded into the internal `s_1` and `s_2` values).
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-dsa-44")] {
    /// use crate::fips204::RngCore;
    /// use fips204::ml_dsa_44; // Could also be ml_dsa_65 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, Signer, Verifier};
    /// use rand_core::OsRng;
    ///
    /// // The signor gets the xi seed from the OS random number generator
    /// let mut xi = [0u8; 32];
    /// OsRng.fill_bytes(&mut xi);
    /// ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_44::KG::keygen_from_seed(&xi); // Generate both public and secret keys
    /// let sig = sk.try_sign(&message, &[0])?; // Use the secret key to generate a message signature
    ///
    /// let res = pk.verify(&message, &sig, &[0]);
    /// assert!(res); // Signature accepted
    /// # }
    /// # Ok(())}
    /// ```
    #[must_use]
    fn keygen_from_seed(xi: &[u8; 32]) -> (Self::PublicKey, Self::PrivateKey);
}

/// The Signer trait is implemented for the `PrivateKey` struct on each of the security parameter sets.
pub trait Signer {
    /// The signature is specific to the chosen security parameter set, e.g., ml-dsa-44, ml-dsa-65 or ml-dsa-87
    type Signature;

    /// The public key that corresponds to the private/secret key
    type PublicKey;

    /// Attempt to sign the given message, returning a digital signature on success, or an error if
    /// something went wrong. This function utilizes the **OS default** random number generator.
    /// This function operates in constant-time relative to secret data (which specifically excludes
    /// the OS default random number generator internals, the `rho` value this is stored in the public
    /// key, the hash-derived `rho_prime` values that are rejection-sampled/expanded into the internal
    /// `s_1` and `s_2` values, and the main signing rejection loop as noted in section 5.5 of
    /// <https://pq-crystals.org/dilithium/data/dilithium-specification-round3-20210208.pdf>).
    ///
    /// # Errors
    /// Returns an error when the random number generator fails or the `ctx` is longer than 255 bytes; propagates internal errors.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(all(feature = "ml-dsa-65", feature = "default-rng"))] {
    /// use fips204::ml_dsa_65; // Could also be ml_dsa_44 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_65::KG::try_keygen()?; // Generate both public and secret keys
    /// let sig = sk.try_sign(&message, &[0])?; // Use the secret key to generate a message signature
    /// let v = pk.verify(&message, &sig, &[0]); // Use the public to verify message signature
    /// assert!(v);
    /// # }
    /// # Ok(())}
    /// ```
    #[cfg(feature = "default-rng")]
    fn try_sign(&self, message: &[u8], ctx: &[u8]) -> Result<Self::Signature, &'static str> {
        self.try_sign_with_rng(&mut OsRng, message, ctx)
    }

    /// Attempt to sign the given message, returning a digital signature on success, or an error if
    /// something went wrong. This function utilizes the **provided** random number generator.
    /// This function operates in constant-time relative to secret data (which specifically excludes
    /// the provided random number generator internals, the `rho` value (also) stored in the public
    /// key, the hash-derived `rho_prime` value that is rejection-sampled/expanded into the internal
    /// `s_1` and `s_2` values, and the main signing rejection loop as noted in section 5.5 of
    /// <https://pq-crystals.org/dilithium/data/dilithium-specification-round3-20210208.pdf>.
    ///
    /// # Errors
    /// Returns an error when the random number generator fails or the `ctx` is longer than 255 bytes; propagates internal errors.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-dsa-65")] {
    /// use fips204::ml_dsa_65; // Could also be ml_dsa_44 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    /// use rand_chacha::rand_core::SeedableRng;
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_65::KG::try_keygen_with_rng(&mut rng)?;  // Generate both public and secret keys
    /// let sig = sk.try_sign_with_rng(&mut rng, &message, &[0])?;  // Use the secret key to generate a message signature
    /// let v = pk.verify(&message, &sig, &[0]); // Use the public to verify message signature
    /// assert!(v);
    /// # }
    /// # Ok(())}
    /// ```
    fn try_sign_with_rng(
        &self,
        rng: &mut impl CryptoRngCore,
        message: &[u8],
        ctx: &[u8],
    ) -> Result<Self::Signature, &'static str>;

    /// Attempt to sign the given message, returning a digital signature on success, or an error if
    /// something went wrong. This function utilizes the **provided seed to support (less common)
    /// deterministic signatures**. This function operates in constant-time relative to secret data
    /// (which specifically excludes the `rho` value stored in the public key, the hash-derived
    /// `rho_prime` value that is rejection-sampled/expanded into the internal `s_1` and `s_2` values,
    /// and the main signing rejection loop as noted in section 5.5 of
    /// <https://pq-crystals.org/dilithium/data/dilithium-specification-round3-20210208.pdf>.
    ///
    /// # Errors
    /// Returns an error when the `ctx` is longer than 255 bytes; propagates internal errors.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(feature = "ml-dsa-65")] {
    /// use fips204::ml_dsa_65; // Could also be ml_dsa_44 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    /// use rand_chacha::rand_core::SeedableRng;
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    /// let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(123);
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_65::KG::try_keygen_with_rng(&mut rng)?;  // Generate both public and secret keys
    /// let sig = sk.try_sign_with_seed(&[0u8;32], &message, &[0])?;  // Use the secret key to generate a message signature
    /// let v = pk.verify(&message, &sig, &[0]); // Use the public to verify message signature
    /// assert!(v);
    /// # }
    /// # Ok(())}
    /// ```
    fn try_sign_with_seed(
        &self,
        seed: &[u8; 32],
        message: &[u8],
        ctx: &[u8],
    ) -> Result<Self::Signature, &'static str> {
        self.try_sign_with_rng(&mut DummyRng { data: *seed }, message, ctx)
    }

    /// Attempt to sign the hash of the given message, returning a digital signature on success,
    /// or an error if something went wrong. This function utilizes the **default OS** random number
    /// generator and allows for several hash algorithms. This function operates in constant-time
    /// relative to secret data (which specifically excludes the provided random number generator
    /// internals, the `rho` value (also) stored in the public key, the hash-derived `rho_prime`
    /// value that is rejection-sampled/expanded into the internal `s_1` and `s_2` values, and the
    /// main signing rejection loop as noted in section 5.5 of
    /// <https://pq-crystals.org/dilithium/data/dilithium-specification-round3-20210208.pdf>.
    ///
    /// # Errors
    /// Returns an error when the random number generator fails or the `ctx` is longer than 255 bytes; propagates internal errors.
    #[cfg(feature = "default-rng")]
    fn try_hash_sign(
        &self,
        message: &[u8],
        ctx: &[u8],
        ph: &Ph,
    ) -> Result<Self::Signature, &'static str> {
        self.try_hash_sign_with_rng(&mut OsRng, message, ctx, ph)
    }

    /// Attempt to sign the hash of the given message, returning a digital signature on success,
    /// or an error if something went wrong. This function utilizes the **provided** random number
    /// generator and allows for several hash algorithms. This function operates in constant-time
    /// relative to secret data (which specifically excludes the provided random number generator
    /// internals, the `rho` value (also) stored in the public key, the hash-derived `rho_prime`
    /// value that is rejection-sampled/expanded into the internal `s_1` and `s_2` values, and the
    /// main signing rejection loop as noted in section 5.5 of
    /// <https://pq-crystals.org/dilithium/data/dilithium-specification-round3-20210208.pdf>.
    ///
    /// # Errors
    /// Returns an error when the random number generator fails or the `ctx` is longer than 255 bytes; propagates internal errors.
    fn try_hash_sign_with_rng(
        &self,
        rng: &mut impl CryptoRngCore,
        message: &[u8],
        ctx: &[u8],
        ph: &Ph,
    ) -> Result<Self::Signature, &'static str>;

    /// Attempt to sign the hash of the given message, returning a digital signature on success,
    /// something went wrong. This function utilizes the **provided seed to support (less common)
    /// deterministic signatures**. This function operates in constant-time relative to secret data
    /// (which specifically excludes the `rho` value stored in the public key, the hash-derived
    /// `rho_prime` value that is rejection-sampled/expanded into the internal `s_1` and `s_2` values,
    /// and the main signing rejection loop as noted in section 5.5 of
    /// <https://pq-crystals.org/dilithium/data/dilithium-specification-round3-20210208.pdf>.
    ///
    /// # Errors
    /// Returns an error when the `ctx` is longer than 255 bytes; propagates internal errors.
    fn try_hash_sign_with_seed(
        &self,
        seed: &[u8; 32],
        message: &[u8],
        ctx: &[u8],
        ph: &Ph,
    ) -> Result<Self::Signature, &'static str> {
        self.try_hash_sign_with_rng(&mut DummyRng { data: *seed }, message, ctx, ph)
    }

    /// Retrieves the public key associated with this private/secret key
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use fips204::ml_dsa_65; // Could also be ml_dsa_44 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    ///
    ///
    /// // Generate both public and secret keys
    /// let (pk1, sk) = ml_dsa_65::KG::try_keygen()?; // Generate both public and secret keys
    ///
    ///
    /// // The public key can be derived from the secret key
    /// let pk2 = sk.get_public_key();
    /// assert_eq!(pk1.into_bytes(), pk2.into_bytes());
    /// # Ok(())
    /// # }
    /// ```
    fn get_public_key(&self) -> Self::PublicKey;
}

// This is for the deterministic signing functions; will be refactored more nicely
struct DummyRng {
    data: [u8; 32],
}

impl RngCore for DummyRng {
    fn next_u32(&mut self) -> u32 {
        unimplemented!()
    }

    fn next_u64(&mut self) -> u64 {
        unimplemented!()
    }

    fn fill_bytes(&mut self, _out: &mut [u8]) {
        unimplemented!()
    }

    fn try_fill_bytes(&mut self, out: &mut [u8]) -> Result<(), rand_core::Error> {
        out.copy_from_slice(&self.data);
        Ok(())
    }
}

impl CryptoRng for DummyRng {}

/// The Verifier trait is implemented for `PublicKey` on each of the security parameter sets.
pub trait Verifier {
    /// The signature is specific to the chosen security parameter set, e.g., ml-dsa-44, ml-dsa-65
    /// or ml-dsa-87
    type Signature;

    /// Verifies a digital signature on a message with respect to a `PublicKey`. As this function
    /// operates on purely public data, it need/does not provide constant-time assurances.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(all(feature = "ml-dsa-65", feature = "default-rng"))] {
    /// use fips204::ml_dsa_65; // Could also be ml_dsa_44 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_65::KG::try_keygen()?; // Generate both public and secret keys
    /// let sig = sk.try_sign(&message, &[0])?; // Use the secret key to generate a message signature
    /// let v = pk.verify(&message, &sig, &[0]); // Use the public to verify message signature
    /// assert!(v);
    /// # }
    /// # Ok(())}
    /// ```
    fn verify(&self, message: &[u8], signature: &Self::Signature, ctx: &[u8]) -> bool;

    /// Verifies a digital signature on the hash of a message with respect to a `PublicKey`. As this
    /// function operates on purely public data, it need/does not provide constant-time assurances.
    fn hash_verify(&self, message: &[u8], sig: &Self::Signature, ctx: &[u8], ph: &Ph) -> bool;
}

/// The `SerDes` trait provides for validated serialization and deserialization of fixed- and correctly-size elements.
///
/// Note that FIPS 204 currently states that outside of exact length checks "ML-DSA is not designed to require any
/// additional public-key validity checks" (perhaps "...designed not to require..." would be better). Nonetheless, a
/// `Result()` is returned during all deserialization operations to preserve the ability to add future checks (and for
/// symmetry across structures). Note that for the current implementation, both of the private and public key
/// deserialization routines invoke an internal decode that catches over-sized coefficients (for early detection).
pub trait SerDes {
    /// The fixed-size byte array to be serialized or deserialized
    type ByteArray;

    /// Produces a byte array of fixed-size specific to the struct being serialized.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(all(feature = "ml-dsa-65", feature = "default-rng"))] {
    /// use fips204::ml_dsa_65; // Could also be ml_dsa_44 or ml_dsa_87.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    ///
    /// let message = [0u8, 1, 2, 3, 4, 5, 6, 7];
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_65::KG::try_keygen()?; // Generate both public and secret keys
    /// let pk_bytes = pk.into_bytes(); // Serialize the public key
    /// let sk_bytes = sk.into_bytes(); // Serialize the private key
    /// # }
    /// # Ok(())}
    /// ```
    fn into_bytes(self) -> Self::ByteArray;

    /// Consumes a byte array of fixed-size specific to the struct being deserialized; performs validation
    ///
    /// # Errors
    /// Returns an error on malformed input.
    ///
    /// # Examples
    /// ```rust
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// # #[cfg(all(feature = "ml-dsa-87", feature = "default-rng"))] {
    /// use fips204::ml_dsa_87; // Could also be ml_dsa_44 or ml_dsa_65.
    /// use fips204::traits::{KeyGen, SerDes, Signer, Verifier};
    ///
    /// // Generate key pair and signature
    /// let (pk, sk) = ml_dsa_87::try_keygen()?; // Generate both public and secret keys
    /// let pk_bytes = pk.into_bytes(); // Serialize the public key
    /// let sk_bytes = sk.into_bytes(); // Serialize the private key
    /// let pk2 = ml_dsa_87::PublicKey::try_from_bytes(pk_bytes)?;
    /// let sk2 = ml_dsa_87::PrivateKey::try_from_bytes(sk_bytes)?;
    /// # }
    /// # Ok(())}
    /// ```
    fn try_from_bytes(ba: Self::ByteArray) -> Result<Self, &'static str>
    where
        Self: Sized;
}
