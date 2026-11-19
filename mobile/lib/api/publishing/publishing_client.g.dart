// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publishing_client.dart';

// dart format off

// **************************************************************************
// RetrofitGenerator
// **************************************************************************

// ignore_for_file: type=lint
// ignore_for_file: unnecessary_brace_in_string_interps,no_leading_underscores_for_local_identifiers,unused_element,unnecessary_string_interpolations,unused_element_parameter,avoid_unused_constructor_parameters,unreachable_from_main,avoid_redundant_argument_values

class _PublishingClient extends PublishingClient {
  _PublishingClient._(
    this._dio,
    Dio _transport, {
    this.baseUrl,
    this.errorLogger,
  }) : super._(_transport);

  final Dio _dio;

  String? baseUrl;

  final ParseErrorLogger? errorLogger;

  @override
  Future<CalendarDto> getCalendar({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String from,
    required String to,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{r'from': from, r'to': to};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<CalendarDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/calendar',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late CalendarDto _value;
    try {
      _value = CalendarDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<DeliveryDto> getDelivery({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<DeliveryDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/deliveries/${id}',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late DeliveryDto _value;
    try {
      _value = DeliveryDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<CancelReportDto> cancelDelivery({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<CancelReportDto>(
      Options(method: 'POST', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/deliveries/${id}/cancel',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late CancelReportDto _value;
    try {
      _value = CancelReportDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<DeliveryDto> resolveDelivery({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
    required ResolveRequest body,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{
      r'Accept': 'application/json',
      r'Content-Type': 'application/json',
    };
    _headers.removeWhere((k, v) => v == null);
    final _data = <String, dynamic>{};
    _data.addAll(body.toJson());
    final _options = _setStreamType<DeliveryDto>(
      Options(
            method: 'POST',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/deliveries/${id}/resolve',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late DeliveryDto _value;
    try {
      _value = DeliveryDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<DeliveryDto> retryDelivery({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
    required RetryRequest body,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{
      r'Accept': 'application/json',
      r'Content-Type': 'application/json',
    };
    _headers.removeWhere((k, v) => v == null);
    final _data = <String, dynamic>{};
    _data.addAll(body.toJson());
    final _options = _setStreamType<DeliveryDto>(
      Options(
            method: 'POST',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/deliveries/${id}/retry',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late DeliveryDto _value;
    try {
      _value = DeliveryDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<PublishJobDto> publishPost({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
    required PublishRequest body,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{
      r'Accept': 'application/json',
      r'Content-Type': 'application/json',
    };
    _headers.removeWhere((k, v) => v == null);
    final _data = <String, dynamic>{};
    _data.addAll(body.toJson());
    final _options = _setStreamType<PublishJobDto>(
      Options(
            method: 'POST',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/posts/${id}/publish',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late PublishJobDto _value;
    try {
      _value = PublishJobDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<PublishJobDto> getPublishJob({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<PublishJobDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/publish-jobs/${id}',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late PublishJobDto _value;
    try {
      _value = PublishJobDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<RescheduleOutcomeDto> reschedulePublishJob({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
    required RescheduleRequest body,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{
      r'Accept': 'application/json',
      r'Content-Type': 'application/json',
    };
    _headers.removeWhere((k, v) => v == null);
    final _data = <String, dynamic>{};
    _data.addAll(body.toJson());
    final _options = _setStreamType<RescheduleOutcomeDto>(
      Options(
            method: 'PATCH',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/publish-jobs/${id}',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late RescheduleOutcomeDto _value;
    try {
      _value = RescheduleOutcomeDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<CancelReportDto> cancelPublishJob({
    Map<String, Object?> extra = const {
      "openapi.security": [
        {"oidc": []},
      ],
      "openapi.securitySchemes": {
        "oidc": {
          "type": "oauth2",
          "flows": {
            "authorizationCode": {
              "scopes": {
                "email": "Email address",
                "offline_access": "Refresh tokens",
                "openid": "OpenID Connect sign-in",
                "profile": "Name and username",
              },
              "authorizationUrl": "https://idp.invalid/authorize",
              "tokenUrl": "https://idp.invalid/token",
            },
          },
        },
      },
    },
    CancelToken? cancelToken,
    Options? options,
    required String id,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<CancelReportDto>(
      Options(method: 'POST', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/publish-jobs/${id}/cancel',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late CancelReportDto _value;
    try {
      _value = CancelReportDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  RequestOptions _setStreamType<T>(RequestOptions requestOptions) {
    if (T != dynamic &&
        !(requestOptions.responseType == ResponseType.bytes ||
            requestOptions.responseType == ResponseType.stream)) {
      if (T == String) {
        requestOptions.responseType = ResponseType.plain;
      } else {
        requestOptions.responseType = ResponseType.json;
      }
    }
    return requestOptions;
  }

  String _combineBaseUrls(String dioBaseUrl, String? baseUrl) {
    if (baseUrl == null || baseUrl.trim().isEmpty) {
      return dioBaseUrl;
    }

    final url = Uri.parse(baseUrl);

    if (url.isAbsolute) {
      return url.toString();
    }

    return Uri.parse(dioBaseUrl).resolveUri(url).toString();
  }
}

// dart format on
