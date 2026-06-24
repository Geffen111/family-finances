// Prevent an extra console/terminal window from opening alongside the app on
// Windows release builds. Kept enabled in debug so dev logs still show.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    family_finances_lib::run();
}
