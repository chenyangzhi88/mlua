// Controlled mlua0.11.6 local extension; original crate MIT license applies.
use super::{get_internal_userdata, WrappedFailure};
use crate::Error;
use std::os::raw::c_int;
use std::ptr;

// The original panic bypass is shared by public safe_xpcall and the scoped
// call entry below. A panic must never enter a Lua message handler.
unsafe fn message_handler(state: *mut ffi::lua_State, protected_status: *mut c_int) -> c_int {
    ffi::luaL_checkstack(state, 2, ptr::null());
    let wf_ud = get_internal_userdata::<WrappedFailure>(state, -1, ptr::null());
    if let Some(WrappedFailure::Panic(_)) = wf_ud.as_ref() {
        return 1;
    }
    if !protected_status.is_null() {
        if let Some(WrappedFailure::Error(error)) = wf_ud.as_ref() {
            if genuine_memory_error(error) {
                return 1;
            }
        }
    }
    ffi::lua_pushvalue(state, ffi::lua_upvalueindex(1));
    ffi::lua_insert(state, 1);
    if protected_status.is_null() {
        ffi::lua_call(state, ffi::lua_gettop(state) - 1, ffi::LUA_MULTRET);
        return ffi::lua_gettop(state);
    }
    // Retain handler allocation failure before Lua folds it into ERRERR.
    *protected_status = ffi::lua_pcall(state, ffi::lua_gettop(state) - 1, 1, 0);
    if *protected_status != ffi::LUA_OK {
        ffi::lua_error(state);
    }
    ffi::lua_gettop(state)
}

fn genuine_memory_error(error: &Error) -> bool {
    match error {
        Error::MemoryError(_) => true,
        Error::CallbackError { cause, .. }
        | Error::BadArgument { cause, .. }
        | Error::WithContext { cause, .. } => genuine_memory_error(cause),
        _ => false,
    }
}

pub(super) unsafe extern "C-unwind" fn xpcall_message_handler(state: *mut ffi::lua_State) -> c_int {
    message_handler(state, ptr::null_mut())
}

unsafe extern "C-unwind" fn protected_message_handler(state: *mut ffi::lua_State) -> c_int {
    let pointer = ffi::lua_touserdata(state, ffi::lua_upvalueindex(2)).cast::<c_int>();
    // The temporary closure is invalidated before its original C stack retires.
    // An escaped handler cannot access a former invocation's stack address.
    if pointer.is_null() {
        return 1;
    }
    message_handler(state, pointer)
}

/// One result plus the actual Lua call status. Arguments are target, handler,
/// followed by target arguments. The status is not inferred from an error value.
pub(crate) unsafe extern "C-unwind" fn call(state: *mut ffi::lua_State) -> c_int {
    ffi::luaL_checkstack(state, 4, ptr::null());
    ffi::luaL_checktype(state, 1, ffi::LUA_TFUNCTION);
    ffi::luaL_checktype(state, 2, ffi::LUA_TFUNCTION);
    let argument_count = ffi::lua_gettop(state) - 2;
    let mut handler_status = ffi::LUA_OK;
    ffi::lua_pushvalue(state, 2);
    ffi::lua_pushlightuserdata(state, (&mut handler_status as *mut c_int).cast());
    ffi::lua_pushcclosure(state, protected_message_handler, 2);
    ffi::lua_copy(state, 1, 2);
    ffi::lua_replace(state, 1);
    let mut status = ffi::lua_pcall(state, argument_count, 1, 1);
    // lua_pcall retains the original handler at slot 1. Clear its pointer before
    // exposing the result or permitting a caught panic to resume Rust unwinding.
    ffi::lua_pushnil(state);
    ffi::lua_setupvalue(state, 1, 2);
    if handler_status == ffi::LUA_ERRMEM {
        status = ffi::LUA_ERRMEM;
    }
    ffi::lua_remove(state, 1);
    ffi::lua_pushinteger(state, status as ffi::lua_Integer);
    ffi::lua_insert(state, 1);
    2
}
