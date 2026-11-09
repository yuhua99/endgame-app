//! Keymap editing over ZMK Studio: physical layout, layers, behaviors, live binding
//! changes, and save/discard. Changes apply immediately and stay in RAM until `save`.

use eg_studio::proto::studio::{notification, request::Subsystem as Req, request_response::Subsystem as Resp};
use eg_studio::proto::{behaviors, core, keymap, studio};
use tokio::sync::broadcast;

pub use eg_studio::proto::behaviors::behavior_parameter_value_description::ValueType;
pub use eg_studio::Studio;
pub use eg_studio::proto::behaviors::{BehaviorParameterValueDescription as ParamDesc, GetBehaviorDetailsResponse as Behavior};
pub use eg_studio::proto::keymap::{BehaviorBinding, KeyPhysicalAttrs, Keymap, Layer, PhysicalLayout};

use crate::{Error, Result};

/// Device-pushed state changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Locked(bool),
    Unsaved(bool),
}

/// Stream of device-pushed events; create it before loading so no change is missed.
pub struct Events(broadcast::Receiver<studio::Notification>);

pub fn events(studio: &Studio) -> Events {
    Events(studio.notifications())
}

impl Events {
    /// Next event, or `None` once the connection is gone.
    pub async fn next(&mut self) -> Option<Event> {
        loop {
            match self.0.recv().await {
                Ok(n) => {
                    if let Some(e) = event(n) {
                        return Some(e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => log::warn!("studio: {n} notifications dropped"),
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

fn event(n: studio::Notification) -> Option<Event> {
    match n.subsystem? {
        notification::Subsystem::Core(c) => match c.notification_type? {
            core::notification::NotificationType::LockStateChanged(s) => Some(Event::Locked(!is_unlocked_state(s))),
        },
        notification::Subsystem::Keymap(k) => match k.notification_type? {
            keymap::notification::NotificationType::UnsavedChangesStatusChanged(u) => Some(Event::Unsaved(u)),
        },
    }
}

pub async fn is_locked(studio: &Studio) -> Result<bool> {
    match core_call(studio, core::request::RequestType::GetLockState(true)).await? {
        core::response::ResponseType::GetLockState(s) => Ok(!is_unlocked_state(s)),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

/// The active physical layout.
pub async fn layout(studio: &Studio) -> Result<PhysicalLayout> {
    match keymap_call(studio, keymap::request::RequestType::GetPhysicalLayouts(true)).await? {
        keymap::response::ResponseType::GetPhysicalLayouts(mut l) => {
            let i = l.active_layout_index as usize;
            if i < l.layouts.len() {
                Ok(l.layouts.swap_remove(i))
            } else {
                Err(Error::Rejected(format!("active layout {i} of {}", l.layouts.len())))
            }
        }
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

pub async fn keymap(studio: &Studio) -> Result<Keymap> {
    match keymap_call(studio, keymap::request::RequestType::GetKeymap(true)).await? {
        keymap::response::ResponseType::GetKeymap(k) => Ok(k),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

/// All behaviors with their parameter metadata, sorted by display name.
pub async fn behaviors(studio: &Studio) -> Result<Vec<Behavior>> {
    let ids = match behaviors_call(studio, behaviors::request::RequestType::ListAllBehaviors(true)).await? {
        behaviors::response::ResponseType::ListAllBehaviors(l) => l.behaviors,
        _ => return Err(eg_studio::Error::UnexpectedResponse.into()),
    };
    let mut list = Vec::with_capacity(ids.len());
    for behavior_id in ids {
        let req = behaviors::request::RequestType::GetBehaviorDetails(behaviors::GetBehaviorDetailsRequest { behavior_id });
        match behaviors_call(studio, req).await? {
            behaviors::response::ResponseType::GetBehaviorDetails(d) => list.push(d),
            _ => return Err(eg_studio::Error::UnexpectedResponse.into()),
        }
    }
    list.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(list)
}

pub async fn set_binding(studio: &Studio, layer_id: u32, key_position: i32, binding: BehaviorBinding) -> Result<()> {
    let req = keymap::request::RequestType::SetLayerBinding(keymap::SetLayerBindingRequest {
        layer_id,
        key_position,
        binding: Some(binding),
    });
    match keymap_call(studio, req).await? {
        keymap::response::ResponseType::SetLayerBinding(code) => match keymap::SetLayerBindingResponse::try_from(code) {
            Ok(keymap::SetLayerBindingResponse::SetLayerBindingRespOk) => Ok(()),
            other => Err(Error::Rejected(format!("set binding: {other:?}"))),
        },
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

pub async fn has_unsaved(studio: &Studio) -> Result<bool> {
    match keymap_call(studio, keymap::request::RequestType::CheckUnsavedChanges(true)).await? {
        keymap::response::ResponseType::CheckUnsavedChanges(u) => Ok(u),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

/// Persists changes to flash.
pub async fn save(studio: &Studio) -> Result<()> {
    match keymap_call(studio, keymap::request::RequestType::SaveChanges(true)).await? {
        keymap::response::ResponseType::SaveChanges(r) => match r.result {
            Some(keymap::save_changes_response::Result::Ok(_)) => Ok(()),
            Some(keymap::save_changes_response::Result::Err(code)) => {
                Err(Error::Rejected(format!("save: {:?}", keymap::SaveChangesErrorCode::try_from(code))))
            }
            None => Err(eg_studio::Error::UnexpectedResponse.into()),
        },
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

/// Reverts unsaved changes on the device.
pub async fn discard(studio: &Studio) -> Result<()> {
    match keymap_call(studio, keymap::request::RequestType::DiscardChanges(true)).await? {
        keymap::response::ResponseType::DiscardChanges(_) => Ok(()),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

/// Parameter choices for `behavior`: param1 merges all metadata sets; param2 comes from the
/// first set whose param1 accepts `param1` (same rule as ZMK Studio).
pub fn param_descs(behavior: &Behavior, param1: u32, layer_ids: &[u32]) -> (Vec<ParamDesc>, Vec<ParamDesc>) {
    let p1 = behavior.metadata.iter().flat_map(|set| set.param1.iter().cloned()).collect();
    let p2 = behavior
        .metadata
        .iter()
        .find(|set| accepts(&set.param1, param1, layer_ids))
        .map(|set| set.param2.clone())
        .unwrap_or_default();
    (p1, p2)
}

/// A valid starting binding when the user picks `behavior`: the first allowed value of each
/// parameter (layer ids from `layer_ids`, HID usages default to Keyboard A).
pub fn default_binding(behavior: &Behavior, layer_ids: &[u32]) -> BehaviorBinding {
    let param1 = behavior.metadata.first().map_or(0, |set| first_value(&set.param1, layer_ids));
    let (_, p2) = param_descs(behavior, param1, layer_ids);
    BehaviorBinding { behavior_id: behavior.id as i32, param1, param2: first_value(&p2, layer_ids) }
}

/// `binding` with a new param1; param2 is kept if the matching set still accepts it,
/// otherwise reset to that set's first allowed value.
pub fn with_param1(behavior: &Behavior, binding: BehaviorBinding, param1: u32, layer_ids: &[u32]) -> BehaviorBinding {
    let (_, p2) = param_descs(behavior, param1, layer_ids);
    let param2 = if accepts(&p2, binding.param2, layer_ids) { binding.param2 } else { first_value(&p2, layer_ids) };
    BehaviorBinding { param1, param2, ..binding }
}

fn first_value(descs: &[ParamDesc], layer_ids: &[u32]) -> u32 {
    match descs.first().and_then(|d| d.value_type.as_ref()) {
        Some(ValueType::Constant(c)) => *c,
        Some(ValueType::Range(r)) => r.min.max(0) as u32,
        Some(ValueType::HidUsage(_)) => 0x0007_0004,
        Some(ValueType::LayerId(_)) => layer_ids.first().copied().unwrap_or(0),
        Some(ValueType::Nil(_)) | None => 0,
    }
}

/// Whether `value` is valid for a parameter described by `descs` (empty = no parameter).
pub fn accepts(descs: &[ParamDesc], value: u32, layer_ids: &[u32]) -> bool {
    if descs.is_empty() {
        return value == 0;
    }
    descs.iter().any(|d| match &d.value_type {
        Some(ValueType::Nil(_)) | None => value == 0,
        Some(ValueType::Constant(c)) => value == *c,
        Some(ValueType::Range(r)) => (r.min..=r.max).contains(&(value as i32)),
        Some(ValueType::HidUsage(_)) => (value >> 16) & 0xff != 0 && value & 0xffff != 0,
        Some(ValueType::LayerId(_)) => layer_ids.contains(&value),
    })
}

fn is_unlocked_state(s: i32) -> bool {
    s == core::LockState::ZmkStudioCoreLockStateUnlocked as i32
}

async fn core_call(studio: &Studio, r: core::request::RequestType) -> Result<core::response::ResponseType> {
    match studio.call(Req::Core(core::Request { request_type: Some(r) })).await? {
        Resp::Core(core::Response { response_type: Some(t) }) => Ok(t),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

async fn keymap_call(studio: &Studio, r: keymap::request::RequestType) -> Result<keymap::response::ResponseType> {
    match studio.call(Req::Keymap(keymap::Request { request_type: Some(r) })).await? {
        Resp::Keymap(keymap::Response { response_type: Some(t) }) => Ok(t),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

async fn behaviors_call(studio: &Studio, r: behaviors::request::RequestType) -> Result<behaviors::response::ResponseType> {
    match studio.call(Req::Behaviors(behaviors::Request { request_type: Some(r) })).await? {
        Resp::Behaviors(behaviors::Response { response_type: Some(t) }) => Ok(t),
        _ => Err(eg_studio::Error::UnexpectedResponse.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eg_studio::proto::behaviors::{
        BehaviorBindingParametersSet, BehaviorParameterHidUsage, BehaviorParameterLayerId, BehaviorParameterNil,
    };

    fn desc(v: ValueType) -> ParamDesc {
        ParamDesc { name: String::new(), value_type: Some(v) }
    }

    fn behavior(sets: Vec<(Vec<ParamDesc>, Vec<ParamDesc>)>) -> Behavior {
        Behavior {
            id: 1,
            display_name: "x".into(),
            metadata: sets
                .into_iter()
                .map(|(param1, param2)| BehaviorBindingParametersSet { param1, param2 })
                .collect(),
        }
    }

    #[test]
    fn accepts_by_kind() {
        let hid = [desc(ValueType::HidUsage(BehaviorParameterHidUsage { keyboard_max: 255, consumer_max: 1023 }))];
        assert!(accepts(&hid, 0x0007_0004, &[]));
        assert!(accepts(&hid, 0x0207_0004, &[])); // LShift implicit mod
        assert!(!accepts(&hid, 0, &[]));
        let layer = [desc(ValueType::LayerId(BehaviorParameterLayerId {}))];
        assert!(accepts(&layer, 2, &[0, 2]));
        assert!(!accepts(&layer, 5, &[0, 2]));
        assert!(accepts(&[desc(ValueType::Nil(BehaviorParameterNil {}))], 0, &[]));
        assert!(accepts(&[], 0, &[]));
        assert!(!accepts(&[], 1, &[]));
    }

    #[test]
    fn param2_follows_matching_param1_set() {
        // e.g. layer-tap: set A (param1 constant 1 -> param2 hid), set B (param1 constant 2 -> param2 layer)
        let b = behavior(vec![
            (
                vec![desc(ValueType::Constant(1))],
                vec![desc(ValueType::HidUsage(BehaviorParameterHidUsage { keyboard_max: 255, consumer_max: 0 }))],
            ),
            (vec![desc(ValueType::Constant(2))], vec![desc(ValueType::LayerId(BehaviorParameterLayerId {}))]),
        ]);
        let (p1, p2) = param_descs(&b, 2, &[0]);
        assert_eq!(p1.len(), 2);
        assert!(matches!(p2[0].value_type, Some(ValueType::LayerId(_))));
        assert!(param_descs(&b, 9, &[0]).1.is_empty());
    }

    #[test]
    fn default_binding_is_accepted() {
        let b = behavior(vec![(
            vec![desc(ValueType::LayerId(BehaviorParameterLayerId {}))],
            vec![desc(ValueType::HidUsage(BehaviorParameterHidUsage { keyboard_max: 255, consumer_max: 0 }))],
        )]);
        let d = default_binding(&b, &[3, 4]);
        assert_eq!((d.behavior_id, d.param1, d.param2), (1, 3, 0x0007_0004));
        let none = behavior(vec![]);
        assert_eq!(default_binding(&none, &[]).param1, 0);
    }

    #[test]
    fn param1_change_resets_invalid_param2() {
        // e.g. BT_SEL takes a profile index as param2, BT_NXT takes none.
        let b = behavior(vec![
            (vec![desc(ValueType::Constant(3))], vec![desc(ValueType::Range(behaviors::BehaviorParameterValueDescriptionRange { min: 0, max: 4 }))]),
            (vec![desc(ValueType::Constant(1))], vec![]),
        ]);
        let sel = BehaviorBinding { behavior_id: 1, param1: 3, param2: 2 };
        assert_eq!(with_param1(&b, sel, 1, &[]).param2, 0);
        let nxt = BehaviorBinding { behavior_id: 1, param1: 1, param2: 0 };
        assert_eq!(with_param1(&b, nxt, 3, &[]).param2, 0);
        assert_eq!(with_param1(&b, sel, 3, &[]).param2, 2);
    }

    #[test]
    fn maps_notifications() {
        let n = studio::Notification {
            subsystem: Some(notification::Subsystem::Keymap(keymap::Notification {
                notification_type: Some(keymap::notification::NotificationType::UnsavedChangesStatusChanged(true)),
            })),
        };
        assert_eq!(event(n), Some(Event::Unsaved(true)));
        let n = studio::Notification {
            subsystem: Some(notification::Subsystem::Core(core::Notification {
                notification_type: Some(core::notification::NotificationType::LockStateChanged(
                    core::LockState::ZmkStudioCoreLockStateUnlocked as i32,
                )),
            })),
        };
        assert_eq!(event(n), Some(Event::Locked(false)));
    }
}
