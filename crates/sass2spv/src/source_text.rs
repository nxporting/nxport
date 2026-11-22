// Rust source as the caches of translated shaders see it: without comments, so a change to comments alone keeps them.
// Also compiled into build.rs (SOURCE_HASH).

/// `src` with every `//` and `/* */` comment removed (string, raw string and character literals kept as they are) and
/// CR LF line ends as LF.
pub fn code_only(src: &[u8]) -> Vec<u8> {
    let s: Vec<u8> = src.iter().enumerate().filter(|&(i, &b)| !(b == b'\r' && src.get(i + 1) == Some(&b'\n'))).map(|(_, &b)| b).collect();
    let n = s.len();
    let ident = |i: usize| s[i].is_ascii_alphanumeric() || s[i] == b'_';
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let b = s[i];
        let next = s.get(i + 1).copied();
        if b == b'/' && next == Some(b'/') {
            while i < n && s[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && next == Some(b'*') {
            let mut depth = 0;
            while i < n {
                if s[i] == b'/' && s.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if s[i] == b'*' && s.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        // Raw strings: r"..", r#".."#, br"..".
        let starts_word = i == 0 || !ident(i - 1) || (s[i - 1] == b'b' && (i == 1 || !ident(i - 2)));
        if b == b'r' && starts_word {
            let mut j = i + 1;
            while j < n && s[j] == b'#' {
                j += 1;
            }
            if j < n && s[j] == b'"' {
                let hashes = j - i - 1;
                let mut k = j + 1;
                while k < n && !(s[k] == b'"' && s[k + 1..].iter().take(hashes).filter(|&&c| c == b'#').count() == hashes) {
                    k += 1;
                }
                let end = (k + 1 + hashes).min(n);
                out.extend_from_slice(&s[i..end]);
                i = end;
                continue;
            }
        }
        if b == b'"' {
            let mut k = i + 1;
            while k < n && s[k] != b'"' {
                k += if s[k] == b'\\' { 2 } else { 1 };
            }
            let end = (k + 1).min(n);
            out.extend_from_slice(&s[i..end]);
            i = end;
            continue;
        }
        // A character literal ('x', '\n', 'é'), else a lifetime.
        if b == b'\'' {
            if next == Some(b'\\') {
                let mut k = i + 3;
                while k < n && s[k] != b'\'' {
                    k += 1;
                }
                let end = (k + 1).min(n);
                out.extend_from_slice(&s[i..end]);
                i = end;
                continue;
            }
            if let Some(c) = next {
                let len = match c {
                    0xf0.. => 4,
                    0xe0.. => 3,
                    0xc0.. => 2,
                    _ => 1,
                };
                if s.get(i + 1 + len) == Some(&b'\'') {
                    out.extend_from_slice(&s[i..i + 2 + len]);
                    i += 2 + len;
                    continue;
                }
            }
        }
        out.push(b);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::code_only;

    fn c(s: &str) -> String {
        String::from_utf8(code_only(s.as_bytes())).unwrap()
    }

    #[test]
    fn comments_go_literals_stay() {
        assert_eq!(c("a // x\nb /* y /* z */ w */ c"), "a \nb  c");
        assert_eq!(c("\"// kept\" // gone"), "\"// kept\" ");
        assert_eq!(c("\"a\\\"// kept\" x"), "\"a\\\"// kept\" x");
        assert_eq!(c("r#\"// \"kept\" \"# // gone"), "r#\"// \"kept\" \"# ");
        assert_eq!(c("br\"//k\" b'\"' '\\'' 'é' // g"), "br\"//k\" b'\"' '\\'' 'é' ");
        assert_eq!(c("fn f<'a>(x: &'a str) -> &'a str { x } // g"), "fn f<'a>(x: &'a str) -> &'a str { x } ");
        assert_eq!(c("for r in v {} // g\r\n"), "for r in v {} \n");
        assert_eq!(c("const S: &str = \"x\n    // emitted\n\";"), "const S: &str = \"x\n    // emitted\n\";");
    }

    /// Text the translator emits that looks like a comment stays in.
    #[test]
    fn emitted_comments_stay() {
        let helpers = c(include_str!("msl/helpers.rs"));
        assert!(helpers.contains("// SPIR-V callers expect a uint4. We must convert."));
        assert!(!helpers.contains("Declared in this order, as SPIRV-Cross does."));
    }
}
