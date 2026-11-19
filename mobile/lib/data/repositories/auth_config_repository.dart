import 'package:postit/api/exports.dart';
import 'package:postit/data/http/api_call.dart';

class AuthConfigRepository {
  AuthConfigRepository(this._api);
  final AuthClient _api;

  Future<AuthConfigDto> load() => apiCall(_api.authConfig);
}
