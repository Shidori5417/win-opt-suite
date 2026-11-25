//! Pure string helpers: registry key normalisation, task-path splitting and
//! UTF-16 conversion for Win32 wide-string parameters.

use shared_types::AppResult;

use crate::errors::internal;

/// Registry sub-key path without leading/trailing backslashes.
pub fn normalize_registry_key(key: &str) -> &str {
    key.trim_matches('\\')
}

/// Splits a task path such as `\Microsoft\Windows\Foo\Bar` into
/// `(folder, task_name)` = (`\Microsoft\Windows\Foo`, `Bar`). A task in the
/// root folder (`\Bar` or `Bar`) yields the folder `\`.
pub fn split_task_path(path: &str) -> AppResult<(String, String)> {
    let trimmed = path.trim();
    let (folder, name) = match trimmed.rfind('\\') {
        Some(idx) => (
            trimmed.get(..idx).unwrap_or(""),
            trimmed.get(idx + 1..).unwrap_or(""),
        ),
        None => ("", trimmed),
    };
    if name.is_empty() {
        return Err(internal(format!("invalid scheduled task path `{path}`")));
    }
    let folder = if folder.is_empty() {
        "\\".to_owned()
    } else if folder.starts_with('\\') {
        folder.to_owned()
    } else {
        format!("\\{folder}")
    };
    Ok((folder, name.to_owned()))
}

/// UTF-16 buffer with a trailing NUL, suitable for `PCWSTR`. Rejects interior
/// NULs, which would silently truncate the argument.
pub fn to_wide_nul(value: &str) -> AppResult<Vec<u16>> {
    if value.contains('\0') {
        return Err(internal("string argument contains an interior NUL"));
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_key_is_trimmed() {
        assert_eq!(normalize_registry_key(r"\A\B\"), r"A\B");
        assert_eq!(normalize_registry_key("A"), "A");
        assert_eq!(normalize_registry_key(""), "");
    }

    #[test]
    fn task_path_split() {
        assert_eq!(
            split_task_path(r"\Microsoft\Windows\Application Experience\ProgramDataUpdater")
                .unwrap(),
            (
                r"\Microsoft\Windows\Application Experience".to_owned(),
                "ProgramDataUpdater".to_owned()
            )
        );
        assert_eq!(
            split_task_path(r"\Root").unwrap(),
            ("\\".to_owned(), "Root".to_owned())
        );
        assert_eq!(
            split_task_path("Root").unwrap(),
            ("\\".to_owned(), "Root".to_owned())
        );
        assert_eq!(
            split_task_path(r"A\B").unwrap(),
            (r"\A".to_owned(), "B".to_owned())
        );
    }

    #[test]
    fn task_path_rejects_empty_name() {
        assert!(split_task_path("").is_err());
        assert!(split_task_path(r"\").is_err());
        assert!(split_task_path(r"\A\B\").is_err());
    }

    #[test]
    fn wide_string_is_nul_terminated() {
        assert_eq!(to_wide_nul("aé").unwrap(), vec![0x61, 0xE9, 0]);
        assert_eq!(to_wide_nul("").unwrap(), vec![0]);
        assert!(to_wide_nul("a\0b").is_err());
    }
}
