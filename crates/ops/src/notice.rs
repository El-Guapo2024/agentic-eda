//! What KiCad tells the user after a command that worked: `EDA_BASE_FRAME::ShowInfoBarMsg` and the `wxLogMessage` lines a tool logs along the way
//! (`EDIT_TOOL::BooleanPolygons`' "Some of the polygons could not be merged.", `BOARD_EDITOR_CONTROL::ZoneMerge`'s "Some zones did not intersect and
//! were not merged."). A verb notes its message here while it runs; whoever ran the command takes them after it (the studio's `/api/cmd` reply carries
//! them as `info`). Kept per thread: one request runs one command on its own thread, and a message never outlives the command it belongs to
//! ([`clear`] before, [`take`] after).

use std::cell::RefCell;

thread_local! {
    static NOTICES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static DRC_VIOLATION: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Note the error KiCad shows in the info bar as a DRC violation (`ShowInfoBarError( .., WX_INFOBAR::MESSAGE_TYPE::DRC_VIOLATION )`) while the command still
/// did its work: Draw Via's "Via location violates DRC." with the router's Allow DRC violations on. The studio's `/api/cmd` reply carries it as
/// `drc_violation`; a reply without it after a via is placed dismisses a standing one (`infobar->Dismiss()` of a `DRC_VIOLATION` bar).
pub fn note_drc_violation(message: impl Into<String>) {
    let message = message.into();
    DRC_VIOLATION.with(|n| *n.borrow_mut() = Some(message));
}

/// The DRC violation noted since [`clear`], and forget it.
pub fn take_drc_violation() -> Option<String> {
    DRC_VIOLATION.with(|n| n.borrow_mut().take())
}

/// Note a message for the command running on this thread; the same message twice is said once.
pub fn note(message: impl Into<String>) {
    let message = message.into();
    NOTICES.with(|n| {
        let mut n = n.borrow_mut();
        if !n.contains(&message) {
            n.push(message);
        }
    });
}

/// Forget what an earlier command noted.
pub fn clear() {
    NOTICES.with(|n| n.borrow_mut().clear());
    DRC_VIOLATION.with(|n| *n.borrow_mut() = None);
}

/// The messages noted since [`clear`], in order, and forget them.
pub fn take() -> Vec<String> {
    NOTICES.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_message_is_said_once_and_taken_once() {
        super::clear();
        super::note("a");
        super::note("b");
        super::note("a");
        assert_eq!(super::take(), vec!["a".to_string(), "b".to_string()]);
        assert!(super::take().is_empty());
    }
}
