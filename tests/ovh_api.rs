mod common;

use std::error::Error;

use jiff::civil::time;
use ovh_autobackup_deferrer::ovh::{self, signature};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

use common::{APPLICATION_KEY, APPLICATION_SECRET, CONSUMER_KEY, provider_now};

const SERVICE: &str = "vps-aaaa.vps.ovh.net";

#[tokio::test]
async fn reschedules_with_a_signed_body() -> Result<(), Box<dyn Error>> {
    let server = common::provider().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/1.0/vps/{SERVICE}/automatedBackup/reschedule"
        )))
        .and(body_json(serde_json::json!({ "schedule": "13:40:00" })))
        .and(Signed::by_client_of(&server))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 1, "type": "rescheduleAutoBackup", "state": "todo",
        })))
        .expect(1)
        .mount(&server)
        .await;

    common::client(&server)
        .await?
        .reschedule(SERVICE, time(13, 40, 0, 0))
        .await?;
    Ok(())
}

#[tokio::test]
async fn refusal_is_an_error_carrying_the_status() -> Result<(), Box<dyn Error>> {
    let server = common::provider().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/1.0/vps/{SERVICE}/automatedBackup/reschedule"
        )))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "class": "Client::Forbidden", "message": "This call has not been granted",
        })))
        .mount(&server)
        .await;

    let error = common::client(&server)
        .await?
        .reschedule(SERVICE, time(13, 40, 0, 0))
        .await
        .err();

    assert!(
        matches!(&error, Some(ovh::Error::Status { status, body, .. })
            if status.as_u16() == 403 && body.contains("not been granted")),
        "{error:?}"
    );
    Ok(())
}

/// Matches only requests carrying a valid signature over a timestamp on the provider's clock.
pub struct Signed {
    /// The origin the client addressed; wiremock reports requests against `localhost` instead.
    origin: String,
}

impl Signed {
    pub fn by_client_of(server: &MockServer) -> Self {
        Self {
            origin: server.uri(),
        }
    }
}

impl Match for Signed {
    fn matches(&self, request: &Request) -> bool {
        let header = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
        };
        let (Some(application), Some(consumer), Some(timestamp), Some(sent)) = (
            header("X-Ovh-Application"),
            header("X-Ovh-Consumer"),
            header("X-Ovh-Timestamp"),
            header("X-Ovh-Signature"),
        ) else {
            return false;
        };
        let Ok(timestamp) = timestamp.parse::<i64>() else {
            return false;
        };
        let body = String::from_utf8_lossy(&request.body);
        let expected = signature(
            APPLICATION_SECRET,
            CONSUMER_KEY,
            request.method.as_str(),
            &format!("{}{}", self.origin, request.url.path()),
            &body,
            timestamp,
        );
        application == APPLICATION_KEY
            && consumer == CONSUMER_KEY
            && (provider_now() - timestamp).abs() <= 5
            && sent == expected
    }
}
