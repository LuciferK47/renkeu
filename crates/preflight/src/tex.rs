//! Reading just enough LaTeX to check a project: commands with their
//! arguments, comments, and where each thing is. It is a scanner, not a TeX
//! engine; macros that build a path or a command at run time are invisible
//! to it, which every check built on it states.

/// A command with the braced arguments that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub name: String,
    /// The `[...]` after the name, if any.
    pub options: Option<String>,
    /// Up to two `{...}` groups after it, as written inside the braces.
    pub args: Vec<String>,
    pub line: u32,
}

/// `line` without a trailing `% comment`; a `\%` is a literal percent.
pub(crate) fn live(line: &str) -> &str {
    let mut backslashes = 0;
    for (i, b) in line.bytes().enumerate() {
        match b {
            b'\\' => backslashes += 1,
            b'%' if backslashes % 2 == 0 => return &line[..i],
            _ => backslashes = 0,
        }
    }
    line
}

/// Index just past the group that opens at `start` (a `{` or `[`), or `None`
/// if it never closes. Braces nest; an escaped brace does not count.
fn group_end(b: &[u8], start: usize) -> Option<usize> {
    let (open, close) = if b[start] == b'{' {
        (b'{', b'}')
    } else {
        (b'[', b']')
    };
    let mut depth = 0;
    let mut i = start;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b'%' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Every `\name[opts]{arg}{arg}` in `text`, comments skipped.
pub(crate) fn commands(text: &str) -> Vec<Cmd> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut line = 1u32;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => line += 1,
            b'%' => {
                while i + 1 < b.len() && b[i + 1] != b'\n' {
                    i += 1;
                }
            }
            b'\\' => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && b[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j == start {
                    i += 1; // `\\`, `\%`, `\{`: skip the escaped character
                    if b.get(i) == Some(&b'\n') {
                        line += 1;
                    }
                } else {
                    let name = &text[start..j];
                    if b.get(j) == Some(&b'*') {
                        j += 1;
                    }
                    let skip_space = |mut k: usize| {
                        while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
                            k += 1;
                        }
                        k
                    };
                    let mut options = None;
                    let k = skip_space(j);
                    if b.get(k) == Some(&b'[')
                        && let Some(end) = group_end(b, k)
                    {
                        options = Some(text[k + 1..end - 1].to_owned());
                        j = end;
                    }
                    let mut args = Vec::new();
                    let mut k = skip_space(j);
                    while args.len() < 2 && b.get(k) == Some(&b'{') {
                        let Some(end) = group_end(b, k) else { break };
                        args.push(text[k + 1..end - 1].to_owned());
                        j = end;
                        k = skip_space(j);
                    }
                    out.push(Cmd {
                        name: name.to_owned(),
                        options,
                        args,
                        line,
                    });
                    // Arguments can hold more commands (`\author{\thanks{..}}`),
                    // and newlines inside them must still be counted: resume
                    // right after the name, not after the arguments.
                    i = start + name.len() - 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Prose of a command argument: commands and braces removed.
pub(crate) fn plain(arg: &str) -> String {
    let mut out = String::new();
    let mut chars = arg.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                    chars.next();
                }
                out.push(' ');
            }
            '{' | '}' | '~' => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_come_with_their_options_arguments_and_lines() {
        let text = "\\input{a}\n% \\input{commented}\n\\includegraphics[width=0.5\\linewidth]{figs/x}\n\\import{dir/}{file}\n";
        // Commands nested in an argument or option are listed too (`\\linewidth`).
        let cmds = commands(text);
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["input", "includegraphics", "linewidth", "import"]);
        assert_eq!(
            cmds[0],
            Cmd {
                name: "input".into(),
                options: None,
                args: vec!["a".into()],
                line: 1
            }
        );
        assert_eq!(cmds[1].options.as_deref(), Some("width=0.5\\linewidth"));
        assert_eq!(cmds[1].args, ["figs/x"]);
        assert_eq!(cmds[1].line, 3);
        assert_eq!(cmds[3].args, ["dir/", "file"]);
    }

    #[test]
    fn nested_braces_and_commands_inside_arguments_are_kept() {
        let cmds = commands("\\author{A\\thanks{B}\n and {C}}\n\\label{x}");
        let author = &cmds[0];
        assert_eq!(author.args[0], "A\\thanks{B}\n and {C}");
        let names: Vec<_> = cmds.iter().map(|c| (c.name.as_str(), c.line)).collect();
        assert_eq!(names, [("author", 1), ("thanks", 1), ("label", 3)]);
    }

    #[test]
    fn an_escaped_percent_does_not_start_a_comment() {
        assert_eq!(live("50\\% off \\input{x} % note"), "50\\% off \\input{x} ");
        assert_eq!(commands("50\\% \\input{x}").len(), 1);
    }

    #[test]
    fn plain_text_drops_commands() {
        assert_eq!(
            plain("Ada \\textsc{Lovelace}\\thanks{x}~~Inc."),
            "Ada Lovelace x Inc."
        );
    }

    #[test]
    fn an_unclosed_group_does_not_hang_or_panic() {
        let cmds = commands("\\author{never closed\n\\input{a}");
        assert!(cmds.iter().any(|c| c.name == "input"));
    }
}
