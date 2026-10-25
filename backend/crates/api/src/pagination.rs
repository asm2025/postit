//! Shared `page`/`page_size` query parameters and the page envelope.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiError, ErrorCode};

/// Page size used when the client sends none.
pub const DEFAULT_PAGE_SIZE: u64 = 20;
/// Largest page size accepted.
pub const MAX_PAGE_SIZE: u64 = 100;
/// Caps the offset well inside `i64`: the repositories fall back to offset 0 when
/// `(page - 1) * page_size` does not fit, which would label page 1's rows as page N.
pub const MAX_PAGE: u64 = 1_000_000;

/// The `page` and `page_size` query parameters.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1-100 (default 20).
    pub page_size: Option<u64>,
}

impl PageQuery {
    /// # Errors
    ///
    /// `validation_failed` when `page` is outside 1-1,000,000 or `page_size` outside 1-100.
    pub fn to_pagination(&self) -> Result<emixdb::dto::Pagination, ApiError> {
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=MAX_PAGE).contains(&page) {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail(format!("page must be between 1 and {MAX_PAGE}")));
        }
        if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail(format!("page_size must be between 1 and {MAX_PAGE_SIZE}")));
        }
        Ok(emixdb::dto::Pagination { page, page_size })
    }
}

/// One page of results with its position in the full result set.
#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T: ToSchema> {
    /// The items on this page.
    pub data: Vec<T>,
    /// Total number of items across all pages.
    pub total: u64,
    /// 1-based number of this page.
    pub page: u64,
    /// Items per page requested.
    pub page_size: u64,
}

impl<T: ToSchema> Page<T> {
    /// A page of `data` taken from the result set described by `total` and `pagination`.
    #[must_use]
    pub fn new(data: Vec<T>, total: u64, pagination: &emixdb::dto::Pagination) -> Self {
        Self {
            data,
            total,
            page: pagination.page,
            page_size: pagination.page_size,
        }
    }
}
