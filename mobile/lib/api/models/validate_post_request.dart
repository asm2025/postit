// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'schedule_input.dart';

part 'validate_post_request.g.dart';

/// A saved post to validate.
@JsonSerializable()
class ValidatePostRequest {
  const ValidatePostRequest({
    this.at,
    this.nativeSchedule,
    this.targets,
  });
  
  factory ValidatePostRequest.fromJson(Map<String, Object?> json) => _$ValidatePostRequestFromJson(json);
  
  /// When it would go out; now when absent.
  final ScheduleInput? at;

  /// Whether platforms that can hold the post until `at` would do so.
  @JsonKey(name: 'native_schedule')
  final bool? nativeSchedule;

  /// The accounts to check; the post's own targets when absent.
  final List<String>? targets;

  Map<String, Object?> toJson() => _$ValidatePostRequestToJson(this);
}
