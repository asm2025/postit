import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/format.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/account_card.dart';
import 'package:postit/ui/widgets/label_text.dart';

String greetingFor(AppLocalizations l, DateTime now, String name) => now.hour < 12
    ? l.home_greeting_morning(name)
    : now.hour < 18
    ? l.home_greeting_afternoon(name)
    : l.home_greeting_evening(name);

class HomeScreen extends ConsumerWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = AppLocalizations.of(context);
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
              LabelText(formatDate(context, now, pattern: 'EEEE, d MMMM')),
              const SizedBox(height: 4),
              Text(greetingFor(l, now, firstName), style: text.headlineMedium),
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
                    title: Text(l.home_admin_warning, style: TextStyle(color: tokens.warnText)),
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
                              LabelText(l.home_queue_title),
                              Text(
                                formatNumber(context, ref.watch(pendingCountProvider).value ?? 0),
                                style: text.displaySmall,
                              ),
                              Text(l.home_queue_waiting, style: text.bodySmall),
                            ],
                          ),
                        ),
                        FilledButton(onPressed: () => context.go('/users'), child: Text(l.home_queue_review)),
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
