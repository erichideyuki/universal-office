//! The document options the session keeps (D2a): track changes (`setTrackChanges`,
//! `toggleTrackChanges`), the author (`setAuthor`), protected mode (`setProtected`), the read view
//! (the editing half of `setView('read')`), the markup view (`setMarkupMode`) and the formatting
//! policy (`formattingPolicy`). None of them takes an undo step (the TS ones do not either).

use super::super::command::{CommandError, Event, Outcome};
use super::super::policy::FormattingPolicy;
use super::super::{ExecCx, MarkupMode, Session};
use crate::js::Str;

/// The settings commands.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCmd {
    /// `toggleTrackChanges(on)`: dirty, `uw-change`, `uw-track-changes` when it changes
    SetTracking { on: bool },
    /// `author = name`
    SetAuthor { name: Str },
    /// `setProtected(on)` (Modo Protegido): `uw-protected`
    SetProtected { on: bool },
    /// the read view locks editing (`view === 'read'`)
    SetReadView { on: bool },
    /// `setMarkupMode(mode)`
    SetMarkup { mode: MarkupMode },
    /// `formattingPolicy = policy` (`None`: no policy); `uw-formatting-policy-change`
    SetPolicy { policy: Option<FormattingPolicy> },
}

impl Session {
    pub(crate) fn exec_settings(&mut self, cmd: SettingsCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            SettingsCmd::SetTracking { on } => {
                // `toggleTrackChanges(on)`: an edit (saved in `w:trackRevisions`) without a step
                if self.opts.tracking != on {
                    self.opts.tracking = on;
                    self.history.note_edit();
                    self.changed();
                    self.emit(Event::TrackChanges { on });
                }
            }
            SettingsCmd::SetAuthor { name } => self.opts.author = name,
            SettingsCmd::SetProtected { on } => {
                if self.opts.protected != on {
                    self.opts.protected = on;
                    let has_fields = false;
                    self.emit(Event::Protected { active: on, has_fields });
                }
            }
            SettingsCmd::SetReadView { on } => self.opts.read_view = on,
            SettingsCmd::SetMarkup { mode } => {
                // `setMarkupMode(mode)`: the caret leaves text the view hides
                if self.opts.markup != mode {
                    self.opts.markup = mode;
                    let sel = self.selection;
                    self.selection =
                        super::super::state::Selection { anchor: self.canon_pos(sel.anchor), focus: self.canon_pos(sel.focus) };
                    self.emit(Event::Markup);
                }
            }
            SettingsCmd::SetPolicy { policy } => {
                self.policy = policy.map(|p| {
                    let r = p.resolve();
                    (p, r)
                });
                self.emit(Event::FormattingPolicyChange);
            }
        }
        Ok(Outcome::Done(self.change_set()))
    }
}
