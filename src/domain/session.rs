use crate::error::{AppError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub name: String,
    pub tag: Option<String>,
    pub repo: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_opened_at: i64,
    pub current_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub name: String,
    pub tag: Option<String>,
    pub repo: Option<String>,
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
    pub id: i64,
    pub name: String,
    pub tag: Option<String>,
    pub repo: Option<String>,
    pub updated_at: i64,
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

pub fn normalize_session_name(input: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_dash = false;

    for character in input.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            normalized.push(character);
            last_was_dash = false;
        } else if !last_was_dash && !normalized.is_empty() {
            normalized.push('-');
            last_was_dash = true;
        }
    }

    normalized.trim_matches('-').to_string()
}

pub fn validate_session_name(name: &str) -> Result<()> {
    validate_slug_like(name, true)
}

pub fn normalize_tag(tag: Option<&str>) -> Result<Option<String>> {
    let Some(raw_tag) = tag.map(str::trim) else {
        return Ok(None);
    };
    if raw_tag.is_empty() {
        return Ok(None);
    }

    let normalized = normalize_session_name(raw_tag);
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
            Err(AppError::InvalidSessionName(value.to_string()))
        } else {
            Err(AppError::InvalidTag(value.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_session_name, normalize_tag, validate_session_name};

    #[test]
    fn normalizes_session_names() {
        assert_eq!(normalize_session_name("Writing Sprint"), "writing-sprint");
        assert_eq!(
            normalize_session_name("  Ghostty + Mouse "),
            "ghostty-mouse"
        );
    }

    #[test]
    fn validates_session_name_rules() {
        assert!(validate_session_name("writing-sprint").is_ok());
        assert!(validate_session_name("Writing-Sprint").is_err());
        assert!(validate_session_name("writing_sprint").is_err());
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
