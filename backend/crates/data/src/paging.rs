//! Shared page arithmetic for repository lists.

use emixdb::dto::Pagination;

/// The `LIMIT` and `OFFSET` for a 1-based `pagination`.
pub(crate) fn bounds(pagination: Pagination) -> (i64, i64) {
    let limit = i64::try_from(pagination.page_size).unwrap_or(10);
    let offset = i64::try_from(
        pagination
            .page
            .saturating_sub(1)
            .saturating_mul(pagination.page_size),
    )
    .unwrap_or(0);
    (limit, offset)
}

/// A `COUNT(*)` result as the `u64` a [`emixdb::dto::ResultSet`] carries.
pub(crate) fn total(count: Option<i64>) -> u64 {
    u64::try_from(count.unwrap_or(0)).unwrap_or(0)
}
