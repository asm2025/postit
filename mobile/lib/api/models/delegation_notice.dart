// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'delegation_event.dart';
import 'delegation_id.dart';

part 'delegation_notice.g.dart';

/// Payload of `delegation_changed`.
@JsonSerializable()
class DelegationNotice {
  const DelegationNotice({
    required this.delegationId,
    required this.event,
    required this.partyName,
  });
  
  factory DelegationNotice.fromJson(Map<String, Object?> json) => _$DelegationNoticeFromJson(json);
  
  /// The delegation.
  @JsonKey(name: 'delegation_id')
  final DelegationId delegationId;

  /// What happened.
  final DelegationEvent event;

  /// The other party's display name.
  @JsonKey(name: 'party_name')
  final String partyName;

  Map<String, Object?> toJson() => _$DelegationNoticeToJson(this);
}
