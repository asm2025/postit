import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/gate.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/screens/home_screen.dart';
import 'package:postit/ui/screens/profile_screen.dart';
import 'package:postit/ui/screens/shell_screen.dart';
import 'package:postit/ui/screens/sign_in_screen.dart';
import 'package:postit/ui/screens/status_screens.dart';
import 'package:postit/ui/screens/user_detail_screen.dart';
import 'package:postit/ui/screens/users_screen.dart';

final routerProvider = Provider<GoRouter>((ref) {
  final refresh = ValueNotifier<int>(0);
  void bump(Object? previous, Object? next) => refresh.value++;
  ref
    ..listen(sessionManagerProvider, bump)
    ..listen(sessionStateProvider, bump)
    ..listen(meProvider, bump)
    ..onDispose(refresh.dispose);

  final router = GoRouter(
    initialLocation: '/startup',
    refreshListenable: refresh,
    redirect: (context, state) {
      final target = gateTarget(
        startup: ref.read(sessionManagerProvider),
        session: ref.read(sessionStateProvider),
        me: ref.read(meProvider),
      );
      final isAdmin = ref.read(meProvider).value?.role == RoleDto.admin;
      return redirectFor(target, state.matchedLocation, isAdmin: isAdmin);
    },
    routes: [
      GoRoute(path: '/', redirect: (_, _) => '/home'),
      GoRoute(path: '/startup', builder: (_, _) => const StartupScreen()),
      GoRoute(path: '/error', builder: (_, _) => const ErrorScreen()),
      GoRoute(path: '/not-configured', builder: (_, _) => const NotConfiguredScreen()),
      GoRoute(path: '/sign-in', builder: (_, _) => const SignInScreen()),
      GoRoute(path: '/pending', builder: (_, _) => const PendingScreen()),
      GoRoute(path: '/unavailable', builder: (_, _) => const UnavailableScreen()),
      StatefulShellRoute.indexedStack(
        builder: (_, _, shell) => ShellScreen(shell: shell),
        branches: [
          StatefulShellBranch(
            routes: [GoRoute(path: '/home', builder: (_, _) => const HomeScreen())],
          ),
          StatefulShellBranch(
            routes: [GoRoute(path: '/profile', builder: (_, _) => const ProfileScreen())],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: '/users',
                builder: (_, _) => const UsersScreen(),
                routes: [
                  GoRoute(
                    path: ':id',
                    builder: (_, state) => UserDetailScreen(id: state.pathParameters['id']!),
                  ),
                ],
              ),
            ],
          ),
        ],
      ),
    ],
  );
  ref.onDispose(router.dispose);
  return router;
});
