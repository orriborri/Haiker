//! S3-compatible object storage client.
//!
//! Wraps the `rust-s3` crate to provide upload, download, delete, presigned URL,
//! and existence-check operations against MinIO or any S3-compatible service.

use s3::creds::Credentials;
use s3::error::S3Error;
use s3::{Bucket, Region};
use std::time::Duration;
use tracing::info;

use crate::config::StorageConfig;

/// Client for interacting with S3-compatible object storage.
#[derive(Clone)]
pub struct ObjectStorageClient {
    bucket: Box<Bucket>,
}

/// Errors that can occur during object storage operations.
#[derive(Debug, thiserror::Error)]
pub enum ObjectStorageError {
    /// The requested object was not found.
    #[error("object not found: {key}")]
    NotFound { key: String },

    /// An error occurred communicating with the storage service.
    #[error("storage error: {0}")]
    Storage(String),
}

impl From<S3Error> for ObjectStorageError {
    fn from(err: S3Error) -> Self {
        ObjectStorageError::Storage(err.to_string())
    }
}

impl ObjectStorageClient {
    /// Create a new object storage client from configuration.
    pub async fn new(config: &StorageConfig) -> Result<Self, ObjectStorageError> {
        let region = Region::Custom {
            region: "us-east-1".to_string(),
            endpoint: config.endpoint.clone(),
        };

        let credentials = Credentials::new(
            Some(&config.access_key_id),
            Some(&config.secret_access_key),
            None,
            None,
            None,
        )
        .map_err(|e| ObjectStorageError::Storage(e.to_string()))?;

        let bucket = Bucket::new(&config.bucket, region, credentials)
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?
            .with_path_style();

        info!(bucket = %config.bucket, endpoint = %config.endpoint, "Object storage client initialized");

        Ok(Self { bucket })
    }

    /// Upload an object to storage.
    pub async fn upload(
        &self,
        key: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<(), ObjectStorageError> {
        let ct = content_type.unwrap_or("application/octet-stream");

        let response = self
            .bucket
            .put_object_with_content_type(key, &body, ct)
            .await?;

        if response.status_code() >= 300 {
            return Err(ObjectStorageError::Storage(format!(
                "upload failed with status {}",
                response.status_code()
            )));
        }

        Ok(())
    }

    /// Download an object from storage.
    pub async fn download(&self, key: &str) -> Result<Vec<u8>, ObjectStorageError> {
        let response = self.bucket.get_object(key).await.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("404") || msg.contains("NoSuchKey") || msg.contains("not found") {
                ObjectStorageError::NotFound {
                    key: key.to_string(),
                }
            } else {
                ObjectStorageError::Storage(msg)
            }
        })?;

        if response.status_code() == 404 {
            return Err(ObjectStorageError::NotFound {
                key: key.to_string(),
            });
        }

        Ok(response.to_vec())
    }

    /// Delete an object from storage.
    pub async fn delete(&self, key: &str) -> Result<(), ObjectStorageError> {
        let response = self.bucket.delete_object(key).await?;

        if response.status_code() >= 300 && response.status_code() != 404 {
            return Err(ObjectStorageError::Storage(format!(
                "delete failed with status {}",
                response.status_code()
            )));
        }

        Ok(())
    }

    /// Generate a presigned upload URL valid for the specified duration.
    pub async fn presigned_upload_url(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStorageError> {
        let expiry_secs = expires_in.as_secs().try_into().unwrap_or(u32::MAX);
        let url = self
            .bucket
            .presign_put(key, expiry_secs, None, None)
            .await
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?;

        Ok(url)
    }

    /// Check whether an object exists in storage.
    pub async fn exists(&self, key: &str) -> Result<bool, ObjectStorageError> {
        match self.bucket.head_object(key).await {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("404") || msg.contains("NotFound") || msg.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(ObjectStorageError::Storage(msg))
                }
            }
        }
    }
}
