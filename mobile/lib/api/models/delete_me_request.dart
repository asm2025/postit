// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'delete_me_request.g.dart';

/// Confirmation for deleting one's own account.
@JsonSerializable()
class DeleteMeRequest {
  const DeleteMeRequest({
    required this.displayName,
  });
  
  factory DeleteMeRequest.fromJson(Map<String, Object?> json) => _$DeleteMeRequestFromJson(json);
  
  /// Must equal the caller's current display name.
  @JsonKey(name: 'display_name')
  final String displayName;

  Map<String, Object?> toJson() => _$DeleteMeRequestToJson(this);
}
