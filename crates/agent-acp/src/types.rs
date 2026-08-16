use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Request(JsonRpcRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub option_type: String,
    #[serde(default, rename = "currentValue")]
    pub current_value: Option<Value>,
    #[serde(default)]
    pub options: Vec<ConfigOptionValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigOptionValue {
    pub value: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdvertisedMenus {
    pub model: Option<ConfigOption>,
    pub thought_level: Option<ConfigOption>,
    pub model_config: Vec<ConfigOption>,
}

impl AdvertisedMenus {
    /// Only categories the agent advertised. Unknown categories are ignored
    /// so we never paint a fake empty menu.
    pub fn from_options(opts: &[ConfigOption]) -> Self {
        let mut menus = Self::default();
        for o in opts {
            match o.category.as_deref() {
                Some("model") => menus.model = Some(o.clone()),
                Some("thought_level") => menus.thought_level = Some(o.clone()),
                Some("model_config") => menus.model_config.push(o.clone()),
                // mode and unknown: still tracked only if we need later; UI must not invent.
                _ => {}
            }
        }
        menus
    }

    pub fn has_any_switch_ui(&self) -> bool {
        self.model.is_some() || self.thought_level.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSwitchOutcome {
    Applied,
    /// Hot switch failed or option not advertised — UI must force new-session+summary copy.
    RequireNewSessionWithSummary { reason: String },
}
