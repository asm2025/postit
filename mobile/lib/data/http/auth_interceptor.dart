import 'package:dio/dio.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';

/// Adds the bearer token; on 401 asks for one refresh and retries the request once. An
/// unreachable identity provider fails the call as a connection error, keeping the session.
class AuthInterceptor extends Interceptor {
  AuthInterceptor(this._session, this._dio);

  final SessionManager _session;
  final Dio _dio;
  static const _retried = 'postit.retried';

  static DioException _unreachable(RequestOptions options) =>
      DioException(requestOptions: options, type: DioExceptionType.connectionError, error: const AuthUnavailable());

  @override
  void onRequest(RequestOptions options, RequestInterceptorHandler handler) async {
    try {
      options.headers['Authorization'] = 'Bearer ${await _session.accessToken()}';
      handler.next(options);
    } on NotSignedIn catch (e) {
      handler.reject(DioException(requestOptions: options, error: e));
    } on AuthUnavailable {
      handler.reject(_unreachable(options));
    }
  }

  @override
  void onError(DioException err, ErrorInterceptorHandler handler) async {
    if (err.response?.statusCode != 401) return handler.next(err);
    if (err.requestOptions.extra[_retried] == true) {
      await _session.expire();
      return handler.next(err);
    }
    try {
      // A 401 that lands after a sibling's refresh already finished carries a stale token:
      // retry with the current one instead of refreshing again.
      final current = await _session.accessToken();
      if ('Bearer $current' == err.requestOptions.headers['Authorization']) await _session.refresh();
      final retry = err.requestOptions..extra[_retried] = true;
      handler.resolve(await _dio.fetch<dynamic>(retry));
    } on NotSignedIn {
      handler.next(err);
    } on AuthUnavailable {
      handler.next(_unreachable(err.requestOptions));
    } on DioException catch (e) {
      handler.next(e);
    }
  }
}
