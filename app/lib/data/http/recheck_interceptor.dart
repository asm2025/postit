import 'package:dio/dio.dart';

/// A pending, disabled or forbidden answer means the user's status or role changed, so
/// `/me` is fetched again and the gate moves the user (a demoted admin loses Users).
class RecheckInterceptor extends Interceptor {
  RecheckInterceptor(this._onRecheck);

  final void Function() _onRecheck;
  static const _codes = {'account_pending', 'account_disabled', 'forbidden'};

  @override
  void onError(DioException err, ErrorInterceptorHandler handler) {
    final data = err.response?.data;
    if (!err.requestOptions.path.endsWith('/api/v1/me') && data is Map && _codes.contains(data['code'])) {
      _onRecheck();
    }
    handler.next(err);
  }
}
