//! S3-compatible object storage client.
//!
//! Wraps the AWS SDK to provide upload, download, delete, presigned URL,
//! and existence-check operations against MinIO or any S3-compatible service.

use aws_sdk_s3::{
    config::{Credentials, Region},
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client,
};
use std::time::Duration;
use tracing::info;

use crate::config::StorageConfig;

/// Client for interacting with S3-compatible object storage.
#[derive(Clone)]
pub struct ObjectStorageClient {
    client: Client,
    bucket: String,
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

impl ObjectStorageClient {
    /// Create a new object storage client from configuration.
    pub async fn new(config: &StorageConfig) -> Self {
        let credentials = Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None,
            None,
            "haiker",
        );

        let sdk_config = aws_config::from_env()
            .endpoint_url(&config.endpoint)
            .region(Region::new("us-east-1"))
            .credentials_provider(credentials)
            .load()
            .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
            .force_path_style(true)
            .build();

        let client = Client::from_conf(s3_config);

        info!(bucket = %config.bucket, endpoint = %config.endpoint, "Object storage client initialized");

        Self {
            client,
            bucket: config.bucket.clone(),
        }
    }

    /// Upload an object to storage.
    pub async fn upload(
        &self,
        key: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<(), ObjectStorageError> {
        let mut req = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(body));

        if let Some(ct) = content_type {
            req = req.content_type(ct);
        }

        req.send()
            .await
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Download an object from storage.
    pub async fn download(&self, key: &str) -> Result<Vec<u8>, ObjectStorageError> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NoSuchKey") || msg.contains("not found") {
                    ObjectStorageError::NotFound {
                        key: key.to_string(),
                    }
                } else {
                    ObjectStorageError::Storage(msg)
                }
            })?;

        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?
            .into_bytes()
            .to_vec();

        Ok(bytes)
    }

    /// Delete an object from storage.
    pub async fn delete(&self, key: &str) -> Result<(), ObjectStorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Generate a presigned upload URL valid for the specified duration.
    pub async fn presigned_upload_url(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, ObjectStorageError> {
        let presigning_config = PresigningConfig::builder()
            .expires_in(expires_in)
            .build()
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?;

        let presigned = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| ObjectStorageError::Storage(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Check whether an object exists in storage.
    pub async fn exists(&self, key: &str) -> Result<bool, ObjectStorageError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") || msg.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(ObjectStorageError::Storage(msg))
                }
            }
        }
    }
}
