// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'account_change_dto.dart';
import 'account_dto.dart';

part 'connected_account_dto.g.dart';

/// One connected account and what happened to it.
@JsonSerializable()
class ConnectedAccountDto {
  const ConnectedAccountDto({
    required this.account,
    required this.change,
  });
  
  factory ConnectedAccountDto.fromJson(Map<String, Object?> json) => _$ConnectedAccountDtoFromJson(json);
  
  /// The account.
  final AccountDto account;

  /// What confirming did.
  final AccountChangeDto change;

  Map<String, Object?> toJson() => _$ConnectedAccountDtoToJson(this);
}
