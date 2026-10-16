import 'dart:convert';
import 'dart:typed_data';

import 'package:dio/dio.dart';

/// A dio adapter that answers from [respond] and records every request.
class ScriptedAdapter implements HttpClientAdapter {
  ScriptedAdapter(this.respond);

  final Future<ResponseBody> Function(RequestOptions options) respond;
  final requests = <RequestOptions>[];

  static ResponseBody json(int status, Object? body) => ResponseBody.fromString(
    jsonEncode(body),
    status,
    headers: {
      Headers.contentTypeHeader: ['application/json'],
    },
  );

  @override
  Future<ResponseBody> fetch(RequestOptions options, Stream<Uint8List>? requestStream, Future<void>? cancelFuture) {
    requests.add(options);
    return respond(options);
  }

  @override
  void close({bool force = false}) {}
}
