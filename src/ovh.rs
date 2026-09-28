//! The OVHcloud API calls the deferrer needs, signed with an application and consumer key.

use std::time::Duration;

use jiff::Timestamp;
use jiff::civil::Time;
use reqwest::header::CONTENT_TYPE;
use sha1::{Digest, Sha1};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Credentials {
    pub application_key: String,
    pub application_secret: String,
    pub consumer_key: String,
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

#[cfg(test)]
pub(crate) mod tests {
    use jiff::civil::time;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;

    const APPLICATION_KEY: &str = "app-key";
    const APPLICATION_SECRET: &str = "app-secret";
    const CONSUMER_KEY: &str = "consumer-key";

    pub(crate) fn client_of(server: &MockServer) -> Client {
        let credentials = Credentials {
            application_key: APPLICATION_KEY.into(),
            application_secret: APPLICATION_SECRET.into(),
            consumer_key: CONSUMER_KEY.into(),
        };
        Client::new(format!("{}/1.0", server.uri()), credentials).unwrap()
    }

    #[test]
    fn signature_matches_the_reference_vector() {
        assert_eq!(
            signature(
                "secret-as",
                "consumer-ck",
                "POST",
                "https://ca.api.ovh.com/1.0/vps/vps-aaaa.vps.ovh.net/automatedBackup/reschedule",
                r#"{"schedule":"14:00:00"}"#,
                1_790_000_000
            ),
            "$1$1ff7993222922400fa5bdad8e8791b36f26ae32c"
        );
    }

    #[tokio::test]
    async fn reschedule_signs_what_it_sends() {
        let server = MockServer::start().await;
        let origin = server.uri();
        Mock::given(method("POST"))
            .and(path(
                "/1.0/vps/vps-aaaa.vps.ovh.net/automatedBackup/reschedule",
            ))
            .and(body_json(serde_json::json!({ "schedule": "13:40:00" })))
            .and(move |request: &Request| is_signed(request, &origin))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        client_of(&server)
            .reschedule("vps-aaaa.vps.ovh.net", time(13, 40, 0, 0))
            .await
            .unwrap();
    }

    /// Whether `request` carries a valid signature over a timestamp from the local clock.
    /// wiremock reports requests against `localhost`, so the signed URL is rebuilt from `origin`.
    fn is_signed(request: &Request, origin: &str) -> bool {
        let header = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
        };
        let timestamp: i64 = header("X-Ovh-Timestamp").parse().unwrap_or_default();
        let expected = signature(
            APPLICATION_SECRET,
            CONSUMER_KEY,
            "POST",
            &format!("{origin}{}", request.url.path()),
            &String::from_utf8_lossy(&request.body),
            timestamp,
        );
        header("X-Ovh-Application") == APPLICATION_KEY
            && header("X-Ovh-Consumer") == CONSUMER_KEY
            && (Timestamp::now().as_second() - timestamp).abs() <= 5
            && header("X-Ovh-Signature") == expected
    }
}
