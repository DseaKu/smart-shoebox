//! Smart Shoebox: the shoebox for your paperwork, except you can actually find things in it.
//!
//! The library holds all the logic. Two front ends use it: the command-line tool
//! `shoebox` (src/main.rs) and, with the `gui` feature, the window `shoebox-gui`
//! (src/bin/shoebox-gui.rs).

pub mod add;
pub mod complete;
pub mod config;
pub mod entry;
pub mod export;
pub mod find;
#[cfg(feature = "gui")]
pub mod gui;
pub mod hash;
pub mod index;
pub mod llm;
pub mod merge;
pub mod naming;
pub mod pages;
pub mod pdf;
pub mod pdfdoc;
pub mod prompt;
pub mod rename;
pub mod roles;
pub mod scan;
pub mod search;
pub mod sort;
pub mod store;
pub mod trash;
pub mod wizard;
pub mod workspace;

#[cfg(test)]
mod flows;
#[cfg(test)]
mod testutil;

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;
