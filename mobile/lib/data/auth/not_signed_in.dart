/// Thrown when an API call needs a session and there is none.
class NotSignedIn implements Exception {
  const NotSignedIn();
}

/// The identity provider could not be reached (offline, DNS, TLS, timeout). The session is
/// kept: this is not a rejection, and retrying later may succeed.
class AuthUnavailable implements Exception {
  const AuthUnavailable();
}
