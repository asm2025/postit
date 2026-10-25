## Admin digest of accounts waiting for approval

pending-subject = { $count ->
    [one] { $count } postit account is waiting for approval
   *[other] { $count } postit accounts are waiting for approval
}
pending-intro = { $count ->
    [one] This account is waiting for approval on postit:
   *[other] These accounts are waiting for approval on postit:
}
pending-more = ...and { $more } more.
pending-review = Review them
pending-footer = You are receiving this email because you are a postit administrator.

## Notice that an account was approved

approved-subject = Your postit account was approved
approved-greeting = Hi { $name },
approved-body = An administrator approved your postit account.
approved-sign-in = Sign in now

## Identity messages from Zitadel

identity-link-fallback = If the button does not work, copy this link into your browser:
identity-code-label = Your code is:
