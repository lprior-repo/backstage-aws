use crate::error::{Error, Result};
use serde_json::{Map, Value};

pub(super) fn object<'a>(
    value: &'a Value,
    allowed: &[&str],
    label: &str,
) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::state_failed(format!("{label} was not a JSON object")))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::state_failed(format!(
            "{label} contained an unexpected JSON field"
        )));
    }
    Ok(object)
}

pub(super) fn value<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a Value> {
    object
        .get(key)
        .ok_or_else(|| Error::state_failed(format!("{label} was missing required field {key}")))
}

pub(super) fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str> {
    value(object, key, label)?
        .as_str()
        .ok_or_else(|| Error::state_failed(format!("{label} field {key} was not a string")))
}

pub(super) fn error_code(stderr: &str) -> Option<String> {
    const MARKER: &str = "An error occurred (";
    let start = stderr.find(MARKER)?.checked_add(MARKER.len())?;
    let rest = stderr.get(start..)?;
    let end = rest.find(')')?;
    let code = rest.get(..end)?;
    if code.is_empty()
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    Some(code.to_owned())
}
