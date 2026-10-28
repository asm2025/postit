// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'start_o_auth_request.g.dart';

/// Where the browser goes back to once the platform's sign-in is over.
@JsonSerializable()
class StartOAuthRequest {
  const StartOAuthRequest({
    required this.returnTo,
  });
  
  factory StartOAuthRequest.fromJson(Map<String, Object?> json) => _$StartOAuthRequestFromJson(json);
  
  /// Exactly one of the deployment's allowed return URLs: the web app's `/oauth-done`, the.
  /// mobile app's `<scheme>://oauth-done`, or a configured entry. No prefix match, query, or.
  /// fragment.
  @JsonKey(name: 'return_to')
  final String returnTo;

  Map<String, Object?> toJson() => _$StartOAuthRequestToJson(this);
}
