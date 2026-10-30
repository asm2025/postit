//! Wire types of the push routes. The endpoint and keys go in, never out: a response carries
//! the device's ID only.

use postit_data::push_endpoints::PushPlatform;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// The deployment's VAPID public key, for `PushManager.subscribe({ applicationServerKey })`.
#[derive(Debug, Serialize, ToSchema)]
pub struct VapidKeyDto {
    /// The uncompressed P-256 public key, base64url without padding.
    pub key: String,
}

/// Which push service reaches the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PushPlatformDto {
    /// An Android device.
    Android,
    /// An iOS device.
    Ios,
    /// A browser.
    Web,
}

impl From<PushPlatformDto> for PushPlatform {
    fn from(platform: PushPlatformDto) -> Self {
        match platform {
            PushPlatformDto::Android => Self::Android,
            PushPlatformDto::Ios => Self::Ios,
            PushPlatformDto::Web => Self::Web,
        }
    }
}

/// A device to receive push notifications, from the browser's or app's push subscription.
/// Registering an endpoint again replaces its keys, platform, and label and keeps its ID.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterPushEndpointRequest {
    /// The push service URL: `https`, or `http` only when the deployment allows private
    /// endpoints (a self-hosted `UnifiedPush` distributor on a LAN). At most 2048 characters.
    pub endpoint: String,
    /// The subscription's public key: a 65-byte uncompressed P-256 point, base64url.
    pub p256dh: String,
    /// The subscription's 16-byte authentication secret, base64url.
    pub auth: String,
    /// The push service.
    pub platform: PushPlatformDto,
    /// A name for the device, shown in the profile; at most 100 characters.
    pub device_label: String,
}

impl std::fmt::Debug for RegisterPushEndpointRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterPushEndpointRequest")
            .field("platform", &self.platform)
            .field("device_label", &self.device_label)
            .finish_non_exhaustive()
    }
}

/// The registered device.
#[derive(Debug, Serialize, ToSchema)]
pub struct PushEndpointCreated {
    /// The device's ID, for `DELETE /api/v1/me/push-endpoints/{id}`.
    pub id: Uuid,
}
