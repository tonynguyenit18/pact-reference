//! Synchronous interactions as a request message to a sequence of response messages

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use itertools::Itertools;
use log::*;
use serde_json::{json, Value};

use pact_models::bodies::OptionalBody;
use pact_models::content_types::ContentType;
use pact_models::provider_states::{self, ProviderState};

use crate::models::{Interaction, RequestResponseInteraction};
use crate::models::json_utils::json_to_string;
use crate::models::matchingrules::MatchingRules;
use crate::models::message::Message;
use crate::models::v4::{AsynchronousMessage, SynchronousHttp, V4Interaction, V4InteractionType};
use crate::models::v4::message_parts::MessageContents;

/// Synchronous interactions as a request message to a sequence of response messages
#[derive(Debug, Clone, Eq)]
pub struct SynchronousMessages {
  /// Interaction ID. This will only be set if the Pact file was fetched from a Pact Broker
  pub id: Option<String>,
  /// Unique key for this interaction
  pub key: Option<String>,
  /// A description for the interaction. Must be unique within the Pact file
  pub description: String,
  /// Optional provider state for the interaction.
  /// See https://docs.pact.io/getting_started/provider_states for more info on provider states.
  pub provider_states: Vec<ProviderState>,
  /// Annotations and comments associated with this interaction
  pub comments: HashMap<String, Value>,
  /// Request message
  pub request: MessageContents,
  /// Response messages
  pub response: Vec<MessageContents>,

  /// If this interaction is pending. Pending interactions will never fail the build if they fail
  pub pending: bool
}

impl SynchronousMessages {
  fn calc_hash(&self) -> String {
    let mut s = DefaultHasher::new();
    self.hash(&mut s);
    format!("{:x}", s.finish())
  }

  /// Creates a new version with a calculated key
  pub fn with_key(&self) -> SynchronousMessages {
    SynchronousMessages {
      key: Some(self.calc_hash()),
      .. self.clone()
    }
  }

  /// Parse the JSON into a SynchronousMessages structure
  pub fn from_json(json: &Value, index: usize) -> anyhow::Result<SynchronousMessages> {
    if json.is_object() {
      let id = json.get("_id").map(|id| json_to_string(id));
      let key = json.get("key").map(|id| json_to_string(id));
      let description = match json.get("description") {
        Some(v) => match *v {
          Value::String(ref s) => s.clone(),
          _ => v.to_string()
        },
        None => format!("Interaction {}", index)
      };
      let comments = match json.get("comments") {
        Some(v) => match v {
          Value::Object(map) => map.iter()
            .map(|(k, v)| (k.clone(), v.clone())).collect(),
          _ => {
            warn!("Interaction comments must be a JSON Object, but received {}. Ignoring", v);
            Default::default()
          }
        },
        None => Default::default()
      };
      let provider_states = provider_states::ProviderState::from_json(json);
      let request = json.get("request")
        .ok_or(anyhow!("JSON for SynchronousMessages does not contain a 'request' object"))?;
      let response = json.get("response")
        .ok_or(anyhow!("JSON for SynchronousMessages does not contain a 'response' array"))?
        .as_array()
        .ok_or(anyhow!("JSON for SynchronousMessages does not contain a 'response' array"))?;
      let responses =
        response.iter()
          .map(|message| MessageContents::from_json(message))
          .collect::<Vec<anyhow::Result<MessageContents>>>();
      if responses.iter().any(|res| res.is_err()) {
        let errors = responses.iter()
          .filter(|res| res.is_err())
          .map(|res| res.as_ref().unwrap_err().to_string())
          .join(", ");
        Err(anyhow!("Failed to parse SynchronousMessages responses - {}", errors))
      } else {
        Ok(SynchronousMessages {
          id,
          key,
          description,
          provider_states,
          comments,
          request: MessageContents::from_json(request)?,
          response: responses.iter().map(|res| res.as_ref().unwrap().clone()).collect(),
          pending: json.get("pending")
            .map(|value| value.as_bool().unwrap_or_default()).unwrap_or_default()
        })
      }
    } else {
      Err(anyhow!("Expected a JSON object for the interaction, got '{}'", json))
    }
  }
}

impl V4Interaction for SynchronousMessages {
  fn to_json(&self) -> Value {
    let mut json = json!({
      "type": V4InteractionType::Synchronous_Messages.to_string(),
      "key": self.key.clone().unwrap_or_else(|| self.calc_hash()),
      "description": self.description.clone(),
      "pending": self.pending,
      "request": self.request.to_json(),
      "response": self.response.iter().map(|m| m.to_json()).collect_vec()
    });

    if !self.provider_states.is_empty() {
      let map = json.as_object_mut().unwrap();
      map.insert("providerStates".to_string(), Value::Array(
        self.provider_states.iter().map(|p| p.to_json()).collect()));
    }

    if !self.comments.is_empty() {
      let map = json.as_object_mut().unwrap();
      map.insert("comments".to_string(), self.comments.iter()
        .map(|(k, v)| (k.clone(), v.clone())).collect());
    }

    json
  }

  fn to_super(&self) -> &dyn Interaction {
    self
  }

  fn key(&self) -> Option<String> {
    self.key.clone()
  }

  fn boxed_v4(&self) -> Box<dyn V4Interaction> {
    Box::new(self.clone())
  }

  fn comments(&self) -> HashMap<String, Value> {
    self.comments.clone()
  }

  fn comments_mut(&mut self) -> &mut HashMap<String, Value> {
    &mut self.comments
  }

  fn v4_type(&self) -> V4InteractionType {
    V4InteractionType::Synchronous_Messages
  }

  fn pending(&self) -> bool {
    self.pending
  }
}

impl Interaction for SynchronousMessages {
  fn type_of(&self) -> String {
    format!("V4 {}", self.v4_type())
  }

  fn is_request_response(&self) -> bool {
    false
  }

  fn as_request_response(&self) -> Option<RequestResponseInteraction> {
    None
  }

  fn is_message(&self) -> bool {
    false
  }

  fn as_message(&self) -> Option<Message> {
    None
  }

  fn id(&self) -> Option<String> {
    self.id.clone()
  }

  fn description(&self) -> String {
    self.description.clone()
  }

  fn provider_states(&self) -> Vec<ProviderState> {
    self.provider_states.clone()
  }

  fn contents(&self) -> OptionalBody {
    OptionalBody::Missing
  }

  fn contents_for_verification(&self) -> OptionalBody {
    self.response.first().map(|message| message.contents.clone()).unwrap_or_default()
  }

  fn content_type(&self) -> Option<ContentType> {
    self.request.message_content_type()
  }

  fn is_v4(&self) -> bool {
    true
  }

  fn as_v4(&self) -> Option<Box<dyn V4Interaction>> {
    Some(self.boxed_v4())
  }

  fn as_v4_http(&self) -> Option<SynchronousHttp> {
    None
  }

  fn as_v4_async_message(&self) -> Option<AsynchronousMessage> {
    None
  }

  fn as_v4_sync_message(&self) -> Option<SynchronousMessages> {
    Some(self.clone())
  }

  fn boxed(&self) -> Box<dyn Interaction + Send> {
    Box::new(self.clone())
  }

  fn arced(&self) -> Arc<dyn Interaction + Send> {
    Arc::new(self.clone())
  }

  fn thread_safe(&self) -> Arc<Mutex<dyn Interaction + Send + Sync>> {
    Arc::new(Mutex::new(self.clone()))
  }

  fn matching_rules(&self) -> Option<MatchingRules> {
    None
  }
}

impl Default for SynchronousMessages {
  fn default() -> Self {
    SynchronousMessages {
      id: None,
      key: None,
      description: "Synchronous/Message Interaction".to_string(),
      provider_states: vec![],
      comments: Default::default(),
      request: Default::default(),
      response: Default::default(),
      pending: false
    }
  }
}

impl PartialEq for SynchronousMessages {
  fn eq(&self, other: &Self) -> bool {
    self.description == other.description && self.provider_states == other.provider_states &&
      self.request == other.request && self.response == other.response &&
      self.pending == other.pending
  }
}

impl Hash for SynchronousMessages {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.description.hash(state);
    self.provider_states.hash(state);
    self.request.hash(state);
    self.response.hash(state);
    self.pending.hash(state);
  }
}

impl Display for SynchronousMessages {
  fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
    let pending = if self.pending { " [PENDING]" } else { "" };
    write!(f, "V4 Synchronous Message Interaction{} ( id: {:?}, description: \"{}\", provider_states: {:?}, request: {}, response: {:?} )",
           pending, self.id, self.description, self.provider_states, self.request, self.response)
  }
}
