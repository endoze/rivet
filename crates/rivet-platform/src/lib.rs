//! OS-specific code that is not specific to this app: Steam discovery, Proton
//! prefixes, standard directories, r2modman's profiles, putting a new build
//! in place of a running program, and listing a program in Linux app menus
//! and the Windows Start Menu.

#[cfg(target_os = "linux")]
pub mod desktop;
pub mod executable;
pub mod paths;
pub mod r2modman;
pub mod start_menu;
pub mod steam;
