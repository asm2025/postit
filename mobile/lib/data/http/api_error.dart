import 'package:dio/dio.dart';
import 'package:postit/data/auth/not_signed_in.dart';

/// A failed API call: the problem+json `code` plus its HTTP status. Status 0 and code
/// `network` mean the server was not reached.
class ApiError implements Exception {
  const ApiError(this.status, this.code, [this.detail]);

  final int status;
  final String code;
  final String? detail;

  factory ApiError.from(int status, Object? body) {
    if (body is Map) {
      final code = body['code'];
      final detail = body['detail'];
      return ApiError(status, code is String ? code : 'unknown', detail is String ? detail : null);
    }
    return ApiError(status, 'unknown');
  }

  factory ApiError.fromDio(DioException e) {
    if (e.error is NotSignedIn) return const ApiError(401, 'unauthenticated');
    if (e.error is AuthUnavailable) return const ApiError(0, 'network');
    final response = e.response;
    if (response == null) return const ApiError(0, 'network');
    return ApiError.from(response.statusCode ?? 0, response.data);
  }

  @override
  String toString() => 'ApiError($status, $code)';
}
