// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'confirm_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConfirmRequest _$ConfirmRequestFromJson(Map<String, dynamic> json) =>
    ConfirmRequest(
      accountIds: (json['account_ids'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
    );

Map<String, dynamic> _$ConfirmRequestToJson(ConfirmRequest instance) =>
    <String, dynamic>{'account_ids': instance.accountIds};
