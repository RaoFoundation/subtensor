//! Local fixed-point conversion macros for coinbase emission math.

macro_rules! as_u96f32 {
    ($val:expr) => {
        ::substrate_fixed::types::U96F32::saturating_from_num($val)
    };
}
pub(crate) use as_u96f32;

macro_rules! to_u64 {
    ($val:expr) => {
        $val.saturating_to_num::<u64>()
    };
}
pub(crate) use to_u64;
