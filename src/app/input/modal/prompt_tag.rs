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
    // Read-only: filtering, navigation and `enter_action` all work off this
    // snapshot. The two arms that mutate the list (`ctrl-d`, Enter on a new
    // name) go through `tags::update` instead of writing this copy back,
    // so a sibling writer between this read and that write is folded in
    // rather than overwritten.
    let list = match tags::load(&app.store) {
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
                // Mutate through `tags::update` (one write transaction) so
                // a sibling writer between this read and the write is
                // folded in rather than overwritten.
                match tags::update(&app.store, |l| {
                    tags::remove(l, &name);
                }) {
                    Ok(written) => {
                        let len = modal.filtered(&written).len();
                        modal.selected = modal.selected.min(len.saturating_sub(1));
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to persist prompt tags");
                        app.modal = Some(Modal::Error {
                            message: format!("could not save prompt tags: {e}"),
                        });
                        return true;
                    }
                }
            }
        }
        KeyCode::Enter => match modal.enter_action(&list) {
            Some(EnterAction::Existing(name)) => modal.stage = TagStage::Body { name },
            Some(EnterAction::Create(name)) => {
                // Same transactional update; the `any` guard folds in a
                // sibling that created this same name meanwhile instead of
                // pushing a duplicate.
                match tags::update(&app.store, |l| {
                    if !l.iter().any(|t| t.name == name) {
                        l.push(tags::PromptTag {
                            name: name.clone(),
                            uses: 0,
                        });
                    }
                }) {
                    Ok(_) => modal.stage = TagStage::Body { name },
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to persist prompt tags");
                        app.modal = Some(Modal::Error {
                            message: format!("could not save prompt tags: {e}"),
                        });
                        return true;
                    }
                }
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
///
/// An insert that cannot be confirmed never costs the draft: the modal
/// stays on the body stage with a one-line `notice` explaining why, so the
/// user can attach a pane (or wait for the agent) and press `^s` again.
async fn body_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    ctrl: bool,
    name: &str,
    modal: &mut PromptTagModal,
) -> bool {
    // Any key acknowledges the previous notice.
    modal.notice = None;
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
                modal.notice =
                    Some("no running agent in the focused pane — draft kept".to_string());
                return false;
            };
            let text = tags::wrap(name, &tags::sanitize_body(&modal.body.text()));
            if !session.insert_text(&text).await {
                // `false` covers both a closed writer (agent exited) and an
                // ack timeout, and a timeout only means delivery is
                // uncertain — so say "not confirmed", not "not running",
                // and count nothing.
                modal.notice = Some(
                    "insert not confirmed (agent exited or not responding) — draft kept"
                        .to_string(),
                );
                return false;
            }
            // The text is already in the composer; only the count is at
            // stake, so the modal closes either way. `tags::update` folds
            // this bump into the table's current value in one write
            // transaction rather than the read/bump/save the modal used to
            // do, which could stomp a sibling writer's change. A failure
            // here says the insert happened so the user does not retry it.
            app.modal = None;
            if let Err(e) = tags::update(&app.store, |l| tags::bump(l, name)) {
                tracing::warn!(error = %e, "inserted prompt tag but failed to bump its use count");
                app.modal = Some(Modal::Error {
                    message: format!("inserted, but could not save the use count: {e}"),
                });
            }
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
        KeyCode::Tab => modal.body.insert_char('\t'),
        KeyCode::Char(c) if !ctrl => modal.body.insert_char(c),
        _ => {}
    }
    false
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
