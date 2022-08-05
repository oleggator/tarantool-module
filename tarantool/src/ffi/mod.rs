#[doc(hidden)]
pub mod helper;
#[doc(hidden)]
pub use ::tlua::ffi as lua;
#[doc(hidden)]
pub mod tarantool;
#[doc(hidden)]
pub mod decimal;
#[doc(hidden)]
pub mod uuid;
#[doc(hidden)]
pub mod sql;

/// Check whether the current tarantool executable supports decimal api.
/// If this function returns `false` using any of the functions in
/// [`tarantool::decimal`] will result in a **panic**.
///
/// [`tarantool::decimal`]: mod@crate::decimal
pub fn has_decimal() -> bool {
    true
}

/// Check whether the current tarantool executable supports fiber::channel api.
/// If this function returns `false` using any of the functions in
/// [`tarantool::fiber::channel`] will result in a **panic**.
///
/// [`tarantool::fiber::channel`]: crate::fiber::channel
pub fn has_fiber_channel() -> bool {
    unsafe {
        let name = crate::c_str!("fiber_channel_new");
        helper::tnt_internal_symbol::<*const ()>(name).is_some() ||
        helper::has_dyn_symbol(name)
    }
}

/// Check whether the current tarantool executable supports getting tuple fields
/// by json pattern.
/// If this function returns `false` then
/// - passing a string to [`Tuple::try_get`] will always result in an `Error`,
/// - passing a string to [`Tuple::get`] will always result in a **panic**.
///
/// [`Tuple::try_get`]: crate::tuple::Tuple::try_get
/// [`Tuple::get`]: crate::tuple::Tuple::get
pub fn has_tuple_field_by_path() -> bool {
    unsafe {
        crate::ffi::helper::has_dyn_symbol(crate::c_str!("tuple_field_raw_by_full_path"))
    }
}
