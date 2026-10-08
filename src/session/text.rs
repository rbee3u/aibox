//! Pure control-character escaping for Session previews and diagnostics.

use std::path::Path;

pub(super) fn terminal_safe(value: &str) -> String {
    terminal_safe_with(value, |_| false)
}

pub(super) fn terminal_safe_with(value: &str, keep_control: impl Fn(char) -> bool) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() && !keep_control(character) {
            output.extend(character.escape_default());
        } else {
            output.push(character);
        }
    }
    output
}

pub(super) fn safe_path(path: &Path) -> String {
    terminal_safe(&path.to_string_lossy())
}
