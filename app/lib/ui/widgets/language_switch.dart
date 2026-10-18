import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/ui/errors/messages.dart';

/// English and Arabic, each named in its own language. With [persist] the choice is also saved to
/// the signed-in user's preferences; before sign-in it stays on this device.
class LanguageSwitch extends ConsumerStatefulWidget {
  const LanguageSwitch({super.key, this.persist = false});

  final bool persist;

  @override
  ConsumerState<LanguageSwitch> createState() => _LanguageSwitchState();
}

class _LanguageSwitchState extends ConsumerState<LanguageSwitch> {
  Object? _error;

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    children: [
      Semantics(
        label: AppLocalizations.of(context).language_label,
        container: true,
        child: SegmentedButton<AppLocale>(
          showSelectedIcon: false,
          segments: const [
            ButtonSegment(value: AppLocale.en, label: Text('English')),
            ButtonSegment(value: AppLocale.ar, label: Text('العربية')),
          ],
          selected: {ref.watch(localeProvider)},
          onSelectionChanged: (s) => _select(s.single),
        ),
      ),
      if (_error != null)
        Padding(
          padding: const EdgeInsets.only(top: 4),
          child: Text(messageFor(context, _error!), style: TextStyle(color: Theme.of(context).colorScheme.error)),
        ),
    ],
  );

  Future<void> _select(AppLocale locale) async {
    setState(() => _error = null);
    try {
      await ref.read(localeProvider.notifier).set(locale, persist: widget.persist);
    } on Object catch (e) {
      // The language went back to the saved one; say why.
      if (mounted) setState(() => _error = e);
    }
  }
}
