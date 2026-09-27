//! Configuration from environment variables, rejected whole at startup if any part is wrong.

use crate::ovh::Credentials;
use crate::schedule::Offset;

pub struct Config {
    pub base_url: String,
    pub credentials: Credentials,
    pub services: Vec<Service>,
    pub dry_run: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub offset: Offset,
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
        let credentials = Credentials::new(
            required("OVH_APPLICATION_KEY")?,
            required("OVH_APPLICATION_SECRET")?,
            required("OVH_CONSUMER_KEY")?,
        );
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
        let (name, minutes) = entry
            .rsplit_once(':')
            .filter(|(name, minutes)| !name.is_empty() && !minutes.is_empty())
            .ok_or_else(malformed)?;
        let minutes: u32 = minutes.parse().map_err(|_| malformed())?;

        if !is_path_segment(name) {
            return Err(ConfigError::InvalidServiceName(name.into()));
        }
        let offset = u8::try_from(minutes)
            .ok()
            .and_then(Offset::from_minutes)
            .ok_or_else(|| ConfigError::OffsetOutOfRange {
                service: name.into(),
            })?;
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

    fn config(overrides: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut vars: HashMap<&str, &str> = HashMap::from([
            ("OVH_APPLICATION_KEY", "ak"),
            ("OVH_APPLICATION_SECRET", "as"),
            ("OVH_CONSUMER_KEY", "ck"),
            ("DEFERRER_SERVICES", "vps-a.example:0,vps-b.example:20"),
        ]);
        for (name, value) in overrides {
            vars.insert(name, value);
        }
        Config::from_lookup(|name| vars.get(name).map(|value| value.to_string()))
    }

    fn rejected(overrides: &[(&str, &str)]) -> ConfigError {
        match config(overrides) {
            Ok(_) => panic!("{overrides:?} was accepted"),
            Err(error) => error,
        }
    }

    fn service(name: &str, minutes: u8) -> Service {
        Service {
            name: name.into(),
            offset: Offset::from_minutes(minutes).unwrap(),
        }
    }

    #[test]
    fn defaults_to_ovh_ca_and_writing() {
        let config = config(&[]).unwrap();
        assert_eq!(config.base_url, "https://ca.api.ovh.com/1.0");
        assert!(!config.dry_run);
        assert_eq!(
            config.services,
            [service("vps-a.example", 0), service("vps-b.example", 20)]
        );
    }

    #[test]
    fn endpoint_selects_the_region() {
        let base = |endpoint| config(&[("OVH_ENDPOINT", endpoint)]).map(|config| config.base_url);
        assert_eq!(base("ovh-eu").unwrap(), "https://eu.api.ovh.com/1.0");
        assert_eq!(base("ovh-us").unwrap(), "https://api.us.ovhcloud.com/1.0");
        assert_eq!(
            base("kimsufi-eu").unwrap_err(),
            ConfigError::UnknownEndpoint("kimsufi-eu".into())
        );
    }

    #[test]
    fn dry_run_accepts_only_true_or_false() {
        assert!(!config(&[("DEFERRER_DRY_RUN", "false")]).unwrap().dry_run);
        assert!(config(&[("DEFERRER_DRY_RUN", "true")]).unwrap().dry_run);
        assert_eq!(
            rejected(&[("DEFERRER_DRY_RUN", "no")]),
            ConfigError::InvalidDryRun("no".into())
        );
    }

    #[test]
    fn missing_or_empty_credential_is_rejected() {
        assert_eq!(
            rejected(&[("OVH_CONSUMER_KEY", "")]),
            ConfigError::Missing("OVH_CONSUMER_KEY")
        );
    }

    #[test]
    fn empty_service_list_is_rejected() {
        assert_eq!(
            rejected(&[("DEFERRER_SERVICES", " ")]),
            ConfigError::Missing("DEFERRER_SERVICES")
        );
    }

    #[test]
    fn service_list_tolerates_spaces() {
        let config =
            config(&[("DEFERRER_SERVICES", " vps-a.example:0 , vps-b.example:40 ")]).unwrap();
        assert_eq!(
            config.services,
            [service("vps-a.example", 0), service("vps-b.example", 40)]
        );
    }

    #[test]
    fn malformed_entries_are_rejected() {
        for entry in [
            "vps-a.example",
            "vps-a.example:",
            ":20",
            "vps-a.example:x",
            "vps-a.example:0,",
        ] {
            assert!(
                matches!(
                    rejected(&[("DEFERRER_SERVICES", entry)]),
                    ConfigError::MalformedService(_)
                ),
                "{entry:?} was accepted"
            );
        }
    }

    #[test]
    fn service_name_cannot_escape_the_url_path() {
        assert_eq!(
            rejected(&[("DEFERRER_SERVICES", "../me:0")]),
            ConfigError::InvalidServiceName("../me".into())
        );
        for name in [".", ".."] {
            assert_eq!(
                rejected(&[("DEFERRER_SERVICES", &format!("{name}:0"))]),
                ConfigError::InvalidServiceName(name.into()),
            );
        }
    }

    #[test]
    fn offset_above_59_is_rejected() {
        assert_eq!(
            rejected(&[("DEFERRER_SERVICES", "vps-a.example:60")]),
            ConfigError::OffsetOutOfRange {
                service: "vps-a.example".into()
            }
        );
    }

    #[test]
    fn duplicate_service_is_rejected() {
        assert_eq!(
            rejected(&[("DEFERRER_SERVICES", "vps-a.example:0,vps-a.example:20")]),
            ConfigError::DuplicateService("vps-a.example".into())
        );
    }

    #[test]
    fn shared_offset_is_rejected() {
        assert_eq!(
            rejected(&[("DEFERRER_SERVICES", "vps-a.example:20,vps-b.example:20")]),
            ConfigError::DuplicateOffset {
                first: "vps-a.example".into(),
                second: "vps-b.example".into()
            }
        );
    }
}
