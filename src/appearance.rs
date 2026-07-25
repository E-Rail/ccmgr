//! Asking the terminal whether it is light or dark.
//!
//! There is no portable "what theme am I" API, but most terminals answer an
//! OSC 11 query with their actual background color, which is the same thing
//! for our purposes. A handful of older ones don't, so `COLORFGBG` is used
//! as a second opinion before falling back to dark.

use crate::config::Appearance;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

/// How long to wait for the terminal to answer. The reply normally arrives
/// within a frame; this only matters for terminals that answer neither
/// query, and it delays startup by that much when they don't.
const REPLY_TIMEOUT: Duration = Duration::from_millis(150);

/// Terminals have no length limit on these replies, so cap what we buffer.
const MAX_REPLY_BYTES: usize = 4096;

/// Detect the terminal's appearance.
///
/// Must be called while the terminal is in raw mode - a cooked terminal
/// buffers the reply until the user presses Enter, so the query would just
/// time out. Callers that can't guarantee raw mode get `Dark`, which is the
/// same answer an unsupported terminal gives.
pub fn detect() -> Appearance {
    if let Some((red, green, blue)) = query_background_color() {
        return for_background(red, green, blue);
    }
    color_fgbg_appearance().unwrap_or(Appearance::Dark)
}

fn for_background(red: u8, green: u8, blue: u8) -> Appearance {
    let luminance = 0.2126 * red as f32 + 0.7152 * green as f32 + 0.0722 * blue as f32;
    if luminance > 127.5 {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

fn query_background_color() -> Option<(u8, u8, u8)> {
    // /dev/tty rather than stdout: the query has to reach the terminal even
    // when ccmgr's output is redirected, and the reply comes back on the
    // same device.
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;

    // The trailing device-attributes query is the trick that makes this
    // safe: every terminal answers it, including the ones that ignore
    // OSC 11. Seeing its reply means the background reply is never coming,
    // so we stop immediately instead of waiting out the whole timeout.
    tty.write_all(b"\x1b]11;?\x1b\\\x1b[c").ok()?;
    tty.flush().ok()?;

    let deadline = Instant::now() + REPLY_TIMEOUT;
    let mut reply = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || !wait_readable(&tty, remaining) {
            break;
        }
        match tty.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => reply.extend_from_slice(&chunk[..read]),
        }
        if reply.len() >= MAX_REPLY_BYTES || device_attributes_complete(&reply) {
            break;
        }
    }

    parse_background_reply(&reply)
}

/// `select` rather than `poll`: macOS's `poll` is unreliable on tty devices
/// and can report a descriptor readable when it has nothing to give, which
/// leaves the following `read` blocking forever.
fn wait_readable(tty: &File, timeout: Duration) -> bool {
    let fd = tty.as_raw_fd();
    if fd < 0 || fd as usize >= libc::FD_SETSIZE {
        return false;
    }
    // SAFETY: `readable` and `remaining` are initialized before the call and
    // live across it, and `tty` keeps `fd` open for the same duration.
    unsafe {
        let mut readable: libc::fd_set = std::mem::zeroed();
        libc::FD_SET(fd, &mut readable);
        let mut remaining = libc::timeval {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_usec: timeout.subsec_micros() as libc::suseconds_t,
        };
        libc::select(
            fd + 1,
            &mut readable,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut remaining,
        ) > 0
    }
}

/// Has a complete `CSI ? ... c` device-attributes reply arrived?
fn device_attributes_complete(reply: &[u8]) -> bool {
    reply.windows(3).enumerate().any(|(start, window)| {
        window == b"\x1b[?"
            && reply[start + 3..]
                .iter()
                .position(|byte| !byte.is_ascii_digit() && *byte != b';')
                .is_some_and(|end| reply[start + 3 + end] == b'c')
    })
}

/// Pull the color out of an `OSC 11 ; rgb:RRRR/GGGG/BBBB ST` reply.
fn parse_background_reply(reply: &[u8]) -> Option<(u8, u8, u8)> {
    const INTRODUCER: &str = "]11;rgb:";

    let reply = String::from_utf8_lossy(reply);
    let start = reply.find(INTRODUCER)? + INTRODUCER.len();
    let body = &reply[start..];
    let end = body.find(['\x07', '\x1b']).unwrap_or(body.len());

    let mut channels = body[..end].split('/');
    let red = parse_channel(channels.next()?)?;
    let green = parse_channel(channels.next()?)?;
    let blue = parse_channel(channels.next()?)?;
    if channels.next().is_some() {
        return None;
    }
    Some((red, green, blue))
}

/// Terminals report each channel with 1-4 hex digits, so scale whatever
/// width this one picked down to 8 bits.
fn parse_channel(hex: &str) -> Option<u8> {
    if hex.is_empty() || hex.len() > 4 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    let full = 16u32.pow(hex.len() as u32) - 1;
    Some(((value * 255 + full / 2) / full) as u8)
}

/// urxvt, konsole and friends export `fg;bg` (occasionally `fg;default;bg`)
/// as palette indexes. Only the coarse light/dark split matters here, and
/// the bright colors in the middle of the range are too ambiguous to guess.
fn color_fgbg_appearance() -> Option<Appearance> {
    let value = std::env::var("COLORFGBG").ok()?;
    let background: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    match background {
        0..=6 | 8 => Some(Appearance::Dark),
        7 | 15 => Some(Appearance::Light),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_terminal_backgrounds_are_classified_correctly() {
        assert_eq!(for_background(0, 0, 0), Appearance::Dark);
        assert_eq!(for_background(40, 44, 52), Appearance::Dark); // One Dark
        assert_eq!(for_background(0, 43, 54), Appearance::Dark); // Solarized dark
        assert_eq!(for_background(255, 255, 255), Appearance::Light);
        assert_eq!(for_background(253, 246, 227), Appearance::Light); // Solarized light
        assert_eq!(for_background(250, 250, 250), Appearance::Light);
    }

    #[test]
    fn replies_are_parsed_at_every_channel_width() {
        assert_eq!(
            parse_background_reply(b"\x1b]11;rgb:2828/2c2c/3434\x1b\\"),
            Some((40, 44, 52))
        );
        assert_eq!(
            parse_background_reply(b"\x1b]11;rgb:ff/ff/ff\x07"),
            Some((255, 255, 255))
        );
        assert_eq!(
            parse_background_reply(b"\x1b]11;rgb:f/f/f\x07"),
            Some((255, 255, 255))
        );
        assert_eq!(
            parse_background_reply(b"\x1b]11;rgb:000000/000000/000000\x07"),
            None,
            "channels wider than 4 hex digits are not a valid reply"
        );
    }

    #[test]
    fn a_reply_mixed_with_other_terminal_output_still_parses() {
        assert_eq!(
            parse_background_reply(b"\x1b[?62;c\x1b]11;rgb:fdfd/f6f6/e3e3\x1b\\"),
            Some((253, 246, 227))
        );
    }

    #[test]
    fn missing_or_malformed_replies_are_rejected() {
        assert_eq!(parse_background_reply(b""), None);
        assert_eq!(parse_background_reply(b"\x1b[?62;c"), None);
        assert_eq!(parse_background_reply(b"\x1b]11;rgb:zz/zz/zz\x07"), None);
        assert_eq!(parse_background_reply(b"\x1b]11;rgb:ff/ff\x07"), None);
    }

    #[test]
    fn a_device_attributes_reply_ends_the_wait() {
        assert!(device_attributes_complete(b"\x1b[?62;1;6c"));
        assert!(device_attributes_complete(b"\x1b[?1;2c"));
        assert!(!device_attributes_complete(b"\x1b[?62;1;6"));
        assert!(!device_attributes_complete(b"\x1b]11;rgb:00/00/00\x07"));
        assert!(
            !device_attributes_complete(b"\x1b]11;rgb:cc/cc/cc\x1b\\"),
            "a 'c' inside a color reply is not a device-attributes terminator"
        );
    }
}
