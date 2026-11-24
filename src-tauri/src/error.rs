use serde::Serialize;

/// Every error that can cross the IPC boundary. Serialised as
/// `{ code, message, help? }` so the UI can show a friendly explanation.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("No CurseForge API key is saved. Add one in Settings, or keep using Modrinth only.")]
    NoCurseForgeKey,

    #[error("{message}")]
    Keychain { message: String, help: String },

    #[error("Network error: {0}")]
    Network(String),

    #[error("{source_name} returned HTTP {status}")]
    Api { source_name: &'static str, status: u16, help: Option<String> },

    #[error("Couldn't read {source_name}'s response: {detail}")]
    Decode { source_name: &'static str, detail: String },

    #[error("{0}")]
    Invalid(String),

    #[error("Storage error: {0}")]
    Storage(String),
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::NoCurseForgeKey => "no_curseforge_key",
            AppError::Keychain { .. } => "keychain",
            AppError::Network(_) => "network",
            AppError::Api { .. } => "api",
            AppError::Decode { .. } => "decode",
            AppError::Invalid(_) => "invalid",
            AppError::Storage(_) => "storage",
        }
    }
    fn help(&self) -> Option<String> {
        match self {
            AppError::Keychain { help, .. } => Some(help.clone()),
            AppError::Api { help, .. } => help.clone(),
            _ => None,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 3)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.serialize_field("help", &self.help())?;
        st.end()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        // Never include the URL: it can carry query text, and keeps logs tidy.
        AppError::Network(e.without_url().to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
