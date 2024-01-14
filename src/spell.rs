use lsp_types::{Diagnostic, Url};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn diags(uri: &Url, text: &str) -> Result<Vec<Diagnostic>> {
    Ok(vec![])
}
