//! VRChat chatbox via OSC.
use std::net::UdpSocket;

// ------------------------------------------------------------------ OSC
fn osc_pad(buf: &mut Vec<u8>) {
    buf.push(0);
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

fn osc_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    osc_pad(buf);
}

pub enum OscArg<'a> {
    Str(&'a str),
    Bool(bool),
}

pub fn osc_message(addr: &str, args: &[OscArg]) -> Vec<u8> {
    let mut b = Vec::new();
    osc_str(&mut b, addr);
    let mut tags = String::from(",");
    for a in args {
        tags.push(match a {
            OscArg::Str(_) => 's',
            OscArg::Bool(true) => 'T',
            OscArg::Bool(false) => 'F',
        });
    }
    osc_str(&mut b, &tags);
    for a in args {
        if let OscArg::Str(s) = a {
            osc_str(&mut b, s);
        }
    }
    b
}

pub struct VrcChatbox {
    sock: UdpSocket,
    target: String,
}

impl VrcChatbox {
    pub fn new(host: &str, port: u16) -> std::io::Result<Self> {
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        Ok(Self { sock, target: format!("{host}:{port}") })
    }

    /// /chatbox/input s T <sound>
    pub fn send(&self, text: &str, sound: bool) {
        let text = truncate_chars(text, 144);
        let msg = osc_message("/chatbox/input", &[OscArg::Str(&text), OscArg::Bool(true), OscArg::Bool(sound)]);
        let _ = self.sock.send_to(&msg, &self.target);
    }

    pub fn clear(&self) {
        self.send("", false);
    }

    /// /chatbox/typing b
    pub fn typing(&self, on: bool) {
        let _ = self.sock.send_to(&osc_message("/chatbox/typing", &[OscArg::Bool(on)]), &self.target);
    }
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn osc_encoding() {
        let m = osc_message("/chatbox/input", &[OscArg::Str("hi"), OscArg::Bool(true), OscArg::Bool(false)]);
        assert_eq!(m.len() % 4, 0);
        assert_eq!(&m[..16], b"/chatbox/input\0\0");
        assert_eq!(&m[16..24], b",sTF\0\0\0\0");
        assert_eq!(&m[24..28], b"hi\0\0");
    }
}
