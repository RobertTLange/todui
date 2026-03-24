use crate::error::{AppError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_opened_at: i64,
    pub current_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub slug: String,
    pub name: String,
    pub last_opened_at: i64,
    pub current_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    pub session: Session,
    pub todos: Vec<crate::domain::todo::Todo>,
}

pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }

    slug.trim_matches('-').to_string()
}

pub fn validate_slug(slug: &str) -> Result<()> {
    let is_valid = !slug.is_empty()
        && slug.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !slug.starts_with('-')
        && !slug.ends_with('-');

    if is_valid {
        Ok(())
    } else {
        Err(AppError::InvalidSlug(slug.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{slugify, validate_slug};

    #[test]
    fn slugifies_names() {
        assert_eq!(slugify("Writing Sprint"), "writing-sprint");
        assert_eq!(slugify("  Ghostty + Mouse "), "ghostty-mouse");
    }

    #[test]
    fn validates_slug_rules() {
        assert!(validate_slug("writing-sprint").is_ok());
        assert!(validate_slug("Writing-Sprint").is_err());
        assert!(validate_slug("writing_sprint").is_err());
    }
}
