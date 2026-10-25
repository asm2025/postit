import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/http/api_error.dart';

/// The screen the app must show; [path] is null for the signed-in shell.
enum GateTarget {
  startup('/startup'),
  error('/error'),
  notConfigured('/not-configured'),
  signIn('/sign-in'),
  pending('/pending'),
  unavailable('/unavailable'),
  shell(null);

  const GateTarget(this.path);
  final String? path;
}

/// Same rules as the web app's gate: startup, then session, then `/me`.
GateTarget gateTarget({
  required AsyncValue<SessionManager?> startup,
  required SessionState session,
  required AsyncValue<MeDto> me,
}) {
  if (startup.isLoading) return GateTarget.startup;
  if (startup.hasError) return GateTarget.error;
  if (startup.value == null) return GateTarget.notConfigured;
  switch (session.status) {
    case SessionStatus.loading:
      return GateTarget.startup;
    case SessionStatus.signedOut:
      return GateTarget.signIn;
    case SessionStatus.signedIn:
      break;
  }
  if (me.isLoading && !me.hasValue) return GateTarget.startup;
  if (me.hasError && !me.isLoading) {
    final error = me.error;
    final code = error is ApiError ? error.code : '';
    return switch (code) {
      'account_disabled' => GateTarget.unavailable,
      'account_pending' => GateTarget.pending,
      _ => GateTarget.error,
    };
  }
  final user = me.value;
  if (user == null) return GateTarget.startup;
  return switch (user.status) {
    StatusDto.pending => GateTarget.pending,
    StatusDto.active => GateTarget.shell,
    _ => GateTarget.unavailable,
  };
}

/// go_router redirect for [target] at [location].
String? redirectFor(GateTarget target, String location, {required bool isAdmin}) {
  final path = target.path;
  if (path != null) return location == path ? null : path;
  final onGateScreen = location == '/' || GateTarget.values.any((t) => t.path == location);
  if (onGateScreen) return '/home';
  if (location.startsWith('/users') && !isAdmin) return '/home';
  return null;
}
