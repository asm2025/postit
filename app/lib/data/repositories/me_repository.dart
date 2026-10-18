import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_call.dart';

class MeRepository {
  MeRepository(this._api);
  final MeApi _api;

  Future<MeDto> get() => apiCall(_api.getMe);

  Future<PreferencesDto> getPreferences() => apiCall(_api.getPreferences);

  /// Saves the interface language ([locale] is an API tag: `en-US` or `ar`).
  Future<PreferencesDto> putPreferences(String locale) =>
      apiCall(() => _api.putPreferences(body: UpdatePreferences(locale: locale)));

  /// Deletes the caller's account; [displayName] must repeat the current display name.
  Future<void> delete(String displayName) =>
      apiCall(() => _api.deleteMe(body: DeleteMeRequest(displayName: displayName)));
}
