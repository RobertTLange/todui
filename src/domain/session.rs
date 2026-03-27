use crate::error::{AppError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub tag: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_opened_at: i64,
    pub current_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub slug: String,
    pub name: String,
    pub tag: Option<String>,
    pub last_opened_at: i64,
    pub current_revision: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionHeadToken {
    pub current_revision: u32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOverview {
    pub slug: String,
    pub name: String,
    pub tag: Option<String>,
    pub last_opened_at: i64,
    pub current_revision: u32,
    pub todo_count: i64,
    pub done_count: i64,
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
    validate_slug_like(slug, true)
}

pub fn normalize_tag(tag: Option<&str>) -> Result<Option<String>> {
    let Some(raw_tag) = tag.map(str::trim) else {
        return Ok(None);
    };
    if raw_tag.is_empty() {
        return Ok(None);
    }

    let normalized = slugify(raw_tag);
    if validate_slug_like(&normalized, false).is_ok() {
        Ok(Some(normalized))
    } else {
        Err(AppError::InvalidTag(raw_tag.to_string()))
    }
}

fn validate_slug_like(value: &str, is_slug: bool) -> Result<()> {
    let is_valid = !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && !value.starts_with('-')
        && !value.ends_with('-');

    if is_valid {
        Ok(())
    } else {
        if is_slug {
            Err(AppError::InvalidSlug(value.to_string()))
        } else {
            Err(AppError::InvalidTag(value.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_tag, slugify, validate_slug};

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

    #[test]
    fn normalizes_optional_tags() {
        assert_eq!(
            normalize_tag(Some("Private Projects")).unwrap(),
            Some(String::from("private-projects"))
        );
        assert_eq!(normalize_tag(Some("   ")).unwrap(), None);
        assert!(normalize_tag(Some("!!!")).is_err());
    }
}
