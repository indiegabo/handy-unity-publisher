//! Starts the desktop shell entrypoint for the bundled local runtime
//! supervisor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    desktop_shell::run();
}