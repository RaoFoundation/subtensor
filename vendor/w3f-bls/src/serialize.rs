use alloc::{vec, vec::Vec};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, SerializationError};

/// Serialization code that is used by multiple modules.
// Note that ark_ff::bytes::ToBytes for projective points export them without converting them to affine
// and so they might leak information about the secret key.
pub trait SerializableToBytes: CanonicalSerialize + CanonicalDeserialize {
    const SERIALIZED_BYTES_SIZE: usize;

    fn to_bytes(&self) -> Vec<u8> {
        let mut serialized_representation: Vec<u8> = vec![0; Self::SERIALIZED_BYTES_SIZE];
        self.serialize_compressed(&mut serialized_representation[..])
            .unwrap();

        return serialized_representation;
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, SerializationError> {
        Self::deserialize_compressed(bytes)
    }
}
