//! Optional modules built on top of the core editor. The core must work
//! without them; each one is (re)connected through the core's hooks.
pub(crate) mod agenda;
pub(crate) mod daily;
pub(crate) mod flow;
pub(crate) mod references;

/// The modules every window starts with (each can be turned off in
/// Settings → Modules & vaults).
pub(crate) fn all() -> Vec<Box<dyn crate::module_api::Module>> {
    vec![Box::new(references::References::default())]
}
