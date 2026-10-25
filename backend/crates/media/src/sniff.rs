//! Content sniffing: what a file is gets decided by its bytes, never by the client.

use postit_core::MediaKind;

/// How many leading bytes the sniffer needs.
pub const SNIFF_LEN: usize = 8192;

/// What the bytes of an accepted file say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sniffed {
    /// The MIME type stored for the asset.
    pub mime: &'static str,
    /// Image or video.
    pub kind: MediaKind,
}

/// Identifies `head` (the first [`SNIFF_LEN`] bytes of a file) as JPEG, PNG, GIF, WebP, MP4, or
/// `QuickTime`. Anything else, SVG, HTML, and HEIC included, is `None`.
#[must_use]
pub fn sniff(head: &[u8]) -> Option<Sniffed> {
    let (mime, kind) = match infer::get(head)?.mime_type() {
        "image/jpeg" => ("image/jpeg", MediaKind::Image),
        "image/png" => ("image/png", MediaKind::Image),
        "image/gif" => ("image/gif", MediaKind::Image),
        "image/webp" => ("image/webp", MediaKind::Image),
        // `.m4v` is an MP4 container.
        "video/mp4" | "video/x-m4v" => ("video/mp4", MediaKind::Video),
        "video/quicktime" => ("video/quicktime", MediaKind::Video),
        _ => return None,
    };
    Some(Sniffed { mime, kind })
}

/// The kind a file name claims by its extension, case-insensitively. The sniffed kind must
/// agree when the upload completes.
#[must_use]
pub fn kind_for_filename(name: &str) -> Option<MediaKind> {
    let ext = std::path::Path::new(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => Some(MediaKind::Image),
        "mp4" | "m4v" | "mov" => Some(MediaKind::Video),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::MediaKind;

    use super::*;

    fn bmff(brand: [u8; 4]) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 0x18];
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(&brand);
        v.extend_from_slice(&[0, 0, 2, 0]);
        v.extend_from_slice(&brand);
        v.extend_from_slice(b"iso2mp41");
        v
    }

    fn webp() -> Vec<u8> {
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&[0x24, 0, 0, 0]);
        v.extend_from_slice(b"WEBPVP8 ");
        v.extend_from_slice(&[0; 16]);
        v
    }

    #[test]
    fn sniff_accepts_the_allowlist() {
        let cases: Vec<(Vec<u8>, &str, MediaKind)> = vec![
            (
                vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0],
                "image/jpeg",
                MediaKind::Image,
            ),
            (
                vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0],
                "image/png",
                MediaKind::Image,
            ),
            (
                b"GIF89a\x01\x00\x01\x00\x00\x00".to_vec(),
                "image/gif",
                MediaKind::Image,
            ),
            (webp(), "image/webp", MediaKind::Image),
            (bmff(*b"isom"), "video/mp4", MediaKind::Video),
            (bmff(*b"qt  "), "video/quicktime", MediaKind::Video),
        ];
        for (head, mime, kind) in cases {
            let got = sniff(&head).unwrap_or_else(|| unreachable!("sniff {mime}"));
            assert_eq!(got.mime, mime);
            assert_eq!(got.kind, kind);
        }
    }

    #[test]
    fn sniff_refuses_svg_html_heic_and_pdf() {
        assert!(sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").is_none());
        assert!(sniff(b"<!doctype html><html><body>x</body></html>").is_none());
        assert!(sniff(&bmff(*b"heic")).is_none());
        assert!(sniff(b"%PDF-1.7\n%....").is_none());
        assert!(sniff(b"").is_none());
    }

    #[test]
    fn kind_for_filename_is_case_insensitive_and_closed() {
        assert_eq!(kind_for_filename("A.JPG"), Some(MediaKind::Image));
        assert_eq!(kind_for_filename("a.jpeg"), Some(MediaKind::Image));
        assert_eq!(kind_for_filename("a.webp"), Some(MediaKind::Image));
        assert_eq!(kind_for_filename("clip.MOV"), Some(MediaKind::Video));
        assert_eq!(kind_for_filename("clip.m4v"), Some(MediaKind::Video));
        assert_eq!(kind_for_filename("x.svg"), None);
        assert_eq!(kind_for_filename("x"), None);
        assert_eq!(kind_for_filename(".jpg.exe"), None);
    }
}
