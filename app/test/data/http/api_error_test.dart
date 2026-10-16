import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';

void main() {
  final req = RequestOptions(path: '/api/v1/users');

  test('reads code and detail from problem+json', () {
    final e = ApiError.from(409, {'code': 'last_admin', 'detail': 'nope', 'status': 409});
    expect((e.status, e.code, e.detail), (409, 'last_admin', 'nope'));
  });

  test('a body without a code is unknown', () {
    expect(ApiError.from(500, 'oops').code, 'unknown');
  });

  test('a dio response error becomes its problem', () {
    final e = ApiError.fromDio(
      DioException(
        requestOptions: req,
        response: Response(requestOptions: req, statusCode: 403, data: {'code': 'forbidden'}),
      ),
    );
    expect((e.status, e.code), (403, 'forbidden'));
  });

  test('no response means a network error', () {
    final e = ApiError.fromDio(DioException(requestOptions: req, type: DioExceptionType.connectionError));
    expect((e.status, e.code), (0, 'network'));
  });

  test('a signed-out session is unauthenticated', () {
    final e = ApiError.fromDio(DioException(requestOptions: req, error: const NotSignedIn()));
    expect((e.status, e.code), (401, 'unauthenticated'));
  });

  test('an unreachable identity provider is a network error', () {
    final e = ApiError.fromDio(DioException(requestOptions: req, error: const AuthUnavailable()));
    expect((e.status, e.code), (0, 'network'));
  });
}
