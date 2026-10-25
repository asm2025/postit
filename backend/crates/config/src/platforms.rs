use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

/// The opaque settings of one platform (`[platforms.<id>]`), owned by that platform's agent.
///
/// The config crate does not know a platform's keys; the agent validates them. Because any
/// value may be a secret (`client_secret`, usually supplied through
/// `POSTIT__PLATFORMS__<ID>__<KEY>_FILE`), `Debug` and `Serialize` emit every key with
/// `"[redacted]"` and never a value. Read the real values through [`PlatformTable::values`].
#[derive(Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct PlatformTable(Map<String, Value>);

impl PlatformTable {
    /// The raw key/value pairs. Callers must not log or return them.
    #[must_use]
    pub fn values(&self) -> &Map<String, Value> {
        &self.0
    }
}

impl std::fmt::Debug for PlatformTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut map = f.debug_map();
        for key in self.0.keys() {
            map.entry(key, &"[redacted]");
        }
        map.finish()
    }
}

impl Serialize for PlatformTable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.keys().map(|key| (key, "[redacted]")))
    }
}
