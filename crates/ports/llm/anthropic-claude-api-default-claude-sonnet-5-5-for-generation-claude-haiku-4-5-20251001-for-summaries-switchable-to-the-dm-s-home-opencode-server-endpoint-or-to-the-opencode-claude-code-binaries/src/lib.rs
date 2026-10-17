//! The adapters of the `llm` port (`crates/ports/llm`), the crate
//! `ports.json` names as its `adapter`.
//!
//! The crate exists so the port's declaration resolves; it holds no provider
//! yet. The API-mode clients and the binary-mode clients land in the tasks
//! that follow the port, each run against `ports_llm::contract::run`.
