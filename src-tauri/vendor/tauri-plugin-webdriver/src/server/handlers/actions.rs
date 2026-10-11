use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use tauri::Runtime;

use crate::platform::{ModifierState, PointerEventType};
use crate::server::response::{WebDriverResponse, WebDriverResult};
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct ActionsRequest {
    pub actions: Vec<ActionSequence>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ActionSequence {
    #[serde(rename = "key")]
    Key {
        #[serde(rename = "id")]
        _id: String,
        actions: Vec<KeyAction>,
    },
    #[serde(rename = "pointer")]
    Pointer {
        id: String,
        actions: Vec<PointerAction>,
    },
    #[serde(rename = "wheel")]
    Wheel {
        #[serde(rename = "id")]
        _id: String,
        actions: Vec<WheelAction>,
    },
    #[serde(rename = "none")]
    None {
        #[serde(rename = "id")]
        _id: String,
        actions: Vec<PauseAction>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum KeyAction {
    #[serde(rename = "keyDown")]
    KeyDown { value: String },
    #[serde(rename = "keyUp")]
    KeyUp { value: String },
    #[serde(rename = "pause")]
    Pause { duration: Option<u64> },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum PointerAction {
    #[serde(rename = "pointerDown")]
    PointerDown { button: u32 },
    #[serde(rename = "pointerUp")]
    PointerUp { button: u32 },
    #[serde(rename = "pointerMove")]
    PointerMove {
        x: i32,
        y: i32,
        duration: Option<u64>,
    },
    #[serde(rename = "pause")]
    Pause { duration: Option<u64> },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WheelAction {
    #[serde(rename = "scroll")]
    Scroll {
        x: i32,
        y: i32,
        #[serde(rename = "deltaX")]
        delta_x: i32,
        #[serde(rename = "deltaY")]
        delta_y: i32,
        #[serde(default)]
        duration: Option<u64>,
    },
    #[serde(rename = "pause")]
    Pause { duration: Option<u64> },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum PauseAction {
    #[serde(rename = "pause")]
    Pause { duration: Option<u64> },
}

/// Current pointer position for actions
struct PointerState {
    x: i32,
    y: i32,
}

/// POST `/session/{session_id}/actions` - Perform actions
#[allow(clippy::too_many_lines)]
pub async fn perform<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
    Json(request): Json<ActionsRequest>,
) -> WebDriverResult {
    // Get session info and executor first
    let (current_window, timeouts, frame_context) = {
        let sessions = state.sessions.read().await;
        let session = sessions.get(&session_id)?;
        (
            session.current_window.clone(),
            session.timeouts.clone(),
            session.frame_context.clone(),
        )
    };

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let mut pointer_state = PointerState { x: 0, y: 0 };
    let mut modifier_state = ModifierState::default();

    for action_seq in &request.actions {
        match action_seq {
            ActionSequence::Key { _id: _, actions } => {
                for action in actions {
                    match action {
                        KeyAction::KeyDown { value } => {
                            modifier_state.update(value, true);
                            executor
                                .dispatch_key_event(value, true, &modifier_state)
                                .await?;
                            // Track pressed key
                            let mut sessions = state.sessions.write().await;
                            if let Ok(session) = sessions.get_mut(&session_id) {
                                session.action_state.pressed_keys.insert(value.clone());
                            }
                        }
                        KeyAction::KeyUp { value } => {
                            executor
                                .dispatch_key_event(value, false, &modifier_state)
                                .await?;
                            modifier_state.update(value, false);
                            // Remove from tracked keys
                            let mut sessions = state.sessions.write().await;
                            if let Ok(session) = sessions.get_mut(&session_id) {
                                session.action_state.pressed_keys.remove(value);
                            }
                        }
                        KeyAction::Pause { duration } => {
                            if let Some(ms) = duration {
                                tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                            }
                        }
                    }
                }
            }
            ActionSequence::Pointer { id, actions } => {
                for action in actions {
                    match action {
                        PointerAction::PointerDown { button } => {
                            executor
                                .dispatch_pointer_event(
                                    PointerEventType::Down,
                                    pointer_state.x,
                                    pointer_state.y,
                                    *button,
                                )
                                .await?;
                            // Track pressed button
                            let mut sessions = state.sessions.write().await;
                            if let Ok(session) = sessions.get_mut(&session_id) {
                                session
                                    .action_state
                                    .pressed_buttons
                                    .entry(id.clone())
                                    .or_default()
                                    .insert(*button);
                            }
                        }
                        PointerAction::PointerUp { button } => {
                            executor
                                .dispatch_pointer_event(
                                    PointerEventType::Up,
                                    pointer_state.x,
                                    pointer_state.y,
                                    *button,
                                )
                                .await?;
                            // Remove from tracked buttons
                            let mut sessions = state.sessions.write().await;
                            if let Ok(session) = sessions.get_mut(&session_id) {
                                if let Some(buttons) =
                                    session.action_state.pressed_buttons.get_mut(id)
                                {
                                    buttons.remove(button);
                                }
                            }
                        }
                        PointerAction::PointerMove { x, y, duration } => {
                            pointer_state.x = *x;
                            pointer_state.y = *y;
                            if let Some(ms) = duration {
                                if *ms > 0 {
                                    tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                                }
                            }
                            executor
                                .dispatch_pointer_event(
                                    PointerEventType::Move,
                                    pointer_state.x,
                                    pointer_state.y,
                                    0,
                                )
                                .await?;
                        }
                        PointerAction::Pause { duration } => {
                            if let Some(ms) = duration {
                                tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                            }
                        }
                    }
                }
            }
            ActionSequence::Wheel { _id: _, actions } => {
                for action in actions {
                    match action {
                        WheelAction::Scroll {
                            x,
                            y,
                            delta_x,
                            delta_y,
                            duration,
                        } => {
                            if let Some(ms) = duration {
                                if *ms > 0 {
                                    tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                                }
                            }
                            executor
                                .dispatch_scroll_event(*x, *y, *delta_x, *delta_y)
                                .await?;
                        }
                        WheelAction::Pause { duration } => {
                            if let Some(ms) = duration {
                                tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                            }
                        }
                    }
                }
            }
            ActionSequence::None { _id: _, actions } => {
                for action in actions {
                    match action {
                        PauseAction::Pause { duration } => {
                            if let Some(ms) = duration {
                                tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(WebDriverResponse::null())
}

/// DELETE `/session/{session_id}/actions` - Release actions
pub async fn release<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(session_id): Path<String>,
) -> WebDriverResult {
    // Get session state and clear tracked actions
    let (current_window, timeouts, frame_context, pressed_keys, pressed_buttons) = {
        let mut sessions = state.sessions.write().await;
        let session = sessions.get_mut(&session_id)?;
        let pressed_keys: Vec<String> = session.action_state.pressed_keys.drain().collect();
        let pressed_buttons = std::mem::take(&mut session.action_state.pressed_buttons);
        (
            session.current_window.clone(),
            session.timeouts.clone(),
            session.frame_context.clone(),
            pressed_keys,
            pressed_buttons,
        )
    };

    let executor = state.get_executor_for_window(&current_window, timeouts, frame_context)?;
    let modifier_state = ModifierState::default();

    // Release all pressed keys (keyUp events)
    for key in pressed_keys {
        executor
            .dispatch_key_event(&key, false, &modifier_state)
            .await?;
    }

    // Release all pressed pointer buttons (pointerUp events)
    for (_source_id, buttons) in pressed_buttons {
        for button in buttons {
            executor
                .dispatch_pointer_event(PointerEventType::Up, 0, 0, button)
                .await?;
        }
    }

    Ok(WebDriverResponse::null())
}
