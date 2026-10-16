import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_call.dart';

class MeRepository {
  MeRepository(this._api);
  final MeApi _api;

  Future<MeDto> get() => apiCall(_api.getMe);

  /// Deletes the caller's account; [displayName] must repeat the current display name.
  Future<void> delete(String displayName) =>
      apiCall(() => _api.deleteMe(body: DeleteMeRequest(displayName: displayName)));
}
