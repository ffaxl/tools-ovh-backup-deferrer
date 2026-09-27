//! The OVHcloud API calls the deferrer needs, signed with an application and consumer key.

use std::fmt;
use std::time::Duration;

use jiff::Timestamp;
use jiff::civil::Time;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Method, RequestBuilder};
use sha1::{Digest, Sha1};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
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

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("application_key", &self.application_key)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot set up the HTTP client")]
    Setup(#[source] reqwest::Error),
    #[error("{method} {url} failed")]
    Transport {
        method: Method,
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{method} {url} returned {status}: {body}")]
    Status {
        method: Method,
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
        self.signed(Method::POST, &url, body).await.map(drop)
    }

    async fn signed(&self, method: Method, url: &str, body: String) -> Result<String, Error> {
        let timestamp = Timestamp::now().as_second();
        let credentials = &self.credentials;
        let mut request = self
            .http
            .request(method.clone(), url)
            .header("X-Ovh-Application", &credentials.application_key)
            .header("X-Ovh-Consumer", &credentials.consumer_key)
            .header("X-Ovh-Timestamp", timestamp.to_string())
            .header(
                "X-Ovh-Signature",
                signature(
                    &credentials.application_secret,
                    &credentials.consumer_key,
                    method.as_str(),
                    url,
                    &body,
                    timestamp,
                ),
            );
        if !body.is_empty() {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        send(&method, url, request).await
    }
}

async fn send(method: &Method, url: &str, request: RequestBuilder) -> Result<String, Error> {
    let response = request
        .send()
        .await
        .map_err(|source| transport(method, url, source))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|source| transport(method, url, source))?;
    if !status.is_success() {
        return Err(Error::Status {
            method: method.clone(),
            url: url.into(),
            status,
            body,
        });
    }
    Ok(body)
}

fn transport(method: &Method, url: &str, source: reqwest::Error) -> Error {
    Error::Transport {
        method: method.clone(),
        url: url.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://ca.api.ovh.com/1.0/vps/vps-aaaa.vps.ovh.net/automatedBackup";

    #[test]
    fn signature_of_a_get_matches_the_reference_vector() {
        assert_eq!(
            signature("secret-as", "consumer-ck", "GET", URL, "", 1_790_000_000),
            "$1$9135387132415f5f58f2258a341f0c034db53819"
        );
    }

    #[test]
    fn signature_of_a_post_covers_the_body() {
        assert_eq!(
            signature(
                "secret-as",
                "consumer-ck",
                "POST",
                &format!("{URL}/reschedule"),
                r#"{"schedule":"14:00:00"}"#,
                1_790_000_000
            ),
            "$1$1ff7993222922400fa5bdad8e8791b36f26ae32c"
        );
    }

    #[test]
    fn debug_output_hides_the_secrets() {
        let credentials =
            Credentials::new("app-key".into(), "app-secret".into(), "consumer".into());
        let printed = format!("{credentials:?}");
        assert!(printed.contains("app-key"));
        assert!(!printed.contains("app-secret"));
        assert!(!printed.contains("consumer"));
    }
}
