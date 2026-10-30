import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/ui/errors/messages.dart';

Future<BuildContext> _context(WidgetTester tester) async {
  late BuildContext captured;
  await tester.pumpWidget(
    MaterialApp(
      locale: const Locale('en'),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      home: Builder(
        builder: (context) {
          captured = context;
          return const SizedBox();
        },
      ),
    ),
  );
  return captured;
}

void main() {
  testWidgets('known codes have messages', (tester) async {
    final context = await _context(tester);
    expect(messageFor(context, const ApiError(409, 'last_admin')), 'That would remove the last active admin.');
    expect(messageFor(context, const ApiError(0, 'network')), contains("Can't reach postit"));
    expect(
      messageFor(context, const ApiError(404, 'push_disabled')),
      "Push notifications aren't available on this server.",
    );
    expect(
      messageFor(context, const ApiError(422, 'push_endpoint_invalid')),
      contains("couldn't be registered for notifications"),
    );
  });

  testWidgets('an unknown code gets the generic message, never the server detail', (tester) async {
    final context = await _context(tester);
    expect(messageFor(context, const ApiError(418, 'teapot', 'Short and stout.')), 'Something went wrong. Try again.');
  });

  testWidgets('an unreachable identity provider reads as a network problem', (tester) async {
    final context = await _context(tester);
    expect(messageFor(context, const AuthUnavailable()), contains("Can't reach postit"));
  });

  testWidgets('anything else gets the generic message', (tester) async {
    final context = await _context(tester);
    expect(messageFor(context, StateError('x')), 'Something went wrong. Try again.');
  });
}
