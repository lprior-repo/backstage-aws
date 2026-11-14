use std::{
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

use backstage_aws::{Error, generate, read_config};

fn run() -> Result<(), Error> {
    let arguments = std::env::args_os().take(4).collect::<Vec<_>>();
    match arguments.as_slice() {
        [_, help] if help == "--help" => {
            writeln!(io::stdout().lock(), "backstage-aws <config.toml> <new-output-directory>\n\nGenerate Backstage, Terraform, workflows and dependency locks offline.")
                .map_err(|source| Error::Io { operation: "write help", path: "stdout".into(), source })
        }
        [_, input, output] => {
            let config = read_config(Path::new(input))?;
            let count = generate(&config, Path::new(output))?;
            writeln!(io::stdout().lock(), "Generated {count} files in {}", Path::new(output).display())
                .map_err(|source| Error::Io { operation: "write result", path: "stdout".into(), source })
        }
        _ => Err(Error::Usage),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if writeln!(io::stderr().lock(), "{error}").is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}
