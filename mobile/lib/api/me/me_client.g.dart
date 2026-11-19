// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'me_client.dart';

// dart format off

// **************************************************************************
// RetrofitGenerator
// **************************************************************************

// ignore_for_file: type=lint
// ignore_for_file: unnecessary_brace_in_string_interps,no_leading_underscores_for_local_identifiers,unused_element,unnecessary_string_interpolations,unused_element_parameter,avoid_unused_constructor_parameters,unreachable_from_main,avoid_redundant_argument_values

class _MeClient extends MeClient {
  _MeClient._(this._dio, Dio _transport, {this.baseUrl, this.errorLogger})
    : super._(_transport);

  final Dio _dio;

  String? baseUrl;

  final ParseErrorLogger? errorLogger;

  @override
  Future<MeDto> getMe({
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
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<MeDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/me',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late MeDto _value;
    try {
      _value = MeDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<void> deleteMe({
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
    required DeleteMeRequest body,
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
    final _options = _setStreamType<void>(
      Options(
            method: 'DELETE',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/me',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    await _dio.fetch<void>(_options);
  }

  @override
  Future<PreferencesDto> getPreferences({
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
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<PreferencesDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/me/preferences',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late PreferencesDto _value;
    try {
      _value = PreferencesDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<PreferencesDto> putPreferences({
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
    required UpdatePreferences body,
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
    final _options = _setStreamType<PreferencesDto>(
      Options(
            method: 'PUT',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/me/preferences',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late PreferencesDto _value;
    try {
      _value = PreferencesDto.fromJson(_result.data!);
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
