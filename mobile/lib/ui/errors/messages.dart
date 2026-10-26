import 'package:flutter/widgets.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/l10n/gen/app_localizations.dart';

/// The user-facing message for an error, in the language on screen. An unknown code gets the
/// generic message: the server's `detail` is English for developers and is never shown.
String messageFor(BuildContext context, Object error) {
  final l = AppLocalizations.of(context);
  if (error is AuthUnavailable) return l.error_network;
  if (error is ApiError) {
    if (error.code == 'network') return l.error_network;
    return {
          'unauthenticated': l.error_unauthenticated,
          'account_pending': l.error_account_pending,
          'account_disabled': l.error_account_disabled,
          'forbidden': l.error_forbidden,
          'not_found': l.error_not_found,
          'validation_failed': l.error_validation_failed,
          'last_admin': l.error_last_admin,
          'user_deleting': l.error_user_deleting,
          'rate_limited': l.error_rate_limited,
          'version_conflict': l.error_version_conflict,
          'precondition_required': l.error_precondition_required,
          'idempotency_key_reused': l.error_idempotency_key_reused,
          'idempotency_in_progress': l.error_idempotency_in_progress,
          'request_timeout': l.error_request_timeout,
          'payload_too_large': l.error_payload_too_large,
          'job_action_not_allowed': l.error_job_action_not_allowed,
          'owner_only': l.error_owner_only,
          'capability_required': l.error_capability_required,
          'delegation_invalid': l.error_delegation_invalid,
          'delegation_exists': l.error_delegation_exists,
          'credentials_invalid': l.error_credentials_invalid,
          'platform_disabled': l.error_platform_disabled,
          'platform_unavailable': l.error_platform_unavailable,
          'capability_unsupported': l.error_capability_unsupported,
          'sending_method_exists': l.error_sending_method_exists,
          'job_active': l.error_job_active,
          'media_in_use': l.error_media_in_use,
          'upload_offset_mismatch': l.error_upload_offset_mismatch,
          'storage_quota_exceeded': l.error_storage_quota_exceeded,
          'unsupported_media_type': l.error_unsupported_media_type,
          'unavailable': l.error_unavailable,
          'internal': l.error_internal,
        }[error.code] ??
        l.error_generic;
  }
  return l.error_generic;
}
