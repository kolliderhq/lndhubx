const MAX_USERNAME_LEN: usize = 128;

trait AllowedUsernameCharacters {
    fn is_username_alphanumeric(&self) -> bool;
    fn is_username_punctuation(&self) -> bool;
    fn is_username_char(&self) -> bool {
        self.is_username_alphanumeric() || self.is_username_punctuation()
    }
}

impl AllowedUsernameCharacters for char {
    fn is_username_alphanumeric(&self) -> bool {
        let c = *self;
        c.is_ascii_lowercase() || c.is_ascii_digit()
    }

    fn is_username_punctuation(&self) -> bool {
        let c = *self;
        c == '-' || c == '.' || c == '_'
    }
}

pub fn check_username_valid(username: &str) -> bool {
    if username.len() > MAX_USERNAME_LEN {
        return false;
    }

    let contains_only_allowed_characters = username.chars().all(|c| c.is_username_char());
    if !contains_only_allowed_characters {
        return false;
    }

    // initialising the flag with false
    // prevents usernames from starting
    // with non-alphanumeric character
    let mut alphanumeric = false;
    for c in username.chars() {
        let prev_alphanumeric = alphanumeric;
        alphanumeric = c.is_username_alphanumeric();
        if !prev_alphanumeric && !alphanumeric {
            return false;
        }
    }
    // check whether the last character
    // was alphanumeric
    if !alphanumeric {
        return false;
    }
    true
}
