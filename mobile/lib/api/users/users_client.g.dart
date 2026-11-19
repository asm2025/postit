// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'users_client.dart';

// dart format off

// **************************************************************************
// RetrofitGenerator
// **************************************************************************

// ignore_for_file: type=lint
// ignore_for_file: unnecessary_brace_in_string_interps,no_leading_underscores_for_local_identifiers,unused_element,unnecessary_string_interpolations,unused_element_parameter,avoid_unused_constructor_parameters,unreachable_from_main,avoid_redundant_argument_values

class _UsersClient extends UsersClient {
  _UsersClient._(this._dio, Dio _transport, {this.baseUrl, this.errorLogger})
    : super._(_transport);

  final Dio _dio;

  String? baseUrl;

  final ParseErrorLogger? errorLogger;

  @override
  Future<PageUserDto> listUsers({
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
    String? status,
    String? role,
    String? sort,
    String? search,
    int? page,
    int? pageSize,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{
      r'status': status,
      r'role': role,
      r'sort': sort,
      r'search': search,
      r'page': page,
      r'page_size': pageSize,
    };
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<PageUserDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/users',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late PageUserDto _value;
    try {
      _value = PageUserDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<LookupResponse> lookupUser({
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
    required String email,
  }) async {
    final _extra = <String, dynamic>{};
    _extra.addAll(extra);
    final queryParameters = <String, dynamic>{r'email': email};
    queryParameters.removeWhere((k, v) => v == null);
    final _headers = <String, dynamic>{r'Accept': 'application/json'};
    _headers.removeWhere((k, v) => v == null);
    const Map<String, dynamic>? _data = null;
    final _options = _setStreamType<LookupResponse>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/users/lookup',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late LookupResponse _value;
    try {
      _value = LookupResponse.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<UserDto> getUser({
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
    final _options = _setStreamType<UserDto>(
      Options(method: 'GET', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/users/${id}',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late UserDto _value;
    try {
      _value = UserDto.fromJson(_result.data!);
    } on Object catch (e, s) {
      errorLogger?.logError(e, s, _options, response: _result);
      rethrow;
    }
    return _value;
  }

  @override
  Future<void> deleteUser({
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
    final _options = _setStreamType<void>(
      Options(method: 'DELETE', headers: _headers, extra: _extra)
          .compose(
            _dio.options,
            '/api/v1/users/${id}',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    await _dio.fetch<void>(_options);
  }

  @override
  Future<UserDto> updateUser({
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
    required PatchUserRequest body,
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
    final _options = _setStreamType<UserDto>(
      Options(
            method: 'PATCH',
            headers: _headers,
            extra: _extra,
            contentType: 'application/json',
          )
          .compose(
            _dio.options,
            '/api/v1/users/${id}',
            queryParameters: queryParameters,
            data: _data,
            cancelToken: cancelToken,
          )
          .copyWith(baseUrl: _combineBaseUrls(_dio.options.baseUrl, baseUrl)),
    );
    final _result = await _dio.fetch<Map<String, Object?>>(_options);
    late UserDto _value;
    try {
      _value = UserDto.fromJson(_result.data!);
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
