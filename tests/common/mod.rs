use ovh_autobackup_deferrer::ovh::{Client, Credentials, Error};
use wiremock::MockServer;

pub const APPLICATION_KEY: &str = "app-key";
pub const APPLICATION_SECRET: &str = "app-secret";
pub const CONSUMER_KEY: &str = "consumer-key";

pub fn credentials() -> Credentials {
    Credentials::new(
        APPLICATION_KEY.into(),
        APPLICATION_SECRET.into(),
        CONSUMER_KEY.into(),
    )
}

pub fn client(server: &MockServer) -> Result<Client, Error> {
    Client::new(format!("{}/1.0", server.uri()), credentials())
}
