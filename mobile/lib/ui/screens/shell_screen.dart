import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/format.dart';

/// Home, Profile and (admins) Users. Plan 03 D6 puts Posts and Calendar in slots 2 and 3.
class ShellScreen extends ConsumerWidget {
  const ShellScreen({super.key, required this.shell});
  final StatefulNavigationShell shell;

  static const _wide = 600.0;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = AppLocalizations.of(context);
    final isAdmin = ref.watch(meProvider).value?.role == RoleDto.admin;
    final pending = isAdmin ? (ref.watch(pendingCountProvider).value ?? 0) : 0;
    Widget usersIcon(IconData icon) =>
        Badge(isLabelVisible: pending > 0, label: Text(formatNumber(context, pending)), child: Icon(icon));
    final items = [
      (icon: const Icon(Icons.home_outlined), selected: const Icon(Icons.home), label: l.nav_home),
      (icon: const Icon(Icons.person_outline), selected: const Icon(Icons.person), label: l.nav_profile),
      if (isAdmin) (icon: usersIcon(Icons.group_outlined), selected: usersIcon(Icons.group), label: l.nav_users),
    ];
    void go(int i) => shell.goBranch(i, initialLocation: i == shell.currentIndex);
    final index = shell.currentIndex.clamp(0, items.length - 1);

    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth >= _wide) {
          return Scaffold(
            body: Row(
              children: [
                NavigationRail(
                  selectedIndex: index,
                  onDestinationSelected: go,
                  labelType: NavigationRailLabelType.all,
                  destinations: [
                    for (final i in items)
                      NavigationRailDestination(icon: i.icon, selectedIcon: i.selected, label: Text(i.label)),
                  ],
                ),
                const VerticalDivider(width: 1),
                Expanded(child: shell),
              ],
            ),
          );
        }
        return Scaffold(
          body: shell,
          bottomNavigationBar: NavigationBar(
            selectedIndex: index,
            onDestinationSelected: go,
            destinations: [
              for (final i in items) NavigationDestination(icon: i.icon, selectedIcon: i.selected, label: i.label),
            ],
          ),
        );
      },
    );
  }
}
