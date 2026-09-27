//! Keeps the OVHcloud VPS automated backup schedule in the recent past so it never fires.

pub mod config;
pub mod deferrer;
pub mod ovh;
pub mod schedule;
