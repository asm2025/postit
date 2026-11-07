// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'connect_method_method.dart';

part 'connect_method.g.dart';

/// How to connect: `oauth`, or `credentials` with the schema of fields to collect.
@JsonSerializable()
class ConnectMethod {
  const ConnectMethod({
    required this.method,
    this.schema,
  });
  
  factory ConnectMethod.fromJson(Map<String, Object?> json) => _$ConnectMethodFromJson(json);
  
  final ConnectMethodMethod method;

  /// A JSON Schema document in the postit options subset (free-form object).
  final dynamic schema;

  Map<String, Object?> toJson() => _$ConnectMethodToJson(this);
}
