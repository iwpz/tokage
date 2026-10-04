//! A small POSIX shell reader for command classification (docs/11 §3.2). It does not run or
//! expand anything: it splits a command line into simple-command segments, removes quotes,
//! and records what it could not resolve statically (variables, substitutions, globs) so the
//! matcher can treat those words conservatively.

/// One shell word after quote removal.
#[derive(Debug, Clone, Default)]
pub struct Word {
    pub text: String,
    /// Contains a variable, command substitution or arithmetic whose value is unknown.
    pub dynamic: bool,
    /// Contains an unquoted `*`, `?` or `[`.
    pub glob: bool,
    /// Starts with an unquoted `~` (or `$HOME`, rewritten to `~`).
    pub tilde: bool,
    /// Sources of `$(…)`, `` `…` ``, `<(…)` and `>(…)` inside the word.
    pub subs: Vec<String>,
    /// Something inside could not be read reliably (`${x:-$(…)}` and the like).
    pub uncertain: bool,
    /// Any part was quoted or escaped (a quoted here-document delimiter disables expansion).
    pub quoted: bool,
    /// The word as written, before quote removal.
    pub raw: String,
    /// Contains an unquoted `{`, so brace expansion may apply.
    brace: bool,
}

/// Stands in for an unknown (dynamic) part of a word's text.
pub const DYN: char = '\u{1}';

impl Word {
    fn mark_dynamic(&mut self) {
        self.dynamic = true;
        self.text.push(DYN);
    }

    pub fn literal(text: &str) -> Word {
        Word {
            text: text.into(),
            ..Word::default()
        }
    }

    pub fn is_static(&self) -> bool {
        !self.dynamic && !self.glob && !self.uncertain
    }
}

#[derive(Debug, Clone)]
pub struct Redirect {
    /// `>`, `>>`, `>|`, `&>`, `&>>`, `<`, `<>`, `<<`, `<<<`, `>&`, `<&`.
    pub op: &'static str,
    pub target: Word,
}

#[derive(Debug, Clone, Default)]
pub struct Segment {
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    /// Stdin comes from the previous segment (`|` or `|&`).
    pub piped: bool,
    /// Here-document bodies and here-strings fed to stdin.
    pub stdin: Vec<String>,
    /// Commands an unquoted here-document body would run while expanding.
    pub stdin_subs: Vec<String>,
    /// Subshell nesting: `(a; b)` puts `a` and `b` at 1.
    pub depth: usize,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub segments: Vec<Segment>,
    /// Unterminated quote, substitution or here-document.
    pub error: bool,
    /// A function definition was seen; its body runs later, under another name.
    pub function_def: bool,
}

/// The command substitutions an unquoted here-document body would run.
fn substitutions(body: &str) -> Vec<String> {
    let mut p = Parser {
        s: body.chars().collect(),
        i: 0,
        out: Parsed::default(),
        cur: Segment::default(),
        heredocs: Vec::new(),
        depth: 0,
    };
    let mut w = Word::default();
    while let Some(c) = p.peek(0) {
        match c {
            '\\' => p.i += 2,
            '$' => p.dollar(&mut w, false, false),
            '`' => match p.backtick() {
                Some(inner) => w.subs.push(inner),
                None => break,
            },
            _ => p.i += 1,
        }
    }
    if p.out.error || w.uncertain {
        // Unreadable: the caller treats U+0001 as an unknown command.
        w.subs.push(DYN.to_string());
    }
    w.subs
}

pub fn parse(src: &str) -> Parsed {
    let mut p = Parser {
        s: src.chars().collect(),
        i: 0,
        out: Parsed::default(),
        cur: Segment::default(),
        heredocs: Vec::new(),
        depth: 0,
    };
    p.run();
    p.out
}

struct Parser {
    s: Vec<char>,
    i: usize,
    out: Parsed,
    cur: Segment,
    /// Here-documents whose bodies start after the current line: delimiter, `<<-`, expands,
    /// index of the segment they feed.
    heredocs: Vec<(String, bool, bool, usize)>,
    depth: usize,
}

fn is_meta(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')')
}

impl Parser {
    fn peek(&self, off: usize) -> Option<char> {
        self.s.get(self.i + off).copied()
    }

    fn end_segment(&mut self, next_piped: bool) {
        let mut seg = std::mem::take(&mut self.cur);
        seg.depth = self.depth;
        if !seg.words.is_empty() || !seg.redirects.is_empty() {
            self.out.segments.push(seg);
        }
        self.cur.piped = next_piped;
    }

    fn run(&mut self) {
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' => self.i += 1,
                '\\' if self.peek(1) == Some('\n') => self.i += 2,
                '\n' => {
                    self.i += 1;
                    self.end_segment(false);
                    self.skip_heredoc_bodies();
                }
                ';' => {
                    self.i += 1;
                    self.end_segment(false);
                }
                '&' => match self.peek(1) {
                    Some('&') => {
                        self.i += 2;
                        self.end_segment(false);
                    }
                    Some('>') => {
                        let op = if self.peek(2) == Some('>') { "&>>" } else { "&>" };
                        self.i += op.len();
                        self.redirect(op);
                    }
                    _ => {
                        self.i += 1;
                        self.end_segment(false);
                    }
                },
                '|' => match self.peek(1) {
                    Some('|') => {
                        self.i += 2;
                        self.end_segment(false);
                    }
                    Some('&') => {
                        self.i += 2;
                        self.end_segment(true);
                    }
                    _ => {
                        self.i += 1;
                        self.end_segment(true);
                    }
                },
                '(' => {
                    // `name()` / `name ()` defines a function.
                    if !self.cur.words.is_empty() {
                        let mut j = self.i + 1;
                        while matches!(self.s.get(j), Some(' ' | '\t')) {
                            j += 1;
                        }
                        if self.s.get(j) == Some(&')') {
                            self.out.function_def = true;
                        }
                    }
                    let subshell = self.cur.words.is_empty();
                    self.i += 1;
                    self.end_segment(false);
                    if subshell {
                        self.depth += 1;
                    }
                }
                ')' => {
                    self.i += 1;
                    self.end_segment(false);
                    self.depth = self.depth.saturating_sub(1);
                }
                '<' | '>' if self.peek(1) == Some('(') => {
                    // Process substitution: a word whose value is a pipe to another command.
                    self.i += 1;
                    let mut w = Word::default();
                    match self.capture_paren() {
                        Some(inner) => w.subs.push(inner),
                        None => self.out.error = true,
                    }
                    w.text.push_str("/dev/fd/63");
                    self.cur.words.push(w);
                }
                '<' | '>' => {
                    let op = self.redirect_op();
                    self.redirect(op);
                }
                '#' => {
                    while let Some(c) = self.peek(0) {
                        if c == '\n' {
                            break;
                        }
                        self.i += 1;
                    }
                }
                _ => {
                    let w = self.word();
                    if w.brace {
                        self.cur.words.extend(expand_braces(&w));
                        continue;
                    }
                    // A bare number right before `<`/`>` is a file descriptor.
                    let fd = !w.text.is_empty()
                        && w.is_static()
                        && !w.tilde
                        && w.text.chars().all(|c| c.is_ascii_digit())
                        && matches!(self.peek(0), Some('<' | '>'));
                    if !fd {
                        self.cur.words.push(w);
                    }
                }
            }
        }
        self.end_segment(false);
        if !self.heredocs.is_empty() {
            self.out.error = true;
        }
    }

    fn redirect_op(&mut self) -> &'static str {
        let ops: [&'static str; 10] = [">>", ">|", ">&", "<<<", "<<-", "<<", "<&", "<>", ">", "<"];
        for op in ops {
            if op.chars().enumerate().all(|(k, c)| self.peek(k) == Some(c)) {
                self.i += op.len();
                return if op == "<<-" { "<<-" } else { op };
            }
        }
        unreachable!("called on < or >")
    }

    fn redirect(&mut self, op: &'static str) {
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.i += 1;
        }
        if self.peek(0).map_or(true, is_meta) {
            self.out.error = true;
            return;
        }
        let target = self.word();
        if op == "<<" || op == "<<-" {
            let seg = self.out.segments.len();
            self.heredocs.push((target.text.clone(), op == "<<-", !target.quoted, seg));
            self.cur.redirects.push(Redirect { op: "<<", target });
            return;
        }
        // `2>&1`, `>&-`: duplicating a descriptor, not a file.
        if (op == ">&" || op == "<&") && target.text.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return;
        }
        if op == "<<<" {
            self.cur.stdin.push(target.text.clone());
        }
        let op = if op == ">&" { "&>" } else { op };
        self.cur.redirects.push(Redirect { op, target });
    }

    fn skip_heredoc_bodies(&mut self) {
        for (delim, strip_tabs, expands, seg) in std::mem::take(&mut self.heredocs) {
            let mut body = String::new();
            loop {
                if self.i >= self.s.len() {
                    self.out.error = true;
                    return;
                }
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i] != '\n' {
                    self.i += 1;
                }
                let line: String = self.s[start..self.i].iter().collect();
                if self.i < self.s.len() {
                    self.i += 1;
                }
                let line = if strip_tabs { line.trim_start_matches('\t') } else { &line };
                if line == delim {
                    break;
                }
                body.push_str(line);
                body.push('\n');
            }
            let subs = if expands { substitutions(&body) } else { Vec::new() };
            if let Some(s) = self.out.segments.get_mut(seg) {
                s.stdin.push(body);
                s.stdin_subs.extend(subs);
            }
        }
    }

    /// Reads from just after `(` to the matching `)`, honouring quotes and nesting.
    fn capture_paren(&mut self) -> Option<String> {
        debug_assert_eq!(self.peek(0), Some('('));
        self.i += 1;
        let start = self.i;
        let mut depth = 1;
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => self.i += 2,
                '\'' => {
                    self.i += 1;
                    while self.peek(0).is_some_and(|c| c != '\'') {
                        self.i += 1;
                    }
                    self.i += 1;
                }
                '"' => {
                    self.i += 1;
                    while let Some(c) = self.peek(0) {
                        if c == '\\' {
                            self.i += 2;
                            continue;
                        }
                        if c == '"' {
                            break;
                        }
                        self.i += 1;
                    }
                    self.i += 1;
                }
                '(' => {
                    depth += 1;
                    self.i += 1;
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner: String = self.s[start..self.i].iter().collect();
                        self.i += 1;
                        return Some(inner);
                    }
                    self.i += 1;
                }
                _ => self.i += 1,
            }
        }
        None
    }

    fn backtick(&mut self) -> Option<String> {
        self.i += 1;
        let mut inner = String::new();
        while let Some(c) = self.peek(0) {
            match c {
                '\\' => {
                    if let Some(n) = self.peek(1) {
                        if !matches!(n, '`' | '\\' | '$') {
                            inner.push('\\');
                        }
                        inner.push(n);
                    }
                    self.i += 2;
                }
                '`' => {
                    self.i += 1;
                    return Some(inner);
                }
                _ => {
                    inner.push(c);
                    self.i += 1;
                }
            }
        }
        None
    }

    fn word(&mut self) -> Word {
        let start = self.i;
        let mut w = self.word_inner();
        w.raw = self.s[start..self.i].iter().collect();
        // `[` / `[[` / `]]` as a whole word are the test command, not a glob.
        if matches!(w.text.as_str(), "[" | "[[" | "]]") {
            w.glob = false;
        }
        w
    }

    fn word_inner(&mut self) -> Word {
        let mut w = Word::default();
        let mut first = true;
        while let Some(c) = self.peek(0) {
            if is_meta(c) {
                break;
            }
            match c {
                '\\' => {
                    w.quoted = true;
                    match self.peek(1) {
                        Some('\n') => {}
                        Some(n) => w.text.push(n),
                        None => {}
                    }
                    self.i += 2;
                }
                '\'' => {
                    w.quoted = true;
                    self.i += 1;
                    loop {
                        match self.peek(0) {
                            None => {
                                self.out.error = true;
                                break;
                            }
                            Some('\'') => {
                                self.i += 1;
                                break;
                            }
                            Some(c) => {
                                w.text.push(c);
                                self.i += 1;
                            }
                        }
                    }
                }
                '"' => {
                    w.quoted = true;
                    self.i += 1;
                    self.double_quoted(&mut w);
                }
                '$' => self.dollar(&mut w, true, first),
                '`' => match self.backtick() {
                    Some(inner) => {
                        w.subs.push(inner);
                        w.mark_dynamic();
                    }
                    None => self.out.error = true,
                },
                '*' | '?' | '[' => {
                    w.glob = true;
                    w.text.push(c);
                    self.i += 1;
                }
                '{' => {
                    w.brace = true;
                    w.text.push(c);
                    self.i += 1;
                }
                '~' if first => {
                    w.tilde = true;
                    w.text.push(c);
                    self.i += 1;
                }
                _ => {
                    w.text.push(c);
                    self.i += 1;
                }
            }
            first = false;
        }
        w
    }

    fn double_quoted(&mut self, w: &mut Word) {
        loop {
            match self.peek(0) {
                None => {
                    self.out.error = true;
                    return;
                }
                Some('"') => {
                    self.i += 1;
                    return;
                }
                Some('\\') => {
                    match self.peek(1) {
                        Some(n @ ('$' | '`' | '"' | '\\')) => w.text.push(n),
                        Some('\n') => {}
                        Some(n) => {
                            w.text.push('\\');
                            w.text.push(n);
                        }
                        None => {}
                    }
                    self.i += 2;
                }
                Some('$') => {
                    let at_start = w.text.is_empty() && !w.tilde;
                    self.dollar(w, false, at_start)
                }
                Some('`') => match self.backtick() {
                    Some(inner) => {
                        w.subs.push(inner);
                        w.mark_dynamic();
                    }
                    None => {
                        self.out.error = true;
                        return;
                    }
                },
                Some(c) => {
                    w.text.push(c);
                    self.i += 1;
                }
            }
        }
    }

    /// At `$`. `at_start`: nothing has been read into the word yet (so `$HOME` can become `~`).
    fn dollar(&mut self, w: &mut Word, unquoted: bool, at_start: bool) {
        match self.peek(1) {
            Some('(') if self.peek(2) == Some('(') => {
                self.i += 1;
                match self.capture_paren() {
                    Some(inner) => {
                        if inner.contains("$(") || inner.contains('`') {
                            w.uncertain = true;
                        }
                    }
                    None => self.out.error = true,
                }
                w.mark_dynamic();
            }
            Some('(') => {
                self.i += 1;
                match self.capture_paren() {
                    Some(inner) => w.subs.push(inner),
                    None => self.out.error = true,
                }
                w.mark_dynamic();
            }
            Some('{') => {
                self.i += 2;
                let start = self.i;
                let mut depth = 1;
                while let Some(c) = self.peek(0) {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                if self.peek(0).is_none() {
                    self.out.error = true;
                    return;
                }
                let inner: String = self.s[start..self.i].iter().collect();
                self.i += 1;
                if inner == "HOME" {
                    // Kept as `~` anywhere (`--key=$HOME/.ssh/x`), a tilde only at the start.
                    w.tilde |= at_start;
                    w.text.push('~');
                } else {
                    if inner.contains("$(") || inner.contains('`') {
                        w.uncertain = true;
                    }
                    w.mark_dynamic();
                }
            }
            Some('\'') if unquoted => {
                // ANSI-C quoting: decode so `$'\x73udo'` is seen as `sudo`.
                self.i += 2;
                loop {
                    match self.peek(0) {
                        None => {
                            self.out.error = true;
                            return;
                        }
                        Some('\'') => {
                            self.i += 1;
                            return;
                        }
                        Some('\\') => {
                            self.i += 1;
                            self.ansi_escape(w);
                        }
                        Some(c) => {
                            w.text.push(c);
                            self.i += 1;
                        }
                    }
                }
            }
            Some('"') if unquoted => {
                self.i += 2;
                self.double_quoted(w);
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                self.i += 1;
                let start = self.i;
                while self.peek(0).is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                    self.i += 1;
                }
                let name: String = self.s[start..self.i].iter().collect();
                if name == "HOME" {
                    w.tilde |= at_start;
                    w.text.push('~');
                } else {
                    w.mark_dynamic();
                }
            }
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                self.i += 2;
                w.mark_dynamic();
            }
            _ => {
                w.text.push('$');
                self.i += 1;
            }
        }
    }

    fn ansi_escape(&mut self, w: &mut Word) {
        let Some(c) = self.peek(0) else { return };
        self.i += 1;
        let simple = match c {
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            'a' => Some('\u{7}'),
            'b' => Some('\u{8}'),
            'e' | 'E' => Some('\u{1b}'),
            'f' => Some('\u{c}'),
            'v' => Some('\u{b}'),
            '\\' | '\'' | '"' | '?' => Some(c),
            _ => None,
        };
        if let Some(ch) = simple {
            w.text.push(ch);
            return;
        }
        let (radix, max) = match c {
            'x' => (16, 2),
            'u' => (16, 4),
            'U' => (16, 8),
            '0'..='7' => {
                self.i -= 1;
                (8, 3)
            }
            _ => {
                w.text.push('\\');
                w.text.push(c);
                return;
            }
        };
        let start = self.i;
        while self.i - start < max && self.peek(0).is_some_and(|d| d.is_digit(radix)) {
            self.i += 1;
        }
        let digits: String = self.s[start..self.i].iter().collect();
        match u32::from_str_radix(&digits, radix).ok().and_then(char::from_u32) {
            Some(ch) => w.text.push(ch),
            None => w.uncertain = true,
        }
    }
}

/// Brace expansion of `a{b,c}d` into `abd acd` (nested lists too). `{1..5}` ranges are left
/// in place and marked as a glob. At most 64 results; past that the word is marked uncertain.
fn expand_braces(w: &Word) -> Vec<Word> {
    let mut out = Vec::new();
    let mut overflow = false;
    expand_into(&w.text, &mut out, &mut overflow);
    if out.len() <= 1 && !overflow {
        let mut w = w.clone();
        w.brace = false;
        if w.text.contains("..") && w.text.contains('{') {
            w.glob = true;
        }
        return vec![w];
    }
    out.into_iter()
        .map(|text| Word {
            glob: w.glob || text.contains(['*', '?', '[']),
            uncertain: w.uncertain || overflow,
            brace: false,
            tilde: text.starts_with('~') && w.tilde,
            text,
            ..w.clone()
        })
        .collect()
}

fn expand_into(text: &str, out: &mut Vec<String>, overflow: &mut bool) {
    if out.len() >= 64 {
        *overflow = true;
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    for open in 0..chars.len() {
        if chars[open] != '{' {
            continue;
        }
        let mut depth = 0;
        let mut commas = Vec::new();
        let mut close = None;
        for (k, &c) in chars.iter().enumerate().skip(open) {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(k);
                        break;
                    }
                }
                ',' if depth == 1 => commas.push(k),
                _ => {}
            }
        }
        let Some(close) = close else { break };
        if commas.is_empty() {
            continue;
        }
        let prefix: String = chars[..open].iter().collect();
        let suffix: String = chars[close + 1..].iter().collect();
        let mut bounds = vec![open];
        bounds.extend(&commas);
        bounds.push(close);
        for pair in bounds.windows(2) {
            let alt: String = chars[pair[0] + 1..pair[1]].iter().collect();
            expand_into(&format!("{prefix}{alt}{suffix}"), out, overflow);
        }
        return;
    }
    out.push(text.to_string());
}
