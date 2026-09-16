//! Keys for the prompt-tag modal (`ui::modal::prompt_tag`): the pick
//! stage's list/filter/manager keys and the body stage's text box, ending
//! in an unsubmitted insert into the focused pane.

use super::*;
use crate::app::{App, SharedApp};
use crate::commands::tags;
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::{EnterAction, Modal, PromptTagModal, TagStage};
use crossterm::event::{KeyCode, KeyModifiers};

/// Pick-stage keys. Returns `true` when the modal was closed (the caller
/// must not put it back).
fn pick_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    ctrl: bool,
    modal: &mut PromptTagModal,
) -> bool {
    let mut list = match tags::load(&app.store) {
        Ok(list) => list,
        Err(e) => {
            // Editing on top of an unreadable list could wipe it on save.
            app.modal = Some(Modal::Error {
                message: format!("could not read prompt tags: {e}"),
            });
            return true;
        }
    };
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
            return true;
        }
        KeyCode::Up => modal.select_up(),
        KeyCode::Down => modal.select_down(modal.filtered(&list).len()),
        KeyCode::Char(c @ '1'..='9') if modal.accelerators_active() && !ctrl => {
            let idx = (c as u8 - b'1') as usize;
            match modal.filtered(&list).get(idx) {
                Some(t) => {
                    modal.stage = TagStage::Body {
                        name: t.name.clone(),
                    };
                }
                // No listed tag at that slot: the digit is just the start
                // of a typed name, not a dead keystroke.
                None => {
                    modal.name_field.push(c);
                    modal.selected = 0;
                }
            }
        }
        KeyCode::Char('d') if ctrl => {
            let name = modal
                .filtered(&list)
                .get(modal.selected)
                .map(|t| t.name.clone());
            if let Some(name) = name {
                tags::remove(&mut list, &name);
                persist(app, &list);
                let len = modal.filtered(&list).len();
                modal.selected = modal.selected.min(len.saturating_sub(1));
            }
        }
        KeyCode::Enter => match modal.enter_action(&list) {
            Some(EnterAction::Existing(name)) => modal.stage = TagStage::Body { name },
            Some(EnterAction::Create(name)) => {
                list.push(tags::PromptTag {
                    name: name.clone(),
                    uses: 0,
                });
                tags::sort(&mut list);
                persist(app, &list);
                modal.stage = TagStage::Body { name };
            }
            None => {}
        },
        KeyCode::Backspace => {
            modal.name_field.pop();
            modal.selected = 0;
        }
        KeyCode::Char(c) if !ctrl => {
            modal.name_field.push(c);
            modal.selected = 0;
        }
        _ => {}
    }
    false
}

/// Body-stage keys. Returns `true` when the modal was replaced or closed
/// (insert succeeded, or an error modal took its place).
async fn body_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    ctrl: bool,
    name: &str,
    modal: &mut PromptTagModal,
) -> bool {
    match k.code {
        KeyCode::Esc => {
            modal.stage = TagStage::Pick;
        }
        KeyCode::Char('s') if ctrl => {
            if modal.body.is_blank() {
                return false;
            }
            // Only a local or remote attached pane has a composer to insert
            // into; the dashboard's digest pane is not a PTY.
            let session = match app.view {
                View::Attached(_) | View::AttachedRemote => active_session(app),
                View::Dashboard => None,
            };
            let Some(session) = session else {
                app.modal = Some(Modal::Error {
                    message: "no running agent in the focused pane".to_string(),
                });
                return true;
            };
            let text = tags::wrap(name, &modal.body.text());
            if !session.insert_text(&text).await {
                app.modal = Some(Modal::Error {
                    message: "agent is not running".to_string(),
                });
                return true;
            }
            let mut list = match tags::load(&app.store) {
                Ok(list) => list,
                Err(e) => {
                    // The text is already in the composer; only the count
                    // is lost. Say so rather than saving over an unread list.
                    app.modal = Some(Modal::Error {
                        message: format!(
                            "inserted, but could not read prompt tags to count the use: {e}"
                        ),
                    });
                    return true;
                }
            };
            tags::bump(&mut list, name);
            app.modal = None;
            persist(app, &list);
            return true;
        }
        KeyCode::Enter => modal.body.newline(),
        KeyCode::Backspace => modal.body.backspace(),
        KeyCode::Delete => modal.body.delete(),
        KeyCode::Left => modal.body.move_left(),
        KeyCode::Right => modal.body.move_right(),
        KeyCode::Up => modal.body.move_up(),
        KeyCode::Down => modal.body.move_down(),
        KeyCode::Home => modal.body.home(),
        KeyCode::End => modal.body.end(),
        KeyCode::Char(c) if !ctrl => modal.body.insert_char(c),
        _ => {}
    }
    false
}

/// Write the list back. A failed write surfaces as an error modal (which
/// replaces the prompt-tag modal — the in-memory edit is lost, but the user
/// sees why) rather than silently reading as saved.
fn persist(app: &mut App, list: &[tags::PromptTag]) {
    if let Err(e) = tags::save(&app.store, list) {
        tracing::warn!(error = %e, "failed to persist prompt tags");
        app.modal = Some(Modal::Error {
            message: format!("could not save prompt tags: {e}"),
        });
    }
}

pub(super) async fn prompt_tag(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    mut modal: PromptTagModal,
) -> Result<()> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let closed = match modal.stage.clone() {
        TagStage::Pick => pick_key(app, k, ctrl, &mut modal),
        TagStage::Body { name } => body_key(app, k, ctrl, &name, &mut modal).await,
    };
    if !closed && !matches!(app.modal, Some(Modal::Error { .. })) {
        app.modal = Some(Modal::PromptTag(modal));
    }
    Ok(())
}
