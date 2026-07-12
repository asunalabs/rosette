//! Feature flags (T5): search and the account/verification layer flip
//! independently, so search can be pulled without taking signup down —
//! e.g. the T3 anti-enumeration gate stayed closed while T1/T2 shipped.

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"),
        Err(_) => default,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DirectoryConfig {
    pub accounts_enabled: bool,
    pub search_enabled: bool,
}

impl DirectoryConfig {
    pub fn from_env() -> Self {
        Self {
            accounts_enabled: env_flag("DIRECTORY_ACCOUNTS_ENABLED", true),
            search_enabled: env_flag("DIRECTORY_SEARCH_ENABLED", true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_both_on() {
        // Only meaningful if the env vars genuinely aren't set in the test
        // process; doesn't assert against a polluted environment.
        if std::env::var("DIRECTORY_ACCOUNTS_ENABLED").is_err()
            && std::env::var("DIRECTORY_SEARCH_ENABLED").is_err()
        {
            let cfg = DirectoryConfig::from_env();
            assert!(cfg.accounts_enabled);
            assert!(cfg.search_enabled);
        }
    }
}
