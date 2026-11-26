//! The spending-limit gate card: the shared state machine plus the answer in flight.

use jcode_session_types::routing::{RoutingGateChoice, RoutingGateOption};
use jcode_session_types::routing_gate::{GateKey, GateOutcome, RoutingGateModel};

/// An open gate: the shared state machine plus the answer in flight.
#[derive(Debug, Clone)]
pub struct GateCard {
    model: RoutingGateModel,
    /// Attempt number of the answer sent and not yet settled.
    pub attempt: Option<u64>,
    /// Why the last answer failed, shown on the card.
    pub error: Option<String>,
}

/// A read-only snapshot of an open gate for rendering.
pub struct GateView<'a> {
    pub gate_id: &'a str,
    pub reason: &'a str,
    /// Display text of each answer row, the cancel row last.
    pub rows: Vec<String>,
    /// The focused row.
    pub cursor: usize,
    /// An answer is in flight, so the card accepts no input.
    pub submitting: bool,
}

impl GateCard {
    pub fn new(gate_id: String, reason: String, options: Vec<RoutingGateOption>) -> Self {
        Self {
            model: RoutingGateModel::new(gate_id, reason, options),
            attempt: None,
            error: None,
        }
    }

    /// What the card shows right now.
    pub fn view(&self) -> GateView<'_> {
        GateView {
            gate_id: &self.model.gate_id,
            reason: &self.model.reason,
            rows: self.model.rows(),
            cursor: self.model.cursor,
            submitting: self.submitting(),
        }
    }

    fn submitting(&self) -> bool {
        self.attempt.is_some()
    }

    pub fn press(&mut self, key: GateKey) -> Option<RoutingGateChoice> {
        if self.submitting() {
            return None;
        }
        match self.model.press(key) {
            GateOutcome::Pending => None,
            GateOutcome::Done(choice) => Some(choice),
        }
    }

    /// A click on `row` answers it, and also moves the focus there.
    pub fn choose_row(&mut self, row: usize) -> Option<RoutingGateChoice> {
        if self.submitting() || row >= self.model.row_count() {
            return None;
        }
        self.model.cursor = row;
        Some(self.model.choice_at(row))
    }

    pub fn mark_submitting(&mut self, attempt: u64) {
        self.attempt = Some(attempt);
        self.error = None;
    }

    pub fn answer_failed(&mut self, error: String) {
        self.attempt = None;
        self.error = Some(error);
    }
}

#[cfg(test)]
mod tests;
