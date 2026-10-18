//! The user name and password nearby devices sign in with. A new password is
//! drawn every time sharing starts, so one shared with a friend last week
//! does not work today.

use std::fmt;

use rand::{Rng, rngs::OsRng};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// The user name is fixed; the password carries the secret.
pub const USERNAME: &str = "ket";
/// 12 characters from a 31-letter alphabet: 59 bits, far beyond what the
/// sign-in limit lets anyone guess.
pub const PASSWORD_LENGTH: usize = 12;
/// Lowercase letters and digits without the ones that are easy to misread
/// when typed on a phone (0/o, 1/l/i).
const ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";

#[derive(Clone)]
pub struct Credentials {
    username: String,
    password: Zeroizing<String>,
}

impl Credentials {
    /// Fresh random credentials from the operating system's generator.
    pub fn generate() -> Self {
        let mut rng = OsRng;
        let password = Zeroizing::new(
            (0..PASSWORD_LENGTH)
                .map(|_| char::from(ALPHABET[rng.gen_range(0..ALPHABET.len())]))
                .collect::<String>(),
        );
        Self::new(USERNAME, &password)
    }

    pub fn new(username: &str, password: &str) -> Self {
        Self {
            username: username.to_owned(),
            password: Zeroizing::new(password.to_owned()),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    /// Compares in constant time, so timing says nothing about how much of a
    /// guess was right.
    pub fn matches(&self, username: &[u8], password: &[u8]) -> bool {
        let user = self.username.as_bytes().ct_eq(username);
        let pass = self.password.as_bytes().ct_eq(password);
        bool::from(user & pass)
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_are_long_unambiguous_and_differ() {
        let first = Credentials::generate();
        let second = Credentials::generate();
        assert_eq!(first.username(), USERNAME);
        assert_eq!(first.password().len(), PASSWORD_LENGTH);
        assert!(
            first
                .password()
                .bytes()
                .all(|byte| ALPHABET.contains(&byte))
        );
        assert!(!first.password().contains(['0', 'o', '1', 'l', 'i']));
        assert_ne!(first.password(), second.password());
    }

    #[test]
    fn only_the_exact_pair_matches() {
        let credentials = Credentials::new("ket", "abcdefghjkmn");
        assert!(credentials.matches(b"ket", b"abcdefghjkmn"));
        assert!(!credentials.matches(b"ket", b"abcdefghjkm"));
        assert!(!credentials.matches(b"ket", b"abcdefghjkmnp"));
        assert!(!credentials.matches(b"kat", b"abcdefghjkmn"));
        assert!(!credentials.matches(b"", b""));
    }

    #[test]
    fn debug_output_hides_the_password() {
        let credentials = Credentials::new("ket", "abcdefghjkmn");
        assert!(!format!("{credentials:?}").contains("abcdefghjkmn"));
    }
}
