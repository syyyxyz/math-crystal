//! LaTeX input: translates the usual subset of LaTeX maths into the plain
//! syntax the parser reads, e.g.
//! `\frac{\sin(xy)}{1+x^2} + e^{-(x^2+y^2)}` → `((sin(x y))/(1+x^(2))) + e^(-(x^(2)+y^(2)))`.
//!
//! Supported: `\frac \dfrac \tfrac`, `\sqrt{..}`, `\sqrt[n]{..}`, `^{..}`,
//! `\left( \right)` and the `\big` family, `\cdot \times \div`, `\pi \tau`,
//! function commands (`\sin \ln \exp \log_b \sin^2 \sin^{-1} ...`, also with
//! implicit arguments like `\sin x`), `\operatorname{..} \mathrm{..}`,
//! comparisons `\le \ge \ne`, `\land \lor`, `|x|` / `\lvert \rvert`,
//! `\lfloor \rfloor \lceil \rceil`, `\begin{cases} .. \end{cases}`, spacing
//! commands, `$...$` and a leading `z =` / `f(x,y) =`.
//! In LaTeX, adjacent letters are a product: `xy` means `x y`.

use super::parse::ParseError;
use crate::i18n::tr;

/// Whether `src` should be read as LaTeX.
pub fn looks_like_latex(src: &str) -> bool {
    src.contains('\\') || src.contains('{') || src.trim_start().starts_with('$')
}

/// Strip a leading `z =`, `f =`, `f(x, y) =` (any single-letter name).
pub fn strip_lhs(src: &str) -> &str {
    let b = src.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c != b'=' {
            continue;
        }
        let prev = if i > 0 { b[i - 1] } else { 0 };
        let next = b.get(i + 1).copied().unwrap_or(0);
        if matches!(prev, b'<' | b'>' | b'!' | b'=') || next == b'=' {
            return src;
        }
        let mut lhs: String = src[..i].chars().filter(|c| !c.is_whitespace()).collect();
        for noise in ["\\left", "\\right", "\\displaystyle", "\\textstyle", "\\,", "\\!", "{", "}", "$"] {
            lhs = lhs.replace(noise, "");
        }
        let mut ch = lhs.chars();
        let ok = match ch.next() {
            Some(n) if n.is_ascii_alphabetic() => {
                let rest: String = ch.collect();
                rest.is_empty() || rest.starts_with('(') && rest.ends_with(')') && rest[1..rest.len() - 1].chars().all(|c| c.is_ascii_alphabetic() || c == ',')
            }
            _ => false,
        };
        return if ok { &src[i + 1..] } else { src };
    }
    src
}

#[derive(Clone, Debug, PartialEq)]
enum T {
    Cmd(String),
    Ch(char),
    Space,
    Open,
    Close,
}

fn lex(src: &str) -> Vec<T> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '\\' => {
                let mut j = i + 1;
                while j < cs.len() && cs[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j == i + 1 && j < cs.len() {
                    // Control symbol such as \, \{ \\ \|
                    j += 1;
                }
                out.push(T::Cmd(cs[i + 1..j].iter().collect()));
                i = j;
                continue;
            }
            '{' => out.push(T::Open),
            '}' => out.push(T::Close),
            '~' => out.push(T::Space),
            c if c.is_whitespace() => {
                if out.last() != Some(&T::Space) {
                    out.push(T::Space);
                }
            }
            c => out.push(T::Ch(c)),
        }
        i += 1;
    }
    out
}

/// Translate LaTeX into the plain syntax.
pub fn to_plain(src: &str) -> Result<String, ParseError> {
    convert(strip_lhs(&clean(src)))
}

/// Like [`to_plain`] but keeps a left-hand side such as `z =` (written as
/// `==`), for equations of surfaces and curves.
pub fn to_plain_equation(src: &str) -> Result<String, ParseError> {
    convert(&clean(src))
}

fn convert(s: &str) -> Result<String, ParseError> {
    let mut c = Conv { t: lex(s), i: 0, cond: false, otherwise: false };
    let out = c.seq(&|_| false)?;
    if c.i < c.t.len() {
        return Err(c.err(tr("Unmatched '}'", "多余的 '}'")));
    }
    Ok(tidy(&out))
}

/// Environments that only wrap a displayed formula.
const DISPLAY_ENVS: [&str; 15] = [
    "equation", "equation*", "align", "align*", "aligned", "alignedat", "gather", "gather*", "gathered", "displaymath", "math", "split",
    "multline", "multline*", "flalign",
];

/// Tidy formula text as it arrives from copy and paste: typographic
/// Unicode (`−ⁿ×·≤²` …), maths delimiters (`$…$`, `$$…$$`, `\[…\]`, `\(…\)`),
/// display environments such as `equation` or `aligned` (with their `&`),
/// `\tag{…}`, `\label{…}`, a leading `\displaystyle` and trailing
/// punctuation or spacing. A left-hand side such as `z =` is kept.
pub fn clean(src: &str) -> String {
    let mut s = strip_commands(&unicode_to_ascii(src));
    loop {
        let before = s.clone();
        s = s.trim().to_string();
        for (a, b) in [("$$", "$$"), ("$", "$"), ("\\[", "\\]"), ("\\(", "\\)")] {
            if s.len() >= a.len() + b.len() && s.starts_with(a) && s.ends_with(b) {
                s = s[a.len()..s.len() - b.len()].to_string();
                break;
            }
        }
        if let Some(body) = unwrap_env(s.trim()) {
            s = body;
        }
        for style in ["\\displaystyle", "\\textstyle"] {
            if let Some(rest) = s.trim_start().strip_prefix(style)
                && !rest.starts_with(|c: char| c.is_ascii_alphabetic())
            {
                s = rest.to_string();
            }
        }
        s = strip_trailing(&s);
        if s == before {
            return s;
        }
    }
}

/// `\begin{equation} … \end{equation}` (and the other display
/// environments) → the body, with alignment `&` removed unless it holds a
/// nested environment such as `cases`.
fn unwrap_env(s: &str) -> Option<String> {
    let rest = s.strip_prefix("\\begin{")?;
    let close = rest.find('}')?;
    let env = &rest[..close];
    if !DISPLAY_ENVS.contains(&env) {
        return None;
    }
    let body = rest[close + 1..].trim_end().strip_suffix(&format!("\\end{{{env}}}"))?;
    let body = if body.contains("\\begin{") { body.to_string() } else { body.replace('&', " ") };
    Some(body)
}

/// Whether `s[at..]` starts with the command `cmd` (not a longer name).
fn command_at(s: &str, at: usize, cmd: &str) -> bool {
    s[at..].starts_with(cmd) && !s[at + cmd.len()..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '*')
}

/// Drop `\tag{…}`, `\tag*{…}`, `\label{…}`, `\nonumber` and `\notag`.
fn strip_commands(s: &str) -> String {
    let mut out = s.to_string();
    for cmd in ["\\tag*", "\\tag", "\\label"] {
        let mut from = 0;
        while let Some(k) = out[from..].find(cmd).map(|k| k + from) {
            let after = k + cmd.len();
            // A longer command name, or `\tag*` when looking for `\tag`.
            if out[after..].starts_with(|c: char| c.is_ascii_alphabetic() || (c == '*' && cmd != "\\tag*")) {
                from = after;
                continue;
            }
            let open = after + (out[after..].len() - out[after..].trim_start().len());
            if !out[open..].starts_with('{') {
                from = after;
                continue;
            }
            let mut depth = 0;
            let mut end = None;
            for (j, c) in out[open..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(open + j + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else { break };
            out.replace_range(k..end, " ");
            from = k;
        }
    }
    for cmd in ["\\nonumber", "\\notag"] {
        let mut from = 0;
        while let Some(k) = out[from..].find(cmd).map(|k| k + from) {
            if command_at(&out, k, cmd) {
                out.replace_range(k..k + cmd.len(), " ");
                from = k;
            } else {
                from = k + cmd.len();
            }
        }
    }
    out
}

/// Remove trailing spacing commands, line breaks and sentence punctuation
/// (but not the `.` of `\right.`).
fn strip_trailing(s: &str) -> String {
    let mut t = s.trim_end();
    loop {
        let before = t.len();
        for suffix in ["\\qquad", "\\quad", "\\\\", "\\,", "\\;", "\\:", "\\!", "\\", "~"] {
            if let Some(rest) = t.strip_suffix(suffix) {
                t = rest.trim_end();
                break;
            }
        }
        if let Some(last) = t.chars().last()
            && matches!(last, ',' | ';' | '.' | '。' | '，' | '；')
        {
            let rest = &t[..t.len() - last.len_utf8()];
            let word = rest.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            // `\right.` and friends are delimiters, not punctuation.
            if !(last == '.' && word.ends_with('\\') && word.len() < rest.len()) {
                t = rest.trim_end();
            }
        }
        if t.len() == before {
            return t.to_string();
        }
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        '⁺' => '+',
        '⁻' => '-',
        '⁽' => '(',
        '⁾' => ')',
        'ⁿ' => 'n',
        _ => return None,
    })
}

/// Whether `src` contains typographic maths characters, i.e. was copied
/// from a rendered formula; it is then read with LaTeX's conventions
/// (`2xy` is a product).
pub fn is_typeset(src: &str) -> bool {
    src.chars().any(|c| {
        superscript(c).is_some() || matches!(c, '−' | '×' | '·' | '⋅' | '∙' | '÷' | '≤' | '≥' | '⩽' | '⩾' | '≠' | 'π' | '√' | '\u{2061}' | '\u{2062}' | '\u{2063}')
    })
}

/// Typographic maths characters (from rendered formulas, Word, PDFs) →
/// what the parser reads: `−` → `-`, `x²` → `x^2`, `×` → `*`, `≤` → `<=` …
fn unicode_to_ascii(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(d) = superscript(c) {
            let mut run = String::from(d);
            while let Some(e) = chars.peek().copied().and_then(superscript) {
                run.push(e);
                chars.next();
            }
            if run.chars().all(|c| c.is_ascii_digit()) && run.len() == 1 {
                out.push('^');
                out.push_str(&run);
            } else {
                // Braces: valid LaTeX, and they send the text down the LaTeX path.
                out.push_str(&format!("^{{{run}}}"));
            }
            continue;
        }
        match c {
            '−' | '–' | '‐' | '‒' | '－' => out.push('-'),
            '×' | '·' | '⋅' | '∙' | '∗' => out.push('*'),
            '÷' | '∕' => out.push('/'),
            '≤' | '⩽' => out.push_str("<="),
            '≥' | '⩾' => out.push_str(">="),
            '≠' => out.push_str("!="),
            'π' => out.push_str(" pi "),
            '√' => out.push_str("sqrt"),
            '（' => out.push('('),
            '）' => out.push(')'),
            '［' => out.push('['),
            '］' => out.push(']'),
            '，' => out.push(','),
            '＝' => out.push('='),
            '＋' => out.push('+'),
            // MathML invisible operators: times, separator, function application.
            '\u{2062}' => out.push('*'),
            '\u{2063}' => out.push(','),
            '\u{2061}' | '\u{200b}' | '\u{feff}' => {}
            c if c != ' ' && c.is_whitespace() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Collapse runs of spaces.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.trim().chars() {
        if c == ' ' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// LaTeX function command → parser name.
fn latex_fn(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "cot" => "cot",
        "sec" => "sec",
        "csc" => "csc",
        "arcsin" => "asin",
        "arccos" => "acos",
        "arctan" => "atan",
        "sinh" => "sinh",
        "cosh" => "cosh",
        "tanh" => "tanh",
        "arsinh" | "arcsinh" => "asinh",
        "arcosh" | "arccosh" => "acosh",
        "artanh" | "arctanh" => "atanh",
        "exp" => "exp",
        "ln" => "ln",
        "log" => "log",
        "lg" => "log10",
        "sgn" | "sign" => "sign",
        "max" => "max",
        "min" => "min",
        "Re" | "re" => "re",
        "Im" | "im" => "im",
        "arg" | "Arg" => "arg",
        "conj" => "conj",
        _ => return None,
    })
}

fn inverse(name: &str) -> Option<&'static str> {
    Some(match name {
        "sin" => "asin",
        "cos" => "acos",
        "tan" => "atan",
        "sinh" => "asinh",
        "cosh" => "acosh",
        "tanh" => "atanh",
        _ => return None,
    })
}

struct Conv {
    t: Vec<T>,
    i: usize,
    /// Inside a `cases` condition: `=` means `==` and `,` means `and`.
    cond: bool,
    /// `\text{otherwise}` was seen in the current condition.
    otherwise: bool,
}

impl Conv {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }

    fn err(&self, msg: &str) -> ParseError {
        ParseError { msg: format!("LaTeX: {msg}"), pos: usize::MAX }
    }

    fn skip_space(&mut self) {
        while self.peek() == Some(&T::Space) {
            self.i += 1;
        }
    }

    fn seq(&mut self, stop: &dyn Fn(&T) -> bool) -> Result<String, ParseError> {
        let mut out = String::new();
        while let Some(t) = self.peek() {
            if stop(t) || *t == T::Close {
                break;
            }
            let piece = self.item()?;
            out.push_str(&piece);
        }
        Ok(out)
    }

    /// `{ ... }` → its content (without braces).
    fn group(&mut self) -> Result<String, ParseError> {
        debug_assert_eq!(self.peek(), Some(&T::Open));
        self.i += 1;
        let inner = self.seq(&|_| false)?;
        if self.peek() != Some(&T::Close) {
            return Err(self.err(tr("missing '}'", "缺少 '}'")));
        }
        self.i += 1;
        Ok(inner)
    }

    /// The raw text of a `{...}` group (for \text, \begin, ...).
    fn raw_group(&mut self) -> Result<String, ParseError> {
        self.skip_space();
        if self.peek() != Some(&T::Open) {
            return Err(self.err(tr("expected '{'", "此处应为 '{'")));
        }
        self.i += 1;
        let mut s = String::new();
        let mut depth = 0;
        loop {
            match self.peek().cloned() {
                None => return Err(self.err(tr("missing '}'", "缺少 '}'"))),
                Some(T::Close) if depth == 0 => {
                    self.i += 1;
                    return Ok(s);
                }
                Some(T::Close) => {
                    depth -= 1;
                    s.push('}');
                }
                Some(T::Open) => {
                    depth += 1;
                    s.push('{');
                }
                Some(T::Space) => s.push(' '),
                Some(T::Ch(c)) => s.push(c),
                Some(T::Cmd(c)) => {
                    s.push('\\');
                    s.push_str(&c);
                }
            }
            self.i += 1;
        }
    }

    /// Argument of `^`, `\frac`, `\sqrt`: a group or a single token.
    fn arg(&mut self) -> Result<String, ParseError> {
        self.skip_space();
        match self.peek().cloned() {
            Some(T::Open) => self.group(),
            Some(T::Ch(c)) if c.is_alphanumeric() || c == '.' => {
                self.i += 1;
                Ok(if c.is_alphabetic() { format!(" {c} ") } else { c.to_string() })
            }
            Some(T::Cmd(_)) => self.item(),
            Some(T::Ch('-')) => {
                // x^-1 (not valid LaTeX but common).
                self.i += 1;
                let a = self.arg()?;
                Ok(format!("-{a}"))
            }
            _ => Err(self.err(tr("missing argument", "缺少参数"))),
        }
    }

    fn item(&mut self) -> Result<String, ParseError> {
        let Some(t) = self.peek().cloned() else { return Ok(String::new()) };
        self.i += 1;
        Ok(match t {
            T::Space => " ".into(),
            T::Open => {
                self.i -= 1;
                format!("({})", self.group()?)
            }
            T::Close => return Err(self.err(tr("unmatched '}'", "多余的 '}'"))),
            T::Ch('^') => format!("^({})", self.arg()?),
            T::Ch('_') => return Err(self.err(tr("subscripts are only supported in \\log_b", "下标只支持 \\log_b 这种写法"))),
            T::Ch('[') => "(".into(),
            T::Ch(']') => ")".into(),
            T::Ch('&') => return Err(self.err(tr("'&' is only allowed in \\begin{cases}", "'&' 只能用在 \\begin{cases} 中"))),
            T::Ch(',') if self.cond => " and ".into(),
            T::Ch('=') => {
                let next_eq = self.peek() == Some(&T::Ch('='));
                if next_eq {
                    self.i += 1;
                }
                " == ".into()
            }
            T::Ch(c @ ('<' | '>' | '!')) if self.peek() == Some(&T::Ch('=')) => {
                self.i += 1;
                format!(" {c}= ")
            }
            T::Ch(c) if c.is_alphabetic() => {
                let mut word = c.to_string();
                while let Some(T::Ch(d)) = self.peek()
                    && d.is_alphabetic()
                {
                    word.push(*d);
                    self.i += 1;
                }
                self.word(&word)?
            }
            T::Ch(c) => c.to_string(),
            T::Cmd(name) => self.command(&name)?,
        })
    }

    /// A run of letters outside commands.
    fn word(&mut self, w: &str) -> Result<String, ParseError> {
        if let Some(f) = latex_fn(w).or_else(|| super::parse::function(w).map(|f| f.name)) {
            return self.apply(f);
        }
        if matches!(w, "pi" | "tau" | "and" | "or" | "not") {
            return Ok(format!(" {w} "));
        }
        // Adjacent letters multiply.
        Ok(w.chars().map(|c| format!(" {c} ")).collect())
    }

    fn command(&mut self, name: &str) -> Result<String, ParseError> {
        if let Some(f) = latex_fn(name) {
            return self.apply(f);
        }
        Ok(match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let a = self.arg()?;
                let b = self.arg()?;
                format!("(({a})/({b}))")
            }
            "sqrt" => {
                self.skip_space();
                let index = if self.peek() == Some(&T::Ch('[')) {
                    self.i += 1;
                    let n = self.seq(&|t| *t == T::Ch(']'))?;
                    if self.peek() != Some(&T::Ch(']')) {
                        return Err(self.err(tr("missing ']'", "缺少 ']'")));
                    }
                    self.i += 1;
                    Some(n)
                } else {
                    None
                };
                let g = self.arg()?;
                match index.as_deref().map(str::trim) {
                    None => format!(" sqrt({g}) "),
                    Some("3") => format!(" cbrt({g}) "),
                    Some(n) => format!(" (({g})^(1/({n}))) "),
                }
            }
            "left" | "right" | "middle" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" => {
                self.skip_space();
                if self.peek() == Some(&T::Ch('.')) {
                    self.i += 1;
                    return Ok(String::new());
                }
                self.item()?
            }
            "cdot" | "times" | "ast" => "*".into(),
            "bar" | "overline" => format!(" conj({}) ", self.arg()?),
            "div" => "/".into(),
            "pi" => " pi ".into(),
            "tau" => " tau ".into(),
            "alpha" => " α ".into(),
            "beta" => " β ".into(),
            "gamma" => " γ ".into(),
            "delta" => " δ ".into(),
            "lambda" => " λ ".into(),
            "mu" => " μ ".into(),
            "sigma" => " σ ".into(),
            "omega" => " ω ".into(),
            "phi" | "varphi" => " φ ".into(),
            "theta" | "vartheta" => " θ ".into(),
            "rho" | "varrho" => " ρ ".into(),
            "le" | "leq" | "leqslant" => " <= ".into(),
            "ge" | "geq" | "geqslant" => " >= ".into(),
            "ne" | "neq" => " != ".into(),
            "lt" => " < ".into(),
            "gt" => " > ".into(),
            "land" | "wedge" => " and ".into(),
            "lor" | "vee" => " or ".into(),
            "lnot" | "neg" => " not ".into(),
            "," | ";" | ":" | "!" | " " | "quad" | "qquad" | "displaystyle" | "textstyle" | "limits" => " ".into(),
            "lvert" | "rvert" | "vert" | "mid" | "|" => "|".into(),
            "lfloor" => " floor(".into(),
            "lceil" => " ceil(".into(),
            "rfloor" | "rceil" => ")".into(),
            "{" | "lbrace" => "(".into(),
            "}" | "rbrace" => ")".into(),
            "mathrm" | "operatorname" | "mathit" | "mathbf" | "textrm" | "text" | "mbox" => {
                let raw = self.raw_group()?;
                let w = raw.trim();
                if name == "text" || name == "textrm" || name == "mbox" {
                    return Ok(self.text_words(w));
                }
                if let Some(f) = latex_fn(w).or_else(|| super::parse::function(w).map(|f| f.name)) {
                    return self.apply(f);
                }
                if w == "e" {
                    " e ".into()
                } else {
                    self.word(w)?
                }
            }
            "begin" => {
                let env = self.raw_group()?;
                match env.trim() {
                    "cases" | "dcases" => self.cases()?,
                    e => return Err(self.err(&format!("{} {e}", tr("unsupported environment", "不支持的环境")))),
                }
            }
            "\\" => return Err(self.err(tr("'\\\\' is only allowed in \\begin{cases}", "'\\\\' 只能用在 \\begin{cases} 中"))),
            "" => return Err(self.err(tr("lone backslash", "单独的反斜杠"))),
            n => return Err(self.err(&format!("{} \\{n}", tr("unknown command", "未知命令")))),
        })
    }

    /// Words inside `\text{...}`: "if", "otherwise", "and", "or".
    fn text_words(&mut self, w: &str) -> String {
        let mut out = String::new();
        for word in w.split(|c: char| c.is_whitespace() || c == ',') {
            let word = word.trim_matches(|c: char| matches!(c, '.' | ';' | ':' | '。' | '，' | '；' | '：'));
            match word.to_lowercase().as_str() {
                "" | "if" | "for" | "when" | "where" | "当" | "若" | "如果" => {}
                "and" | "且" | "并且" => out.push_str(" and "),
                "or" | "或" => out.push_str(" or "),
                "otherwise" | "else" | "否则" | "其他" | "其它" => self.otherwise = true,
                other => out.push_str(&format!(" {other} ")),
            }
        }
        out
    }

    /// Apply a function command: modifiers (`^2`, `^{-1}`, `_b`), then the
    /// argument in parentheses, braces, or implicit (`\sin 2x`).
    fn apply(&mut self, name: &'static str) -> Result<String, ParseError> {
        let mut name = name;
        let mut power: Option<String> = None;
        let mut base: Option<String> = None;
        loop {
            self.skip_space();
            match self.peek() {
                Some(T::Ch('^')) => {
                    self.i += 1;
                    power = Some(self.arg()?);
                }
                Some(T::Ch('_')) => {
                    self.i += 1;
                    base = Some(self.arg()?);
                }
                _ => break,
            }
        }
        if power.as_deref().map(str::trim) == Some("-1")
            && let Some(inv) = inverse(name)
        {
            name = inv;
            power = None;
        }
        self.skip_space();
        let arg = match self.peek() {
            _ if self.bar_next() => self.abs_arg()?,
            Some(T::Ch('(')) | Some(T::Cmd(_)) if self.paren_next() => self.paren_arg()?,
            Some(T::Open) => format!("({})", self.group()?),
            _ => format!("({})", self.implicit_arg(name)?),
        };
        let core = match base {
            Some(b) if name == "log" => format!("log({b}, {})", &arg[1..arg.len() - 1]),
            Some(_) => return Err(self.err(&format!("{} {name}", tr("subscript not supported on", "不支持下标:")))),
            None => format!("{name}{arg}"),
        };
        Ok(match power {
            Some(p) => format!(" ({core})^({p}) "),
            None => format!(" {core} "),
        })
    }

    /// The token after a sizing command such as `\left` at `i`, if any.
    fn after_sizing(&self, i: usize) -> Option<&T> {
        match self.t.get(i) {
            Some(T::Cmd(c)) if matches!(c.as_str(), "left" | "right" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" | "big" | "Big") => {
                let mut j = i + 1;
                while self.t.get(j) == Some(&T::Space) {
                    j += 1;
                }
                self.t.get(j)
            }
            t => t,
        }
    }

    /// Whether the next token opens a parenthesis: `(`, `\{`, `\left(` …
    fn paren_next(&self) -> bool {
        let opens = |t: Option<&T>| matches!(t, Some(T::Ch('('))) || matches!(t, Some(T::Cmd(c)) if c == "{" || c == "lbrace");
        opens(self.after_sizing(self.i))
    }

    /// Whether the token at `i` is a `|` (also `\left|`, `\lvert`, `\vert`).
    fn bar_at(&self, i: usize) -> bool {
        let bar = |t: Option<&T>| matches!(t, Some(T::Ch('|'))) || matches!(t, Some(T::Cmd(c)) if matches!(c.as_str(), "lvert" | "rvert" | "vert" | "|"));
        bar(self.after_sizing(i))
    }

    fn bar_next(&self) -> bool {
        self.bar_at(self.i)
    }

    /// `|…|` as a function argument (`\ln|x|`) → `(abs(…))`.
    fn abs_arg(&mut self) -> Result<String, ParseError> {
        self.item()?;
        let mut inner = String::new();
        loop {
            if self.peek().is_none() {
                return Err(self.err(tr("missing closing '|'", "缺少右边的 '|'")));
            }
            if self.bar_next() {
                self.item()?;
                return Ok(format!("(abs({}))", inner.trim()));
            }
            inner.push_str(&self.item()?);
        }
    }

    /// A parenthesised argument, returned with its parentheses.
    fn paren_arg(&mut self) -> Result<String, ParseError> {
        let mut out = String::new();
        let mut depth = 0i32;
        loop {
            if self.peek().is_none() {
                return Err(self.err(tr("missing ')'", "缺少 ')'")));
            }
            let piece = self.item()?;
            for c in piece.chars() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            out.push_str(&piece);
            if depth <= 0 && !out.trim().is_empty() {
                return Ok(out.trim().to_string());
            }
        }
    }

    /// `\sin 2x` → `2x`: letters, digits, constants, groups and powers up to
    /// the next space, operator or function.
    fn implicit_arg(&mut self, name: &str) -> Result<String, ParseError> {
        let mut out = String::new();
        loop {
            let take = match self.peek() {
                Some(T::Ch(c)) => c.is_alphanumeric() || *c == '.' || (*c == '^' && !out.is_empty()),
                Some(T::Open) => true,
                Some(T::Cmd(c)) => matches!(
                    c.as_str(),
                    "pi" | "tau" | "frac" | "dfrac" | "tfrac" | "sqrt" | "mathrm" | "theta" | "vartheta" | "phi" | "varphi" | "alpha" | "beta" | "omega" | "rho" | "lambda" | "mu"
                ),
                _ => false,
            };
            if !take {
                break;
            }
            // A letter run that names a function ends the argument: \sin x cos y.
            if let Some(T::Ch(c)) = self.peek()
                && c.is_alphabetic()
            {
                let mut j = self.i;
                let mut w = String::new();
                while let Some(T::Ch(d)) = self.t.get(j)
                    && d.is_alphabetic()
                {
                    w.push(*d);
                    j += 1;
                }
                if latex_fn(&w).is_some() || super::parse::function(&w).is_some() {
                    break;
                }
            }
            out.push_str(&self.item()?);
        }
        if out.trim().is_empty() {
            return Err(self.err(&format!("\\{name} {}", tr("needs an argument", "缺少参数"))));
        }
        Ok(out)
    }

    /// A cases condition comparing points, `(x,y) \neq (0,0)` →
    /// `((x)!=(0) or (y)!=(0))`; `None` (nothing consumed) for any other.
    fn tuple_condition(&mut self) -> Result<Option<String>, ParseError> {
        let end = (self.i..self.t.len()).find(|&j| matches!(&self.t[j], T::Cmd(c) if c == "\\" || c == "end")).unwrap_or(self.t.len());
        let sizing = |c: &str| matches!(c, "left" | "right" | "bigl" | "bigr" | "Bigl" | "Bigr" | "big" | "Big");
        let mut toks: Vec<T> = self.t[self.i..end].iter().filter(|t| !matches!(t, T::Space) && !matches!(t, T::Cmd(c) if sizing(c))).cloned().collect();
        // A leading \text{if } / \text{当 }.
        if matches!(toks.first(), Some(T::Cmd(c)) if c == "text" || c == "textrm" || c == "mbox")
            && let Some(close) = toks.iter().position(|t| *t == T::Close)
        {
            let words: String = toks[..close].iter().filter_map(|t| if let T::Ch(c) = t { Some(*c) } else { None }).collect();
            if matches!(words.trim(), "if" | "when" | "for" | "当" | "若" | "如果") {
                toks.drain(..=close);
            }
        }
        while matches!(toks.last(), Some(T::Ch(',' | '.' | ';'))) {
            toks.pop();
        }
        // ( a , b , … ) op ( c , d , … )
        fn tuple(toks: &[T], k: &mut usize) -> Option<Vec<Vec<T>>> {
            if toks.get(*k) != Some(&T::Ch('(')) {
                return None;
            }
            *k += 1;
            let mut parts = vec![Vec::new()];
            loop {
                match toks.get(*k)? {
                    T::Ch(')') => break,
                    T::Ch(',') => parts.push(Vec::new()),
                    T::Ch('(') => return None,
                    t => parts.last_mut()?.push(t.clone()),
                }
                *k += 1;
            }
            *k += 1;
            (parts.len() >= 2 && parts.iter().all(|p| !p.is_empty())).then_some(parts)
        }
        let mut k = 0;
        let Some(lhs) = tuple(&toks, &mut k) else { return Ok(None) };
        let equal = match (toks.get(k), toks.get(k + 1)) {
            (Some(T::Ch('=')), _) => {
                k += 1;
                true
            }
            (Some(T::Ch('!')), Some(T::Ch('='))) => {
                k += 2;
                false
            }
            (Some(T::Cmd(c)), _) if c == "ne" || c == "neq" => {
                k += 1;
                false
            }
            _ => return Ok(None),
        };
        let Some(rhs) = tuple(&toks, &mut k) else { return Ok(None) };
        if k != toks.len() || lhs.len() != rhs.len() {
            return Ok(None);
        }
        let conv = |t: &Vec<T>| -> Result<String, ParseError> {
            let mut c = Conv { t: t.clone(), i: 0, cond: false, otherwise: false };
            c.seq(&|_| false)
        };
        let mut terms = Vec::new();
        for (a, b) in lhs.iter().zip(&rhs) {
            let (a, b) = (conv(a)?, conv(b)?);
            terms.push(if equal { format!("({a})==({b})") } else { format!("({a})!=({b})") });
        }
        self.i = end;
        Ok(Some(format!("({})", terms.join(if equal { " and " } else { " or " }))))
    }

    /// Body of `\begin{cases}` up to and including `\end{cases}`.
    fn cases(&mut self) -> Result<String, ParseError> {
        let mut rows: Vec<(String, Option<String>)> = Vec::new();
        loop {
            let stop = |t: &T| matches!(t, T::Ch('&')) || matches!(t, T::Cmd(c) if c == "\\" || c == "end");
            // Rows are often written `value, & condition.`
            let expr = self.seq(&stop)?.trim_end().trim_end_matches([',', ';']).to_string();
            let mut cond = None;
            if self.peek() == Some(&T::Ch('&')) {
                self.i += 1;
                if let Some(c) = self.tuple_condition()? {
                    cond = Some(c);
                } else {
                    self.cond = true;
                    self.otherwise = false;
                    let c = self.seq(&|t| matches!(t, T::Cmd(c) if c == "\\" || c == "end"))?;
                    self.cond = false;
                    let c = c.trim().trim_end_matches(['.', ';']).trim_end();
                    let c = c.strip_suffix("and").unwrap_or(c).trim();
                    if !self.otherwise && !c.is_empty() {
                        cond = Some(c.to_string());
                    }
                }
            }
            if !expr.trim().is_empty() {
                rows.push((expr, cond));
            }
            match self.peek() {
                Some(T::Cmd(c)) if c == "\\" => self.i += 1,
                Some(T::Cmd(c)) if c == "end" => {
                    self.i += 1;
                    self.raw_group()?;
                    break;
                }
                _ => return Err(self.err(tr("missing \\end{cases}", "缺少 \\end{cases}"))),
            }
        }
        if rows.is_empty() {
            return Err(self.err(tr("empty cases", "cases 为空")));
        }
        // Rows without a condition (or "otherwise") are the fallback.
        let mut acc = match rows.last() {
            Some((e, None)) => {
                let e = e.clone();
                rows.pop();
                format!("({e})")
            }
            _ => "(0/0)".to_string(),
        };
        for (e, c) in rows.into_iter().rev() {
            let c = c.ok_or_else(|| self.err(tr("only the last row of cases may omit its condition", "cases 中只有最后一行可以省略条件")))?;
            acc = format!("if({c}, {e}, {acc})");
        }
        Ok(format!(" {acc} "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Pool, Program, Vars, parse};

    fn eval(src: &str, x: f64, y: f64) -> f64 {
        let mut pool = Pool::new();
        let id = parse(src, &mut pool, &Vars(vec!["x", "y"]), false).unwrap_or_else(|e| panic!("{src}: {e}"));
        Program::compile(&pool, &[id]).eval(&[x, y])[0]
    }

    fn same(latex: &str, plain: &str) {
        for (x, y) in [(0.3, -0.7), (1.1, 0.4), (-0.6, 1.3)] {
            let a = eval(latex, x, y);
            let b = eval(plain, x, y);
            assert!((a - b).abs() < 1e-12 || (a.is_nan() && b.is_nan()), "{latex} vs {plain} at ({x},{y}): {a} {b}; read as {:?}", to_plain(latex));
        }
    }

    #[test]
    fn latex_basics() {
        same(r"\frac{x^2}{1+y^2}", "x^2/(1+y^2)");
        same(r"\dfrac12 x", "x/2");
        same(r"e^{-(x^2+y^2)}", "exp(-(x^2+y^2))");
        same(r"\sqrt{x^2+y^2}", "sqrt(x^2+y^2)");
        same(r"\sqrt[3]{x}", "cbrt(x)");
        same(r"\sqrt[4]{x^2+1}", "(x^2+1)^(1/4)");
        same(r"x^{23}", "x^23");
        // LaTeX takes one token after ^ (only applies when read as LaTeX).
        same(r"\, x^23", "x^2*3");
        same(r"\sin(xy) + \cos x \cdot \tan y", "sin(x*y) + cos(x)*tan(y)");
        same(r"\sin x \cos y", "sin(x)*cos(y)");
        same(r"\sin^2 x + \cos^{2}(y)", "sin(x)^2 + cos(y)^2");
        same(r"\sin^{-1} \frac{x}{2}", "asin(x/2)");
        same(r"\ln\left(1 + x^2\right) \times 2", "ln(1+x^2)*2");
        same(r"\log_{2} (x^2+1)", "log2(x^2+1)");
        same(r"\left| x \right| - \lvert y \rvert", "abs(x) - abs(y)");
        same(r"2\pi xy", "2*pi*x*y");
        same(r"\mathrm{e}^{x}", "exp(x)");
        same(r"\operatorname{sgn}(x) \, y", "sign(x)*y");
        same(r"\lfloor x \rfloor + \lceil y \rceil", "floor(x) + ceil(y)");
        same(r"z = \frac{x}{y}", "x/y");
        same(r"$f(x, y) = x^2 + y^2$", "x^2 + y^2");
        same(r"\exp{x y}", "exp(x*y)");
        same(r"\max\left(x, y\right)", "max(x, y)");
        // Region conditions.
        same(r"x^2 + y^2 \le 1", "x^2 + y^2 <= 1");
        same(r"0 \leq x \leq 1 \land |y| < \frac12", "0 <= x <= 1 and abs(y) < 1/2");
    }

    #[test]
    fn latex_cases() {
        same(
            r"\begin{cases} 0 & x^2 + y^2 = 0 \\ \frac{xy(x^2-y^2)}{x^2+y^2} & \text{otherwise} \end{cases}",
            "if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))",
        );
        same(r"\begin{cases} x & \text{if } x \ge 0, y \ge 0 \\ -y & \text{else} \end{cases}", "if(x >= 0 and y >= 0, x, -y)");
        same(r"\begin{cases} 1 & x < 0 \end{cases}", "if(x < 0, 1, 0/0)");
    }

    #[test]
    fn latex_errors() {
        assert!(to_plain(r"\foo{x}").is_err());
        assert!(to_plain(r"\frac{x}{y").is_err());
        assert!(to_plain(r"x_1").is_err());
    }

    #[test]
    fn lhs_strip() {
        assert_eq!(strip_lhs("z = x"), " x");
        assert_eq!(strip_lhs("f(x,y)= x"), " x");
        assert_eq!(strip_lhs("x <= 1"), "x <= 1");
        assert_eq!(strip_lhs("x + y == 1"), "x + y == 1");
    }

    /// Formulas as they come out of the clipboard: MathJax / Wikipedia,
    /// Typora, ChatGPT, Mathpix, Word and rendered (Unicode) maths.
    #[test]
    fn pasted_formulas() {
        // Delimiters, environments, tags, style commands, trailing punctuation.
        same(r"$$z = \frac{\sin\left(\sqrt{x^2+y^2}\right)}{\sqrt{x^2+y^2}}$$", "sin(sqrt(x^2+y^2))/sqrt(x^2+y^2)");
        same("\\[\n  f(x, y) = x^2 - y^2\n\\]", "x^2 - y^2");
        same(r"\( f(x,y)=x^{2}y \)", "x^2*y");
        same("z = x^2 + y^2,", "x^2 + y^2");
        same("z = x^2 + y^2.", "x^2 + y^2");
        same(r"z = \sqrt{1 - x^2 - y^2}\;", "sqrt(1 - x^2 - y^2)");
        same(r"z=x^{2}+y^{2}\quad", "x^2 + y^2");
        same(r"\displaystyle z=\frac{1}{2}xy", "x*y/2");
        same(r"f\left(x,y\right)=x^2y", "x^2*y");
        same(r"z = x^2 + y^2 \tag{1}", "x^2 + y^2");
        same(r"z = x y \label{eq:1} \nonumber", "x*y");
        same(r"\begin{equation} z = x y \end{equation}", "x*y");
        same(r"$$\begin{aligned} z &= x y \end{aligned}$$", "x*y");
        same(r"\left. x^2 \right.", "x^2");
        same("x^2 +\r\n  y", "x^2 + y");
        // Function arguments in bars and braces.
        same(r"\ln\left|x\right| + y", "ln(abs(x)) + y");
        same(r"\ln|x| + y", "ln(abs(x)) + y");
        same(r"\max\{x, y\}", "max(x, y)");
        same(r"\min\left\{x, y\right\}", "min(x, y)");
        // Rendered maths: Unicode operators and superscripts; xy is a product.
        same("x² − y²", "x^2 - y^2");
        same("x^2 − 2xy", "x^2 - 2*x*y");
        same("x⁻¹ + y³ × 2", "x^(-1) + y^3*2");
        same("x\u{2062}y + 2\u{a0}x", "x*y + 2*x");
        same("x² + y² ≤ 1", "x^2 + y^2 <= 1");
        // Cases rows written `value, & condition`, and point conditions.
        let classic = r"f(x,y)=\begin{cases} \frac{xy}{x^2+y^2}, & (x,y)\neq(0,0) \\ 0, & (x,y)=(0,0) \end{cases}";
        same(classic, "x*y/(x^2+y^2)");
        assert_eq!(eval(classic, 0.0, 0.0), 0.0);
        let other = r"\begin{cases} \dfrac{xy}{x^2+y^2}, & \text{if } (x, y) \ne (0, 0), \\ 0, & \text{otherwise.} \end{cases}";
        same(other, "x*y/(x^2+y^2)");
        assert_eq!(eval(other, 0.0, 0.0), 0.0);
        same(r"\begin{cases}1, & x<1\\0, & \text{otherwise}\end{cases}", "if(x < 1, 1, 0)");
        // Complex parts and conjugates.
        assert_eq!(to_plain(r"\bar{z}").unwrap(), "conj( z )");
        assert_eq!(to_plain(r"\overline{z}+1").unwrap(), "conj( z ) +1");
        assert_eq!(to_plain(r"\mathrm{Re}(z)").unwrap(), "re( z )");
        assert_eq!(to_plain(r"\operatorname{Im} z").unwrap(), "im( z )");
        let cz = Vars(vec!["z", "x", "y"]);
        for src in [r"\bar{z}", r"\mathrm{Re}(z)+\operatorname{Im} z", r"\arg z", r"|z|^2"] {
            parse(src, &mut Pool::new(), &cz, true).unwrap_or_else(|e| panic!("{src}: {e}"));
        }
    }

    #[test]
    fn equations_keep_their_left_side() {
        // A pasted cone z = √(x²+y²) must stay an equation, not lose `z =`.
        assert_eq!(to_plain_equation(r"z=\sqrt{x^2+y^2}").unwrap(), "z == sqrt( x ^(2)+ y ^(2))");
        assert_eq!(clean("  $x^{2}+y^{2}=2x$. "), "x^{2}+y^{2}=2x");
        assert_eq!(clean(r"x^2 \right."), r"x^2 \right.");
        assert_eq!(clean("x² + y² = 1"), "x^2 + y^2 = 1");
    }
}

#[cfg(test)]
mod roundtrip {
    use crate::expr::{Pool, Program, Vars, parse};

    /// Plain → LaTeX → plain gives the same function, including derivatives.
    #[test]
    fn latex_roundtrip() {
        let srcs = [
            "3(1-x)^2 exp(-x^2-(y+1)^2) - 10(x/5 - x^3 - y^5) exp(-x^2-y^2) - exp(-(x+1)^2-y^2)/3",
            "if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))",
            "sin(3*sqrt(x^2 + y^2)) / (1 + x^2 + y^2)",
            "atan2(y, x) + |x - y| + cbrt(x) - 2^-x + 1.5e-7*x + ln(2 + cos(y))",
            "max(x, y) - asin(x/3) * tanh(y)",
        ];
        let v = Vars(vec!["x", "y"]);
        for src in srcs {
            let mut pool = Pool::new();
            let f = parse(src, &mut pool, &v, false).unwrap();
            let fx = pool.diff(f, 0);
            for id in [f, fx] {
                let tex = pool.to_latex(id, &["x", "y"]);
                let mut p2 = Pool::new();
                let g = parse(&tex, &mut p2, &v, false).unwrap_or_else(|e| panic!("{tex}: {e}"));
                let a = Program::compile(&pool, &[id]);
                let b = Program::compile(&p2, &[g]);
                for (x, y) in [(0.3, -0.7), (1.1, 0.4), (-0.6, 1.3), (0.0, 0.0)] {
                    let (u, w) = (a.eval(&[x, y])[0], b.eval(&[x, y])[0]);
                    assert!((u - w).abs() <= 1e-9 * u.abs().max(1.0) || (u.is_nan() && w.is_nan()), "{src} -> {tex} at ({x},{y}): {u} vs {w}");
                }
            }
        }
    }
}
