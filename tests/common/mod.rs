use ovh_autobackup_deferrer::ovh::{Client, Credentials};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const APPLICATION_KEY: &str = "app-key";
pub const APPLICATION_SECRET: &str = "app-secret";
pub const CONSUMER_KEY: &str = "consumer-key";

/// Far enough from the local clock that a client ignoring `/auth/time` fails the signature check.
pub const PROVIDER_CLOCK_AHEAD_BY: i64 = 100_000;

pub fn credentials() -> Credentials {
    Credentials::new(
        APPLICATION_KEY.into(),
        APPLICATION_SECRET.into(),
        CONSUMER_KEY.into(),
    )
}

pub fn provider_now() -> i64 {
    jiff::Timestamp::now().as_second() + PROVIDER_CLOCK_AHEAD_BY
}

/// A provider stand-in whose `/auth/time` runs ahead of the local clock.
pub async fn provider() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1.0/auth/time"))
        .respond_with(ResponseTemplate::new(200).set_body_string(provider_now().to_string()))
        .mount(&server)
        .await;
    server
}

pub async fn client(server: &MockServer) -> Result<Client, ovh_autobackup_deferrer::ovh::Error> {
    Client::connect(format!("{}/1.0", server.uri()), credentials()).await
}
