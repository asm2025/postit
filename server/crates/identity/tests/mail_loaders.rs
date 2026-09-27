use chrono::Utc;
use postit_core::UserId;
use postit_data::users::UsersRepo;
use postit_identity::mail::ApprovedLoader;
use postit_mail::{LoadOutcome, MailContent, MailContextLoader, MailParams};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "../data/migrations")]
async fn approved_loader_greets_by_current_display_name(pool: PgPool) {
    let mut conn = pool
        .acquire()
        .await
        .unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(Uuid::now_v7());
    let (_, user) = UsersRepo::provision(&mut conn, id, "https://i.test", "s", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    let outcome = ApprovedLoader
        .load(&mut conn, &user, &MailParams::None, Utc::now())
        .await
        .unwrap_or_else(|e| unreachable!("load: {e}"));
    assert_eq!(
        outcome,
        LoadOutcome::Send(MailContent::UserApproved {
            display_name: "Ada".into()
        })
    );
    ApprovedLoader
        .mark_sent(&mut conn, id, Utc::now())
        .await
        .unwrap_or_else(|e| unreachable!("mark_sent: {e}"));
}
