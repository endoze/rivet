//! OS-specific code that is not specific to this app: Steam discovery, Proton
//! prefixes, standard directories, r2modman's profiles, and putting a new
//! build in place of a running program.

pub mod executable;
pub mod paths;
pub mod r2modman;
pub mod steam;
