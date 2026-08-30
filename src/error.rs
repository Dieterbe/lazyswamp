use std::process::ExitStatus;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not start `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("swamp command failed ({status}{code_suffix}): {message}", code_suffix = code.as_deref().map(|value| format!(", {value}")).unwrap_or_default())]
    Command {
        status: ExitStatus,
        message: String,
        code: Option<String>,
    },

    #[error("swamp returned incompatible JSON for {context}: {source}")]
    Json {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("swamp returned an incomplete {0} response")]
    Incomplete(&'static str),

    #[error("invalid method input: {0}")]
    Validation(String),

    #[error("no matching historical data version was returned")]
    VersionNotFound,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
