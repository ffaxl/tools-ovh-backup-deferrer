//! The OVHcloud API calls the deferrer needs, signed with an application and consumer key.

use std::time::Duration;

use jiff::Timestamp;
use jiff::civil::Time;
use reqwest::header::CONTENT_TYPE;
use sha1::{Digest, Sha1};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Credentials {
    application_key: String,
    application_secret: String,
    consumer_key: String,
}

impl Credentials {
    pub fn new(application_key: String, application_secret: String, consumer_key: String) -> Self {
        Self {
            application_key,
            application_secret,
            consumer_key,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot set up the HTTP client")]
    Setup(#[source] reqwest::Error),
    #[error("POST {url} failed")]
    Transport {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("POST {url} returned {status}: {body}")]
    Status {
        url: String,
        status: reqwest::StatusCode,
        body: String,
    },
}

/// The `X-Ovh-Signature` value for one request.
pub fn signature(
    application_secret: &str,
    consumer_key: &str,
    method: &str,
    url: &str,
    body: &str,
    timestamp: i64,
) -> String {
    let signed = format!("{application_secret}+{consumer_key}+{method}+{url}+{body}+{timestamp}");
    format!("$1${}", hex::encode(Sha1::digest(signed)))
}

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    credentials: Credentials,
}

impl Client {
    pub fn new(base_url: impl Into<String>, credentials: Credentials) -> Result<Self, Error> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(Error::Setup)?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            credentials,
        })
    }

    /// Queues the change; the provider applies it asynchronously, minutes later.
    pub async fn reschedule(&self, service: &str, schedule: Time) -> Result<(), Error> {
        let url = format!("{}/vps/{service}/automatedBackup/reschedule", self.base_url);
        let body = serde_json::json!({ "schedule": schedule }).to_string();
        let timestamp = Timestamp::now().as_second();
        let credentials = &self.credentials;
        let signature = signature(
            &credentials.application_secret,
            &credentials.consumer_key,
            "POST",
            &url,
            &body,
            timestamp,
        );
        let transport = |source| Error::Transport {
            url: url.clone(),
            source,
        };

        let response = self
            .http
            .post(&url)
            .header("X-Ovh-Application", &credentials.application_key)
            .header("X-Ovh-Consumer", &credentials.consumer_key)
            .header("X-Ovh-Timestamp", timestamp.to_string())
            .header("X-Ovh-Signature", signature)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        Err(Error::Status {
            status,
            body: response.text().await.map_err(transport)?,
            url,
        })
    }
}
