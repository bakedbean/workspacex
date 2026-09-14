//! Style-string grammar: `fg:<c> bg:<c> <c> bold dimmed italic underline none $var`.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleSpec {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleError(pub String);

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl StyleSpec {
    pub fn parse(src: &str) -> Result<Self, StyleError> {
        Ok(Self {
            raw: src.to_string(),
        })
    }
}
