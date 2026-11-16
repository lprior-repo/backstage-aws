//! The explicitly selected local container command; never silently falls back.
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ContainerEngine {
    #[default]
    Docker,
    Podman,
}

impl ContainerEngine {
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }
}

impl fmt::Display for ContainerEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.program())
    }
}
