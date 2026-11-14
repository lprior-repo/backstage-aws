mod config;
mod embedded;
mod error;
mod validation;

pub use config::Config;
pub use error::Error;

use std::{
    borrow::Cow,
    fs,
    io::{Read, Write},
    path::{Component, Path},
};

pub(crate) struct Asset {
    path: &'static str,
    source: Source,
    executable: bool,
}

pub(crate) enum Source {
    File(&'static [u8]),
    Template(&'static [u8]),
    DeploymentConfig,
}

struct PreparedFile {
    path: &'static str,
    bytes: Cow<'static, [u8]>,
    executable: bool,
}

/// Read a bounded configuration file before generating any output.
///
/// # Errors
/// Returns a contextual I/O error or a configuration validation error.
pub fn read_config(path: &Path) -> Result<Config, Error> {
    let file = fs::File::open(path).map_err(|source| Error::io("read config", path, source))?;
    let mut text = String::new();
    file.take(config::MAX_CONFIG_BYTES.saturating_add(1))
        .read_to_string(&mut text)
        .map_err(|source| Error::io("read config", path, source))?;
    Config::parse(&text)
}

/// Render the embedded project into a new directory without invoking other tools.
///
/// # Errors
/// Existing destinations are rejected. I/O errors preserve existing files and may
/// leave a partial newly-created output directory for inspection.
pub fn generate(config: &Config, destination: &Path) -> Result<usize, Error> {
    let files = embedded::ASSETS
        .iter()
        .map(|asset| prepare_file(config, asset))
        .collect::<Result<Vec<_>, _>>()?;
    fs::create_dir(destination)
        .map_err(|source| Error::io("create new output directory", destination, source))?;
    files
        .iter()
        .try_for_each(|file| write_file(destination, file))?;
    Ok(files.len())
}

fn prepare_file(config: &Config, asset: &Asset) -> Result<PreparedFile, Error> {
    if !Path::new(asset.path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(Error::InvalidTemplate(asset.path));
    }
    let bytes = match asset.source {
        Source::File(bytes) => Cow::Borrowed(bytes),
        Source::Template(bytes) => render_template(config, asset.path, bytes)?,
        Source::DeploymentConfig => Cow::Owned(config.deployment_json()?),
    };
    Ok(PreparedFile {
        path: asset.path,
        bytes,
        executable: asset.executable,
    })
}

fn render_template(
    config: &Config,
    path: &'static str,
    bytes: &'static [u8],
) -> Result<Cow<'static, [u8]>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidTemplate(path))?;
    let rendered = config.render(text);
    if rendered.contains("__BACKSTAGE_AWS_") {
        return Err(Error::InvalidTemplate(path));
    }
    Ok(Cow::Owned(rendered.into_bytes()))
}

fn write_file(root: &Path, file: &PreparedFile) -> Result<(), Error> {
    let path = root.join(file.path);
    let parent = path.parent().ok_or(Error::InvalidTemplate(file.path))?;
    fs::create_dir_all(parent)
        .map_err(|source| Error::io("create output parent", parent, source))?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut output| output.write_all(&file.bytes))
        .map_err(|source| Error::io("write output file (generation incomplete)", &path, source))?;
    set_executable(&path, file.executable)
}

fn set_executable(path: &Path, executable: bool) -> Result<(), Error> {
    if !executable {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|source| Error::io("set executable permissions", path, source))?;
    }
    Ok(())
}
