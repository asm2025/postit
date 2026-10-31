// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'contact_list_request.g.dart';

/// A list's name, to create or rename it.
@JsonSerializable()
class ContactListRequest {
  const ContactListRequest({
    required this.name,
  });
  
  factory ContactListRequest.fromJson(Map<String, Object?> json) => _$ContactListRequestFromJson(json);
  
  /// The name; cannot be blank.
  final String name;

  Map<String, Object?> toJson() => _$ContactListRequestToJson(this);
}
