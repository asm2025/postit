// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'resolve_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ResolveRequest _$ResolveRequestFromJson(Map<String, dynamic> json) =>
    ResolveRequest(
      outcome: ResolvedOutcome.fromJson(json['outcome'] as String),
    );

Map<String, dynamic> _$ResolveRequestToJson(ResolveRequest instance) =>
    <String, dynamic>{'outcome': _$ResolvedOutcomeEnumMap[instance.outcome]!};

const _$ResolvedOutcomeEnumMap = {
  ResolvedOutcome.published: 'published',
  ResolvedOutcome.failed: 'failed',
  ResolvedOutcome.$unknown: r'$unknown',
};
