import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/locale.dart';

/// English and Arabic, each named in its own language. With [persist] the choice is also saved to
/// the signed-in user's preferences; before sign-in it stays on this device.
class LanguageSwitch extends ConsumerWidget {
  const LanguageSwitch({super.key, this.persist = false});

  final bool persist;

  @override
  Widget build(BuildContext context, WidgetRef ref) => Semantics(
    label: AppLocalizations.of(context).language_label,
    container: true,
    child: SegmentedButton<AppLocale>(
      showSelectedIcon: false,
      segments: const [
        ButtonSegment(value: AppLocale.en, label: Text('English')),
        ButtonSegment(value: AppLocale.ar, label: Text('العربية')),
      ],
      selected: {ref.watch(localeProvider)},
      onSelectionChanged: (s) => _select(ref, s.single),
    ),
  );

  Future<void> _select(WidgetRef ref, AppLocale locale) async {
    try {
      await ref.read(localeProvider.notifier).set(locale, persist: persist);
    } on Object {
      // Saving to the account failed (offline): the language still changed on this device.
    }
  }
}
