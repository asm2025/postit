import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:intl/intl.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/account_card.dart';

String greetingFor(DateTime now) => now.hour < 12
    ? 'Good morning'
    : now.hour < 18
    ? 'Good afternoon'
    : 'Good evening';

class HomeScreen extends ConsumerWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final me = ref.watch(meProvider).value;
    if (me == null) return const SizedBox.shrink();
    final now = ref.watch(clockProvider)();
    final isAdmin = me.role == RoleDto.admin;
    final text = Theme.of(context).textTheme;
    final tokens = context.tokens;
    final firstName = me.displayName.split(' ').first;

    return Scaffold(
      body: SafeArea(
        child: RefreshIndicator(
          onRefresh: () async {
            ref
              ..invalidate(meProvider)
              ..invalidate(pendingCountProvider)
              ..invalidate(activeAdminCountProvider);
          },
          child: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Text(DateFormat('EEEE, d MMMM').format(now).toUpperCase(), style: text.labelSmall),
              const SizedBox(height: 4),
              Text('${greetingFor(now)}, $firstName', style: text.headlineMedium),
              const SizedBox(height: 16),
              if (isAdmin && ref.watch(activeAdminCountProvider).value == 1)
                Card(
                  color: tokens.warnBg,
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                    side: BorderSide(color: tokens.warnLine),
                  ),
                  child: ListTile(
                    leading: Icon(Icons.warning_amber_rounded, color: tokens.warnIcon),
                    title: Text(
                      "You're the only active admin. Make someone else an admin so the team isn't locked out.",
                      style: TextStyle(color: tokens.warnText),
                    ),
                  ),
                ),
              if (isAdmin) ...[
                const SizedBox(height: 8),
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Row(
                      children: [
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text('APPROVAL QUEUE', style: text.labelSmall),
                              Text('${ref.watch(pendingCountProvider).value ?? 0}', style: text.displaySmall),
                              Text('waiting for approval', style: text.bodySmall),
                            ],
                          ),
                        ),
                        FilledButton(onPressed: () => context.go('/users'), child: const Text('Review')),
                      ],
                    ),
                  ),
                ),
              ],
              const SizedBox(height: 8),
              AccountCard(me: me),
            ],
          ),
        ),
      ),
    );
  }
}
