//! Media: content sniffing, storage keys, the file store, probing, thumbnails, URL signing, and
//! the service the API calls.

mod error;
mod intake;
mod probe;
mod service;
mod signing;
mod sniff;
mod storage;
#[cfg(feature = "testkit")]
pub mod testkit;
mod thumbnail;

pub use error::MediaServiceError;
#[cfg(feature = "ffprobe")]
pub use probe::FfprobeProbe;
pub use probe::NativeProbe;
pub use service::{MediaService, MediaServiceDeps, PublicMediaError, ServedFile, Usage, ViewUrls};
pub use signing::{SignedTarget, SignedUrlMediaHost, UrlSigner};
pub use sniff::{SNIFF_LEN, Sniffed, kind_for_filename, sniff};
pub use storage::{FsMediaStore, StorageKey};
pub use thumbnail::Thumbnailer;
