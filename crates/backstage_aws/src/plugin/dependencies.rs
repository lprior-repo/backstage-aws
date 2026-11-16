use crate::error::{Error, Result};
use regex::Regex;
use serde_json::{Map, Value};

const DEPENDENCY_BLOCK: &str =
    r#"(?ms)(?P<head>^[ \t]*"dependencies"\s*:\s*\{\s*\r?\n)(?P<body>.*?)(?P<tail>^[ \t]*\})"#;

pub(super) fn dependency_version(source: &str, package: &str) -> Result<Option<String>> {
    let document: Value = serde_json::from_str(source)
        .map_err(|error| Error::state_failed(format!("invalid package.json: {error}")))?;
    let dependencies = dependency_object(&document)?;
    Ok(dependencies
        .get(package)
        .and_then(Value::as_str)
        .map(str::to_owned))
}

pub(super) fn set_dependency(source: &str, package: &str, version: &str) -> Result<String> {
    let document: Value = serde_json::from_str(source)
        .map_err(|error| Error::state_failed(format!("invalid package.json: {error}")))?;
    let dependencies = dependency_object(&document)?;
    if dependencies.get(package).and_then(Value::as_str) == Some(version) {
        return Ok(source.to_owned());
    }
    let block = dependency_match(source)?;
    let updated_body = if dependencies.contains_key(package) {
        update_line(block.body, package, version)?
    } else {
        insert_line(block.head, block.body, package, version)?
    };
    block.rebuild(source, &updated_body)
}

pub(super) fn remove_dependency(source: &str, package: &str) -> Result<String> {
    let document: Value = serde_json::from_str(source)
        .map_err(|error| Error::state_failed(format!("invalid package.json: {error}")))?;
    let dependencies = dependency_object(&document)?;
    if !dependencies.contains_key(package) {
        return Ok(source.to_owned());
    }
    let block = dependency_match(source)?;
    let updated_body = remove_line(block.body, package)?;
    block.rebuild(source, &updated_body)
}

fn dependency_object(value: &Value) -> Result<&Map<String, Value>> {
    value
        .get("dependencies")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::precondition_failed("package.json must contain a dependencies object")
        })
}

struct DependencyBlock<'a> {
    start: usize,
    end: usize,
    head: &'a str,
    body: &'a str,
    tail: &'a str,
}

impl DependencyBlock<'_> {
    fn rebuild(&self, source: &str, body: &str) -> Result<String> {
        let prefix = source
            .get(..self.start)
            .ok_or_else(|| Error::state_failed("dependency block starts outside package.json"))?;
        let suffix = source
            .get(self.end..)
            .ok_or_else(|| Error::state_failed("dependency block ends outside package.json"))?;
        Ok(format!("{prefix}{}{body}{}{suffix}", self.head, self.tail))
    }
}

fn dependency_match(source: &str) -> Result<DependencyBlock<'_>> {
    let matcher = Regex::new(DEPENDENCY_BLOCK).map_err(|error| {
        Error::state_failed(format!("cannot build dependency matcher: {error}"))
    })?;
    let captures = matcher.captures(source).ok_or_else(|| {
        Error::precondition_failed("package.json must contain a multiline dependencies object")
    })?;
    let whole = captures
        .get(0)
        .ok_or_else(|| Error::state_failed("dependency block match is empty"))?;
    let head = captures
        .name("head")
        .ok_or_else(|| Error::state_failed("dependency block has no header"))?
        .as_str();
    let body = captures
        .name("body")
        .ok_or_else(|| Error::state_failed("dependency block has no body"))?
        .as_str();
    let tail = captures
        .name("tail")
        .ok_or_else(|| Error::state_failed("dependency block has no footer"))?
        .as_str();
    Ok(DependencyBlock {
        start: whole.start(),
        end: whole.end(),
        head,
        body,
        tail,
    })
}

fn update_line(body: &str, package: &str, version: &str) -> Result<String> {
    let pattern = format!(
        r#"(?m)^([ \t]*"{}"[ \t]*:[ \t]*)"[^"]*"([ \t]*,?)([ \t]*\r?\n|$)"#,
        regex::escape(package)
    );
    let matcher = Regex::new(&pattern).map_err(|error| {
        Error::state_failed(format!("cannot build dependency line matcher: {error}"))
    })?;
    let captures = matcher.captures(body).ok_or_else(|| {
        Error::precondition_failed(format!("cannot locate dependency line for '{package}'"))
    })?;
    let whole = captures
        .get(0)
        .ok_or_else(|| Error::state_failed("dependency line match is empty"))?;
    let prefix = capture(&captures, 1, "dependency line prefix")?;
    let comma = capture(&captures, 2, "dependency line comma")?;
    let ending = capture(&captures, 3, "dependency line ending")?;
    let before = body
        .get(..whole.start())
        .ok_or_else(|| Error::state_failed("dependency line begins outside the block"))?;
    let after = body
        .get(whole.end()..)
        .ok_or_else(|| Error::state_failed("dependency line ends outside the block"))?;
    Ok(format!(
        "{before}{prefix}\"{version}\"{comma}{ending}{after}"
    ))
}

fn insert_line(head: &str, body: &str, package: &str, version: &str) -> Result<String> {
    let newline = line_ending(head);
    let matcher = Regex::new(r#"(?m)^([ \t]*)"([^"]+)"[ \t]*:"#).map_err(|error| {
        Error::state_failed(format!("cannot build dependency key matcher: {error}"))
    })?;
    let next = matcher
        .captures_iter(body)
        .find(|captures| captures.get(2).is_some_and(|key| key.as_str() > package));
    match next {
        Some(captures) => {
            let whole = captures
                .get(0)
                .ok_or_else(|| Error::state_failed("dependency key match is empty"))?;
            let indent = capture(&captures, 1, "dependency indentation")?;
            let before = body.get(..whole.start()).ok_or_else(|| {
                Error::state_failed("dependency insertion begins outside the block")
            })?;
            let after = body.get(whole.start()..).ok_or_else(|| {
                Error::state_failed("dependency insertion ends outside the block")
            })?;
            Ok(format!(
                "{before}{indent}\"{package}\": \"{version}\",{newline}{after}"
            ))
        }
        None => append_line(head, body, package, version, newline),
    }
}

fn append_line(
    head: &str,
    body: &str,
    package: &str,
    version: &str,
    newline: &str,
) -> Result<String> {
    let matcher =
        Regex::new(r#"(?m)^([ \t]*"[^"]+"[ \t]*:[ \t]*"[^"]*")([ \t]*,?)([ \t]*\r?\n|$)"#)
            .map_err(|error| {
                Error::state_failed(format!("cannot build dependency line matcher: {error}"))
            })?;
    if let Some(captures) = matcher.captures_iter(body).last() {
        let whole = captures
            .get(0)
            .ok_or_else(|| Error::state_failed("last dependency match is empty"))?;
        let entry = capture(&captures, 1, "last dependency line")?;
        let indent: String = entry
            .chars()
            .take_while(|character| character.is_whitespace())
            .collect();
        let ending = capture(&captures, 3, "last dependency line ending")?;
        let before = body
            .get(..whole.start())
            .ok_or_else(|| Error::state_failed("last dependency begins outside the block"))?;
        let after = body
            .get(whole.end()..)
            .ok_or_else(|| Error::state_failed("last dependency ends outside the block"))?;
        Ok(format!(
            "{before}{entry},{ending}{indent}\"{package}\": \"{version}\"{newline}{after}"
        ))
    } else {
        let matcher = Regex::new(r#"(?m)^([ \t]*)"dependencies""#).map_err(|error| {
            Error::state_failed(format!(
                "cannot build dependencies indentation matcher: {error}"
            ))
        })?;
        let captures = matcher
            .captures(head)
            .ok_or_else(|| Error::state_failed("cannot determine dependencies indentation"))?;
        let parent_indent = capture(&captures, 1, "dependencies indentation")?;
        let indent = format!("{parent_indent}  ");
        Ok(format!(
            "{body}{newline}{indent}\"{package}\": \"{version}\"{newline}"
        ))
    }
}

fn remove_line(body: &str, package: &str) -> Result<String> {
    let pattern = format!(
        r#"(?m)^([ \t]*"{}"[ \t]*:[ \t]*"[^"]*")([ \t]*,?)([ \t]*\r?\n|$)"#,
        regex::escape(package)
    );
    let matcher = Regex::new(&pattern).map_err(|error| {
        Error::state_failed(format!("cannot build dependency line matcher: {error}"))
    })?;
    let captures = matcher.captures(body).ok_or_else(|| {
        Error::precondition_failed(format!("cannot locate dependency line for '{package}'"))
    })?;
    let whole = captures
        .get(0)
        .ok_or_else(|| Error::state_failed("dependency line match is empty"))?;
    let comma = capture(&captures, 2, "dependency line comma")?;
    let before = body
        .get(..whole.start())
        .ok_or_else(|| Error::state_failed("dependency line begins outside the block"))?;
    let after = body
        .get(whole.end()..)
        .ok_or_else(|| Error::state_failed("dependency line ends outside the block"))?;
    let without = format!("{before}{after}");
    if comma.is_empty() {
        remove_trailing_comma(&without)
    } else {
        Ok(without)
    }
}

fn remove_trailing_comma(body: &str) -> Result<String> {
    let matcher = Regex::new(r#"(?m)^([ \t]*"[^"]+"[ \t]*:[ \t]*"[^"]*")([ \t]*,)([ \t]*\r?\n|$)"#)
        .map_err(|error| {
            Error::state_failed(format!("cannot build trailing comma matcher: {error}"))
        })?;
    let Some(captures) = matcher.captures_iter(body).last() else {
        return Ok(body.to_owned());
    };
    let whole = captures
        .get(0)
        .ok_or_else(|| Error::state_failed("last dependency match is empty"))?;
    let entry = capture(&captures, 1, "last dependency line")?;
    let ending = capture(&captures, 3, "last dependency line ending")?;
    let before = body
        .get(..whole.start())
        .ok_or_else(|| Error::state_failed("last dependency begins outside the block"))?;
    let after = body
        .get(whole.end()..)
        .ok_or_else(|| Error::state_failed("last dependency ends outside the block"))?;
    Ok(format!("{before}{entry}{ending}{after}"))
}

fn capture<'a>(captures: &'a regex::Captures<'_>, index: usize, label: &str) -> Result<&'a str> {
    captures
        .get(index)
        .map(|value| value.as_str())
        .ok_or_else(|| Error::state_failed(format!("{label} capture is missing")))
}

fn line_ending(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}
