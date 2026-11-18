//! nxf keeps a NixOS machine's local flake updated.

pub mod auto_update;
pub mod cli;
pub mod doctor;
pub mod flake;
pub mod fleet;
pub mod git;
pub mod health;
pub mod hold;
pub mod hooks;
pub mod hosts;
pub mod init;
pub mod install;
pub mod nixos;
pub mod output;
pub mod progress;
pub mod reconcile;
pub mod status;
pub mod tasks;
pub mod text;
