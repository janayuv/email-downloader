//! Content-addressed blob storage. Files are stored at
//! `storage/attachments/<ab>/<sha256>.<ext>` where `ab` is the first two hex
//! characters of the digest. Hash-naming gives us dedupe (same bytes → one
//! file), no filename collisions, and trivial integrity checks.

use crate::error::Result;
use crate::hashing::sha256_hex;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BlobStore {
    attachments_root: PathBuf,
    messages_root: PathBuf,
}

impl BlobStore {
    pub fn new(storage_dir: &Path) -> Self {
        Self {
            attachments_root: storage_dir.join("attachments"),
            messages_root: storage_dir.join("messages"),
        }
    }

    fn shard_path(root: &Path, sha: &str, ext: &str) -> PathBuf {
        let shard = if sha.len() >= 2 { &sha[0..2] } else { "00" };
        let name = if ext.is_empty() {
            sha.to_string()
        } else {
            format!("{sha}.{ext}")
        };
        root.join(shard).join(name)
    }

    pub fn attachment_path(&self, sha: &str, ext: &str) -> PathBuf {
        Self::shard_path(&self.attachments_root, sha, ext)
    }

    pub fn message_path(&self, sha: &str) -> PathBuf {
        Self::shard_path(&self.messages_root, sha, "eml")
    }

    /// Store attachment bytes by content hash. Returns `(sha256, path)`.
    /// If a blob with the same hash already exists, the write is skipped
    /// (dedupe) and the existing path returned.
    pub fn put_attachment(&self, ext: &str, bytes: &[u8]) -> Result<(String, PathBuf)> {
        let sha = sha256_hex(bytes);
        let path = self.attachment_path(&sha, ext);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, bytes)?;
        }
        Ok((sha, path))
    }

    /// Persist the raw RFC822 bytes of a message keyed by its hash.
    pub fn put_message(&self, sha: &str, bytes: &[u8]) -> Result<PathBuf> {
        let path = self.message_path(sha);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, bytes)?;
        }
        Ok(path)
    }

    pub fn attachments_root(&self) -> &Path {
        &self.attachments_root
    }
}
