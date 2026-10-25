import 'package:flutter/widgets.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';

String statusLabel(BuildContext context, StatusDto s) {
  final l = AppLocalizations.of(context);
  return switch (s) {
    StatusDto.pending => l.user_status_pending,
    StatusDto.active => l.user_status_active,
    StatusDto.disabled => l.user_status_disabled,
    _ => l.user_status_deleting,
  };
}

String roleLabel(BuildContext context, RoleDto r) {
  final l = AppLocalizations.of(context);
  return r == RoleDto.admin ? l.role_admin : l.role_member;
}
