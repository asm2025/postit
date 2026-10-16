import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/ui/errors/messages.dart';

void main() {
  test('known codes have messages', () {
    expect(messageFor(const ApiError(409, 'last_admin')), 'That would remove the last active admin.');
    expect(messageFor(const ApiError(0, 'network')), contains("Can't reach postit"));
  });

  test('an unknown code shows the server detail', () {
    expect(messageFor(const ApiError(418, 'teapot', 'Short and stout.')), 'Short and stout.');
  });

  test('an unreachable identity provider reads as a network problem', () {
    expect(messageFor(const AuthUnavailable()), contains("Can't reach postit"));
  });

  test('anything else gets the generic message', () {
    expect(messageFor(StateError('x')), 'Something went wrong. Try again.');
  });
}
