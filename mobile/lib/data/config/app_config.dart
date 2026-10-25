/// The deployment environment a build targets.
enum AppEnv { development, qa, production }

/// Build-time settings from `--dart-define-from-file=config/<env>.json`.
class AppConfig {
  const AppConfig({required this.apiBaseUrl, required this.env});

  final Uri apiBaseUrl;
  final AppEnv env;

  factory AppConfig.fromEnvironment() => AppConfig.parse(
    apiBaseUrl: const String.fromEnvironment('API_BASE_URL'),
    env: const String.fromEnvironment('ENV'),
  );

  factory AppConfig.parse({required String apiBaseUrl, required String env}) {
    if (apiBaseUrl.isEmpty) {
      throw const FormatException('API_BASE_URL is not set; build with --dart-define-from-file=config/<env>.json');
    }
    final trimmed = apiBaseUrl.endsWith('/') ? apiBaseUrl.substring(0, apiBaseUrl.length - 1) : apiBaseUrl;
    final parsed = AppEnv.values.where((e) => e.name == env).firstOrNull;
    if (parsed == null) {
      throw FormatException('ENV must be development, qa or production, got "$env"');
    }
    return AppConfig(apiBaseUrl: Uri.parse(trimmed), env: parsed);
  }

  /// The custom URL scheme registered for this flavor.
  String get scheme => switch (env) {
    AppEnv.development => 'postit-dev',
    AppEnv.qa => 'postit-qa',
    AppEnv.production => 'postit',
  };

  String get redirectUrl => '$scheme://auth/callback';
  String get postLogoutRedirectUrl => '$scheme://auth/logout';
}
