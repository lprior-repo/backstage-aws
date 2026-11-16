//! Maintained HCL syntax trees, not hand-built Terraform text.
use crate::error::{Error, Result};
use hcl::{Block, Body};
use serde_json::Value;
use std::path::Path;

/// Keep reviewed blueprint comments, formatting, expressions and heredocs intact.
/// The lossless encoder belongs to the same upstream project as `hcl-rs`.
pub fn blueprint(path: &Path, contents: &[u8]) -> Result<Vec<u8>> {
    let invalid = |error: &dyn std::fmt::Display| {
        Error::state_failed(format!("generate Terraform {}: {error}", path.display()))
    };
    let source = std::str::from_utf8(contents).map_err(|error| invalid(&error))?;
    let body: hcl_edit::structure::Body = source.parse().map_err(|error| invalid(&error))?;
    Ok(body.to_string().into_bytes())
}

/// Serialize values as literals; interpolation markers in strings are escaped.
pub fn variables(values: &Value) -> Result<Vec<u8>> {
    hcl::to_string(values)
        .map(String::into_bytes)
        .map_err(|error| Error::state_failed(format!("generate Terraform variables: {error}")))
}

pub fn backend() -> Result<Vec<u8>> {
    let body = Body::builder()
        .add_block(
            Block::builder("terraform")
                .add_block(Block::builder("backend").add_label("s3").build())
                .build(),
        )
        .build();
    hcl::to_string(&body)
        .map(String::into_bytes)
        .map_err(|error| Error::state_failed(format!("generate Terraform backend: {error}")))
}

pub fn local_backend() -> Result<Vec<u8>> {
    let body = Body::builder()
        .add_block(
            Block::builder("terraform")
                .add_block(Block::builder("backend").add_label("local").build())
                .build(),
        )
        .build();
    hcl::to_string(&body)
        .map(String::into_bytes)
        .map_err(|error| Error::state_failed(format!("generate local Terraform backend: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn variable_values_remain_literals_when_terraform_expressions_are_evaluated() -> TestResult {
        let values = json!({
            "name": "literal ${file(\"/private/credential\")} and %{if true}directive%{endif}",
            "nested": {"quote": "\"\\\n", "enabled": true, "count": 3},
            "subnets": ["subnet-one", "subnet-two"],
            "optional": null,
        });
        let generated = String::from_utf8(variables(&values)?)?;
        let actual: Value = hcl::eval::from_str(&generated, &hcl::eval::Context::new())?;
        assert_eq!(actual, values);
        Ok(())
    }

    #[test]
    fn malformed_blueprints_are_rejected_before_they_can_be_written() {
        assert!(blueprint(Path::new("infra/main.tf"), b"resource { invalid =").is_err());
    }
}
