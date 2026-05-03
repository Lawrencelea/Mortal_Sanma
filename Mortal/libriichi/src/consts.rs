use crate::py_helper::add_submodule;

use pyo3::prelude::*;

pub const MAX_VERSION: u32 = 5;

pub const ACTION_RIICHI: usize = 37;
pub const ACTION_NUKIDORA: usize = 38;
pub const ACTION_PON: usize = 39;
pub const ACTION_KAN: usize = 40;
pub const ACTION_AGARI: usize = 41;
pub const ACTION_RYUKYOKU: usize = 42;
pub const ACTION_PASS: usize = 43;

pub const ACTION_SPACE: usize = 37 // discard | kan tile choice
                              + 1  // riichi
                              + 1  // nukidora
                              + 1  // pon
                              + 1  // kan decision
                              + 1  // agari
                              + 1  // ryukyoku
                              + 1; // pass
// = 44
pub const GRP_SIZE: usize = 7;
// GRP input size for sanma (3-player): grand_kyoku + honba + kyotaku + 3 scores
pub const GRP_SANMA_SIZE: usize = 6;

#[pyfunction]
#[inline]
pub const fn obs_shape(version: u32) -> (usize, usize) {
    match version {
        1 => (936, 34),
        2 => (940, 34),
        3 => (932, 34),
        4 => (1010, 34),
        // version 5: sanma (3-player) variant of version 4 without chi planes,
        // with nukidora as its own legal action plane.
        5 => (774, 34),
        _ => unreachable!(),
    }
}

#[pyfunction]
#[inline]
pub const fn oracle_obs_shape(version: u32) -> (usize, usize) {
    match version {
        1 => (211, 34),
        2 | 3 | 4 => (217, 34),
        5 => (170, 34),
        _ => unreachable!(),
    }
}

pub(crate) fn register_module(
    py: Python<'_>,
    prefix: &str,
    super_mod: &Bound<'_, PyModule>,
) -> PyResult<()> {
    let m = PyModule::new(py, "consts")?;
    m.add_function(wrap_pyfunction!(obs_shape, &m)?)?;
    m.add_function(wrap_pyfunction!(oracle_obs_shape, &m)?)?;
    m.add("MAX_VERSION", MAX_VERSION)?;
    m.add("ACTION_SPACE", ACTION_SPACE)?;
    m.add("ACTION_RIICHI", ACTION_RIICHI)?;
    m.add("ACTION_NUKIDORA", ACTION_NUKIDORA)?;
    m.add("ACTION_PON", ACTION_PON)?;
    m.add("ACTION_KAN", ACTION_KAN)?;
    m.add("ACTION_AGARI", ACTION_AGARI)?;
    m.add("ACTION_RYUKYOKU", ACTION_RYUKYOKU)?;
    m.add("ACTION_PASS", ACTION_PASS)?;
    m.add("GRP_SIZE", GRP_SIZE)?;
    m.add("GRP_SANMA_SIZE", GRP_SANMA_SIZE)?;
    add_submodule(py, prefix, super_mod, &m)
}
