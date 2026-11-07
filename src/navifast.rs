//! Navifast desktop command.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod entrypoint;

fn main() -> Result<(), String> {
    entrypoint::run()
}
