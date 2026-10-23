// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_account_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdateAccountRequest _$UpdateAccountRequestFromJson(
  Map<String, dynamic> json,
) => UpdateAccountRequest(
  displayName: json['display_name'] as String?,
  testAccount: json['test_account'] as bool?,
);

Map<String, dynamic> _$UpdateAccountRequestToJson(
  UpdateAccountRequest instance,
) => <String, dynamic>{
  'display_name': instance.displayName,
  'test_account': instance.testAccount,
};
