// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_override.dart';

part 'put_override_request.g.dart';

/// An override to store.
@JsonSerializable()
class PutOverrideRequest {
  const PutOverrideRequest({
    this.options,
    this.patch,
  });
  
  factory PutOverrideRequest.fromJson(Map<String, Object?> json) => _$PutOverrideRequestFromJson(json);
  
  /// Platform options for this account, checked against the agent's schema when publishing.
  /// or validating.
  final dynamic options;

  /// The fields that differ from the post: text, link, tags, a subset of its media, the.
  /// email document for an email target.
  final PostOverride? patch;

  Map<String, Object?> toJson() => _$PutOverrideRequestToJson(this);
}
