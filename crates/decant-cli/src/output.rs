use std::io::IsTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Table,
    Json,
    Md,
}

#[derive(Debug, Clone, Copy)]
pub struct OutputCtx {
    pub format: Format,
    pub color: bool,
    pub quiet: bool,
}

impl OutputCtx {
    pub fn new(json: bool, format: Option<&str>, no_color: bool, quiet: bool) -> Self {
        let format = if json {
            Format::Json
        } else {
            match format {
                Some("json") => Format::Json,
                Some("md") => Format::Md,
                _ => Format::Table,
            }
        };
        let color = should_color(no_color);
        OutputCtx { format, color, quiet }
    }
}

/// Color only when: not disabled by flag, NO_COLOR unset, and stdout is a TTY.
pub fn should_color(no_color_flag: bool) -> bool {
    if no_color_flag || std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
