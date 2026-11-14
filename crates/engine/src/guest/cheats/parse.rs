// Atmosphère's cheat files: `cheats/<build id>.txt` and `cheats/toggles.txt`, read as dmnt reads them
// (stratosphere/dmnt/source/cheat/impl/dmnt_cheat_api.cpp: CheatProcessManager::ParseCheats, ParseCheatToggles).
//
// A cheat file is a run of tokens separated by whitespace: `[name]` opens a cheat, `{name}` the master cheat (one per
// file, run before the others and never disabled), and each 8 hex digits are one opcode word of the cheat last opened.
// Anything else fails the whole file, and dmnt then loads none of its cheats. As dmnt keeps them:
// - a name is everything up to the closing bracket, newlines included, of which dmnt keeps the first 0x40 bytes;
// - a cheat that ends without opcodes leaves its slot free: the next `[name]` renames it (a heading line), and it is
//   neither listed nor run;
// - at most 0x7F cheats besides the master, and 0x100 opcode words per cheat;
// - the text ends at the first NUL (dmnt reads the file as a C string).
// One difference: a UTF-8 byte order mark at the start is skipped. Windows editors write one, and dmnt would reject
// the file for it.

/// Words a cheat may hold (CheatDefinition::opcodes).
pub const MAX_OPCODES: usize = 0x100;
/// Cheats besides the master (MaxCheatCount less the master's slot).
pub const MAX_CHEATS: usize = 0x7F;
/// Bytes of a name dmnt keeps (CheatDefinition::readable_name).
const NAME_LEN: usize = 0x40;

#[derive(Clone, Debug, PartialEq)]
pub struct Cheat {
    pub name: String,
    pub opcodes: Vec<u32>,
}

#[derive(Debug, Default, PartialEq)]
pub struct CheatFile {
    pub master: Option<Cheat>,
    pub cheats: Vec<Cheat>,
}

/// Why a file was rejected, at which line (1-based).
#[derive(Debug, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// C's isspace in the "C" locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// The bytes dmnt would parse: up to the first NUL, without a leading byte order mark.
fn text_of(data: &[u8]) -> &[u8] {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

fn error(s: &[u8], at: usize, message: impl Into<String>) -> ParseError {
    ParseError { line: 1 + s[..at.min(s.len())].iter().filter(|&&b| b == b'\n').count(), message: message.into() }
}

/// The name between s[i] (the opening bracket) and `close`, and the index after `close`.
fn name_at(s: &[u8], i: usize, close: u8) -> Result<(String, usize), ParseError> {
    let Some(j) = s[i + 1..].iter().position(|&b| b == close).map(|j| i + 1 + j) else {
        return Err(error(s, i, format!("'{}' is not closed by '{}'", s[i] as char, close as char)));
    };
    let name = &s[i + 1..j];
    Ok((String::from_utf8_lossy(&name[..name.len().min(NAME_LEN)]).into_owned(), j + 1))
}

/// Which cheat opcode words go to.
#[derive(Clone, Copy)]
enum Current {
    None,
    Master,
    Cheat(usize),
}

pub fn parse_cheats(data: &[u8]) -> Result<CheatFile, ParseError> {
    let s = text_of(data);
    let mut master: Option<Cheat> = None;
    let mut cheats: Vec<Cheat> = Vec::new();
    let mut cur = Current::None;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if is_space(c) {
            i += 1;
        } else if c == b'[' {
            // GetFreeCheatEntry: the first slot without opcodes, which can only be the last one opened.
            let slot = match cheats.last() {
                Some(last) if last.opcodes.is_empty() => cheats.len() - 1,
                _ if cheats.len() == MAX_CHEATS => return Err(error(s, i, format!("more than {MAX_CHEATS} cheats"))),
                _ => {
                    cheats.push(Cheat { name: String::new(), opcodes: Vec::new() });
                    cheats.len() - 1
                }
            };
            let (name, next) = name_at(s, i, b']')?;
            cheats[slot].name = name;
            cur = Current::Cheat(slot);
            i = next;
        } else if c == b'{' {
            if master.as_ref().is_some_and(|m| !m.opcodes.is_empty()) {
                return Err(error(s, i, "a second master cheat"));
            }
            let (name, next) = name_at(s, i, b'}')?;
            master.get_or_insert_with(|| Cheat { name: String::new(), opcodes: Vec::new() }).name = name;
            cur = Current::Master;
            i = next;
        } else if c.is_ascii_hexdigit() {
            let cheat = match cur {
                Current::None => return Err(error(s, i, "an opcode before the first cheat name")),
                Current::Master => master.as_mut().unwrap(),
                Current::Cheat(n) => &mut cheats[n],
            };
            if cheat.opcodes.len() >= MAX_OPCODES {
                return Err(error(s, i, format!("'{}' has more than {MAX_OPCODES} opcode words", cheat.name)));
            }
            if s.len() < i + 8 || !s[i..i + 8].iter().all(u8::is_ascii_hexdigit) {
                return Err(error(s, i, "an opcode word is not 8 hex digits"));
            }
            cheat.opcodes.push(u32::from_str_radix(std::str::from_utf8(&s[i..i + 8]).unwrap(), 16).unwrap());
            i += 8;
        } else {
            let ch = String::from_utf8_lossy(&s[i..(i + 4).min(s.len())]).chars().next().unwrap_or('?');
            return Err(error(s, i, format!("unexpected character '{ch}'")));
        }
    }
    cheats.retain(|c| !c.opcodes.is_empty());
    Ok(CheatFile { master: master.filter(|m| !m.opcodes.is_empty()), cheats })
}

/// Names a cheat compares by: its first 0x40 bytes (strncmp over the name field).
fn same_name(a: &str, b: &str) -> bool {
    a.as_bytes()[..a.len().min(NAME_LEN)] == b.as_bytes()[..b.len().min(NAME_LEN)]
}

/// toggles.txt: `[name]` then `true`/`false` (also 1/0, on/off, any case) for each cheat, applied to `enabled` (one
/// flag per cheat of `cheats`). A toggle for a cheat the file does not have, or a word that is none of those, changes
/// nothing. The first cheat of a name takes the toggle, as dmnt looks it up.
pub fn parse_toggles(data: &[u8], cheats: &[Cheat], enabled: &mut [bool]) -> Result<(), ParseError> {
    let s = text_of(data);
    let mut i = 0;
    while i < s.len() {
        if is_space(s[i]) {
            i += 1;
        } else if s[i] == b'[' {
            let (name, next) = name_at(s, i, b']')?;
            i = next;
            while i < s.len() && is_space(s[i]) {
                i += 1;
            }
            if i >= s.len() {
                return Err(error(s, i, format!("no toggle after [{name}]")));
            }
            let mut j = i + 1;
            while j < s.len() && !is_space(s[j]) {
                j += 1;
                if j - i >= 8 {
                    return Err(error(s, i, "a toggle longer than 7 characters"));
                }
            }
            let toggle = String::from_utf8_lossy(&s[i..j]).to_ascii_lowercase();
            if let Some(n) = cheats.iter().position(|c| same_name(&c.name, &name)) {
                match toggle.as_str() {
                    "1" | "true" | "on" => enabled[n] = true,
                    "0" | "false" | "off" => enabled[n] = false,
                    _ => {}
                }
            }
            i = j + 1;
        } else {
            return Err(error(s, i, "unexpected character outside [name] toggle pairs"));
        }
    }
    Ok(())
}

/// toggles.txt as dmnt writes it (SaveCheatToggles): every cheat but the master, in file order.
#[cfg_attr(target_os = "ios", allow(dead_code))] // the desktop menus
pub fn format_toggles(cheats: &[Cheat], enabled: &[bool]) -> String {
    cheats.iter().zip(enabled).map(|(c, &on)| format!("[{}]\n{}\n", c.name, if on { "true" } else { "false" })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cheat(name: &str, opcodes: &[u32]) -> Cheat {
        Cheat { name: name.into(), opcodes: opcodes.to_vec() }
    }

    #[test]
    fn cheats_and_master() {
        let f = parse_cheats(
            b"\xEF\xBB\xBF{Master Code}\n580F0000 01234567\n\n[Infinite Hearts]\r\n04000000 00ABCDEF 00000028\n\
              [Section --]\n[Two\nlines]\n0400000000ABCDEF\t00000001",
        )
        .unwrap();
        assert_eq!(f.master, Some(cheat("Master Code", &[0x580F_0000, 0x0123_4567])));
        // The heading without opcodes was renamed by the cheat after it; 16 digits in a row are two words.
        assert_eq!(f.cheats, vec![cheat("Infinite Hearts", &[0x0400_0000, 0x00AB_CDEF, 0x28]), cheat("Two\nlines", &[0x0400_0000, 0x00AB_CDEF, 1])]);
    }

    #[test]
    fn ends_at_nul_and_names_are_cut() {
        let long = "x".repeat(70);
        let f = parse_cheats(format!("[{long}]\n00000000\0garbage").as_bytes()).unwrap();
        assert_eq!(f.cheats, vec![cheat(&long[..0x40], &[0])]);
        assert_eq!(parse_cheats(b"").unwrap(), CheatFile::default());
    }

    #[test]
    fn errors_name_their_line() {
        let err = |t: &[u8]| parse_cheats(t).unwrap_err();
        assert_eq!(err(b"[A]\n0400000\n").line, 2);
        assert_eq!(err(b"[A]\n04000000 0000000G").line, 2);
        assert_eq!(err(b"\n\n04000000").line, 3);
        assert_eq!(err(b"[A]\n04000000\n; note").line, 3);
        assert_eq!(err(b"[A\n04000000").line, 1);
        assert_eq!(err(b"{M}\n00000000\n{N}\n00000000").line, 3);
        let mut many = String::new();
        for n in 0..=MAX_CHEATS {
            many += &format!("[c{n}]\n20000000\n");
        }
        assert!(err(many.as_bytes()).message.contains("more than"));
        let words = "[A]\n".to_string() + &"20000000 ".repeat(MAX_OPCODES + 1);
        assert!(err(words.as_bytes()).message.contains("opcode words"));
        // An empty master can be named again.
        assert!(parse_cheats(b"{M}\n{N}\n20000000").is_ok());
    }

    #[test]
    fn toggles() {
        let cheats = [cheat("A", &[0]), cheat("B", &[0]), cheat("C", &[0])];
        let mut on = [true; 3];
        parse_toggles(b"[A]\nfalse\n[B] OFF\n[Missing]\n0\n[C]\nmaybe\n", &cheats, &mut on).unwrap();
        assert_eq!(on, [false, false, true]);
        parse_toggles(b"[A] 1 [B] On", &cheats, &mut on).unwrap();
        assert_eq!(on, [true, true, true]);
        assert_eq!(parse_toggles(b"[A]\n", &cheats, &mut on).unwrap_err().line, 2);
        assert!(parse_toggles(b"[A] enabled1", &cheats, &mut on).is_err());
        assert!(parse_toggles(b"A true", &cheats, &mut on).is_err());
        assert_eq!(format_toggles(&cheats, &[true, false, true]), "[A]\ntrue\n[B]\nfalse\n[C]\ntrue\n");
    }
}
