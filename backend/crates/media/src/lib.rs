//! Media: content sniffing, storage keys, the file store, probing, thumbnails, URL signing, and
//! the service the API calls.

mod error;
mod sniff;
mod storage;

pub use error::MediaServiceError;
pub use sniff::{SNIFF_LEN, Sniffed, kind_for_filename, sniff};
pub use storage::{FsMediaStore, StorageKey};
