## Email listing critical notifications not yet emailed

alert-subject = { $count ->
    [one] Something needs your attention on postit
   *[other] { $count } things need your attention on postit
}
alert-intro = { $count ->
    [one] This needs your attention:
   *[other] These need your attention:
}
alert-open = Open postit
alert-footer = You are receiving this email because email notifications are on in your postit settings.

## One line per notification kind

notice-delivery_dead_lettered = A post to { $account } on { $platform } could not be published after several attempts.
notice-delivery_failed = A post to { $account } on { $platform } failed.
notice-delivery_outcome_unknown = postit could not confirm whether a post to { $account } on { $platform } went out.
notice-delivery_parked = A post to { $account } on { $platform } is on hold: { $reason }.
notice-delivery_stale = A post to { $account } on { $platform } has been on hold too long and needs you: { $reason }.
notice-job_partial = { $count ->
    [one] A post went out, but one of its accounts failed.
   *[other] A post went out, but { $count } of its accounts failed.
}
notice-account_needs_reconnect = { $account } on { $platform } needs to be reconnected.
notice-account_revoked_by_platform = { $platform } revoked postit's access to { $account }.
notice-reconnect_by_passed = The date to reconnect { $account } on { $platform } has passed.
notice-recurrence_skipped = A recurring post skipped an occurrence: { $reason }.
notice-delivery_published = A post to { $account } on { $platform } was published.
notice-reconnect_reminder = Reconnect { $account } on { $platform } soon to keep publishing.
notice-delegation_changed = Your shared access with { $name } changed.
notice-new_comments = { $count ->
    [one] { $account } on { $platform } has a new comment.
   *[other] { $account } on { $platform } has { $count } new comments.
}
notice-user_pending = A new user is waiting for approval.

## Why a post is on hold

park-reason-owner_disabled = the workspace owner's account is disabled
park-reason-account_needs_reconnect = the account must be reconnected
park-reason-platform_unavailable = the platform is not available
park-reason-platform_disabled = an administrator turned the platform off
park-reason-live_publish_disabled = live publishing is turned off
park-reason-quota_exceeded = the platform's posting limit was reached
park-reason-outcome_unresolvable = postit cannot tell whether it went out, so you need to decide
park-reason-unknown = something outside postit is blocking it

## Why a recurring post skipped an occurrence

skip-reason-ai_provider_missing = no AI provider is set up for the rewrite
skip-reason-rewrite_invalid = the AI rewrite was not usable
skip-reason-job_active = the previous occurrence is still publishing

## Email with the daily digest of new comments

digest-subject = { $count ->
    [one] You have 1 new comment on postit
   *[other] You have { $count } new comments on postit
}
digest-intro = { $count ->
    [one] Here is the new comment since your last digest:
   *[other] Here are the new comments since your last digest:
}
digest-open = Open postit
digest-footer = You are receiving this email because the daily comment digest is on in your postit settings.

## Closing line of a long list

list-more = ...and { $more } more.

## Push notification titles, one per kind; the body repeats the kind's line above

push-title-delivery_dead_lettered = Post not published
push-title-delivery_failed = Post failed
push-title-delivery_outcome_unknown = Post status unknown
push-title-delivery_parked = Post on hold
push-title-delivery_stale = Post needs you
push-title-job_partial = Post partly published
push-title-account_needs_reconnect = Reconnect an account
push-title-account_revoked_by_platform = Account access revoked
push-title-reconnect_by_passed = Reconnect date passed
push-title-recurrence_skipped = Recurring post skipped
push-title-delivery_published = Post published
push-title-reconnect_reminder = Reconnect soon
push-title-delegation_changed = Shared access changed
push-title-new_comments = New comments
push-title-user_pending = New user waiting

push-body-delivery_dead_lettered = { notice-delivery_dead_lettered }
push-body-delivery_failed = { notice-delivery_failed }
push-body-delivery_outcome_unknown = { notice-delivery_outcome_unknown }
push-body-delivery_parked = { notice-delivery_parked }
push-body-delivery_stale = { notice-delivery_stale }
push-body-job_partial = { notice-job_partial }
push-body-account_needs_reconnect = { notice-account_needs_reconnect }
push-body-account_revoked_by_platform = { notice-account_revoked_by_platform }
push-body-reconnect_by_passed = { notice-reconnect_by_passed }
push-body-recurrence_skipped = { notice-recurrence_skipped }
push-body-delivery_published = { notice-delivery_published }
push-body-reconnect_reminder = { notice-reconnect_reminder }
push-body-delegation_changed = { notice-delegation_changed }
push-body-new_comments = { notice-new_comments }
push-body-user_pending = { notice-user_pending }
