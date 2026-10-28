//! Visibility and focus rules for the companion panel (PLAN section 0), as a
//! pure state machine. The Tauri shell feeds it events and performs the
//! returned `Outcome`; the Win32 calls stay in thin functions on the other side.
//!
//! The invariant the rules protect: only a deliberate user action (summon,
//! click) ever takes keyboard focus. Everything the app or an agent causes
//! shows the panel passively.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Nothing on screen.
    Hidden,
    /// The mascot tucked against the screen edge.
    Tab,
    /// Pinned ticker: last request, status, agent activity.
    Compact,
    /// The full panel.
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    Summon,
    Hover,
    Passive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Global hotkey: opens and takes focus, or closes when already focused.
    Summon,
    /// Esc inside the panel.
    Dismiss,
    /// The user clicked into the panel.
    Clicked,
    /// Focus moved to another window.
    FocusLost,
    /// A request is in flight, an agent is working, a response just finished.
    Activity,
    /// Nothing is happening any more.
    Idle,
    /// The agent needs a decision; the full panel must be visible.
    ApprovalNeeded,
    ApprovalResolved,
    HoverEnter,
    HoverLeave,
    TogglePin,
    SetAlwaysShowTab(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Unchanged,
    /// Remember the foreground window, then activate the panel.
    Take,
    /// Return focus to the remembered window (if the panel still has it).
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub mode: Mode,
    pub mode_changed: bool,
    pub focus: Focus,
}

#[derive(Debug, Clone, Copy)]
pub struct PanelState {
    mode: Mode,
    cause: Cause,
    focused: bool,
    pinned: bool,
    always_show_tab: bool,
    approval_pending: bool,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            mode: Mode::Hidden,
            cause: Cause::Passive,
            focused: false,
            pinned: false,
            always_show_tab: false,
            approval_pending: false,
        }
    }
}

impl PanelState {
    pub fn new(always_show_tab: bool, pinned: bool) -> Self {
        let mut s = Self {
            always_show_tab,
            pinned,
            ..Self::default()
        };
        s.mode = s.resting();
        s
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Where the panel sits when nothing is asking for attention.
    fn resting(&self) -> Mode {
        if self.pinned {
            Mode::Compact
        } else if self.always_show_tab {
            Mode::Tab
        } else {
            Mode::Hidden
        }
    }

    pub fn step(&mut self, event: Event) -> Outcome {
        let before = self.mode;
        let focus = self.apply(event);
        Outcome {
            mode: self.mode,
            mode_changed: self.mode != before,
            focus,
        }
    }

    fn apply(&mut self, event: Event) -> Focus {
        match event {
            Event::Summon => {
                if self.mode == Mode::Open && self.focused {
                    self.close()
                } else {
                    self.mode = Mode::Open;
                    self.cause = Cause::Summon;
                    self.focused = true;
                    Focus::Take
                }
            }
            Event::Dismiss => {
                // Esc never dismisses a pending approval: that needs a decision.
                if self.approval_pending {
                    Focus::Unchanged
                } else {
                    self.close()
                }
            }
            Event::Clicked => {
                if self.mode == Mode::Open && !self.focused {
                    self.focused = true;
                    Focus::Take
                } else {
                    Focus::Unchanged
                }
            }
            Event::FocusLost => {
                self.focused = false;
                if self.mode == Mode::Open && !self.pinned && !self.approval_pending {
                    self.mode = self.resting();
                }
                Focus::Release
            }
            Event::Activity => {
                if matches!(self.mode, Mode::Hidden | Mode::Tab) {
                    self.mode = Mode::Compact;
                    self.cause = Cause::Passive;
                }
                Focus::Unchanged
            }
            Event::Idle => {
                if self.mode == Mode::Compact && self.cause == Cause::Passive && !self.pinned {
                    self.mode = self.resting();
                }
                Focus::Unchanged
            }
            Event::ApprovalNeeded => {
                self.approval_pending = true;
                if self.mode != Mode::Open {
                    self.mode = Mode::Open;
                    self.cause = Cause::Passive;
                }
                // Never focus: keystrokes meant for another window must not land here.
                Focus::Unchanged
            }
            Event::ApprovalResolved => {
                self.approval_pending = false;
                if self.mode == Mode::Open && !self.focused && self.cause == Cause::Passive {
                    self.mode = self.resting();
                }
                Focus::Unchanged
            }
            Event::HoverEnter => {
                if matches!(self.mode, Mode::Tab | Mode::Compact) {
                    self.mode = Mode::Open;
                    self.cause = Cause::Hover;
                }
                Focus::Unchanged
            }
            Event::HoverLeave => {
                if self.mode == Mode::Open
                    && self.cause == Cause::Hover
                    && !self.focused
                    && !self.approval_pending
                {
                    self.mode = self.resting();
                }
                Focus::Unchanged
            }
            Event::TogglePin => {
                self.pinned = !self.pinned;
                if !self.pinned && self.mode == Mode::Compact {
                    self.mode = self.resting();
                } else if self.pinned && matches!(self.mode, Mode::Hidden | Mode::Tab) {
                    self.mode = Mode::Compact;
                }
                Focus::Unchanged
            }
            Event::SetAlwaysShowTab(on) => {
                self.always_show_tab = on;
                if matches!(self.mode, Mode::Hidden | Mode::Tab) {
                    self.mode = self.resting();
                }
                Focus::Unchanged
            }
        }
    }

    fn close(&mut self) -> Focus {
        self.mode = self.resting();
        let had_focus = std::mem::take(&mut self.focused);
        if had_focus {
            Focus::Release
        } else {
            Focus::Unchanged
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(state: &mut PanelState, events: &[Event]) -> Outcome {
        events.iter().map(|e| state.step(*e)).last().unwrap()
    }

    #[test]
    fn idle_panel_is_hidden_by_default() {
        assert_eq!(PanelState::default().mode(), Mode::Hidden);
        assert_eq!(PanelState::new(true, false).mode(), Mode::Tab);
        assert_eq!(PanelState::new(false, true).mode(), Mode::Compact);
    }

    #[test]
    fn summon_opens_and_takes_focus_then_summon_again_closes_and_releases() {
        let mut s = PanelState::default();
        let o = s.step(Event::Summon);
        assert_eq!((o.mode, o.focus), (Mode::Open, Focus::Take));
        let o = s.step(Event::Summon);
        assert_eq!((o.mode, o.focus), (Mode::Hidden, Focus::Release));
    }

    #[test]
    fn esc_closes_to_the_resting_mode_and_hands_focus_back() {
        let mut s = PanelState::new(true, false);
        s.step(Event::Summon);
        let o = s.step(Event::Dismiss);
        assert_eq!((o.mode, o.focus), (Mode::Tab, Focus::Release));
    }

    #[test]
    fn clicking_away_closes_unless_pinned() {
        let mut s = PanelState::default();
        s.step(Event::Summon);
        assert_eq!(s.step(Event::FocusLost).mode, Mode::Hidden);

        let mut pinned = PanelState::new(false, true);
        pinned.step(Event::Summon);
        assert_eq!(pinned.step(Event::FocusLost).mode, Mode::Open);
    }

    #[test]
    fn passive_events_never_take_focus() {
        let mut s = PanelState::default();
        for e in [
            Event::Activity,
            Event::ApprovalNeeded,
            Event::HoverEnter,
            Event::Idle,
        ] {
            assert_ne!(s.step(e).focus, Focus::Take, "{e:?}");
        }
    }

    #[test]
    fn activity_shows_the_ticker_and_idle_hides_it_again() {
        let mut s = PanelState::default();
        assert_eq!(s.step(Event::Activity).mode, Mode::Compact);
        assert_eq!(s.step(Event::Idle).mode, Mode::Hidden);
    }

    #[test]
    fn idle_does_not_hide_a_pinned_ticker() {
        let mut s = PanelState::new(false, true);
        run(&mut s, &[Event::Activity, Event::Idle]);
        assert_eq!(s.mode(), Mode::Compact);
    }

    #[test]
    fn activity_never_shrinks_an_open_panel() {
        let mut s = PanelState::default();
        s.step(Event::Summon);
        assert_eq!(s.step(Event::Activity).mode, Mode::Open);
        assert_eq!(s.step(Event::Idle).mode, Mode::Open);
    }

    #[test]
    fn approval_opens_without_focus_and_survives_esc_and_focus_loss() {
        let mut s = PanelState::default();
        let o = s.step(Event::ApprovalNeeded);
        assert_eq!((o.mode, o.focus), (Mode::Open, Focus::Unchanged));
        assert_eq!(s.step(Event::Dismiss).mode, Mode::Open);
        assert_eq!(s.step(Event::FocusLost).mode, Mode::Open);
        assert_eq!(s.step(Event::HoverLeave).mode, Mode::Open);
        assert_eq!(s.step(Event::ApprovalResolved).mode, Mode::Hidden);
    }

    #[test]
    fn resolving_an_approval_keeps_a_panel_the_user_is_working_in() {
        let mut s = PanelState::default();
        s.step(Event::ApprovalNeeded);
        s.step(Event::Clicked);
        assert_eq!(s.step(Event::ApprovalResolved).mode, Mode::Open);
    }

    #[test]
    fn hover_opens_from_the_tab_and_closes_on_leave_unless_clicked() {
        let mut s = PanelState::new(true, false);
        assert_eq!(s.step(Event::HoverEnter).mode, Mode::Open);
        assert_eq!(s.step(Event::HoverLeave).mode, Mode::Tab);

        s.step(Event::HoverEnter);
        s.step(Event::Clicked);
        assert_eq!(s.step(Event::HoverLeave).mode, Mode::Open);
    }

    #[test]
    fn hover_does_nothing_when_fully_hidden() {
        let mut s = PanelState::default();
        assert!(!s.step(Event::HoverEnter).mode_changed);
    }

    #[test]
    fn pin_toggles_the_resting_mode() {
        let mut s = PanelState::default();
        assert_eq!(s.step(Event::TogglePin).mode, Mode::Compact);
        assert_eq!(s.step(Event::TogglePin).mode, Mode::Hidden);
    }

    #[test]
    fn always_show_tab_setting_applies_immediately() {
        let mut s = PanelState::default();
        assert_eq!(s.step(Event::SetAlwaysShowTab(true)).mode, Mode::Tab);
        assert_eq!(s.step(Event::SetAlwaysShowTab(false)).mode, Mode::Hidden);
    }
}
