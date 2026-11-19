// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'lookup_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LookupResponse _$LookupResponseFromJson(Map<String, dynamic> json) =>
    LookupResponse(
      user: json['user'] == null
          ? null
          : LookupUserDto.fromJson(json['user'] as Map<String, dynamic>),
      additionalProperties:
          LookupResponse._extras(json, 'additionalProperties') == null
          ? const {}
          : LookupResponse._readExtras(
              LookupResponse._extras(json, 'additionalProperties'),
            ),
    );

Map<String, dynamic> _$LookupResponseToJson(LookupResponse instance) =>
    <String, dynamic>{'user': ?instance.user?.toJson()};
