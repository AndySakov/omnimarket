//! One WebSocket connection's protocol (contract v0, stream.proto): subscribe and unsubscribe
//! per topic, a snapshot first and then numbered deltas. Pure: the server hands it the client's
//! frames, the feed's ticks and the read model, and sends what it returns.

use std::collections::BTreeMap;

use proto::api::v1::{
    ClientMessage, Delta, Heartbeat, ServerMessage, Snapshot, StreamError, client_message, delta,
    server_message, snapshot, stream_error,
};

use crate::feed::{Published, token_topic};
use crate::model::ReadModel;

/// The topics this server serves. Others in the contract arrive with their issues (#79–#81,
/// #85); subscribing to one is an unknown topic until then.
#[derive(Debug, PartialEq)]
pub enum Topic {
    /// By lowercase 0x address.
    Token(String),
}

/// Parses a topic name, accepting an address in either case.
pub fn parse_topic(name: &str) -> Option<Topic> {
    let address = name.strip_prefix("token:")?.to_ascii_lowercase();
    let digits = address.strip_prefix("0x")?;
    if digits.len() == 40 && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(Topic::Token(address))
    } else {
        None
    }
}

#[derive(Default)]
pub struct Session {
    /// Subscribed topics, by canonical name.
    topics: BTreeMap<String, TopicState>,
}

#[derive(Default)]
struct TopicState {
    /// Unset until the topic's snapshot has gone out; a token with no price yet gets its
    /// snapshot with its first tick.
    sent: Option<Sent>,
}

struct Sent {
    seq: u64,
    block_number: u64,
}

impl Session {
    /// Handles one text frame from the client.
    pub fn on_text(&mut self, text: &str, model: &ReadModel) -> Vec<ServerMessage> {
        let message: ClientMessage = match serde_json::from_str(text) {
            Ok(message) => message,
            Err(e) => {
                return vec![error(
                    stream_error::Code::Unspecified,
                    "",
                    &format!("not a ClientMessage: {e}"),
                )];
            }
        };
        match message.kind {
            Some(client_message::Kind::Subscribe(subscribe)) => {
                match parse_topic(&subscribe.topic) {
                    Some(Topic::Token(address)) => {
                        let topic = token_topic(&address);
                        let state = self.topics.entry(topic.clone()).or_default();
                        state.sent = None;
                        snapshot_of(&topic, state, model).into_iter().collect()
                    }
                    None => vec![error(
                        stream_error::Code::UnknownTopic,
                        &subscribe.topic,
                        "this server doesn't serve that topic",
                    )],
                }
            }
            Some(client_message::Kind::Unsubscribe(unsubscribe)) => {
                if let Some(Topic::Token(address)) = parse_topic(&unsubscribe.topic) {
                    self.topics.remove(&token_topic(&address));
                }
                Vec::new()
            }
            None => vec![error(
                stream_error::Code::Unspecified,
                "",
                "an empty message",
            )],
        }
    }

    /// Turns a published tick into this connection's message for it, if it's subscribed: the
    /// topic's snapshot if none has gone out yet, otherwise the next delta. A tick no newer than
    /// what the client already has (one applied before its snapshot was taken) is skipped.
    pub fn on_published(
        &mut self,
        published: &Published,
        model: &ReadModel,
    ) -> Option<ServerMessage> {
        let state = self.topics.get_mut(&published.topic)?;
        let Some(sent) = &mut state.sent else {
            return snapshot_of(&published.topic, state, model);
        };
        if published.tick.block_number <= sent.block_number {
            return None;
        }
        sent.seq += 1;
        sent.block_number = published.tick.block_number;
        Some(ServerMessage {
            kind: Some(server_message::Kind::Delta(Delta {
                topic: published.topic.clone(),
                seq: sent.seq,
                payload: Some(delta::Payload::TokenTick(published.tick.clone())),
            })),
        })
    }

    /// Fresh snapshots of every subscribed topic: for a client that fell too far behind to
    /// catch up from deltas.
    pub fn resync(&mut self, model: &ReadModel) -> Vec<ServerMessage> {
        self.topics
            .iter_mut()
            .filter_map(|(topic, state)| {
                state.sent = None;
                snapshot_of(topic, state, model)
            })
            .collect()
    }
}

/// The topic's snapshot from the read model, seq 0, if the token has a price yet.
fn snapshot_of(topic: &str, state: &mut TopicState, model: &ReadModel) -> Option<ServerMessage> {
    let address = topic.strip_prefix("token:")?;
    let token = model.token(address)?.clone();
    state.sent = Some(Sent {
        seq: 0,
        block_number: token.block_number,
    });
    Some(ServerMessage {
        kind: Some(server_message::Kind::Snapshot(Snapshot {
            topic: topic.to_string(),
            seq: 0,
            payload: Some(snapshot::Payload::Token(token)),
        })),
    })
}

/// The heartbeat: the server's time (a liveness signal, not a record) and the chain head the
/// read model has applied.
pub fn heartbeat(server_time_ms: u64, model: &ReadModel) -> ServerMessage {
    ServerMessage {
        kind: Some(server_message::Kind::Heartbeat(Heartbeat {
            server_time_ms,
            head_block_number: model.head_block_number(),
        })),
    }
}

fn error(code: stream_error::Code, topic: &str, message: &str) -> ServerMessage {
    ServerMessage {
        kind: Some(server_message::Kind::Error(StreamError {
            code: code.into(),
            topic: topic.to_string(),
            message: message.to_string(),
        })),
    }
}

/// A server message as one proto3 JSON text frame (D91).
pub fn to_json(message: &ServerMessage) -> String {
    serde_json::to_string(message).expect("generated messages always serialize")
}
