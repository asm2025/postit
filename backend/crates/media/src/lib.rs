//! Media: content sniffing, storage keys, the file store, probing, thumbnails, URL signing, and
//! the service the API calls.

mod error;
mod probe;
mod sniff;
mod storage;
mod thumbnail;

pub use error::MediaServiceError;
#[cfg(feature = "ffprobe")]
pub use probe::FfprobeProbe;
pub use probe::NativeProbe;
pub use sniff::{SNIFF_LEN, Sniffed, kind_for_filename, sniff};
pub use storage::{FsMediaStore, StorageKey};
pub use thumbnail::Thumbnailer;
