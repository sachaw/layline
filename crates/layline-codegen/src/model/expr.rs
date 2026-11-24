//! Expressions: the one language every value-taking attribute is written in.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// A pure expression over earlier fields, parameters, constants and literals.
///
/// Counts, lengths, strides, presence conditions, switch selectors and seek offsets are all
/// expressions. The grammar, loosest-binding first, is Rust's:
///
/// | Operators | Operands |
/// |---|---|
/// | `\|\|` | conditions |
/// | `&&` | conditions |
/// | `==` `!=` `<` `<=` `>` `>=` | integers, or two `bool`s for `==` and `!=`; not chained |
/// | `\|` | integers |
/// | `^` | integers |
/// | `&` | integers |
/// | `<<` `>>` | integers |
/// | `+` `-` | integers |
/// | `*` `/` `%` | integers |
/// | unary `!` `-` | a `bool`; an integer |
/// | `if c { a } else { b }` | a condition, then two of a kind |
///
/// An operand is an integer literal (`12`, `0x0f`, `0b1010`, `1_000`), a constant (a
/// `SCREAMING_CASE` name or a path with `::`), an earlier field (`n`, or `hdr.n` for a field of the
/// nested layout `hdr`), a parameter, or a parenthesised expression.
///
/// `if c { a } else { b }` is `a` where `c` holds and `b` where it does not, as in Rust: `b` may be
/// another `if`, and the two give the same type. It is an operand, so `if c { 1 } else { 2 } * 4`
/// scales either.
///
/// A condition is a `bool`, or the result of `&`, `|` or `^`, which holds when any bit is set.
/// `!` takes a `bool`: on an integer, Rust would flip its bits rather than test them.
///
/// Evaluation is in `i64` and total. An operation that overflows, divides by zero, or shifts by a
/// count outside `0..64` gives no number, and decode refuses the bytes with `Malformed` rather
/// than wrapping. `<<` is multiplication by a power of two, so a set bit shifted out overflows.
/// `/` and `%` round toward zero. `&&` and `||` short-circuit, and `if` evaluates only the branch it
/// takes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Expr {
    /// An integer literal.
    Int(i64),
    /// A Rust constant, by path: `FLAG_EXT` or `proto::FLAG_EXT`.
    Const(String),
    /// An earlier field, a field of an earlier nested layout (`hdr.n`), or a parameter.
    Name(String),
    /// `!e`, on a `bool`.
    Not(Box<Expr>),
    /// `-e`.
    Neg(Box<Expr>),
    /// `a op b`.
    Binary(Op, Box<Expr>, Box<Expr>),
    /// `if c { a } else { b }`.
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Op {
    /// `*`
    Mul,
    /// `/`, rounding toward zero.
    Div,
    /// `%`, with the sign of the dividend.
    Rem,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `<<`
    Shl,
    /// `>>`, rounding down.
    Shr,
    /// `&`
    BitAnd,
    /// `^`
    BitXor,
    /// `|`
    BitOr,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    And,
    /// `||`
    Or,
}

const OPS: [(&str, Op); 18] = [
    ("<<", Op::Shl),
    (">>", Op::Shr),
    ("<=", Op::Le),
    (">=", Op::Ge),
    ("==", Op::Eq),
    ("!=", Op::Ne),
    ("&&", Op::And),
    ("||", Op::Or),
    ("*", Op::Mul),
    ("/", Op::Div),
    ("%", Op::Rem),
    ("+", Op::Add),
    ("-", Op::Sub),
    ("&", Op::BitAnd),
    ("^", Op::BitXor),
    ("|", Op::BitOr),
    ("<", Op::Lt),
    (">", Op::Gt),
];

impl Op {
    /// The operator as written.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        OPS.iter().find(|(_, op)| *op == self).map_or("?", |(s, _)| s)
    }

    /// Binding strength: higher binds tighter.
    fn precedence(self) -> u8 {
        match self {
            Op::Mul | Op::Div | Op::Rem => 10,
            Op::Add | Op::Sub => 9,
            Op::Shl | Op::Shr => 8,
            Op::BitAnd => 7,
            Op::BitXor => 6,
            Op::BitOr => 5,
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => 4,
            Op::And => 3,
            Op::Or => 2,
        }
    }

    /// Whether the operator compares two operands.
    #[must_use]
    pub fn compares(self) -> bool {
        self.precedence() == 4
    }

    /// Whether the operator joins two conditions: `&&` or `||`.
    #[must_use]
    pub fn joins(self) -> bool {
        matches!(self, Op::And | Op::Or)
    }

    /// Whether the operator works on bits: `&`, `|` or `^`. Its result can be a condition.
    #[must_use]
    pub fn on_bits(self) -> bool {
        matches!(self, Op::BitAnd | Op::BitOr | Op::BitXor)
    }
}

impl Expr {
    /// An integer literal.
    #[must_use]
    pub fn int(value: i64) -> Self {
        Expr::Int(value)
    }

    /// An earlier field (`n`, `hdr.n`) or a parameter.
    #[must_use]
    pub fn name(name: &str) -> Self {
        Expr::Name(String::from(name))
    }

    /// A Rust constant, by path.
    #[must_use]
    pub fn constant(path: &str) -> Self {
        Expr::Const(String::from(path))
    }

    /// `a op b`.
    #[must_use]
    pub fn binary(op: Op, a: Expr, b: Expr) -> Self {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    /// `if c { a } else { b }`.
    #[must_use]
    pub fn if_else(c: Expr, a: Expr, b: Expr) -> Self {
        Expr::If(Box::new(c), Box::new(a), Box::new(b))
    }

    /// Parses an expression, such as `ext_len * 4 - (13 + markings_len)`.
    ///
    /// # Errors
    ///
    /// Why the text is not an expression.
    pub fn parse(text: &str) -> Result<Self, String> {
        let tokens = lex(text)?;
        let mut p = Parser { tokens: &tokens, at: 0 };
        let e = p.expr(0)?;
        match p.tokens.get(p.at) {
            None => Ok(e),
            Some(t) => Err(format!("`{text}`: unexpected `{t}` after `{e}`")),
        }
    }

    /// The bare field or parameter this is, if it is one.
    #[must_use]
    pub fn as_name(&self) -> Option<&str> {
        match self {
            Expr::Name(n) => Some(n),
            _ => None,
        }
    }

    /// The fields and parameters the expression reads, once each, in the order written.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        self.visit(&mut |e| {
            if let Expr::Name(n) = e
                && !out.contains(&n.as_str())
            {
                out.push(n);
            }
        });
        out
    }

    /// Whether the expression reads `name`.
    #[must_use]
    pub fn reads(&self, name: &str) -> bool {
        self.names().contains(&name)
    }

    /// The value, if the expression reads only literals.
    #[must_use]
    pub fn fold(&self) -> Option<i64> {
        use layline_core::__private::value as v;
        match self {
            Expr::Int(n) => Some(*n),
            Expr::Const(_) | Expr::Name(_) | Expr::Not(_) | Expr::If(..) => None,
            Expr::Neg(e) => v::neg(e.fold()),
            Expr::Binary(op, a, b) => {
                let (a, b) = (a.fold(), b.fold());
                match op {
                    Op::Mul => v::mul(a, b),
                    Op::Div => v::div(a, b),
                    Op::Rem => v::rem(a, b),
                    Op::Add => v::add(a, b),
                    Op::Sub => v::sub(a, b),
                    Op::Shl => v::shl(a, b),
                    Op::Shr => v::shr(a, b),
                    Op::BitAnd => v::bit_and(a, b),
                    Op::BitXor => v::bit_xor(a, b),
                    Op::BitOr => v::bit_or(a, b),
                    _ => None,
                }
            }
        }
    }

    fn visit<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        f(self);
        match self {
            Expr::Not(e) | Expr::Neg(e) => e.visit(f),
            Expr::Binary(_, a, b) => {
                a.visit(f);
                b.visit(f);
            }
            Expr::If(c, a, b) => {
                c.visit(f);
                a.visit(f);
                b.visit(f);
            }
            Expr::Int(_) | Expr::Const(_) | Expr::Name(_) => {}
        }
    }

    /// How encode works this expression back to the one field it reads, so that encode can write
    /// that field: `None` unless it is invertible in exactly one field.
    ///
    /// `is_field` tells a field from a parameter. A parameter is stated by the holder, so it is
    /// part of the expression's constant side. The steps take the expression's value to the
    /// field's, outermost first: `n * 4 + 1` is `- 1`, then `/ 4` exactly.
    ///
    /// `if c { a } else { b }` is invertible where `a` and `b` both are, in the same field, and
    /// `c` does not read it: encode tests `c`, then works back through the branch it takes.
    #[cfg_attr(not(feature = "walk"), allow(dead_code))]
    pub(crate) fn inverse(&self, is_field: &dyn Fn(&str) -> bool) -> Option<Inverse<'_>> {
        let fields: Vec<&str> = self.names().into_iter().filter(|n| is_field(n)).collect();
        let [field] = fields[..] else { return None };
        Some(Inverse { field, steps: self.undo(field)? })
    }

    /// The steps from this expression's value back to `field`'s, outermost first.
    fn undo<'a>(&'a self, field: &str) -> Option<Vec<Undo<'a>>> {
        let mut steps = Vec::new();
        let mut e = self;
        loop {
            match e {
                Expr::Name(n) if n == field => return Some(steps),
                Expr::Neg(x) => {
                    steps.push(Undo::Neg);
                    e = x;
                }
                Expr::If(c, a, b) if !c.reads(field) => {
                    steps.push(Undo::Pick(c, a.undo(field)?, b.undo(field)?));
                    return Some(steps);
                }
                Expr::Binary(Op::Mul, a, b) if a == b => {
                    steps.push(Undo::Root);
                    e = a;
                }
                Expr::Binary(op, a, b) => {
                    let (at, other) = match (a.reads(field), b.reads(field)) {
                        (true, false) => (&**a, &**b),
                        (false, true) => (&**b, &**a),
                        _ => return None,
                    };
                    let left = core::ptr::eq(at, &**a);
                    steps.push(match (op, left) {
                        (Op::Add, _) => Undo::Sub(other),
                        (Op::Sub, true) => Undo::Add(other),
                        (Op::Sub, false) => Undo::SubFrom(other),
                        (Op::Mul, _) => Undo::Exactly(other),
                        (Op::Div, true) => Undo::Times(other),
                        (Op::Shl, true) => Undo::Unshift(other),
                        (Op::Shr, true) => Undo::Shift(other),
                        _ => return None,
                    });
                    e = at;
                }
                _ => return None,
            }
        }
    }

    /// The test of one name this condition is, which encode can set or clear for an optional
    /// field: `f` for a `bool`, `f & MASK` for bits with a literal or constant mask, and `!` or
    /// `== 0` for their absence.
    ///
    /// `None` for any other condition, which encode checks instead.
    #[must_use]
    pub fn test(&self) -> Option<Test<'_>> {
        match self {
            Expr::Name(field) => Some(Test { field, mask: None, set: true }),
            Expr::Not(e) => e.test().filter(|t| t.mask.is_none()).map(Test::flipped),
            Expr::Binary(Op::BitAnd, a, b) => match (&**a, &**b) {
                (Expr::Name(field), k) | (k, Expr::Name(field))
                    if k.fold().is_some() || matches!(k, Expr::Const(_)) =>
                {
                    Some(Test { field, mask: Some(k), set: true })
                }
                _ => None,
            },
            Expr::Binary(op @ (Op::Eq | Op::Ne), a, b) => {
                let bits = match (&**a, &**b) {
                    (Expr::Int(0), e) | (e, Expr::Int(0)) => e.test().filter(|t| t.mask.is_some()),
                    _ => None,
                }?;
                Some(if *op == Op::Eq { bits.flipped() } else { bits })
            }
            _ => None,
        }
    }
}

/// What an expression gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    /// An integer.
    Int,
    /// A `bool`.
    Bool,
}

impl core::ops::Not for Expr {
    type Output = Expr;

    /// `!e`.
    fn not(self) -> Expr {
        Expr::Not(Box::new(self))
    }
}

impl core::ops::Neg for Expr {
    type Output = Expr;

    /// `-e`.
    fn neg(self) -> Expr {
        Expr::Neg(Box::new(self))
    }
}

/// One step of working an expression's value back to its field's: see [`Expr::inverse`].
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "walk"), allow(dead_code))]
pub(crate) enum Undo<'a> {
    /// `t - k`, undoing `+ k`.
    Sub(&'a Expr),
    /// `t + k`, undoing `- k`.
    Add(&'a Expr),
    /// `k - t`, undoing `k - f`.
    SubFrom(&'a Expr),
    /// `t / k`, exactly, undoing `* k`.
    Exactly(&'a Expr),
    /// `t * k`, undoing `/ k`.
    Times(&'a Expr),
    /// `t >> k`, exactly, undoing `<< k`.
    Unshift(&'a Expr),
    /// `t << k`, undoing `>> k`.
    Shift(&'a Expr),
    /// `-t`, undoing `-f`.
    Neg,
    /// The square root, exactly, undoing `f * f`.
    Root,
    /// The steps of the branch the condition takes, undoing `if c { a } else { b }`.
    Pick(&'a Expr, Vec<Undo<'a>>, Vec<Undo<'a>>),
}

/// The field an invertible expression reads, and how to work back to it.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "walk"), allow(dead_code))]
pub(crate) struct Inverse<'a> {
    pub(crate) field: &'a str,
    pub(crate) steps: Vec<Undo<'a>>,
}

/// A condition that tests one name: see [`Expr::test`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Test<'a> {
    /// The field or parameter tested.
    pub field: &'a str,
    /// The bits tested, or `None` for a `bool`.
    pub mask: Option<&'a Expr>,
    /// Whether the test holds when the bits are set, or the `bool` is true.
    pub set: bool,
}

impl Test<'_> {
    fn flipped(self) -> Self {
        Self { set: !self.set, ..self }
    }
}

impl fmt::Display for Expr {
    /// The canonical spelling, as `parse` reads it back. `{:#}` leaves out the spaces.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let compact = f.alternate();
        write_expr(f, self, None, compact)
    }
}

fn write_expr(
    f: &mut fmt::Formatter<'_>,
    e: &Expr,
    parent: Option<Op>,
    compact: bool,
) -> fmt::Result {
    match e {
        Expr::Int(n) if *n >= 0 && parent.is_some_and(Op::on_bits) => write!(f, "{n:#x}"),
        Expr::Int(n) if *n < 0 && parent.is_some() => write!(f, "({n})"),
        Expr::Int(n) => write!(f, "{n}"),
        Expr::Const(c) | Expr::Name(c) => f.write_str(c),
        Expr::If(c, a, b) => {
            let (open, close) = if compact { ("{", "}") } else { (" { ", " }") };
            f.write_str("if ")?;
            write_expr(f, c, None, compact)?;
            f.write_str(open)?;
            write_expr(f, a, None, compact)?;
            f.write_str(close)?;
            f.write_str(if compact { "else" } else { " else" })?;
            if let Expr::If(..) = &**b {
                f.write_str(" ")?;
                write_expr(f, b, None, compact)
            } else {
                f.write_str(open)?;
                write_expr(f, b, None, compact)?;
                f.write_str(close)
            }
        }
        Expr::Not(x) | Expr::Neg(x) => {
            f.write_str(if matches!(e, Expr::Not(_)) { "!" } else { "-" })?;
            match &**x {
                Expr::Binary(..) | Expr::Neg(_) | Expr::Int(_) => {
                    f.write_str("(")?;
                    write_expr(f, x, None, compact)?;
                    f.write_str(")")
                }
                _ => write_expr(f, x, None, compact),
            }
        }
        Expr::Binary(op, a, b) => {
            let side = |child: &Expr, right: bool| match child {
                Expr::Binary(inner, ..) => {
                    let (p, q) = (inner.precedence(), op.precedence());
                    p < q || (p == q && (right || op.compares()))
                }
                _ => false,
            };
            let operand = |f: &mut fmt::Formatter<'_>, child: &Expr, right: bool| {
                if side(child, right) {
                    f.write_str("(")?;
                    write_expr(f, child, None, compact)?;
                    f.write_str(")")
                } else {
                    write_expr(f, child, Some(*op), compact)
                }
            };
            operand(f, a, false)?;
            if compact {
                f.write_str(op.spelling())?;
            } else {
                write!(f, " {} ", op.spelling())?;
            }
            operand(f, b, true)
        }
    }
}

/// A token of an expression.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    /// A name, constant or path, with any `.field`.
    Word(String),
    Op(Op),
    Not,
    Open,
    Close,
    /// `if`.
    If,
    /// `else`.
    Else,
    /// `{`.
    Begin,
    /// `}`.
    End,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Int(n) => write!(f, "{n}"),
            Tok::Word(w) => f.write_str(w),
            Tok::Op(op) => f.write_str(op.spelling()),
            Tok::Not => f.write_str("!"),
            Tok::Open => f.write_str("("),
            Tok::Close => f.write_str(")"),
            Tok::If => f.write_str("if"),
            Tok::Else => f.write_str("else"),
            Tok::Begin => f.write_str("{"),
            Tok::End => f.write_str("}"),
        }
    }
}

fn lex(text: &str) -> Result<Vec<Tok>, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    while i < bytes.len() {
        let c = bytes[i];
        let rest = &text[i..];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && ident_char(bytes[i]) {
                i += 1;
            }
            out.push(Tok::Int(literal(&text[start..i])?));
        } else if let Some((k, len)) = keyword(rest) {
            out.push(k);
            i += len;
        } else if ident_char(c) {
            if rest.starts_with("r#") {
                i += 2;
            }
            let mut word = String::new();
            loop {
                let from = i;
                while i < bytes.len() && ident_char(bytes[i]) {
                    i += 1;
                }
                word.push_str(text[from..i].trim_start_matches("r#"));
                let after = text[i..].trim_start();
                let skipped = text.len() - i - after.len();
                let sep = if after.starts_with("::") {
                    "::"
                } else if after.starts_with('.') {
                    "."
                } else {
                    break;
                };
                word.push_str(sep);
                i += skipped + sep.len();
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if text[i..].starts_with("r#") {
                    i += 2;
                }
                if i >= bytes.len() || !ident_char(bytes[i]) {
                    return Err(format!("`{text}`: `{word}` must be followed by a name"));
                }
            }
            out.push(Tok::Word(word));
        } else if c == b'(' {
            out.push(Tok::Open);
            i += 1;
        } else if c == b'{' {
            out.push(Tok::Begin);
            i += 1;
        } else if c == b'}' {
            out.push(Tok::End);
            i += 1;
        } else if c == b')' {
            out.push(Tok::Close);
            i += 1;
        } else if let Some((s, op)) = OPS.iter().find(|(s, _)| rest.starts_with(s)) {
            out.push(Tok::Op(*op));
            i += s.len();
        } else if c == b'!' {
            out.push(Tok::Not);
            i += 1;
        } else {
            let ch = rest.chars().next().unwrap_or('?');
            return Err(format!("`{text}`: `{ch}` is not part of an expression"));
        }
    }
    Ok(out)
}

/// `if` or `else` at the start of `rest`, as a whole word, and its length. `r#if` is a name.
fn keyword(rest: &str) -> Option<(Tok, usize)> {
    let (word, tok) =
        [("if", Tok::If), ("else", Tok::Else)].into_iter().find(|(w, _)| rest.starts_with(w))?;
    let after = rest.as_bytes().get(word.len());
    (!after.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')).then_some((tok, word.len()))
}

/// An integer literal, with Rust's prefixes, `_` separators and type suffixes.
///
/// A hexadecimal, octal or binary literal may run to `u64::MAX`, read as the `i64` of the same
/// bits, so a mask can name the top bit.
fn literal(text: &str) -> Result<i64, String> {
    let (radix, digits) = match text.get(..2) {
        Some("0x" | "0X") => (16, &text[2..]),
        Some("0o" | "0O") => (8, &text[2..]),
        Some("0b" | "0B") => (2, &text[2..]),
        _ => (10, text),
    };
    let suffixes = ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "usize", "isize"];
    let digits = suffixes
        .iter()
        .find_map(|s| digits.strip_suffix(s).filter(|_| radix != 16 || s.starts_with('u')))
        .unwrap_or(digits);
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    let bad = || format!("`{text}` is not an integer literal that fits 64 bits");
    if radix == 10 {
        clean.parse::<i64>().map_err(|_| bad())
    } else {
        u64::from_str_radix(&clean, radix).map(|n| n as i64).map_err(|_| bad())
    }
}

struct Parser<'t> {
    tokens: &'t [Tok],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.at)
    }

    /// Precedence climbing: operators that bind at least as tightly as `min`.
    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let mut left = self.unary()?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            if op.precedence() < min {
                break;
            }
            self.at += 1;
            let right = self.expr(op.precedence() + 1)?;
            if op.compares()
                && let Some(Tok::Op(next)) = self.peek()
                && next.compares()
            {
                return Err(format!(
                    "`{left} {} {right} {} ..` chains comparisons. Join them with `&&`",
                    op.spelling(),
                    next.spelling()
                ));
            }
            left = Expr::binary(op, left, right);
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        let Some(t) = self.peek().cloned() else {
            return Err(String::from("an expression ends where an operand should be"));
        };
        self.at += 1;
        match t {
            Tok::Int(n) => Ok(Expr::Int(n)),
            Tok::Word(w) => word(w),
            Tok::Not => Ok(!self.unary()?),
            Tok::Op(Op::Sub) => match self.unary()? {
                Expr::Int(n) => Ok(Expr::Int(n.wrapping_neg())),
                e => Ok(-e),
            },
            Tok::Open => {
                let e = self.expr(0)?;
                match self.peek() {
                    Some(Tok::Close) => {
                        self.at += 1;
                        Ok(e)
                    }
                    _ => Err(format!("`({e}` is missing its `)`")),
                }
            }
            Tok::If => {
                let c = self.expr(0)?;
                let a = self.block(&c)?;
                if self.peek() != Some(&Tok::Else) {
                    return Err(format!(
                        "`if {c} {{ {a} }}` has no `else`. An expression has a value either way"
                    ));
                }
                self.at += 1;
                let b = if self.peek() == Some(&Tok::If) { self.unary()? } else { self.block(&c)? };
                Ok(Expr::if_else(c, a, b))
            }
            other => Err(format!("`{other}` where an operand should be")),
        }
    }
}

impl Parser<'_> {
    /// `{ e }`, a branch of `if c`.
    fn block(&mut self, c: &Expr) -> Result<Expr, String> {
        if self.peek() != Some(&Tok::Begin) {
            return Err(format!("`if {c}` needs `{{` before its branch"));
        }
        self.at += 1;
        let e = self.expr(0)?;
        if self.peek() != Some(&Tok::End) {
            return Err(format!("`{{ {e}` is missing its `}}`"));
        }
        self.at += 1;
        Ok(e)
    }
}

/// A name: a constant if it is a path or `SCREAMING_CASE`, else a field or parameter.
fn word(w: String) -> Result<Expr, String> {
    if w.contains("::") {
        return Ok(Expr::Const(w));
    }
    if w.split('.').count() > 2 {
        let reach: Vec<&str> = w.split('.').take(2).collect();
        return Err(format!(
            "`{}.…` reaches more than one level deep. \
             Name a field of this struct, or a field of one of its nested layouts",
            reach.join(".")
        ));
    }
    let screaming = w.chars().any(|c| c.is_ascii_uppercase())
        && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    Ok(if screaming { Expr::Const(w) } else { Expr::Name(w) })
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    fn p(s: &str) -> Expr {
        Expr::parse(s).unwrap()
    }

    #[test]
    fn precedence_is_rusts() {
        assert_eq!(p("flags & 4 == 0").to_string(), "flags & 0x4 == 0");
        assert_eq!(p("a + b * c").to_string(), "a + b * c");
        assert_eq!(p("(a + b) * c").to_string(), "(a + b) * c");
        assert_eq!(p("a - (b - c)").to_string(), "a - (b - c)");
        assert_eq!(p("a - b - c"), p("(a - b) - c"));
        assert_eq!(p("x && y || z"), p("(x && y) || z"));
        assert_eq!(
            p("ext_len*4 - (13+markings_len)").to_string(),
            "ext_len * 4 - (13 + markings_len)"
        );
        assert_eq!(format!("{:#}", p("has_ext && kind != 3")), "has_ext&&kind!=3");
    }

    #[test]
    fn names_constants_and_literals() {
        assert_eq!(p("hdr.n * 2").names(), ["hdr.n"]);
        assert_eq!(p("r#type + 1").names(), ["type"]);
        assert_eq!(
            p("flags & FLAG_EXT"),
            Expr::binary(Op::BitAnd, Expr::name("flags"), Expr::constant("FLAG_EXT"))
        );
        assert_eq!(p("proto::K"), Expr::constant("proto::K"));
        assert_eq!(p("0xffff_ffff_ffff_ffff"), Expr::Int(-1));
        assert_eq!(p("0x10u8"), Expr::Int(16));
        assert_eq!(p("-5"), Expr::Int(-5));
        assert!(Expr::parse("a.b.c").is_err());
        assert!(Expr::parse("a < b < c").is_err());
        assert!(Expr::parse("(a").is_err());
        assert!(Expr::parse("a $ b").is_err());
        assert!(Expr::parse("99999999999999999999").is_err());
    }

    #[test]
    fn every_spelling_reads_back() {
        for s in
            ["-(a + 1)", "!(a && b)", "a % 3 << 2", "a | b ^ c & d", "a * (-3)", "!x", "-a * b"]
        {
            let e = p(s);
            assert_eq!(p(&e.to_string()), e, "{s} -> {e}");
            assert_eq!(p(&format!("{e:#}")), e, "{s} -> {e:#}");
        }
    }

    #[test]
    fn one_field_works_back() {
        let field = |n: &str| n != "p";
        let steps = |s: &str| {
            let e = p(s);
            let inv = e.inverse(&field).unwrap();
            assert_eq!(inv.field, "n");
            format!("{:?}", inv.steps.iter().map(core::mem::discriminant).collect::<Vec<_>>())
        };
        let like = |steps: &[Undo<'_>]| {
            format!("{:?}", steps.iter().map(core::mem::discriminant).collect::<Vec<_>>())
        };
        let k = Expr::int(1);
        assert_eq!(steps("n * 4 + 1"), like(&[Undo::Sub(&k), Undo::Exactly(&k)]));
        assert_eq!(steps("n * n"), like(&[Undo::Root]));
        assert_eq!(steps("n * p"), like(&[Undo::Exactly(&k)]));
        assert_eq!(steps("10 - n"), like(&[Undo::SubFrom(&k)]));
        assert!(p("a * b").inverse(&field).is_none());
        assert!(p("n % 4").inverse(&field).is_none());
        assert!(p("n + n").inverse(&field).is_none());
        assert!(p("p * 2").inverse(&field).is_none());
    }

    #[test]
    fn a_conditional_is_an_operand_with_rusts_spelling() {
        let e = p("if marker || pt == 127 { DESCRIBED } else { pt }");
        assert_eq!(
            e,
            Expr::if_else(p("marker || pt == 127"), Expr::constant("DESCRIBED"), Expr::name("pt"))
        );
        assert_eq!(e.names(), ["marker", "pt"]);
        assert_eq!(p("if a { 1 } else { 2 } * 4"), p("(if a { 1 } else { 2 }) * 4"));
        assert_eq!(p("r#if + 1").names(), ["if"]);
        assert_eq!(p("iffy + elsewhere").names(), ["iffy", "elsewhere"]);
        for s in [
            "if a { n } else { n * 2 }",
            "if a { 1 } else if b { 2 } else { 3 }",
            "-if a && !b { x } else { y } + 1",
            "if if a { b } else { c } { 1 } else { 2 }",
        ] {
            let e = p(s);
            assert_eq!(e.to_string(), s);
            assert_eq!(p(&format!("{e:#}")), e, "{e:#}");
        }
        assert!(Expr::parse("if a { 1 }").is_err(), "no else");
        assert!(Expr::parse("if a 1 else 2").is_err(), "no braces");
        assert!(Expr::parse("if a { 1 else { 2 }").is_err(), "unclosed");
        assert!(Expr::parse("a ? 1 : 2").is_err(), "C's spelling");
    }

    #[test]
    fn a_conditional_works_back_through_the_branch_it_takes() {
        let field = |n: &str| n != "p";
        let e = p("if p { n * 2 } else { n } + 1");
        let inv = e.inverse(&field).unwrap();
        assert_eq!(inv.field, "n");
        let [Undo::Sub(_), Undo::Pick(c, then, otherwise)] = &inv.steps[..] else {
            panic!("{:?}", inv.steps)
        };
        assert_eq!(**c, Expr::name("p"));
        assert!(matches!(then[..], [Undo::Exactly(_)]));
        assert!(otherwise.is_empty());
        assert!(p("if p { n } else { 0 }").inverse(&field).is_none(), "one branch reads no field");
        assert!(p("if n > 1 { n } else { n * 2 }").inverse(&field).is_none(), "the test reads it");
        assert!(p("if p { n } else { m }").inverse(&field).is_none(), "two fields");
        assert!(p("if m > 0 { n } else { n }").inverse(&field).is_none(), "the test reads a field");
        assert!(p("if p { n } else { 1 }").test().is_none());
        assert_eq!(p("if p { 1 } else { 2 }").fold(), None);
    }

    #[test]
    fn a_test_of_one_field() {
        let cleared = p("flags & 0x4 == 0");
        let t = cleared.test().unwrap();
        assert_eq!((t.field, t.set), ("flags", false));
        assert_eq!(t.mask.and_then(Expr::fold), Some(4));
        let off = p("!on");
        let t = off.test().unwrap();
        assert_eq!((t.field, t.mask, t.set), ("on", None, false));
        assert!(p("!(f & 1)").test().is_none());
        assert!(p("a && b").test().is_none());
        assert!(p("n > 0").test().is_none());
    }
}
