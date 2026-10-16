//! Filesystem blob store. Keys are `{org}/{doc}`; we sanitise them so a
//! caller can never escape the configured root directory. Originals are
//! written once and never mutated in place.

use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

use medassist_application::errors::AppError;
use medassist_application::ports::BlobStore;
use uuid::Uuid;

pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    fn resolve(&self, key: &str) -> Result<PathBuf, AppError> {
        // Reject traversal outright; keys must be exactly org/uuid.
        let mut parts = key.split('/');
        let (org, id) = match (parts.next(), parts.next(), parts.next()) {
            (Some(o), Some(i), None) => (o, i),
            _ => return Err(AppError::Unprocessable("invalid blob key".into())),
        };
        let parse = |s: &str| Uuid::parse_str(s).map_err(|_| AppError::Unprocessable("invalid blob key".into()));
        let (_o, _i) = (parse(org)?, parse(id)?);
        let p = self.root.join(org).join(id);
        Ok(p)
    }
}

#[async_trait::async_trait]
impl BlobStore for FsBlobStore {
    async fn put(&self, org_id: Uuid, doc_id: Uuid, bytes: &[u8]) -> Result<String, AppError> {
        let key = format!("{org_id}/{doc_id}");
        let path = self.resolve(&key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(AppError::internal)?;
        }
        // create_new fails if the blob already exists — originals are immutable.
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        let mut f = opts.open(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                AppError::Conflict("blob already exists".into())
            } else {
                AppError::internal(e)
            }
        })?;
        f.write_all(bytes).await.map_err(AppError::internal)?;
        f.flush().await.map_err(AppError::internal)?;
        Ok(key)
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, AppError> {
        let path = self.resolve(key)?;
        tokio::fs::read(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::NotFound("blob")
            } else {
                AppError::internal(e)
            }
        })
    }

    async fn delete(&self, key: &str) -> Result<(), AppError> {
        let path = self.resolve(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::internal(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_and_reject_traversal() {
        let dir = std::env::temp_dir().join(format!("medassist-blob-test-{}", Uuid::new_v4()));
        let s = FsBlobStore::new(&dir);
        let org = Uuid::new_v4();
        let doc = Uuid::new_v4();
        let key = s.put(org, doc, b"hello").await.unwrap();
        assert_eq!(s.get(&key).await.unwrap(), b"hello");
        // immutable: second put conflicts
        assert!(matches!(s.put(org, doc, b"other").await, Err(AppError::Conflict(_))));
        assert!(s.get("../../etc/passwd").await.is_err());
        assert!(s.get(&format!("{org}/{doc}/extra")).await.is_err());
        s.delete(&key).await.unwrap();
        assert!(matches!(s.get(&key).await, Err(AppError::NotFound(_))));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
