// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'test_email_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TestEmailRequest _$TestEmailRequestFromJson(Map<String, dynamic> json) =>
    TestEmailRequest(
      email: EmailDocument.fromJson(json['email'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$TestEmailRequestToJson(TestEmailRequest instance) =>
    <String, dynamic>{'email': instance.email};
