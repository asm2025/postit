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
