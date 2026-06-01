//! SHA-256 helpers used for message + attachment integrity and dedupe.

use sha2::{Digest, Sha256};

/// Hash a byte slice and return lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Incremental hasher for streaming large payloads without buffering.
#[allow(dead_code)] // used by streaming attachment hashing in later milestones
#[derive(Default)]
pub struct Hasher {
    inner: Sha256,
}

#[allow(dead_code)] // used by streaming attachment hashing in later milestones
impl Hasher {
    pub fn new() -> Self {
        Self {
            inner: Sha256::new(),
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    pub fn finalize_hex(self) -> String {
        hex::encode(self.inner.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn streaming_matches_oneshot() {
        let mut h = Hasher::new();
        h.update(b"hello ");
        h.update(b"world");
        assert_eq!(h.finalize_hex(), sha256_hex(b"hello world"));
    }
}
