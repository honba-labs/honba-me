//! Automated instrument master data downloaders.

use reqwest;
use thiserror::Error;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Error)]
pub enum DownloaderError {
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct MasterDownloader {
    client: reqwest::Client,
}

impl MasterDownloader {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn download_zerodha_instruments(&self, path: &Path) -> Result<(), DownloaderError> {
        let url = "https://api.kite.trade/instruments";
        let response = self.client.get(url).send().await?.bytes().await?;
        let mut file = File::create(path).await?;
        file.write_all(&response).await?;
        Ok(())
    }
}
