//! Aru Machine Protocol (AMP): typed communication between Aru and machines.
//!
//! AMP describes intent and results. It does not grant permissions. External,
//! destructive, and LPL actions remain controlled by the autonomy policy.

use kore_core::{Column, DataBlock, Value};
use kore_store::{KoreReader, KoreWriter};

pub const AMP_VERSION: &str = "amp/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineIntent {
    DiscoverCapabilities,
    Observe,
    Compute,
    Research,
    Plan,
    RequestApproval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineAction {
    ReadApprovedInput,
    RunBoundedComputation,
    StoreResult,
    RequestExternalRead,
    RequestExternalWrite,
    RequestLplAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineStatus {
    Accepted,
    Completed,
    Rejected,
    NeedsApproval,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LifeStatusMessage {
    pub protocol: String,
    pub status: String,
    pub pulse_count: u64,
    pub continuity_count: u64,
    pub maintenance_count: u64,
    pub lpl_access: bool,
}

impl LifeStatusMessage {
    pub fn from_state(state: &crate::life_state::LifeState) -> Self {
        Self {
            protocol: AMP_VERSION.to_string(),
            status: format!("{:?}", state.status),
            pulse_count: state.pulse_count,
            continuity_count: state.continuity_count,
            maintenance_count: state.maintenance_count,
            lpl_access: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MachineRequest {
    pub protocol: String,
    pub request_id: String,
    pub intent: MachineIntent,
    pub action: MachineAction,
    pub payload: String,
    pub lpl_access: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MachineResponse {
    pub protocol: String,
    pub request_id: String,
    pub status: MachineStatus,
    pub result: String,
    pub verified: bool,
    pub lpl_access: bool,
    pub reason: String,
}

impl MachineRequest {
    pub fn new(request_id: &str, intent: MachineIntent, action: MachineAction, payload: &str) -> Self {
        Self {
            protocol: AMP_VERSION.to_string(),
            request_id: request_id.to_string(),
            intent,
            action,
            payload: payload.to_string(),
            lpl_access: false,
        }
    }

    /// Native KORE wire encoding. No JSON is used for machine transport.
    pub fn to_kore_bytes(&self) -> Result<Vec<u8>, String> {
        let block = DataBlock::new(vec![
            Column::str_col("protocol", vec![Some(self.protocol.clone())]),
            Column::str_col("request_id", vec![Some(self.request_id.clone())]),
            Column::str_col("intent", vec![Some(format!("{:?}", self.intent))]),
            Column::str_col("action", vec![Some(format!("{:?}", self.action))]),
            Column::str_col("payload", vec![Some(self.payload.clone())]),
            Column::bool_col("lpl_access", vec![Some(self.lpl_access)]),
        ]).map_err(|error| error.to_string())?;
        Ok(KoreWriter::to_bytes(&block))
    }

    pub fn from_kore_bytes(bytes: &[u8]) -> Result<Self, String> {
        let block = KoreReader::from_bytes(bytes).map_err(|error| error.to_string())?;
        let text = |name: &str| -> Result<String, String> {
            block.columns.iter()
                .find(|column| column.name == name)
                .and_then(|column| column.data.get_str(0))
                .map(str::to_string)
                .ok_or_else(|| format!("missing KORE field: {name}"))
        };
        let lpl_access = block.columns.iter()
            .find(|column| column.name == "lpl_access")
            .and_then(|column| match column.data.get_value(0) { Value::Bool(value) => Some(value), _ => None })
            .ok_or_else(|| "missing KORE field: lpl_access".to_string())?;
        let intent = parse_intent(&text("intent")?)?;
        let action = parse_action(&text("action")?)?;
        let request = Self { protocol: text("protocol")?, request_id: text("request_id")?, intent, action, payload: text("payload")?, lpl_access };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != AMP_VERSION { return Err("unsupported AMP version".to_string()); }
        if self.request_id.trim().is_empty() { return Err("request_id is required".to_string()); }
        if self.lpl_access || self.action == MachineAction::RequestLplAccess {
            return Err("LPL access is permanently denied by Aru Machine Protocol".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trips_machine_request() {
        let request = MachineRequest::new(
            "req-1",
            MachineIntent::Compute,
            MachineAction::RunBoundedComputation,
            "job=pell",
        );
        let encoded = request.to_kore_bytes().unwrap();
        let decoded = MachineRequest::from_kore_bytes(&encoded).unwrap();
        assert_eq!(decoded, request);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn protocol_rejects_lpl_access() {
        let request = MachineRequest::new(
            "req-2",
            MachineIntent::RequestApproval,
            MachineAction::RequestLplAccess,
            "",
        );
        assert!(request.validate().is_err());
    }

    #[test]
    fn machine_can_observe_life_without_lpl_access() {
        let state = crate::life_state::LifeState::new("t0");
        let message = LifeStatusMessage::from_state(&state);
        assert_eq!(message.protocol, AMP_VERSION);
        assert!(!message.lpl_access);
    }
}

fn parse_intent(value: &str) -> Result<MachineIntent, String> {
    match value {
        "DiscoverCapabilities" => Ok(MachineIntent::DiscoverCapabilities),
        "Observe" => Ok(MachineIntent::Observe),
        "Compute" => Ok(MachineIntent::Compute),
        "Research" => Ok(MachineIntent::Research),
        "Plan" => Ok(MachineIntent::Plan),
        "RequestApproval" => Ok(MachineIntent::RequestApproval),
        _ => Err(format!("unknown machine intent: {value}")),
    }
}

fn parse_action(value: &str) -> Result<MachineAction, String> {
    match value {
        "ReadApprovedInput" => Ok(MachineAction::ReadApprovedInput),
        "RunBoundedComputation" => Ok(MachineAction::RunBoundedComputation),
        "StoreResult" => Ok(MachineAction::StoreResult),
        "RequestExternalRead" => Ok(MachineAction::RequestExternalRead),
        "RequestExternalWrite" => Ok(MachineAction::RequestExternalWrite),
        "RequestLplAccess" => Ok(MachineAction::RequestLplAccess),
        _ => Err(format!("unknown machine action: {value}")),
    }
}
