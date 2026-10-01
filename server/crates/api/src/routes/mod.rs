pub mod probes;
#[cfg(any(test, feature = "testkit"))]
pub mod testing;

pub fn v1_router() -> axum::Router<crate::state::AppState> {
    let router = axum::Router::new();
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test", testing::router());
    router
}
