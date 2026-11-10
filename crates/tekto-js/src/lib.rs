//! JS / TypeScript host.
//!
//! Desktop and Android will later embed QuickJS-ng (rquickjs) behind the
//! `quickjs` feature, so everyday Rust builds stay fast.
//! On the web, this crate is not used as an engine: the TypeScript app
//! runs in the browser, and talks to a WASM build of the Rust core
//! through the same op protocol defined here.
//!
//! The first slice only defines that protocol.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum JsError {
    #[error("{0}")]
    Message(String),
    #[error("the QuickJS engine is not enabled; build tekto-js with --features quickjs (desktop) or use the browser engine on web")]
    EngineDisabled,
}

/// One mutation sent from the TS signals / JSX runtime into the Rust tree.
#[derive(Clone, Debug)]
pub enum HostOp {
    Create { kind: String },
    SetText { id: u64, text: String },
    SetProp { id: u64, key: String, value: String },
    Append { parent: u64, child: u64 },
    Remove { id: u64 },
}

/// Host that applies ops. The desktop QuickJS runtime and the web WASM
/// glue both call this.
pub trait Host {
    fn apply(&mut self, ops: &[HostOp]) -> Result<(), JsError>;
}

/// Placeholder until the QuickJS feature lands.
pub fn eval_module(_source: &str, _host: &mut dyn Host) -> Result<(), JsError> {
    Err(JsError::EngineDisabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Noop;
    impl Host for Noop {
        fn apply(&mut self, _ops: &[HostOp]) -> Result<(), JsError> {
            Ok(())
        }
    }

    #[test]
    fn engine_is_disabled_by_default() {
        let err = eval_module("export {}", &mut Noop).unwrap_err();
        assert!(matches!(err, JsError::EngineDisabled));
    }
}
