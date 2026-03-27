use crate::error::{AppError, Result};

pub fn normalize_optional_repo(repo: Option<&str>) -> Result<Option<String>> {
    let Some(raw_repo) = repo.map(str::trim) else {
        return Ok(None);
    };
    if raw_repo.is_empty() {
        return Ok(None);
    }

    Ok(Some(normalize_repo(raw_repo)?))
}

pub fn normalize_repo(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidGitHubRepo(input.to_string()));
    }

    let candidate = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("github.com/"))
        .unwrap_or(trimmed);
    let candidate = candidate.strip_prefix('@').unwrap_or(candidate);
    let candidate = candidate.trim_end_matches('/');
    let candidate = candidate.strip_suffix(".git").unwrap_or(candidate);

    let mut segments = candidate.split('/');
    let Some(owner) = segments.next() else {
        return Err(AppError::InvalidGitHubRepo(input.to_string()));
    };
    let Some(repo) = segments.next() else {
        return Err(AppError::InvalidGitHubRepo(input.to_string()));
    };
    if segments.next().is_some() || !is_valid_repo_part(owner) || !is_valid_repo_part(repo) {
        return Err(AppError::InvalidGitHubRepo(input.to_string()));
    }

    Ok(format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    ))
}

fn is_valid_repo_part(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || character == '-'
                || character == '_'
                || character == '.'
        })
}

#[cfg(test)]
mod tests {
    use super::{normalize_optional_repo, normalize_repo};

    #[test]
    fn normalizes_url_at_prefix_and_plain_repo_forms() {
        assert_eq!(
            normalize_repo("https://github.com/ExampleOrg/todui-keymove").unwrap(),
            "exampleorg/todui-keymove"
        );
        assert_eq!(
            normalize_repo("@ExampleOrg/todui-keymove").unwrap(),
            "exampleorg/todui-keymove"
        );
        assert_eq!(
            normalize_repo("ExampleOrg/todui-keymove").unwrap(),
            "exampleorg/todui-keymove"
        );
        assert_eq!(
            normalize_repo("https://github.com/ExampleOrg/todui-keymove.git").unwrap(),
            "exampleorg/todui-keymove"
        );
    }

    #[test]
    fn optional_repo_treats_blank_as_none() {
        assert_eq!(normalize_optional_repo(None).unwrap(), None);
        assert_eq!(normalize_optional_repo(Some("   ")).unwrap(), None);
    }

    #[test]
    fn rejects_non_root_or_non_github_inputs() {
        assert!(normalize_repo("https://gitlab.com/org/repo").is_err());
        assert!(normalize_repo("https://github.com/org/repo/issues/1").is_err());
        assert!(normalize_repo("@org").is_err());
        assert!(normalize_repo("not a repo").is_err());
    }
}
