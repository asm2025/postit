use postit_config::{BootstrapSettings, Environment};
use postit_core::UserId;
use postit_data::users::UsersRepo;
use postit_identity::IdentityError;
use postit_identity::bootstrap::check_startup;
use sqlx::PgPool;

fn none() -> BootstrapSettings {
    BootstrapSettings {
        admin_email: None,
        admin_subject: None,
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn production_refuses_with_no_admin_and_no_rule(pool: PgPool) {
    let result = check_startup(&pool, &none(), Environment::Production).await;
    assert!(
        matches!(result, Err(IdentityError::BootstrapRequired)),
        "got {result:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn production_passes_with_a_rule(pool: PgPool) {
    let rule = BootstrapSettings {
        admin_email: Some("admin@postit.test".into()),
        admin_subject: None,
    };
    assert!(
        check_startup(&pool, &rule, Environment::Production)
            .await
            .is_ok()
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn production_passes_with_an_active_admin(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, id, "https://i.test", "root", "Root")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("grant: {e}"));
    assert!(
        check_startup(&pool, &none(), Environment::Production)
            .await
            .is_ok()
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn development_and_qa_never_refuse(pool: PgPool) {
    assert!(
        check_startup(&pool, &none(), Environment::Development)
            .await
            .is_ok()
    );
    assert!(check_startup(&pool, &none(), Environment::Qa).await.is_ok());
}
