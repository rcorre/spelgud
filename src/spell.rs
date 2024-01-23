use std::{
    io::{BufRead, Write},
    process::{Command, Stdio},
};

use lsp_types::Diagnostic;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// The context placed into Diagnostic.Data
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct DiagnosticData {
    pub original: String,
    pub fixes: Vec<String>,
    pub range: lsp_types::Range,
}

pub struct Process(std::process::Child);

pub enum Program {
    Aspell,
    Ispell,
    Hunspell,
}

impl Program {
    fn command(&self) -> &str {
        match self {
            Program::Aspell => "aspell",
            Program::Ispell => "ispell",
            Program::Hunspell => "hunspell",
        }
    }
}

impl Process {
    pub fn new(prog: Program) -> Result<Process> {
        let cmd = prog.command();
        let mut proc = Command::new(cmd)
            .arg("-a")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        log::info!("Started {cmd} with pid {}", proc.id());

        let stdin = proc.stdin.as_mut().unwrap();
        let mut stdout = std::io::BufReader::new(proc.stdout.as_mut().unwrap());

        // Read the initial version line.
        let mut output = String::new();
        stdout.read_line(&mut output)?;
        log::trace!("Read line '{output}'");

        // Enable terse mode, so we don't need to read "*" for every ok word.
        log::trace!("Read line '{output}'");
        stdin.write_all("!\n".as_bytes())?;
        Ok(Process(proc))
    }

    pub fn diags(&mut self, text: &str) -> Result<Vec<Diagnostic>> {
        let stdin = self.0.stdin.as_mut().unwrap();
        let mut stdout = std::io::BufReader::new(self.0.stdout.as_mut().unwrap());
        let mut diags = vec![];
        for (line, input) in text.lines().enumerate() {
            let line = line.try_into()?;
            if input.is_empty() {
                continue;
            }

            log::trace!("Writing '{input}'");
            stdin.write_all(input.as_bytes())?;
            stdin.write_all("\n".as_bytes())?;
            stdin.flush()?;

            loop {
                let mut output = String::new();
                stdout.read_line(&mut output)?;
                log::trace!("Read line {line}: '{output}'");

                // http://aspell.net/man-html/Through-A-Pipe.html#Through-A-Pipe
                // OK: *
                // Suggestions: & original count offset: miss, miss, …
                // None: # original offset
                // Offset is a character offset.
                let parts: Vec<&str> = output.split(&[' ', ':', ',']).collect();
                let diag = match parts.as_slice() {
                    ["&", original, _count, offset, misses @ ..] => {
                        let range = lsp_types::Range {
                            start: lsp_types::Position {
                                line,
                                character: offset.parse::<u32>()?,
                            },
                            end: lsp_types::Position {
                                line,
                                character: offset.parse::<u32>()?
                                    + u32::try_from(original.chars().count())?,
                            },
                        };
                        lsp_types::Diagnostic {
                            range,
                            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                            message: original.to_string(),
                            data: Some(serde_json::to_value(DiagnosticData {
                                range,
                                original: original.to_string(),
                                fixes: misses
                                    .iter()
                                    .filter(|s| !s.is_empty())
                                    .map(|s| s.trim().to_string())
                                    .collect(),
                            })?),
                            ..Default::default()
                        }
                    }
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
                        log::trace!("Done parsing diagnostics for line {line}");
                        break; // done with results for this line
                    }
                    _ => Err(format!("Unexpected line: {output}: {parts:?}"))?,
                };
                diags.push(diag);
            }
        }
        Ok(diags)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        log::info!("Closing process {}", self.0.id());
        if let Err(err) = self.0.wait() {
            log::error!("Failed to close process {}: {err}", self.0.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_diags() {
        let mut proc = Process::new(Program::Aspell).unwrap();
        let actual = proc
            .diags(
                [
                    "The quick brown fox jumped over the lazy dog",
                    "The quick brown fox jumped over the lazy dog",
                    "The kwick brown fox jumped over the lazzy dog",
                    "",
                    "The quick brown fox jumped over the lazy dog",
                ]
                .join("\n")
                .as_str(),
            )
            .unwrap();

        eprintln!("{:?}", actual);
        assert_eq!(actual.len(), 2);
        assert_eq!(
            actual[0].range,
            lsp_types::Range {
                start: lsp_types::Position {
                    line: 2,
                    character: 4,
                },
                end: lsp_types::Position {
                    line: 2,
                    character: 9,
                },
            },
        );
        assert_eq!(
            actual[0].severity,
            Some(lsp_types::DiagnosticSeverity::ERROR)
        );
        assert_eq!(actual[0].message, "kwick");
        assert_eq!(
            actual[1].range,
            lsp_types::Range {
                start: lsp_types::Position {
                    line: 2,
                    character: 36,
                },
                end: lsp_types::Position {
                    line: 2,
                    character: 41,
                },
            }
        );
        assert_eq!(
            actual[1].severity,
            Some(lsp_types::DiagnosticSeverity::ERROR)
        );
        assert_eq!(actual[1].message, "lazzy");
    }
}
