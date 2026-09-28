//! Configuration from environment variables, rejected whole at startup if any part is wrong.

use crate::ovh::Credentials;

pub struct Config {
    pub base_url: String,
    pub credentials: Credentials,
    pub services: Vec<Service>,
    pub dry_run: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    /// Minutes past the hour at which the schedule is set, keeping this service's backup window
    /// apart from every other service's.
    pub offset: u8,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{0} is not set")]
    Missing(&'static str),
    #[error("OVH_ENDPOINT {0:?} is not one of ovh-eu, ovh-ca, ovh-us")]
    UnknownEndpoint(String),
    #[error("DEFERRER_DRY_RUN {0:?} is neither true nor false")]
    InvalidDryRun(String),
    #[error("DEFERRER_SERVICES entry {0:?} is not service:offset")]
    MalformedService(String),
    #[error(
        "service name {0:?} must start with a letter or digit and hold only those, '.' and '-'"
    )]
    InvalidServiceName(String),
    #[error("offset of {service} must be 0 to 59 minutes")]
    OffsetOutOfRange { service: String },
    #[error("{0} is listed twice")]
    DuplicateService(String),
    #[error("{first} and {second} share an offset, so their backup windows would coincide")]
    DuplicateOffset { first: String, second: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let optional = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let required = |name: &'static str| optional(name).ok_or(ConfigError::Missing(name));

        let endpoint = optional("OVH_ENDPOINT").unwrap_or_else(|| "ovh-ca".into());
        let base_url = base_url(&endpoint).ok_or(ConfigError::UnknownEndpoint(endpoint))?;
        let credentials = Credentials {
            application_key: required("OVH_APPLICATION_KEY")?,
            application_secret: required("OVH_APPLICATION_SECRET")?,
            consumer_key: required("OVH_CONSUMER_KEY")?,
        };
        let services = parse_services(&required("DEFERRER_SERVICES")?)?;
        let dry_run = match optional("DEFERRER_DRY_RUN").as_deref() {
            Some("true") => true,
            None | Some("false") => false,
            Some(other) => return Err(ConfigError::InvalidDryRun(other.into())),
        };

        Ok(Self {
            base_url: base_url.into(),
            credentials,
            services,
            dry_run,
        })
    }
}

fn base_url(endpoint: &str) -> Option<&'static str> {
    match endpoint {
        "ovh-eu" => Some("https://eu.api.ovh.com/1.0"),
        "ovh-ca" => Some("https://ca.api.ovh.com/1.0"),
        "ovh-us" => Some("https://api.us.ovhcloud.com/1.0"),
        _ => None,
    }
}

/// Whether `name` can go into the request path verbatim: no separators, and no `.` or `..`
/// that URL normalisation would resolve into a different path than the one signed.
fn is_path_segment(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn parse_services(list: &str) -> Result<Vec<Service>, ConfigError> {
    let mut services: Vec<Service> = Vec::new();
    for entry in list.split(',').map(str::trim) {
        let malformed = || ConfigError::MalformedService(entry.into());
        let (name, minutes) = entry.rsplit_once(':').ok_or_else(malformed)?;
        let offset: u8 = minutes.parse().map_err(|_| malformed())?;

        if !is_path_segment(name) {
            return Err(ConfigError::InvalidServiceName(name.into()));
        }
        if offset > 59 {
            return Err(ConfigError::OffsetOutOfRange {
                service: name.into(),
            });
        }
        if services.iter().any(|service| service.name == name) {
            return Err(ConfigError::DuplicateService(name.into()));
        }
        if let Some(other) = services.iter().find(|service| service.offset == offset) {
            return Err(ConfigError::DuplicateOffset {
                first: other.name.clone(),
                second: name.into(),
            });
        }
        services.push(Service {
            name: name.into(),
            offset,
        });
    }
    Ok(services)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn load(overrides: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut vars: HashMap<&str, &str> = HashMap::from([
            ("OVH_APPLICATION_KEY", "ak"),
            ("OVH_APPLICATION_SECRET", "as"),
            ("OVH_CONSUMER_KEY", "ck"),
            ("DEFERRER_SERVICES", "vps-a.example:0"),
        ]);
        for (name, value) in overrides {
            vars.insert(name, value);
        }
        Config::from_lookup(|name| vars.get(name).map(|value| value.to_string()))
    }

    fn service(name: &str, offset: u8) -> Service {
        Service {
            name: name.into(),
            offset,
        }
    }

    #[test]
    fn accepts_valid_configuration() {
        let defaults = load(&[]).unwrap();
        assert_eq!(defaults.base_url, "https://ca.api.ovh.com/1.0");
        assert!(!defaults.dry_run);

        for (endpoint, base_url) in [
            ("ovh-eu", "https://eu.api.ovh.com/1.0"),
            ("ovh-us", "https://api.us.ovhcloud.com/1.0"),
        ] {
            assert_eq!(
                load(&[("OVH_ENDPOINT", endpoint)]).unwrap().base_url,
                base_url
            );
        }
        assert!(load(&[("DEFERRER_DRY_RUN", "true")]).unwrap().dry_run);
        assert_eq!(
            load(&[("DEFERRER_SERVICES", " vps-a.example:0 , vps-b.example:40 ")])
                .unwrap()
                .services,
            [service("vps-a.example", 0), service("vps-b.example", 40)]
        );
    }

    #[test]
    fn rejects_invalid_configuration() {
        use ConfigError::*;
        let cases = [
            (
                "OVH_ENDPOINT",
                "kimsufi-eu",
                UnknownEndpoint("kimsufi-eu".into()),
            ),
            ("OVH_CONSUMER_KEY", "", Missing("OVH_CONSUMER_KEY")),
            ("DEFERRER_DRY_RUN", "no", InvalidDryRun("no".into())),
            ("DEFERRER_SERVICES", " ", Missing("DEFERRER_SERVICES")),
            (
                "DEFERRER_SERVICES",
                "vps-a.example",
                MalformedService("vps-a.example".into()),
            ),
            (
                "DEFERRER_SERVICES",
                "vps-a.example:",
                MalformedService("vps-a.example:".into()),
            ),
            ("DEFERRER_SERVICES", ":20", InvalidServiceName("".into())),
            (
                "DEFERRER_SERVICES",
                "vps-a.example:x",
                MalformedService("vps-a.example:x".into()),
            ),
            (
                "DEFERRER_SERVICES",
                "vps-a.example:0,",
                MalformedService("".into()),
            ),
            (
                "DEFERRER_SERVICES",
                "../me:0",
                InvalidServiceName("../me".into()),
            ),
            ("DEFERRER_SERVICES", ".:0", InvalidServiceName(".".into())),
            ("DEFERRER_SERVICES", "..:0", InvalidServiceName("..".into())),
            (
                "DEFERRER_SERVICES",
                "vps-a.example:60",
                OffsetOutOfRange {
                    service: "vps-a.example".into(),
                },
            ),
            (
                "DEFERRER_SERVICES",
                "vps-a.example:0,vps-a.example:20",
                DuplicateService("vps-a.example".into()),
            ),
            (
                "DEFERRER_SERVICES",
                "vps-a.example:20,vps-b.example:20",
                DuplicateOffset {
                    first: "vps-a.example".into(),
                    second: "vps-b.example".into(),
                },
            ),
        ];
        for (name, value, expected) in cases {
            match load(&[(name, value)]) {
                Ok(_) => panic!("{name}={value:?} was accepted"),
                Err(error) => assert_eq!(error, expected, "{name}={value:?}"),
            }
        }
    }
}
