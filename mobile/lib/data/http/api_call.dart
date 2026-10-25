import 'package:dio/dio.dart';
import 'package:postit/data/http/api_error.dart';

/// Runs a generated-client call and turns its `DioException` into an [ApiError].
Future<T> apiCall<T>(Future<T> Function() call) async {
  try {
    return await call();
  } on DioException catch (e) {
    throw ApiError.fromDio(e);
  }
}
