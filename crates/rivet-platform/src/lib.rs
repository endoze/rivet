//! OS-specific code that is not specific to this app: Steam discovery, Proton
//! prefixes, standard directories, r2modman's profiles, putting a new build
//! in place of a running program, keeping one copy of a program running, and
//! listing a program in Linux app menus and the Windows Start Menu, and
//! opening a kind of file with it.

#[cfg(target_os = "linux")]
pub mod desktop;
pub mod executable;
pub mod file_types;
pub mod instance;
pub mod paths;
pub mod r2modman;
pub mod start_menu;
pub mod steam;
