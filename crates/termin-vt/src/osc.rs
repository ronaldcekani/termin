//! Operating System Command (OSC) handling.
//!
//! See `docs/specs/terminal-core.md §5`.

use termin_core::Rgba;

use crate::block::BlockList;
use crate::cell::AbsLine;
use crate::style::HyperlinkTable;
use crate::{Effect, EffectSink, Osc52Policy};

pub const OSC_MAX_PAYLOAD: usize = 64 * 1024;
pub const OSC52_MAX_PAYLOAD: usize = 1024 * 1024;

/// Decode a percent-encoded string (e.g. for URLs or cwd).
pub fn percent_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h1 << 4) | h2);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Simple standard base64 decode for OSC 52 clipboard payloads.
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity((input.len() * 3) / 4);
    let mut buf = 0u32;
    let mut bits = 0;

    for &b in input.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;

        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Dispatch parsed OSC parameters.
#[allow(clippy::too_many_arguments)]
pub fn handle_osc(
    params: &[&[u8]],
    _bell_terminated: bool,
    sink: &mut impl EffectSink,
    blocks: &mut BlockList,
    hyperlinks: &mut HyperlinkTable,
    current_link: &mut u16,
    current_line: AbsLine,
    osc52_policy: Osc52Policy,
    palette: &[Rgba; 256],
    default_fg: Rgba,
    default_bg: Rgba,
) {
    if params.is_empty() {
        return;
    }

    // Enforce payload caps
    let total_len: usize = params.iter().map(|p| p.len()).sum();
    let num = match std::str::from_utf8(params[0]).ok().and_then(|s| s.parse::<u32>().ok()) {
        Some(n) => n,
        None => return,
    };

    let cap = if num == 52 {
        OSC52_MAX_PAYLOAD
    } else {
        OSC_MAX_PAYLOAD
    };

    if total_len > cap {
        // Cap exceeded, discard
        return;
    }

    match num {
        // OSC 0, 1, 2: Window title
        0..=2 => {
            if params.len() > 1 {
                if let Ok(title) = std::str::from_utf8(params[1]) {
                    sink.effect(Effect::Title(title.into()));
                }
            }
        }

        // OSC 4: Color palette query / set
        4 => {
            // OSC 4 ; c ; spec [ ; c ; spec ... ]
            let mut i = 1;
            while i + 1 < params.len() {
                if let (Ok(idx_str), Ok(spec)) = (
                    std::str::from_utf8(params[i]),
                    std::str::from_utf8(params[i + 1]),
                ) {
                    if let Ok(c) = idx_str.parse::<u8>() {
                        if spec == "?" {
                            // Query
                            let color = palette[c as usize];
                            let r = (color.0 >> 24) as u8;
                            let g = (color.0 >> 16) as u8;
                            let b = (color.0 >> 8) as u8;
                            let reply = format!("\x1b]4;{};rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}\x1b\\", c, r, r, g, g, b, b);
                            sink.write_back(reply.as_bytes());
                        } else {
                            // Set color override (future palette override)
                            sink.effect(Effect::ColorQuery(c));
                        }
                    }
                }
                i += 2;
            }
        }

        // OSC 10: FG color query / set
        10 => {
            if params.len() > 1 {
                if let Ok(spec) = std::str::from_utf8(params[1]) {
                    if spec == "?" {
                        let r = (default_fg.0 >> 24) as u8;
                        let g = (default_fg.0 >> 16) as u8;
                        let b = (default_fg.0 >> 8) as u8;
                        let reply = format!("\x1b]10;rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}\x1b\\", r, r, g, g, b, b);
                        sink.write_back(reply.as_bytes());
                    }
                }
            }
        }

        // OSC 11: BG color query / set
        11 => {
            if params.len() > 1 {
                if let Ok(spec) = std::str::from_utf8(params[1]) {
                    if spec == "?" {
                        let r = (default_bg.0 >> 24) as u8;
                        let g = (default_bg.0 >> 16) as u8;
                        let b = (default_bg.0 >> 8) as u8;
                        let reply = format!("\x1b]11;rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}\x1b\\", r, r, g, g, b, b);
                        sink.write_back(reply.as_bytes());
                    }
                }
            }
        }

        // OSC 7: CWD `file://host/path`
        7 => {
            if params.len() > 1 {
                if let Ok(url) = std::str::from_utf8(params[1]) {
                    let path = if let Some(stripped) = url.strip_prefix("file://") {
                        if let Some(slash_idx) = stripped.find('/') {
                            &stripped[slash_idx..]
                        } else {
                            stripped
                        }
                    } else {
                        url
                    };
                    let decoded = percent_decode(path);
                    blocks.set_cwd(&decoded);
                    sink.effect(Effect::Cwd(decoded.into_boxed_str()));
                }
            }
        }

        // OSC 8: Hyperlinks `8;params;url`
        8 => {
            let url = if params.len() > 2 {
                std::str::from_utf8(params[2]).unwrap_or("")
            } else {
                ""
            };

            let id_param = if params.len() > 1 {
                let p = std::str::from_utf8(params[1]).unwrap_or("");
                p.strip_prefix("id=")
            } else {
                None
            };

            if url.is_empty() {
                *current_link = 0;
            } else {
                *current_link = hyperlinks.insert(id_param, url);
                sink.effect(Effect::Hyperlink);
            }
        }

        // OSC 9: iTerm2 notification
        9 => {
            if params.len() > 1 {
                if let Ok(msg) = std::str::from_utf8(params[1]) {
                    sink.effect(Effect::Notify {
                        title: Box::from(""),
                        body: msg.into(),
                    });
                }
            }
        }

        // OSC 777: `notify;title;body`
        777 => {
            if params.len() > 1 {
                if let Ok(action) = std::str::from_utf8(params[1]) {
                    if action == "notify" && params.len() > 3 {
                        if let (Ok(title), Ok(body)) = (
                            std::str::from_utf8(params[2]),
                            std::str::from_utf8(params[3]),
                        ) {
                            sink.effect(Effect::Notify {
                                title: title.into(),
                                body: body.into(),
                            });
                        }
                    }
                }
            }
        }

        // OSC 99: Kitty notification
        99 => {
            if params.len() > 1 {
                // Last param is body
                if let Ok(body) = std::str::from_utf8(params[params.len() - 1]) {
                    sink.effect(Effect::Notify {
                        title: Box::from(""),
                        body: body.into(),
                    });
                }
            }
        }

        // OSC 52: Clipboard
        52 => {
            if params.len() > 2 {
                if let Ok(data) = std::str::from_utf8(params[2]) {
                    if data == "?" {
                        if osc52_policy == Osc52Policy::ReadWrite {
                            sink.effect(Effect::ClipboardRequest);
                        }
                    } else if (osc52_policy == Osc52Policy::Write || osc52_policy == Osc52Policy::ReadWrite)
                        && !data.is_empty()
                    {
                        if let Some(decoded) = base64_decode(data) {
                            sink.effect(Effect::ClipboardSet(decoded.into_boxed_slice()));
                        }
                    }
                }
            }
        }

        // OSC 133: FinalTerm command blocks
        133 => {
            if params.len() > 1 {
                if let Ok(mark) = std::str::from_utf8(params[1]) {
                    let mut chars = mark.chars();
                    if let Some(cmd) = chars.next() {
                        match cmd {
                            'A' => {
                                let ev = blocks.on_prompt_start(current_line);
                                sink.effect(Effect::Block(ev));
                            }
                            'B' => {
                                blocks.on_input_start(current_line);
                            }
                            'C' => {
                                let mut cmdline = None;
                                for p in &params[2..] {
                                    if let Ok(s) = std::str::from_utf8(p) {
                                        if let Some(url_enc) = s.strip_prefix("cmdline_url=") {
                                            let dec = percent_decode(url_enc);
                                            cmdline = Some(dec);
                                        }
                                    }
                                }
                                if let Some(ev) = blocks.on_output_start(current_line, cmdline.as_deref()) {
                                    sink.effect(Effect::Block(ev));
                                }
                            }
                            'D' => {
                                let exit_code = if params.len() > 2 {
                                    std::str::from_utf8(params[2])
                                        .ok()
                                        .and_then(|s| s.parse::<i32>().ok())
                                        .unwrap_or(0)
                                } else {
                                    0
                                };
                                if let Some(ev) = blocks.on_command_finished(current_line, exit_code) {
                                    sink.effect(Effect::Block(ev));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // OSC 1337: iTerm2 CurrentDir
        1337 => {
            if params.len() > 1 {
                if let Ok(kv) = std::str::from_utf8(params[1]) {
                    if let Some(dir) = kv.strip_prefix("CurrentDir=") {
                        blocks.set_cwd(dir);
                        sink.effect(Effect::Cwd(dir.into()));
                    }
                }
            }
        }

        // OSC 6973: Termin private channel
        6973 if params.len() > 2 => {
            if let Ok(prefix) = std::str::from_utf8(params[1]) {
                if prefix == "termin" {
                    if let Ok(kv) = std::str::from_utf8(params[2]) {
                        // Format: key=value
                        let _ = kv;
                    }
                }
            }
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("/Users/test/foo%2Fbar"), "/Users/test/foo/bar");
    }

    #[test]
    fn test_base64_decode() {
        let dec = base64_decode("aGVsbG8=").unwrap();
        assert_eq!(dec, b"hello");
        let dec2 = base64_decode("d29ybGQ=").unwrap();
        assert_eq!(dec2, b"world");
    }
}
