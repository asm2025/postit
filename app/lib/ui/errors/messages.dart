import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';

const _messages = <String, String>{
  'unauthenticated': 'Your session expired. Sign in again.',
  'account_pending': 'Your account is waiting for approval.',
  'account_disabled': 'Your account is disabled.',
  'forbidden': 'You are not allowed to do that.',
  'not_found': 'That item no longer exists.',
  'validation_failed': 'The request was not valid.',
  'last_admin': 'That would remove the last active admin.',
  'user_deleting': 'That user is being deleted.',
  'rate_limited': 'Too many requests. Wait a moment and try again.',
  'version_conflict': 'Someone else changed this. Reload and try again.',
  'precondition_required': 'Reload and try again.',
  'idempotency_key_reused': 'That request was already used with different data.',
  'idempotency_in_progress': 'That request is already running.',
  'request_timeout': 'The request took too long. Try again.',
  'payload_too_large': 'That is too large to send.',
  'job_action_not_allowed': "That job can't take this action right now. It may have changed; refresh and try again.",
  'unavailable': 'The service is starting up. Try again shortly.',
  'internal': 'Something went wrong on the server.',
  'network': "Can't reach postit. Check your connection and try again.",
};

/// The user-facing message for an error.
String messageFor(Object error) {
  if (error is AuthUnavailable) return _messages['network']!;
  if (error is ApiError) {
    return _messages[error.code] ?? error.detail ?? 'Something went wrong. Try again.';
  }
  return 'Something went wrong. Try again.';
}
