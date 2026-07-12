//! Username charset + discriminator rendering (T6, T7).
//!
//! Charset is deliberately ASCII-only: rejecting anything outside
//! alphanumeric + a small punctuation set also rejects confusable-Unicode
//! registration attempts (e.g. Cyrillic а vs Latin a) as a side effect of
//! the allowlist, not a separate confusables table to maintain.
//!
//! Discriminator width is a property of the *nickname*, not the individual
//! user: two accounts under the same nickname always render at the same
//! width, and that width only ever widens, never shrinks. That's what
//! keeps an existing "05" from colliding with a newly-widened "050" — it
//! becomes "005" instead, same integer, wider zero-padding.

pub const MIN_NICKNAME_LEN: usize = 2;
pub const MAX_NICKNAME_LEN: usize = 32;
pub const MIN_DISCRIMINATOR_WIDTH: u32 = 2;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UsernameError {
    #[error("nickname must be {MIN_NICKNAME_LEN}-{MAX_NICKNAME_LEN} chars")]
    BadLength,
    #[error("nickname contains a disallowed character: {0:?}")]
    BadChar(char),
    #[error("discriminator space exhausted at every width up to 9 digits")]
    DiscriminatorSpaceExhausted,
}

fn is_allowed_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

/// Validates charset and length. Does not check availability — that's
/// `DirectoryStore::claim_username`'s job, since it needs the DB.
pub fn validate_nickname(nickname: &str) -> Result<(), UsernameError> {
    if nickname.len() < MIN_NICKNAME_LEN || nickname.len() > MAX_NICKNAME_LEN {
        return Err(UsernameError::BadLength);
    }
    if let Some(bad) = nickname.chars().find(|c| !is_allowed_char(*c)) {
        return Err(UsernameError::BadChar(bad));
    }
    Ok(())
}

/// Zero-pads `value` to `width` digits — the *current* width for the
/// nickname, not whatever width existed when this particular discriminator
/// was first claimed.
pub fn render_discriminator(value: u32, width: u32) -> String {
    format!("{value:0width$}", width = width as usize)
}

pub fn format_handle(nickname: &str, discriminator: u32, width: u32) -> String {
    format!("{nickname}#{}", render_discriminator(discriminator, width))
}

/// Exclusive upper bound on discriminator values at a given width — e.g.
/// width 2 allows 1..=99 (0 reserved, matching the convention that "00" is
/// never a real handle).
pub fn slot_count(width: u32) -> u32 {
    10u32.saturating_pow(width) - 1
}

/// First free integer in `1..=slot_count(width)` not present in `taken`.
/// `taken` should already be scoped to one nickname by the caller.
pub fn first_free_slot(
    taken: &std::collections::HashSet<u32>,
    width: u32,
) -> Result<u32, UsernameError> {
    let max = slot_count(width);
    (1..=max)
        .find(|v| !taken.contains(v))
        .ok_or(UsernameError::DiscriminatorSpaceExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rejects_confusable_unicode() {
        // Cyrillic а (U+0430) looks identical to Latin a but isn't ASCII.
        assert_eq!(validate_nickname("аlice"), Err(UsernameError::BadChar('а')));
    }

    #[test]
    fn accepts_allowed_ascii() {
        assert!(validate_nickname("alice_99.b-c").is_ok());
    }

    #[test]
    fn rejects_bad_length() {
        assert_eq!(validate_nickname("a"), Err(UsernameError::BadLength));
        assert_eq!(
            validate_nickname(&"a".repeat(MAX_NICKNAME_LEN + 1)),
            Err(UsernameError::BadLength)
        );
    }

    #[test]
    fn existing_discriminator_renders_wider_after_widen_without_changing_value() {
        assert_eq!(render_discriminator(5, 2), "05");
        assert_eq!(render_discriminator(5, 3), "005");
        assert_ne!(render_discriminator(5, 3), render_discriminator(50, 3));
    }

    #[test]
    fn first_free_slot_fills_width_2_then_needs_width_3() {
        let taken: HashSet<u32> = (1..=99).collect();
        assert_eq!(
            first_free_slot(&taken, 2),
            Err(UsernameError::DiscriminatorSpaceExhausted)
        );
        assert_eq!(first_free_slot(&taken, 3), Ok(100));
    }

    #[test]
    fn first_free_slot_finds_gap() {
        let mut taken: HashSet<u32> = (1..=99).collect();
        taken.remove(&42);
        assert_eq!(first_free_slot(&taken, 2), Ok(42));
    }
}
