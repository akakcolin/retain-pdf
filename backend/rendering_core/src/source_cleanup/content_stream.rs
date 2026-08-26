//! PDF content-stream tokenizer/serializer (pure std, zero dependencies).
//!
//! Mirrors the `(operands, operator)` instruction stream that pikepdf's
//! `parse_content_stream` / `unparse_content_stream` produce, so the
//! `source_cleanup` decision state machine can be driven over the same token
//! shape in Rust. Strings are preserved as raw bytes (`Operand::Bytes`) so a
//! rewritten stream round-trips losslessly (CJK UTF-16BE text is not mangled).
//!
//! The serializer emits a *valid* stream, not qpdf-identical bytes: the
//! differential verifies semantic equivalence (rendered pixels / extracted
//! text), so number formatting and spacing are free to differ.

use super::pdf_math::Operand;

/// A single content-stream instruction: `(operands, operator)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentToken {
    pub operands: Vec<Operand>,
    pub operator: String,
}

/// A lexical error in the content stream.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, LexError> {
    Err(LexError(msg.into()))
}

fn is_whitespace(b: u8) -> bool {
    matches!(b, b'\x00' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn is_delimiter(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// Tokenize a PDF content stream into `(operands, operator)` instructions.
pub fn tokenize(content: &[u8]) -> Result<Vec<ContentToken>, LexError> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let n = content.len();
    while i < n {
        let b = content[i];
        if is_whitespace(b) {
            i += 1;
            continue;
        }
        if b == b'%' {
            // Comment: skip to end of line.
            while i < n && !matches!(content[i], b'\n' | b'\r') {
                i += 1;
            }
            continue;
        }
        match b {
            b'(' => {
                let (bytes, next) = scan_literal_string(content, i)?;
                i = next;
                push_operand(&mut tokens, Operand::Bytes(bytes));
            }
            b'<' => {
                if i + 1 < n && content[i + 1] == b'<' {
                    return err("unexpected dictionary << in content stream");
                }
                let (bytes, next) = scan_hex_string(content, i)?;
                i = next;
                push_operand(&mut tokens, Operand::Bytes(bytes));
            }
            b'[' => {
                let (items, next) = scan_array(content, i)?;
                i = next;
                push_operand(&mut tokens, Operand::Array(items));
            }
            b'/' => {
                let (name, next) = scan_name(content, i);
                i = next;
                push_operand(&mut tokens, Operand::Name(name));
            }
            b'{' | b'}' | b']' | b')' | b'>' => {
                return err(format!("unexpected delimiter `{}`", b as char));
            }
            _ => {
                // Maximal run of regular characters: a number or an operator.
                let start = i;
                while i < n && !is_whitespace(content[i]) && !is_delimiter(content[i]) {
                    i += 1;
                }
                let word = &content[start..i];
                if !word.is_empty() {
                    match std::str::from_utf8(word) {
                        Ok(s) => match s.parse::<f64>() {
                            Ok(v) => push_operand(&mut tokens, Operand::Num(v)),
                            Err(_) => flush_operator(&mut tokens, s.to_string()),
                        },
                        Err(_) => flush_operator(&mut tokens, format!("{:?}", word)),
                    }
                }
            }
        }
    }
    flush_operator(&mut tokens, String::new());
    Ok(tokens)
}

/// Push an operand onto the current instruction; if a previous instruction has
/// no operator yet, start a fresh one.
fn push_operand(tokens: &mut Vec<ContentToken>, operand: Operand) {
    if let Some(last) = tokens.last_mut() {
        if last.operator.is_empty() {
            last.operands.push(operand);
            return;
        }
    }
    tokens.push(ContentToken {
        operands: vec![operand],
        operator: String::new(),
    });
}

/// Finalize the current instruction (if any) with `op`. An empty `op` with no
/// pending operands is a no-op.
fn flush_operator(tokens: &mut Vec<ContentToken>, op: String) {
    if let Some(last) = tokens.last_mut() {
        if last.operator.is_empty() {
            last.operator = op;
            return;
        }
    }
    if !op.is_empty() {
        tokens.push(ContentToken {
            operands: Vec::new(),
            operator: op,
        });
    }
}

/// Scan a literal string `(...)` starting at `content[i] == b'('`.
/// Returns the decoded bytes and the index just past the closing `)`.
fn scan_literal_string(content: &[u8], i: usize) -> Result<(Vec<u8>, usize), LexError> {
    let mut out = Vec::new();
    let mut j = i + 1;
    let n = content.len();
    let mut depth = 1usize;
    loop {
        if j >= n {
            return err("unterminated literal string");
        }
        let b = content[j];
        match b {
            b'\\' => {
                j += 1;
                if j >= n {
                    return err("unterminated literal string escape");
                }
                let e = content[j];
                j += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'(' => out.push(b'('),
                    b')' => out.push(b')'),
                    b'\\' => out.push(b'\\'),
                    b'0'..=b'7' => {
                        let mut val = (e - b'0') as u32;
                        let mut k = 0;
                        while k < 2 && j < n && matches!(content[j], b'0'..=b'7') {
                            val = val * 8 + (content[j] - b'0') as u32;
                            j += 1;
                            k += 1;
                        }
                        out.push((val & 0xff) as u8);
                    }
                    b'\r' => {
                        if j < n && content[j] == b'\n' {
                            j += 1;
                        }
                        out.push(b'\n');
                    }
                    b'\n' => out.push(b'\n'),
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
                j += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((out, j + 1));
                }
                out.push(b')');
                j += 1;
            }
            b'\r' => {
                if j + 1 < n && content[j + 1] == b'\n' {
                    j += 1;
                }
                out.push(b'\n');
                j += 1;
            }
            _ => {
                out.push(b);
                j += 1;
            }
        }
    }
}

/// Scan a hex string `<...>` starting at `content[i] == b'<'`.
fn scan_hex_string(content: &[u8], i: usize) -> Result<(Vec<u8>, usize), LexError> {
    let mut out = Vec::new();
    let mut j = i + 1;
    let n = content.len();
    let mut hi: Option<u8> = None;
    loop {
        if j >= n {
            return err("unterminated hex string");
        }
        let b = content[j];
        if b == b'>' {
            if let Some(h) = hi {
                out.push(h << 4);
            }
            return Ok((out, j + 1));
        }
        if is_whitespace(b) {
            j += 1;
            continue;
        }
        let v = hex_val(b).ok_or_else(|| LexError(format!("invalid hex digit `{}`", b as char)))?;
        match hi {
            None => hi = Some(v),
            Some(h) => {
                out.push((h << 4) | v);
                hi = None;
            }
        }
        j += 1;
    }
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Scan a name `/...` starting at `content[i] == b'/'`. Returns the decoded
/// (lossy UTF-8) name text and the index past the name.
fn scan_name(content: &[u8], i: usize) -> (String, usize) {
    let mut out = Vec::new();
    let mut j = i + 1;
    let n = content.len();
    while j < n && !is_whitespace(content[j]) && !is_delimiter(content[j]) {
        if content[j] == b'#' && j + 2 < n {
            if let (Some(h), Some(l)) = (hex_val(content[j + 1]), hex_val(content[j + 2])) {
                out.push((h << 4) | l);
                j += 3;
                continue;
            }
        }
        out.push(content[j]);
        j += 1;
    }
    (String::from_utf8_lossy(&out).into_owned(), j)
}

/// Scan an array `[...]` starting at `content[i] == b'['`. Nested arrays are
/// supported; the array element scan shares the operand-token rules (numbers,
/// names, strings, nested arrays, operators-as-elements are not expected but
/// tolerated as no-ops).
fn scan_array(content: &[u8], i: usize) -> Result<(Vec<Operand>, usize), LexError> {
    let mut items = Vec::new();
    let mut j = i + 1;
    let n = content.len();
    loop {
        while j < n && is_whitespace(content[j]) {
            j += 1;
        }
        if j >= n {
            return err("unterminated array");
        }
        let b = content[j];
        match b {
            b']' => return Ok((items, j + 1)),
            b'%' => {
                while j < n && !matches!(content[j], b'\n' | b'\r') {
                    j += 1;
                }
            }
            b'[' => {
                let (inner, next) = scan_array(content, j)?;
                items.push(Operand::Array(inner));
                j = next;
            }
            b'(' => {
                let (bytes, next) = scan_literal_string(content, j)?;
                items.push(Operand::Bytes(bytes));
                j = next;
            }
            b'<' => {
                if j + 1 < n && content[j + 1] == b'<' {
                    return err("unexpected dictionary << in content stream array");
                }
                let (bytes, next) = scan_hex_string(content, j)?;
                items.push(Operand::Bytes(bytes));
                j = next;
            }
            b'/' => {
                let (name, next) = scan_name(content, j);
                items.push(Operand::Name(name));
                j = next;
            }
            b'{' | b'}' | b')' | b'>' => {
                return err(format!("unexpected delimiter `{}` in array", b as char));
            }
            _ => {
                let start = j;
                while j < n && !is_whitespace(content[j]) && !is_delimiter(content[j]) {
                    j += 1;
                }
                let word = &content[start..j];
                if !word.is_empty() {
                    if let Ok(s) = std::str::from_utf8(word) {
                        if let Ok(v) = s.parse::<f64>() {
                            items.push(Operand::Num(v));
                        }
                    }
                }
            }
        }
    }
}

/// Serialize instructions back into a content stream.
pub fn serialize(tokens: &[ContentToken]) -> Vec<u8> {
    let mut out = Vec::new();
    for token in tokens {
        for operand in &token.operands {
            write_operand(&mut out, operand);
            out.push(b' ');
        }
        out.extend_from_slice(token.operator.as_bytes());
        out.push(b'\n');
    }
    out
}

fn write_operand(out: &mut Vec<u8>, operand: &Operand) {
    match operand {
        Operand::Num(v) => {
            // Rust's {} prints integers without a decimal point, which is a
            // valid PDF number; re-format large exponents for safety.
            out.extend_from_slice(format!("{}", v).as_bytes());
        }
        Operand::Str(s) => {
            out.push(b'(');
            for b in s.bytes() {
                escape_byte(out, b);
            }
            out.push(b')');
        }
        Operand::Bytes(bytes) => {
            out.push(b'(');
            for &b in bytes {
                escape_byte(out, b);
            }
            out.push(b')');
        }
        Operand::Name(name) => {
            out.push(b'/');
            for &b in name.as_bytes() {
                if is_whitespace(b) || is_delimiter(b) || b == b'#' {
                    out.push(b'#');
                    out.push(hex_digit(b >> 4));
                    out.push(hex_digit(b & 0x0f));
                } else {
                    out.push(b);
                }
            }
        }
        Operand::Array(items) => {
            out.push(b'[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(b' ');
                }
                write_operand(out, item);
            }
            out.push(b']');
        }
    }
}

fn hex_digit(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'A' + (v - 10)
    }
}

/// Escape a byte inside a literal string: backslash, parens, control bytes
/// (octal), and CR/LF/TAB as short escapes.
fn escape_byte(out: &mut Vec<u8>, b: u8) {
    match b {
        b'\\' => out.extend_from_slice(b"\\\\"),
        b'(' => out.extend_from_slice(b"\\("),
        b')' => out.extend_from_slice(b"\\)"),
        b'\n' => out.extend_from_slice(b"\\n"),
        b'\r' => out.extend_from_slice(b"\\r"),
        b'\t' => out.extend_from_slice(b"\\t"),
        b'\x08' => out.extend_from_slice(b"\\b"),
        b'\x0c' => out.extend_from_slice(b"\\f"),
        0x00..=0x1f | 0x7f => {
            out.push(b'\\');
            out.push(b'0' + (b >> 6));
            out.push(b'0' + ((b >> 3) & 0o7));
            out.push(b'0' + (b & 0o7));
        }
        _ => out.push(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(stream: &str) {
        let tokens = tokenize(stream.as_bytes()).expect("tokenize");
        let out = serialize(&tokens);
        let tokens2 = tokenize(&out).expect("tokenize output");
        assert_eq!(tokens, tokens2, "token stream not stable for {stream:?}");
    }

    #[test]
    fn tokenizes_basic_operators() {
        let tokens = tokenize(b"q 1 0 0 1 50 60 cm Q").unwrap();
        assert_eq!(
            tokens,
            vec![
                ContentToken { operands: vec![], operator: "q".into() },
                ContentToken {
                    operands: vec![
                        Operand::Num(1.0),
                        Operand::Num(0.0),
                        Operand::Num(0.0),
                        Operand::Num(1.0),
                        Operand::Num(50.0),
                        Operand::Num(60.0)
                    ],
                    operator: "cm".into()
                },
                ContentToken { operands: vec![], operator: "Q".into() },
            ]
        );
    }

    #[test]
    fn tokenizes_text_shows_with_bytes() {
        let tokens =
            tokenize(b"BT /F1 14 Tf (Hello) Tj [(A) -2 (BC) 3] TJ ET").unwrap();
        assert_eq!(tokens.len(), 5);
        assert_eq!(
            tokens[2],
            ContentToken {
                operands: vec![Operand::Bytes(b"Hello".to_vec())],
                operator: "Tj".into()
            }
        );
        assert_eq!(
            tokens[3].operands,
            vec![Operand::Array(vec![
                Operand::Bytes(b"A".to_vec()),
                Operand::Num(-2.0),
                Operand::Bytes(b"BC".to_vec()),
                Operand::Num(3.0)
            ])]
        );
    }

    #[test]
    fn preserves_high_bytes_in_strings() {
        // UTF-16BE CJK bytes must round-trip byte-exact (no UTF-8 mangling).
        let cjk = "\u{4e2d}\u{6587}".encode_utf16().flat_map(|u| u.to_be_bytes());
        let bytes: Vec<u8> = cjk.collect();
        let mut stream = b"BT ".to_vec();
        stream.push(b'(');
        stream.extend_from_slice(&bytes);
        stream.extend_from_slice(b") Tj ET");
        let tokens = tokenize(&stream).unwrap();
        assert_eq!(tokens[1].operands, vec![Operand::Bytes(bytes.clone())]);
        let out = serialize(&tokens);
        let tokens2 = tokenize(&out).unwrap();
        assert_eq!(tokens2[1].operands, vec![Operand::Bytes(bytes)]);
    }

    #[test]
    fn handles_escapes_and_hex() {
        let tokens = tokenize(b"(a\\(b\\)c\\n) Tj <48656c6c6f> Tj").unwrap();
        assert_eq!(tokens[0].operands, vec![Operand::Bytes(b"a(b)c\n".to_vec())]);
        assert_eq!(tokens[1].operands, vec![Operand::Bytes(b"Hello".to_vec())]);
    }

    #[test]
    fn handles_octal_and_comment() {
        // `\101` (octal) is byte 65 == 'A'.
        let tokens = tokenize(b"% a comment\n(\\101B) Tj").unwrap();
        assert_eq!(tokens[0].operands, vec![Operand::Bytes(b"AB".to_vec())]);
    }

    #[test]
    fn negative_and_decimal_numbers() {
        let tokens = tokenize(b"-2.5 1. .5 +3 1e2 cm").unwrap();
        assert_eq!(
            tokens[0].operands,
            vec![
                Operand::Num(-2.5),
                Operand::Num(1.0),
                Operand::Num(0.5),
                Operand::Num(3.0),
                Operand::Num(100.0)
            ]
        );
    }

    #[test]
    fn round_trip_is_stable() {
        round_trip("q 1 0 0 1 50 60 cm Q");
        round_trip("BT /F1 14 Tf (Hello) Tj ET");
        round_trip("BT /F1 12 Tf [(A) -120 (B) -120] TJ ET");
        round_trip("0.5 0.5 0 0 rg re f* S");
        // Escaped paren and escaped backslash inside a literal string:
        // PDF source `(a\)b\\c)` decodes to bytes `a)b\c`.
        round_trip("(a\\)b\\\\c) Tj");
    }

    #[test]
    fn nested_arrays() {
        let tokens = tokenize(b"[[1 2] (x)] TJ").unwrap();
        assert_eq!(
            tokens[0].operands,
            vec![Operand::Array(vec![
                Operand::Array(vec![Operand::Num(1.0), Operand::Num(2.0)]),
                Operand::Bytes(b"x".to_vec()),
            ])]
        );
    }

    #[test]
    fn errors_on_unterminated_string() {
        assert!(tokenize(b"(unterminated Tj").is_err());
    }
}
