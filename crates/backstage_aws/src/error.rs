use std::{fmt, io, path::Path};

#[derive(Debug)]
pub enum Error {
    Usage,
    InvalidConfig(&'static str),
    InvalidTemplate(&'static str),
    Io {
        operation: &'static str,
        path: std::path::PathBuf,
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_owned(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => {
                output.write_str("usage: backstage-aws <config.toml> <new-output-directory>")
            }
            Self::InvalidConfig(field) => write!(output, "invalid configuration: {field}"),
            Self::InvalidTemplate(path) => write!(output, "invalid embedded template: {path}"),
            Self::Io {
                operation,
                path,
                source,
            } => {
                write!(output, "{operation} {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
