use super::*;
use nez_protocol::error::NezErrorDetails;

pub(super) fn environment_selection_error(err: NezErr) -> JSONRPCErrorError {
    match err.details() {
        NezErrorDetails::InvalidRequest(message) => invalid_request(message.clone()),
        _ => internal_error(format!("failed to validate environment selections: {err}")),
    }
}
