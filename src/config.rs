use std::path::PathBuf;

use clap::Parser;

pub const DEFAULT_PREVIEW_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Config {
    /// Swamp repository to browse
    #[arg(long, default_value = ".", value_name = "PATH")]
    pub repo_dir: PathBuf,

    /// Swamp executable to invoke
    #[arg(long, default_value = "swamp", value_name = "PATH")]
    pub swamp_bin: PathBuf,

    #[arg(skip = DEFAULT_PREVIEW_LIMIT)]
    pub preview_limit: u64,
}
