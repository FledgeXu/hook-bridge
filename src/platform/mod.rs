use clap::ValueEnum;

pub mod capability;
pub mod claude;
pub mod codex;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Platform {
    Claude,
    Codex,
}

impl Platform {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Platform;

    #[test]
    fn platform_as_str_returns_stable_values() {
        assert_eq!(Platform::Claude.as_str(), "claude");
        assert_eq!(Platform::Codex.as_str(), "codex");
    }
}
