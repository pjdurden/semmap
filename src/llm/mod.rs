pub mod fake;
pub mod http;
pub mod schema;

use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribeRequest {
    pub custom_id: String,
    pub dir_path: String,
    pub symbol_names: Vec<String>,
    pub file_names: Vec<String>,
    /// (subdirectory name, that subdirectory's `What` line)
    pub child_summaries: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DescribeResponse {
    pub custom_id: String,
    pub what: String,
    pub entry_symbol: Option<String>,
    /// (symbol name, description) — names only, never file or line
    pub key_symbols: Vec<(String, String)>,
    pub look_here_for: Vec<String>,
    /// (subdirectory name, one-line note)
    pub subdir_notes: Vec<(String, String)>,
}

pub trait Llm {
    fn describe(&self, reqs: Vec<DescribeRequest>) -> Result<Vec<DescribeResponse>>;
}
