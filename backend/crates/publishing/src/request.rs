//! Request assembly: the variant for one account and its media, resolved to full assets.

use std::collections::HashSet;

use postit_core::{
    AccountId, MediaAsset, MediaAssetId, MediaAssetState, PublishSnapshot, UserId, Variant,
};
use postit_data::{DataError, OwnerScope};
use postit_media::{MediaService, MediaServiceError};

/// What an agent needs to publish one delivery.
#[derive(Debug, Clone)]
pub struct Assembled {
    /// The frozen variant for the account.
    pub variant: Variant,
    /// The variant's media, in the variant's order.
    pub assets: Vec<MediaAsset>,
}

/// Why a request could not be assembled.
#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    /// The snapshot has no variant for the account.
    #[error("the snapshot has no variant for the account")]
    VariantMissing,
    /// These assets no longer exist or are not ready.
    #[error("{} media asset(s) are missing or not ready", .0.len())]
    MediaMissing(Vec<MediaAssetId>),
    /// A database failure.
    #[error(transparent)]
    Data(DataError),
}

/// Resolves `account`'s variant in `snapshot` and its media through `media`.
///
/// # Errors
///
/// [`AssembleError::VariantMissing`] when the snapshot has no variant for `account`;
/// [`AssembleError::MediaMissing`] for every asset that was deleted or is not `Ready`;
/// [`AssembleError::Data`] on a database failure.
pub async fn assemble(
    snapshot: &PublishSnapshot,
    account: AccountId,
    media: &MediaService,
    owner: UserId,
) -> Result<Assembled, AssembleError> {
    let variant = snapshot
        .variant_for(account)
        .ok_or(AssembleError::VariantMissing)?
        .clone();
    let ids: Vec<MediaAssetId> = variant.media.iter().map(|item| item.asset).collect();
    if ids.is_empty() {
        return Ok(Assembled {
            variant,
            assets: Vec::new(),
        });
    }
    let scope = OwnerScope::own(owner);
    let assets = match media.assets(&scope, &ids).await {
        Ok(assets) => assets,
        // One missing ID fails the whole call; ask one by one to name every missing asset.
        Err(MediaServiceError::NotFound) => {
            let gone = missing(media, &scope, &ids).await?;
            if !gone.is_empty() {
                return Err(AssembleError::MediaMissing(gone));
            }
            // Every asset turned up when asked one by one, so the set changed between the
            // calls: ask again as a whole, and name them all if it still refuses.
            match media.assets(&scope, &ids).await {
                Ok(assets) => assets,
                Err(MediaServiceError::NotFound) => {
                    return Err(AssembleError::MediaMissing(dedup(ids)));
                }
                Err(other) => return Err(AssembleError::Data(data_error(other))),
            }
        }
        Err(other) => return Err(AssembleError::Data(data_error(other))),
    };
    let unready: Vec<MediaAssetId> = assets
        .iter()
        .filter(|asset| asset.state != MediaAssetState::Ready)
        .map(|asset| asset.id)
        .collect();
    if !unready.is_empty() {
        return Err(AssembleError::MediaMissing(dedup(unready)));
    }
    Ok(Assembled { variant, assets })
}

/// The IDs among `ids` that are gone or not ready, in the order given.
async fn missing(
    media: &MediaService,
    scope: &OwnerScope,
    ids: &[MediaAssetId],
) -> Result<Vec<MediaAssetId>, AssembleError> {
    let mut gone = Vec::new();
    for id in dedup(ids.to_vec()) {
        match media.assets(scope, &[id]).await {
            Ok(found) if found.iter().all(|a| a.state == MediaAssetState::Ready) => {}
            Ok(_) | Err(MediaServiceError::NotFound) => gone.push(id),
            Err(other) => return Err(AssembleError::Data(data_error(other))),
        }
    }
    Ok(gone)
}

fn dedup(ids: Vec<MediaAssetId>) -> Vec<MediaAssetId> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// `assets` only fails with `NotFound`, a scope error (never, for the owner's own scope), or a
/// database failure; anything else is reported as a conflict naming the error.
fn data_error(error: MediaServiceError) -> DataError {
    match error {
        MediaServiceError::Data(error) => error,
        other => DataError::Conflict(other.to_string()),
    }
}
