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
    expect(messageFor(context, const ApiError(409, 'not_retryable')), "This delivery can't be sent again.");
    expect(
      messageFor(context, const ApiError(409, 'rotation_in_progress')),
      'A vault key rotation is already running.',
    );
    expect(messageFor(context, const ApiError(0, 'network')), contains("Can't reach postit"));
    expect(
      messageFor(context, const ApiError(404, 'push_disabled')),
      "Push notifications aren't available on this server.",
    );
    expect(
      messageFor(context, const ApiError(422, 'push_endpoint_invalid')),
      contains("couldn't be registered for notifications"),
    );
    expect(
      messageFor(context, const ApiError(413, 'csv_too_large')),
      'That file is too large. Split it into smaller files.',
    );
    expect(messageFor(context, const ApiError(409, 'contact_limit_exceeded')), contains('contacts limit'));
    expect(
      messageFor(context, const ApiError(409, 'email_address_missing')),
      'Your profile has no verified email address to send the test to.',
    );
    expect(
      messageFor(context, const ApiError(409, 'live_publish_disabled')),
      'Live publishing is switched off on this server. Ask an admin to turn it on.',
    );
    expect(
      messageFor(context, const ApiError(409, 'test_send_refused', 'smtp_5_1_1')),
      "The mail server didn't take the test email for your address. It refused it or couldn't take it right now.",
    );
    expect(messageFor(context, const ApiError(409, 'capability_unsupported')), "This platform doesn't support that.");
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
