//! Where a workflow phase's agents are launched: a lexical scan of the run
//! record's inline script (spec §4.4), the "fix line points at the script"
//! half of the verdict.
//!
//! The scan tokenises the JavaScript just far enough to tell code from text:
//! the contents of `'…'`, `"…"` and `` `…` `` literals and of `//` and
//! `/* */` comments are skipped (their newlines still count), while the
//! expression inside a template's `${…}` is lexed as code, template literals
//! nested in it included. A prompt that says `phase: 'Verify'` or
//! `parallel(` therefore never matches. Regex literals are recognised by the
//! usual heuristic — a `/` where an operand is expected — and skipped to their
//! closing `/` (a `/` inside `[…]` does not close; a newline gives up).
//!
//! Nothing from the script leaves [`pointer`]: the few literals it keeps (a
//! `phase` value, a `label` head, `phase(`'s argument, each ≤ 64 chars) live in
//! a local token list dropped on return, and the result is a line number and
//! a call kind (spec §4.6).

/// The call a pointer lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Call {
    Parallel,
    Pipeline,
    /// No single enclosing call: the phase's `phase('<title>')` marker.
    PhaseMarker,
}

/// A 1-based line in the script and what sits there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Pointer {
    pub line: u32,
    pub call: Call,
}

/// The longest literal the scan keeps; anything longer is a prompt, not a
/// phase title or a label head.
const KEEP_MAX: usize = 64;

/// The `parallel(` or `pipeline(` call enclosing `phase`'s `agent(` calls —
/// those whose options carry `phase: '<phase>'` and a label starting with
/// `<prefix>:` for one of `label_prefixes`. Zero or several distinct calls
/// fall back to the line of `phase('<phase>')`; without one, `None`.
pub fn pointer(script: &str, phase: &str, label_prefixes: &[String]) -> Option<Pointer> {
    let toks = Lexer::new(script).run();
    let is_label = |t: &str| {
        label_prefixes.iter().any(|p| {
            t.len() > p.len() && t.starts_with(p.as_str()) && t.as_bytes()[p.len()] == b':'
        })
    };

    // Each token's enclosing parens: `Some((line, kind))` for a `parallel(`
    // or `pipeline(` frame, `None` for any other `(`.
    let mut frames: Vec<Option<(u32, Call)>> = Vec::new();
    let mut found: Vec<Option<(u32, Call)>> = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        match tok.kind {
            Kind::Open => {
                let frame = match i.checked_sub(1).map(|j| &toks[j]) {
                    Some(Tok {
                        kind: Kind::Name(Name::Parallel),
                        line,
                    }) => Some((*line, Call::Parallel)),
                    Some(Tok {
                        kind: Kind::Name(Name::Pipeline),
                        line,
                    }) => Some((*line, Call::Pipeline)),
                    _ => None,
                };
                frames.push(frame);
            }
            Kind::Close => {
                frames.pop();
            }
            Kind::Name(Name::Agent) if matches!(toks.get(i + 1), Some(t) if t.kind == Kind::Open) =>
            {
                let args = &toks[i + 1..matching_close(&toks, i + 1)];
                let has_phase = keyed(args, Name::Phase).any(|t| t == phase);
                let has_label = keyed(args, Name::Label).any(is_label);
                if has_phase && has_label {
                    found.push(frames.iter().rev().find_map(|f| *f));
                }
            }
            _ => {}
        }
    }

    // One distinct call means every entry is equal, so dropping consecutive
    // repeats leaves exactly one; any other mix leaves two or more.
    found.dedup();
    if let [Some((line, call))] = found[..] {
        return Some(Pointer { line, call });
    }
    toks.windows(3)
        .find_map(|w| match (&w[0].kind, &w[1].kind, &w[2].kind) {
            (Kind::Name(Name::Phase), Kind::Open, Kind::Lit(Some(t))) if t == phase => {
                Some(Pointer {
                    line: w[0].line,
                    call: Call::PhaseMarker,
                })
            }
            _ => None,
        })
}

/// The index of the `)` matching the `(` at `open` (or the end of the list).
fn matching_close(toks: &[Tok], open: usize) -> usize {
    let mut depth = 0usize;
    for (j, t) in toks.iter().enumerate().skip(open) {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

/// The kept literals that follow `<key> :` in `args`.
fn keyed(args: &[Tok], key: Name) -> impl Iterator<Item = &str> {
    args.windows(3)
        .filter_map(move |w| match (&w[0].kind, &w[1].kind, &w[2].kind) {
            (Kind::Name(n), Kind::Colon, Kind::Lit(Some(t))) if *n == key => Some(t.as_str()),
            _ => None,
        })
}

/// The identifiers the scan reads; every other identifier is [`Kind::Ident`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Name {
    Agent,
    Parallel,
    Pipeline,
    Phase,
    Label,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Name(Name),
    Ident,
    /// A string, template or regex literal; the text only when kept.
    Lit(Option<String>),
    Open,
    Close,
    Colon,
    Other(char),
}

#[derive(Debug)]
struct Tok {
    kind: Kind,
    line: u32,
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    toks: Vec<Tok>,
}

impl Lexer {
    fn new(script: &str) -> Self {
        Lexer {
            chars: script.chars().collect(),
            pos: 0,
            line: 1,
            toks: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<Tok> {
        self.code(false);
        self.toks
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    /// Consume one char, counting newlines.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn push(&mut self, kind: Kind, line: u32) {
        self.toks.push(Tok { kind, line });
    }

    /// Whether the literal about to be pushed is one the scan reads:
    /// after `phase :`, `label :` or `phase (`.
    fn keeps_next_literal(&self) -> bool {
        let n = self.toks.len();
        if n < 2 {
            return false;
        }
        matches!(
            (&self.toks[n - 2].kind, &self.toks[n - 1].kind),
            (Kind::Name(Name::Phase | Name::Label), Kind::Colon)
                | (Kind::Name(Name::Phase), Kind::Open)
        )
    }

    /// Lex code until the end, or — inside a template's `${…}` — until the
    /// `}` that closes it (consumed).
    fn code(&mut self, in_substitution: bool) {
        let mut braces = 0usize;
        while let Some(c) = self.peek(0) {
            let line = self.line;
            match c {
                c if c.is_whitespace() => {
                    self.bump();
                }
                c if c.is_alphanumeric() || c == '_' || c == '$' => {
                    let start = self.pos;
                    while matches!(self.peek(0), Some(c) if c.is_alphanumeric() || c == '_' || c == '$')
                    {
                        self.bump();
                    }
                    let word: String = self.chars[start..self.pos].iter().collect();
                    let kind = match word.as_str() {
                        "agent" => Kind::Name(Name::Agent),
                        "parallel" => Kind::Name(Name::Parallel),
                        "pipeline" => Kind::Name(Name::Pipeline),
                        "phase" => Kind::Name(Name::Phase),
                        "label" => Kind::Name(Name::Label),
                        _ => Kind::Ident,
                    };
                    self.push(kind, line);
                }
                '\'' | '"' => {
                    let keep = self.keeps_next_literal();
                    self.bump();
                    let text = self.quoted(c);
                    self.push(Kind::Lit(text.filter(|_| keep)), line);
                }
                '`' => {
                    self.bump();
                    self.template(line);
                }
                '/' if self.peek(1) == Some('/') => {
                    while matches!(self.peek(0), Some(c) if c != '\n') {
                        self.bump();
                    }
                }
                '/' if self.peek(1) == Some('*') => {
                    self.bump();
                    self.bump();
                    while self.peek(0).is_some()
                        && !(self.peek(0) == Some('*') && self.peek(1) == Some('/'))
                    {
                        self.bump();
                    }
                    self.bump();
                    self.bump();
                }
                '/' if !self.operand_before() => {
                    self.bump();
                    self.regex();
                    self.push(Kind::Lit(None), line);
                }
                '{' => {
                    braces += 1;
                    self.bump();
                    self.push(Kind::Other('{'), line);
                }
                '}' if in_substitution && braces == 0 => {
                    self.bump();
                    return;
                }
                '}' => {
                    braces = braces.saturating_sub(1);
                    self.bump();
                    self.push(Kind::Other('}'), line);
                }
                '(' => {
                    self.bump();
                    self.push(Kind::Open, line);
                }
                ')' => {
                    self.bump();
                    self.push(Kind::Close, line);
                }
                ':' => {
                    self.bump();
                    self.push(Kind::Colon, line);
                }
                c => {
                    self.bump();
                    self.push(Kind::Other(c), line);
                }
            }
        }
    }

    /// Whether the previous token ends an operand, so a `/` divides.
    fn operand_before(&self) -> bool {
        matches!(
            self.toks.last().map(|t| &t.kind),
            Some(Kind::Name(_) | Kind::Ident | Kind::Lit(_) | Kind::Close | Kind::Other(']' | '}'))
        )
    }

    /// Skip a `'…'` / `"…"` body after its opening quote; its text when
    /// short enough to keep.
    fn quoted(&mut self, quote: char) -> Option<String> {
        let mut text = String::new();
        let mut long = false;
        while let Some(c) = self.bump() {
            match c {
                c if c == quote => break,
                '\\' => {
                    self.bump();
                    long = true; // an escaped literal is never a title or a label head
                }
                '\n' => break, // unterminated: stop at the line's end
                c => {
                    if text.chars().count() < KEEP_MAX {
                        text.push(c);
                    } else {
                        long = true;
                    }
                }
            }
        }
        (!long).then_some(text)
    }

    /// Skip a template body after its opening backtick, lexing each `${…}`
    /// as code. The literal token (its head up to the first `${`) is pushed
    /// before the substitutions' tokens so `label :` sees it.
    fn template(&mut self, line: u32) {
        let keep = self.keeps_next_literal();
        let mut head = Some(String::new());
        let mut pushed = false;
        while let Some(c) = self.bump() {
            match c {
                '`' => break,
                '\\' => {
                    self.bump();
                    if !pushed {
                        head = None;
                    }
                }
                '$' if self.peek(0) == Some('{') => {
                    self.bump();
                    if !pushed {
                        self.push(Kind::Lit(head.take().filter(|_| keep)), line);
                        pushed = true;
                    }
                    self.code(true);
                }
                c if !pushed => {
                    if let Some(h) = head.as_mut() {
                        if h.chars().count() < KEEP_MAX {
                            h.push(c);
                        } else {
                            head = None;
                        }
                    }
                }
                _ => {}
            }
        }
        if !pushed {
            self.push(Kind::Lit(head.filter(|_| keep)), line);
        }
    }

    /// Skip a regex body after its opening `/`, and its flags.
    fn regex(&mut self) {
        let mut class = false;
        while let Some(c) = self.peek(0) {
            if c == '\n' {
                return;
            }
            self.bump();
            match c {
                '\\' => {
                    if self.peek(0) != Some('\n') {
                        self.bump();
                    }
                }
                '[' => class = true,
                ']' => class = false,
                '/' if !class => break,
                _ => {}
            }
        }
        while matches!(self.peek(0), Some(c) if c.is_alphanumeric()) {
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r#"
phase('Sweep')
const sweep = await parallel(READERS.map(r => () =>
  agent(r.prompt, { label: `sweep:${r.key}`, phase: 'Sweep' })))
phase('Verify')
const v = await pipeline(canon,
  c => agent(`Check:
this line says phase: 'Verify' and parallel( inside a prompt
${JSON.stringify(c)}`, { label: `verify:${c.id}`, phase: "Verify" }))
phase('Lonely')
"#;

    #[test]
    fn parallel_and_pipeline_are_found_by_phase_and_label_prefix() {
        assert_eq!(
            pointer(SCRIPT, "Sweep", &["sweep".into()]),
            Some(Pointer {
                line: 3,
                call: Call::Parallel
            })
        );
        assert_eq!(
            pointer(SCRIPT, "Verify", &["verify".into()]),
            Some(Pointer {
                line: 6,
                call: Call::Pipeline
            })
        );
    }

    #[test]
    fn text_inside_a_prompt_is_never_a_match() {
        // Review Focus 2: the prompt's `phase: 'Verify'` / `parallel(` must not count.
        let only_prompt = "const x = `phase: 'Verify' parallel( agent(`\nphase('Verify')\n";
        assert_eq!(
            pointer(only_prompt, "Verify", &["verify".into()]),
            Some(Pointer {
                line: 2,
                call: Call::PhaseMarker
            })
        );
    }

    #[test]
    fn an_unmatched_phase_falls_back_to_its_marker_or_nothing() {
        // Line 1 is the empty line after `r#"`; the template literal spans lines 7–9.
        assert_eq!(
            pointer(SCRIPT, "Lonely", &["x".into()]),
            Some(Pointer {
                line: 10,
                call: Call::PhaseMarker
            })
        );
        assert_eq!(pointer(SCRIPT, "Absent", &["x".into()]), None);
    }

    #[test]
    fn nested_parallel_inside_a_pipeline_stage_is_the_nearest() {
        let s = "pipeline(xs,\n x => parallel(L.map(l => () =>\n  agent(p, {label: `judge:${l}`, phase: 'Design'}))))\n";
        assert_eq!(
            pointer(s, "Design", &["judge".into()]),
            Some(Pointer {
                line: 2,
                call: Call::Parallel
            })
        );
    }

    #[test]
    fn a_label_template_with_no_prefix_falls_back_to_the_marker() {
        let s = "phase('Verify')\nawait parallel(cs.map(c => () =>\n  agent(p, { label: `${c.id}`, phase: 'Verify' })))\n";
        assert_eq!(
            pointer(s, "Verify", &["verify".into()]),
            Some(Pointer {
                line: 1,
                call: Call::PhaseMarker
            })
        );
    }

    #[test]
    fn a_plain_string_label_matches_its_prefix() {
        let s = "phase(\"Verify\")\nawait parallel([\n  () => agent(p, { label: \"verify:all\", phase: \"Verify\" })])\n";
        assert_eq!(
            pointer(s, "Verify", &["verify".into()]),
            Some(Pointer {
                line: 2,
                call: Call::Parallel
            })
        );
    }

    #[test]
    fn comments_and_nested_templates_are_skipped_but_counted() {
        let s = "// parallel(agent(p, { label: `v:1`, phase: 'V' }))\n\
/* pipeline(\n agent(p, {label:'v:2', phase:'V'}) */\n\
const t = `a ${ `nested ${ \"parallel(\" } }` } phase: 'V'\n\
\n`;\n\
phase('V')\n";
        // Lines: 1 comment, 2–3 block comment, 4–6 template, 7 `phase('V')`.
        assert_eq!(
            pointer(s, "V", &["v".into()]),
            Some(Pointer {
                line: 7,
                call: Call::PhaseMarker
            })
        );
    }

    #[test]
    fn a_parallel_inside_a_substitution_is_real_code() {
        let s = "phase('V')\nconst r = `${await parallel(xs.map(x => () =>\n agent(p, { label: 'v:' + x, phase: 'V' })))}`\n";
        // The label is `'v:'` followed by `+ x`: the literal is still the label value.
        assert_eq!(
            pointer(s, "V", &["v".into()]),
            Some(Pointer {
                line: 2,
                call: Call::Parallel
            })
        );
    }

    #[test]
    fn several_distinct_calls_fall_back_to_the_marker() {
        let s = "phase('V')\nparallel([() => agent(p, {label:'v:a', phase:'V'})])\npipeline(xs, x => agent(p, {label:'v:b', phase:'V'}))\n";
        assert_eq!(
            pointer(s, "V", &["v".into()]),
            Some(Pointer {
                line: 1,
                call: Call::PhaseMarker
            })
        );
    }

    #[test]
    fn a_regex_with_a_quote_does_not_open_a_string() {
        let s = "const q = s.replace(/'/g, '')\nphase('V')\nparallel([() => agent(p, {label:'v:a', phase:'V'})])\n";
        assert_eq!(
            pointer(s, "V", &["v".into()]),
            Some(Pointer {
                line: 3,
                call: Call::Parallel
            })
        );
    }
}
