use crate::{Error, Function, Lua, LuaOptions, StdLib, Value};

fn vm() -> Lua {
    Lua::new_with(
        StdLib::TABLE | StdLib::STRING,
        LuaOptions::default().catch_rust_panics(false),
    )
    .unwrap()
}
fn identity(lua: &Lua) -> Function {
    lua.load("return function(e) return e end").eval().unwrap()
}

#[test]
fn protected_call_retains_original_binary_table_and_single_result() {
    let lua = vm();
    for (code, expected) in [
        ("error('a'..string.char(255,0)..'b',0)", b"a\xff\0b".as_slice()),
        (
            "error({err='a'..string.char(255,0)..'b'},0)",
            b"a\xff\0b".as_slice(),
        ),
    ] {
        let function = lua.load(code).into_function().unwrap();
        let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
        let (status, value) = thread.resume::<(i32, Value)>((function, identity(&lua))).unwrap();
        assert_eq!(status, ffi::LUA_ERRRUN);
        let value = match value {
            Value::Table(t) => t.get::<Value>("err").unwrap(),
            v => v,
        };
        let Value::String(value) = value else {
            panic!("lost original binary body")
        };
        assert_eq!(&*value.as_bytes(), expected);
    }
    let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
    let function = lua.load("return 41,'ignored' ").into_function().unwrap();
    let (status, value) = thread.resume::<(i32, Value)>((function, identity(&lua))).unwrap();
    assert_eq!(status, 0);
    assert!(matches!(value, Value::Integer(41)));
}

#[test]
fn protected_call_bypasses_real_rust_panic_before_lua_handler() {
    let lua = vm();
    let target = lua
        .create_function(|_, ()| -> crate::Result<()> { panic!("actual protected host panic") })
        .unwrap();
    let handler = lua
        .load("return function(e) handler_entered=true;return 'masked' end")
        .eval::<Function>()
        .unwrap();
    let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        thread.resume::<(i32, Value)>((target, handler))
    }))
    .expect_err("original WrappedPanic must resume Rust unwinding");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"actual protected host panic"));
    assert!(lua.globals().get::<Value>("handler_entered").unwrap().is_nil());
}

#[test]
fn protected_call_preserves_genuine_allocator_status_in_body_and_handler() {
    for handler_failure in [false, true] {
        let lua = vm();
        let target = lua
            .load(if handler_failure {
                "error('body',0)"
            } else {
                "local t={};for i=1,1000000 do t[i]=i end;return t"
            })
            .into_function()
            .unwrap();
        let handler = if handler_failure {
            lua.load("return function(e) local t={};for i=1,1000000 do t[i]=i end;return e end")
                .eval()
                .unwrap()
        } else {
            identity(&lua)
        };
        let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
        lua.set_memory_limit(lua.used_memory() + 8192).unwrap();
        let (status, _value) = thread.resume::<(i32, Value)>((target, handler)).unwrap();
        assert_eq!(
            status,
            ffi::LUA_ERRMEM,
            "real handler OOM must not become ERRERR/business"
        );
    }
}

#[test]
fn protected_call_retains_typed_memory_cause_without_message_inference() {
    let lua = vm();
    // Typed component proof, distinct from the actual allocator test above.
    let target = lua
        .create_function(|_, ()| -> crate::Result<()> {
            Err(Error::MemoryError("typed allocator event".into()))
        })
        .unwrap();
    let handler = lua
        .load("return function(e) handler_entered=true;return 'masked' end")
        .eval::<Function>()
        .unwrap();
    let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
    let (status, value) = thread.resume::<(i32, Value)>((target, handler)).unwrap();
    assert_eq!(status, ffi::LUA_ERRRUN);
    assert!(matches!(value, Value::Error(_)));
    assert!(lua.globals().get::<Value>("handler_entered").unwrap().is_nil());
    let target = lua
        .load("error('typed allocator event',0)")
        .into_function()
        .unwrap();
    let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
    let (status, value) = thread.resume::<(i32, Value)>((target, identity(&lua))).unwrap();
    assert_eq!(status, ffi::LUA_ERRRUN);
    assert!(matches!(value, Value::String(_)));
}

#[cfg(not(feature = "luau"))]
#[test]
fn escaped_message_handler_cannot_borrow_retired_call_stack() {
    let lua = unsafe {
        Lua::unsafe_new_with(
            StdLib::TABLE | StdLib::DEBUG,
            LuaOptions::default().catch_rust_panics(false),
        )
    };
    let handler = lua
        .load("return function(e) escaped=debug.getinfo(2,'f').func;return e end")
        .eval::<Function>()
        .unwrap();
    let target = lua.load("error('first',0)").into_function().unwrap();
    let thread = lua.create_thread(lua.create_protected_call().unwrap()).unwrap();
    let (status, _) = thread.resume::<(i32, Value)>((target, handler)).unwrap();
    assert_eq!(status, ffi::LUA_ERRRUN);
    let escaped: Function = lua.globals().get("escaped").unwrap();
    let value: crate::String = escaped.call("later").unwrap();
    assert_eq!(value.to_str().unwrap(), "later");
}
