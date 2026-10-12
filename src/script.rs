//! Bounded parser and executor for AutoCAD-style `.scr` command files.

const DEFAULT_MAX_COMMANDS: usize = 100_000;
const DEFAULT_MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptCommand {
    pub line: usize,
    pub tokens: Vec<String>,
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptErrorKind {
    UnterminatedQuote,
    InvalidEscape,
    TooManyCommands,
    IncludeDepthExceeded,
    MissingScriptPath,
    Loader(String),
    Callback(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub path: String,
    pub line: usize,
    pub kind: ScriptErrorKind,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {:?}", self.path, self.line, self.kind)
    }
}
impl std::error::Error for ScriptError {}

pub fn parse_script(input: &str) -> Result<Vec<ScriptCommand>, ScriptError> {
    parse_script_named(input, "<script>")
}

pub fn parse_script_named(
    input: &str,
    path: impl Into<String>,
) -> Result<Vec<ScriptCommand>, ScriptError> {
    let path = path.into();
    let mut commands = Vec::new();
    for (index, source) in input.lines().enumerate() {
        let line = index + 1;
        if source.trim_start().starts_with(';') {
            continue;
        }
        let tokens = tokenize_line(source).map_err(|kind| ScriptError {
            path: path.clone(),
            line,
            kind,
        })?;
        if commands.len() == DEFAULT_MAX_COMMANDS {
            return Err(ScriptError {
                path,
                line,
                kind: ScriptErrorKind::TooManyCommands,
            });
        }
        // A blank line is an Enter key in a script, so retain it as an empty command.
        commands.push(ScriptCommand {
            line,
            tokens,
            raw: source.trim().to_owned(),
        });
    }
    Ok(commands)
}

pub struct ScriptRunner {
    pub max_commands: usize,
    pub max_depth: usize,
}

impl Default for ScriptRunner {
    fn default() -> Self {
        Self {
            max_commands: DEFAULT_MAX_COMMANDS,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

impl ScriptRunner {
    pub fn run<F, L>(
        &self,
        input: &str,
        path: impl Into<String>,
        mut load: L,
        mut callback: F,
    ) -> Result<usize, ScriptError>
    where
        F: FnMut(&ScriptCommand) -> Result<(), String>,
        L: FnMut(&str, &str) -> Result<String, String>,
    {
        let mut count = 0;
        self.run_inner(input, path.into(), 0, &mut load, &mut callback, &mut count)?;
        Ok(count)
    }

    fn run_inner<F, L>(
        &self,
        input: &str,
        path: String,
        depth: usize,
        load: &mut L,
        callback: &mut F,
        count: &mut usize,
    ) -> Result<(), ScriptError>
    where
        F: FnMut(&ScriptCommand) -> Result<(), String>,
        L: FnMut(&str, &str) -> Result<String, String>,
    {
        if depth > self.max_depth {
            return Err(ScriptError {
                path,
                line: 1,
                kind: ScriptErrorKind::IncludeDepthExceeded,
            });
        }
        for command in parse_script_named(input, path.clone())? {
            if *count >= self.max_commands {
                return Err(ScriptError {
                    path: path.clone(),
                    line: command.line,
                    kind: ScriptErrorKind::TooManyCommands,
                });
            }
            *count += 1;
            let include = command
                .tokens
                .first()
                .map(|token| token.trim_start_matches('_').to_ascii_uppercase());
            if matches!(include.as_deref(), Some("SCRIPT" | "SCRIPTCALL")) {
                let include_path = command.tokens.get(1).ok_or_else(|| ScriptError {
                    path: path.clone(),
                    line: command.line,
                    kind: ScriptErrorKind::MissingScriptPath,
                })?;
                let included = load(&path, include_path).map_err(|message| ScriptError {
                    path: path.clone(),
                    line: command.line,
                    kind: ScriptErrorKind::Loader(message),
                })?;
                self.run_inner(
                    &included,
                    include_path.clone(),
                    depth + 1,
                    load,
                    callback,
                    count,
                )?;
                if include.as_deref() == Some("SCRIPT") {
                    return Ok(());
                }
            } else {
                callback(&command).map_err(|message| ScriptError {
                    path: path.clone(),
                    line: command.line,
                    kind: ScriptErrorKind::Callback(message),
                })?;
            }
        }
        Ok(())
    }
}

fn tokenize_line(source: &str) -> Result<Vec<String>, ScriptErrorKind> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if !quoted && ch == ';' {
            break;
        }
        match ch {
            '"' => {
                if quoted && chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    quoted = !quoted;
                }
            }
            '\\' if quoted => {
                let Some(next) = chars.next() else {
                    return Err(ScriptErrorKind::InvalidEscape);
                };
                current.push(next);
            }
            c if !quoted && c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if quoted {
        return Err(ScriptErrorKind::UnterminatedQuote);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}
