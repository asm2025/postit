import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_call.dart';

class AuthConfigRepository {
  AuthConfigRepository(this._api);
  final AuthApi _api;

  Future<AuthConfigDto> load() => apiCall(_api.authConfig);
}
