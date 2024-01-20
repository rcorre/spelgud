use std::{
    io::{BufRead, Write},
    process::{Command, Stdio},
};

use lsp_types::{Diagnostic, Url};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn diags(uri: &Url, text: &str) -> Result<Vec<Diagnostic>> {
    let mut proc = Command::new("aspell")
        .arg("--pipe")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let stdin = proc.stdin.as_mut().unwrap();
    let mut stdout = std::io::BufReader::new(proc.stdout.as_mut().unwrap());

    // Read the initial version line.
    let mut output = String::new();
    stdout.read_line(&mut output)?;
    eprintln!("Read line {output}");

    // Enable terse mode, so we don't need to read "*" for every ok word.
    stdin.write_all("!\n".as_bytes())?;

    let mut diags = vec![];
    for (line, input) in text.lines().enumerate() {
        let line = line.try_into()?;

        eprintln!("Writing {input}");
        stdin.write_all(input.as_bytes())?;
        stdin.write_all("\n".as_bytes())?;
        stdin.flush()?;

        loop {
            let mut output = String::new();
            stdout.read_line(&mut output)?;
            eprintln!("Read line {output}");

            // http://aspell.net/man-html/Through-A-Pipe.html#Through-A-Pipe
            // OK: *
            // Suggestions: & original count offset: miss, miss, …
            // None: # original offset
            // Offset is a character offset.
            let parts: Vec<&str> = output.split(&[' ', ':']).collect();
            let diag = match parts.as_slice() {
                ["&", original, _count, offset, _misses @ ..] => lsp_types::Diagnostic {
                    range: lsp_types::Range {
                        start: lsp_types::Position {
                            line,
                            character: offset.parse::<u32>()?,
                        },
                        end: lsp_types::Position {
                            line,
                            character: offset.parse::<u32>()?
                                + u32::try_from(original.chars().count())?,
                        },
                    },
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: original.to_string(),
                    ..Default::default()
                },
                ["#", original, offset] => lsp_types::Diagnostic {
                    range: lsp_types::Range {
                        start: lsp_types::Position {
                            line,
                            character: offset.parse::<u32>()?,
                        },
                        end: lsp_types::Position {
                            line,
                            character: offset.parse::<u32>()?
                                + u32::try_from(original.chars().count())?,
                        },
                    },
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: original.to_string(),
                    ..Default::default()
                },
                ["\n"] => {
                    eprintln!("done");
                    break; // done with results for this line
                }
                _ => Err(format!("Unexpected line: {output}: {parts:?}"))?,
            };
            diags.push(diag);
        }
    }
    eprintln!("Closing process");
    proc.wait_with_output()?;
    Ok(diags)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_diags() {
        let uri = Url::from_file_path("/tmp/foo.txt").unwrap();
        let actual = diags(
            &uri,
            [
                "The quick brown fox jumped over the lazy dog",
                "The quick brown fox jumped over the lazy doge",
                "The kwick brown fox jumped over the lazzy dog",
            ]
            .join("\n")
            .as_str(),
        )
        .unwrap();

        assert_eq!(
            actual,
            vec![
                Diagnostic {
                    range: lsp_types::Range {
                        start: lsp_types::Position {
                            line: 2,
                            character: 4
                        },
                        end: lsp_types::Position {
                            line: 2,
                            character: 9
                        }
                    },
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: "kwick".into(),
                    ..Default::default()
                },
                Diagnostic {
                    range: lsp_types::Range {
                        start: lsp_types::Position {
                            line: 2,
                            character: 36
                        },
                        end: lsp_types::Position {
                            line: 2,
                            character: 41
                        }
                    },
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: "lazzy".into(),
                    ..Default::default()
                }
            ]
        );
    }
}
