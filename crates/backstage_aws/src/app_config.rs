use crate::error::{Error, Result};
use serde_json::{Map, Number, Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};
use tempfile::NamedTempFile;
use yaml_edit::{
    Document, Mapping, Scalar, Sequence, YamlFile, YamlNode,
    path::{PathError, YamlPath, try_parse_path},
};

pub fn get(app: &Path, file: &str, path: &str) -> Result<Value> {
    let (_, source, _) = read_config(app, file)?;
    let yaml = parse_yaml(&source, file)?;
    validate_path(path)?;

    let node = match yaml.document() {
        Some(document) => match document.try_get_path(path) {
            Ok(node) => Some(node),
            Err(PathError::NotFound { .. } | PathError::NoRoot) => None,
            Err(error) => return Err(path_error(path, &error)),
        },
        None => None,
    };
    let found = node.is_some();
    let value = node.map_or(Value::Null, |node| node_to_json(&node));

    Ok(json!({
        "file": file,
        "path": path,
        "found": found,
        "value": value,
    }))
}

pub fn set(app: &Path, file: &str, path: &str, value: &str) -> Result<Value> {
    let (file_path, source, permissions) = read_config(app, file)?;
    let updated = set_text(&source, path, value)?;
    let changed = source != updated;
    if changed {
        write_atomically(&file_path, &updated, &permissions, file)?;
    }

    Ok(json!({
        "file": file,
        "path": path,
        "value": input_to_json(value),
        "changed": changed,
    }))
}

pub fn set_text(source: &str, path: &str, value: &str) -> Result<String> {
    let yaml = parse_yaml(source, "configuration")?;
    validate_path(path)?;
    let document = yaml.ensure_document();
    set_yaml_value(&document, path, value)?;
    Ok(yaml.to_string())
}

pub fn remove_text(source: &str, path: &str) -> Result<String> {
    let yaml = parse_yaml(source, "configuration")?;
    validate_path(path)?;
    let Some(document) = yaml.document() else {
        return Ok(source.to_owned());
    };

    match document.try_remove_path(path) {
        Ok(_) => Ok(yaml.to_string()),
        Err(PathError::NotFound { .. } | PathError::NoRoot) => Ok(source.to_owned()),
        Err(error) => Err(path_error(path, &error)),
    }
}

fn set_yaml_value(document: &Document, path: &str, value: &str) -> Result<()> {
    match value {
        "true" => document.try_set_path(path, true),
        "false" => document.try_set_path(path, false),
        _ => value.parse::<i64>().map_or_else(
            |_| document.try_set_path(path, value),
            |integer| document.try_set_path(path, integer),
        ),
    }
    .map_err(|error| path_error(path, &error))
}

fn input_to_json(value: &str) -> Value {
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => value.parse::<i64>().map_or_else(
            |_| Value::String(value.to_owned()),
            |integer| Value::Number(Number::from(integer)),
        ),
    }
}

fn node_to_json(node: &YamlNode) -> Value {
    match node {
        YamlNode::Scalar(scalar) => scalar_to_json(scalar),
        YamlNode::Mapping(mapping) => mapping_to_json(mapping),
        YamlNode::Sequence(sequence) => sequence_to_json(sequence),
        YamlNode::Alias(alias) => Value::String(alias.value()),
        YamlNode::TaggedNode(tagged) => tagged.value().map_or_else(
            || match (tagged.as_mapping(), tagged.as_sequence()) {
                (Some(mapping), _) => mapping_to_json(&mapping),
                (_, Some(sequence)) => sequence_to_json(&sequence),
                (None, None) => Value::Null,
            },
            |scalar| scalar_to_json(&scalar),
        ),
    }
}

fn scalar_to_json(scalar: &Scalar) -> Value {
    if scalar.is_null() {
        return Value::Null;
    }
    if let Some(value) = scalar.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = scalar.as_i64() {
        return Value::Number(Number::from(value));
    }
    if let Some(value) = scalar.as_f64() {
        return Number::from_f64(value)
            .map_or_else(|| Value::String(scalar.as_string()), Value::Number);
    }
    Value::String(scalar.as_string())
}

fn mapping_to_json(mapping: &Mapping) -> Value {
    let values = mapping
        .iter()
        .map(|(key, value)| (node_key(&key), node_to_json(&value)));
    Value::Object(values.collect::<Map<String, Value>>())
}

fn sequence_to_json(sequence: &Sequence) -> Value {
    Value::Array(
        sequence
            .values()
            .map(|value| node_to_json(&value))
            .collect(),
    )
}

fn node_key(node: &YamlNode) -> String {
    match node {
        YamlNode::Scalar(scalar) => scalar.as_string(),
        YamlNode::Alias(alias) => alias.value(),
        YamlNode::TaggedNode(tagged) => tagged.as_string().unwrap_or_else(|| node.to_string()),
        YamlNode::Mapping(_) | YamlNode::Sequence(_) => node.to_string(),
    }
}

fn read_config(app: &Path, file: &str) -> Result<(PathBuf, String, fs::Permissions)> {
    let path = config_path(app, file)?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| read_error(file, &error))?;
    if !metadata.file_type().is_file() {
        return Err(Error::precondition_failed(format!(
            "configuration file '{file}' is not a regular file"
        )));
    }
    let source = fs::read_to_string(&path).map_err(|error| read_error(file, &error))?;
    Ok((path, source, metadata.permissions()))
}

fn read_error(file: &str, error: &std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        Error::precondition_failed(format!("configuration file '{file}' does not exist"))
    } else {
        Error::state_failed(format!("read configuration file '{file}': {error}"))
    }
}

fn config_path(app: &Path, file: &str) -> Result<PathBuf> {
    let candidate = Path::new(file);
    let supported_extension = candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "yaml" | "yml"));
    let valid_name = !file.is_empty()
        && !file.contains("..")
        && !file.contains('/')
        && !file.contains('\\')
        && !file.contains('\0')
        && !candidate.is_absolute()
        && candidate.file_name().and_then(|name| name.to_str()) == Some(file)
        && supported_extension;
    if !valid_name {
        return Err(Error::invalid_input(format!(
            "configuration file must be a plain .yaml or .yml filename: '{file}'"
        )));
    }
    Ok(app.join(file))
}

fn validate_path(path: &str) -> Result<()> {
    match try_parse_path(path) {
        Ok(segments) if !segments.is_empty() => Ok(()),
        Ok(_) => Err(Error::invalid_input("configuration path must not be empty")),
        Err(error) => Err(Error::invalid_input(format!(
            "invalid configuration path '{path}': {error}"
        ))),
    }
}

fn parse_yaml(source: &str, file: &str) -> Result<YamlFile> {
    YamlFile::from_str(source)
        .map_err(|error| Error::invalid_input(format!("invalid YAML in '{file}': {error}")))
}

fn path_error(path: &str, error: &PathError) -> Error {
    Error::invalid_input(format!("invalid configuration path '{path}': {error}"))
}

fn write_atomically(
    path: &Path,
    contents: &str,
    permissions: &fs::Permissions,
    file: &str,
) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::state_failed(format!(
            "configuration file '{file}' has no parent directory"
        ))
    })?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| {
        Error::state_failed(format!(
            "create temporary configuration file for '{file}': {error}"
        ))
    })?;
    temporary.write_all(contents.as_bytes()).map_err(|error| {
        Error::state_failed(format!(
            "write temporary configuration file for '{file}': {error}"
        ))
    })?;
    temporary.flush().map_err(|error| {
        Error::state_failed(format!(
            "flush temporary configuration file for '{file}': {error}"
        ))
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        Error::state_failed(format!(
            "sync temporary configuration file for '{file}': {error}"
        ))
    })?;
    temporary
        .as_file()
        .set_permissions(permissions.clone())
        .map_err(|error| {
            Error::state_failed(format!("preserve permissions for '{file}': {error}"))
        })?;
    temporary.persist(path).map_err(|error| {
        Error::state_failed(format!("replace configuration file '{file}': {error}"))
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "app_config/tests.rs"]
mod tests;
