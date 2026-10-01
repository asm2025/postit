pub mod audit;
pub mod auth;
pub mod me;
pub mod probes;
#[cfg(any(test, feature = "testkit"))]
pub mod testing;
pub mod users;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn v1_router() -> Router<AppState> {
    let router = Router::new()
        .route("/auth/config", get(auth::config))
        .route("/me", get(me::get_me).delete(me::delete_me))
        .route("/admin/audit", get(audit::list))
        .route("/users", get(users::list))
        .route(
            "/users/{id}",
            get(users::get).patch(users::patch).delete(users::delete),
        );
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test", testing::router());
    router
}
