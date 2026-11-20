//! nxf keeps a NixOS machine's local flake updated.

pub mod auto_update;
pub mod cli;
pub mod doctor;
pub mod env;
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
pub mod process;
pub mod progress;
pub mod rollout;
pub mod ssh;
pub mod state;
pub mod status;
pub mod strategy;
pub mod tasks;
pub mod text;
pub mod time;
