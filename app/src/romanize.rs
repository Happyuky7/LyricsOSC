//! Offline romanization of lyric lines:
//!   Japanese (kanji/kana → romaji, via `kakasi`), Chinese (→ pinyin with tones, via `pinyin`),
//!   Korean (→ Revised Romanization), Russian/Ukrainian (→ Latin transliteration).
//! Returns None for text that is already Latin or a script we don't handle.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Script {
    Japanese,
    Chinese,
    Korean,
    Cyrillic,
    Other,
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}')
}
fn is_han(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}
fn is_hangul(c: char) -> bool {
    matches!(c, '\u{AC00}'..='\u{D7A3}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}
fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Does any line of the song contain kana? Then Han-only lines are Japanese, not Chinese.
pub fn japanese_context<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    lines.into_iter().any(|l| l.chars().any(is_kana))
}

pub fn detect(line: &str, japanese_ctx: bool) -> Script {
    let (mut kana, mut han, mut hangul, mut cyr) = (0, 0, 0, 0);
    for c in line.chars() {
        if is_kana(c) { kana += 1 } else if is_han(c) { han += 1 } else if is_hangul(c) { hangul += 1 } else if is_cyrillic(c) { cyr += 1 }
    }
    if kana > 0 || (han > 0 && japanese_ctx) { Script::Japanese }
    else if han > 0 { Script::Chinese }
    else if hangul > 0 { Script::Korean }
    else if cyr > 0 { Script::Cyrillic }
    else { Script::Other }
}

/// Romanized version of `line`, or None if there is nothing to romanize.
pub fn romanize(line: &str, japanese_ctx: bool) -> Option<String> {
    let out = match detect(line, japanese_ctx) {
        Script::Japanese => kakasi::convert(line).romaji,
        Script::Chinese => chinese(line),
        Script::Korean => korean(line),
        Script::Cyrillic => cyrillic(line),
        Script::Other => return None,
    };
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() || out == line { None } else { Some(out) }
}

// ------------------------------------------------------------------ Chinese
fn chinese(line: &str) -> String {
    use pinyin::ToPinyin;
    let mut out = String::new();
    let mut prev_han = false;
    for c in line.chars() {
        match c.to_pinyin() {
            Some(p) => {
                if !out.is_empty() && !out.ends_with(' ') { out.push(' '); }
                out.push_str(p.with_tone());
                prev_han = true;
            }
            None => {
                if prev_han && !c.is_whitespace() && !is_punct(c) { out.push(' '); }
                out.push(full_to_half(c));
                prev_han = false;
            }
        }
    }
    out
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || "，。！？、；：「」『』（）《》…—～".contains(c)
}

/// Full-width CJK punctuation → ASCII, so the romanized line reads naturally.
fn full_to_half(c: char) -> char {
    match c {
        '，' | '、' => ',', '。' => '.', '！' => '!', '？' => '?', '；' => ';', '：' => ':',
        '（' => '(', '）' => ')', '「' | '」' | '『' | '』' | '《' | '》' => '"', '～' => '~', '　' => ' ',
        _ => c,
    }
}

// ------------------------------------------------------------------ Korean
const L: [&str; 19] = ["g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h"];
const V: [&str; 21] = ["a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i"];
// Final consonant as pronounced at the end of a syllable…
const T: [&str; 28] = ["", "k", "k", "k", "n", "n", "n", "t", "l", "k", "m", "l", "l", "l", "p", "l", "m", "p", "p", "t", "t", "ng", "t", "t", "k", "t", "p", "t"];
// …and when the next syllable starts with a silent ㅇ (the consonant moves over: 음악 → eumak)
const T_LINK: [&str; 28] = ["", "g", "kk", "ks", "n", "nj", "nh", "d", "r", "lg", "lm", "lb", "ls", "lt", "lp", "lh", "m", "b", "bs", "s", "ss", "ng", "j", "ch", "k", "t", "p", "h"];

fn korean(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let code = c as u32;
        if !(0xAC00..=0xD7A3).contains(&code) {
            out.push(c);
            continue;
        }
        let s = code - 0xAC00;
        let (l, v, t) = ((s / 588) as usize, ((s % 588) / 28) as usize, (s % 28) as usize);
        // initial ㄹ at the start of a word reads as "r"; ㅇ initial is silent
        out.push_str(L[l]);
        out.push_str(V[v]);
        let next_silent = chars.get(i + 1).map_or(false, |&n| {
            let n = n as u32;
            (0xAC00..=0xD7A3).contains(&n) && (n - 0xAC00) / 588 == 11
        });
        out.push_str(if next_silent { T_LINK[t] } else { T[t] });
    }
    out
}

// ------------------------------------------------------------------ Cyrillic
fn cyrillic(line: &str) -> String {
    // Ukrainian letters decide how и/г are read
    let ukrainian = line.chars().any(|c| "іїєґІЇЄҐ".contains(c));
    let mut out = String::new();
    for c in line.chars() {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let lat: &str = match lower {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => if ukrainian { "h" } else { "g" }, 'ґ' => "g", 'д' => "d",
            'е' => "e", 'ё' => "yo", 'є' => "ye", 'ж' => "zh", 'з' => "z", 'и' => if ukrainian { "y" } else { "i" },
            'і' => "i", 'ї' => "yi", 'й' => "y", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o",
            'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "kh", 'ц' => "ts",
            'ч' => "ch", 'ш' => "sh", 'щ' => "shch", 'ъ' | 'ь' => "", 'ы' => "y", 'э' => "e", 'ю' => "yu", 'я' => "ya",
            _ => { out.push(c); continue; }
        };
        if c.is_uppercase() {
            let mut ch = lat.chars();
            if let Some(f) = ch.next() { out.extend(f.to_uppercase()); out.push_str(ch.as_str()); }
        } else {
            out.push_str(lat);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts() {
        assert_eq!(romanize("こんにちは世界", false).as_deref(), Some("konnichiha sekai"));
        assert_eq!(romanize("勇者", true).is_some(), true); // Han in a Japanese song → romaji
        assert_eq!(romanize("你好世界", false).as_deref(), Some("nǐ hǎo shì jiè"));
        assert_eq!(romanize("사랑해요", false).as_deref(), Some("saranghaeyo"));
        assert_eq!(romanize("음악", false).as_deref(), Some("eumak"));
        assert_eq!(romanize("Привет мир", false).as_deref(), Some("Privet mir"));
        assert_eq!(romanize("Hello world", false), None);
        assert!(japanese_context(["夜に駆ける", "沈むように"]));
    }
}
