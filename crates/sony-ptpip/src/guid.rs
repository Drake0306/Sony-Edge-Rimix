//! The host GUID that identifies this computer to the camera. It must stay
//! the same across runs so the camera remembers the pairing.

use std::io::{self, ErrorKind};
use std::path::Path;

use crate::error::Error;

/// Reads the GUID stored at `path`, or creates one (random, UUID v4) and
/// stores it there, creating parent folders as needed.
///
/// The file holds 32 lowercase hex digits and a newline; surrounding
/// whitespace is ignored when reading. A file that holds anything else is an
/// error rather than being replaced, because a new GUID means pairing again.
pub fn load_or_create_guid(path: &Path) -> Result<[u8; 16], Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).ok_or_else(|| {
            Error::Io(io::Error::new(
                ErrorKind::InvalidData,
                format!("{} does not hold a 32-digit hex GUID", path.display()),
            ))
        }),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let guid = *uuid::Uuid::new_v4().as_bytes();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, format!("{}\n", hex(&guid)))?;
            Ok(guid)
        }
        Err(e) => Err(e.into()),
    }
}

fn hex(guid: &[u8; 16]) -> String {
    guid.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse(text: &str) -> Option<[u8; 16]> {
    let text = text.trim();
    if text.len() != 32 || !text.is_ascii() {
        return None;
    }
    let mut guid = [0u8; 16];
    for (i, byte) in guid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_is_stable_across_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a").join("b").join("guid");
        let first = load_or_create_guid(&path).unwrap();
        assert_eq!(load_or_create_guid(&path).unwrap(), first);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, format!("{}\n", hex(&first)));
        assert!(text
            .trim()
            .chars()
            .all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }

    #[test]
    fn parse_tolerates_whitespace_and_uppercase() {
        let want: [u8; 16] = core::array::from_fn(|i| i as u8 * 17);
        let text = format!("  {}\r\n\n", hex(&want).to_uppercase());
        assert_eq!(parse(&text), Some(want));
    }

    #[test]
    fn bad_contents_are_an_error_and_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("guid");
        for bad in [
            "",
            "xyz",
            "00112233445566778899aabbccddeeff00",
            "zz112233445566778899aabbccddeeff",
        ] {
            std::fs::write(&path, bad).unwrap();
            let err = load_or_create_guid(&path).unwrap_err();
            assert!(
                matches!(&err, Error::Io(e) if e.kind() == ErrorKind::InvalidData),
                "{bad:?}: {err:?}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
        }
    }
}
