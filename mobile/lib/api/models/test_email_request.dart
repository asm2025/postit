// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'email_document.dart';

part 'test_email_request.g.dart';

/// An email to send once, as a test, to the signed-in user's own address.
@JsonSerializable()
class TestEmailRequest {
  const TestEmailRequest({
    required this.email,
  });
  
  factory TestEmailRequest.fromJson(Map<String, Object?> json) => _$TestEmailRequestFromJson(json);
  
  /// The document, saved or not.
  final EmailDocument email;

  Map<String, Object?> toJson() => _$TestEmailRequestToJson(this);
}
