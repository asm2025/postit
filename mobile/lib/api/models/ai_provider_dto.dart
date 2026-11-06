// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_delegate_provider_dto.dart';
import 'ai_model_dto.dart';
import 'ai_owner_provider_dto.dart';
import 'ai_protocol_dto.dart';


part 'ai_provider_dto.g.dart';

/// A provider in the view the caller may see: `owner` or `delegate`.
@JsonSerializable(createFactory: false)
sealed class AiProviderDto {
  const AiProviderDto();
  
  factory AiProviderDto.fromJson(Map<String, dynamic> json) =>
      AiProviderDtoSealedDeserializer.tryDeserialize(json);
  
  Map<String, dynamic> toJson();
}

extension AiProviderDtoSealedDeserializer on AiProviderDto {
  static AiProviderDto tryDeserialize(
    Map<String, dynamic> json, {
    String key = 'view',
    Map<Type, Object?>? mapping,
  }) {
    final mappingFallback = const <Type, Object?>{
      AiProviderDtoAiDelegateProviderDto: 'delegate',
      AiProviderDtoAiOwnerProviderDto: 'owner',
    };
    final value = json[key];
    final effective = mapping ?? mappingFallback;
    return switch (value) {
      _ when value == effective[AiProviderDtoAiDelegateProviderDto] => AiProviderDtoAiDelegateProviderDto.fromJson(json),
      _ when value == effective[AiProviderDtoAiOwnerProviderDto] => AiProviderDtoAiOwnerProviderDto.fromJson(json),
      _ => throw FormatException('Unknown discriminator value "${json[key]}" for AiProviderDto'),
    };
  }
}

@JsonSerializable()
class AiProviderDtoAiDelegateProviderDto extends AiProviderDto implements AiDelegateProviderDto {
  @override
  final String? defaultModel;
  @override
  final String id;
  @override
  final bool isDefault;
  @override
  final List<AiModelDto> models;
  @override
  final String name;
  @override
  final String preset;
  @override
  final dynamic view;

  const AiProviderDtoAiDelegateProviderDto({
    required this.defaultModel,
    required this.id,
    required this.isDefault,
    required this.models,
    required this.name,
    required this.preset,
    required this.view,
  });
  
  factory AiProviderDtoAiDelegateProviderDto.fromJson(Map<String, dynamic> json) =>
      _$AiProviderDtoAiDelegateProviderDtoFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$AiProviderDtoAiDelegateProviderDtoToJson(this);
}
@JsonSerializable()
class AiProviderDtoAiOwnerProviderDto extends AiProviderDto implements AiOwnerProviderDto {
  @override
  final String? baseUrl;
  @override
  final String? defaultModel;
  @override
  final bool hasKey;
  @override
  final List<String> headerNames;
  @override
  final String id;
  @override
  final bool isDefault;
  @override
  final List<AiModelDto> models;
  @override
  final String name;
  @override
  final String preset;
  @override
  final AiProtocolDto protocol;
  @override
  final dynamic view;

  const AiProviderDtoAiOwnerProviderDto({
    required this.baseUrl,
    required this.defaultModel,
    required this.hasKey,
    required this.headerNames,
    required this.id,
    required this.isDefault,
    required this.models,
    required this.name,
    required this.preset,
    required this.protocol,
    required this.view,
  });
  
  factory AiProviderDtoAiOwnerProviderDto.fromJson(Map<String, dynamic> json) =>
      _$AiProviderDtoAiOwnerProviderDtoFromJson(json);
      
  @override
  Map<String, dynamic> toJson() => _$AiProviderDtoAiOwnerProviderDtoToJson(this);
}
