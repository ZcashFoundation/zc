//! ANSI styling. Disabled when stdout is not a terminal, when `NO_COLOR` is set to a
//! non-empty value (<https://no-color.org>), or in `--json`/`--changelog` mode.

#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    pub red: &'static str,
    pub green: &'static str,
    pub yellow: &'static str,
    pub bold: &'static str,
    pub dim: &'static str,
    pub reset: &'static str,
}

impl Style {
    /// Colors on only for an interactive stdout in a document-free mode.
    pub fn detect(document_mode: bool) -> Style {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        if document_mode || no_color || !stdout_is_tty() {
            Style::default()
        } else {
            Style {
                red: "\x1b[31m",
                green: "\x1b[32m",
                yellow: "\x1b[33m",
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                reset: "\x1b[0m",
            }
        }
    }
}

pub fn stdout_is_tty() -> bool {
    is_tty(1)
}

pub fn stderr_is_tty() -> bool {
    is_tty(2)
}

fn is_tty(fd: i32) -> bool {
    // SAFETY: isatty is a pure query on a file descriptor.
    unsafe { libc_isatty(fd) == 1 }
}

extern "C" {
    #[link_name = "isatty"]
    fn libc_isatty(fd: i32) -> i32;
}
