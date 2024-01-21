use std::collections::hash_map;

use crate::file;

use super::spell;
use lsp_types::Url;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Workspace {
    files: std::collections::HashMap<Url, file::File>,
}

impl Workspace {
    pub fn new() -> Workspace {
        Workspace {
            files: hash_map::HashMap::new(),
        }
    }

    fn get(self: &Self, uri: &Url) -> Result<&file::File> {
        Ok(self
            .files
            .get(uri)
            .ok_or(format!("File not loaded: {uri}"))?)
    }

    pub fn open(&mut self, uri: Url, text: String) -> Result<Vec<lsp_types::Diagnostic>> {
        let diags = spell::diags(&uri, &text);
        self.files.insert(uri, file::File::new(text)?);
        diags
    }

    pub fn save(&mut self, uri: Url) -> Result<Vec<lsp_types::Diagnostic>> {
        let file = self.get(&uri)?;
        spell::diags(&uri, &file.text())
    }

    pub fn edit(
        &mut self,
        uri: &Url,
        changes: Vec<lsp_types::TextDocumentContentChangeEvent>,
    ) -> Result<()> {
        log::trace!("edit");
        self.files
            .get_mut(uri)
            .ok_or(format!("File not loaded: {uri}"))?
            .edit(changes)
            .into()
    }

    pub fn complete(
        &self,
        uri: &Url,
        line: usize,
        character: usize,
    ) -> Result<Option<lsp_types::CompletionResponse>> {
        Ok(None)
    }

    pub fn symbols(&self, uri: &Url) -> Result<Vec<lsp_types::SymbolInformation>> {
        self.get(uri)?.symbols(uri)
    }
}
