#[derive(Debug, thiserror::Error)]
pub enum LabelError {
    #[error("label pack {pack}: invalid TOML: {detail}")]
    Toml { pack: String, detail: String },
    #[error("label pack {pack}: {detail}")]
    Invalid { pack: String, detail: String },
    #[error("OFAC list: {0}")]
    Ofac(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
