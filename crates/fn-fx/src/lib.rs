//! fn-fx: the world-space harvest FX of the 1.8 port, built from the original packages (see the crate description). Modules:
//! * [`cooked`]   shader programs of a material package with their vertex factory / shader type ownership
#[cfg(feature = "render")]
pub mod bridge;
pub mod cooked;
pub mod data;
#[cfg(feature = "render")]
pub mod gpu;
pub mod material;
#[cfg(feature = "render")]
pub mod numbers;
pub mod particles;
pub mod program;
#[cfg(feature = "render")]
pub mod ring;
pub mod uniform;
pub mod view;

pub use fn_pak::PakSet;

/// The install the research tools and the tests read when `FN_INSTALL` is not set (read only; the client is never run).
pub fn default_install() -> std::path::PathBuf {
    fn_dxbc_cli::spec::default_install()
}
