use std::mem::size_of;

use crate::{Error, IntoLua, Lua, LuaOptions, Result, StdLib, Table, Value, ffi};

fn vm(limit: usize) -> Lua {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING,
        LuaOptions::default().catch_rust_panics(false),
    )
    .unwrap();
    lua.set_memory_limit(limit).unwrap();
    lua
}

fn target(lua: &Lua, metatable: bool) -> Table {
    let table = lua.create_sequence_from([2, 1]).unwrap();
    table.set("original", 17).unwrap();
    if metatable {
        let meta = lua.create_table().unwrap();
        meta.set(
            "__newindex",
            lua.load("return function() seen = true end")
                .eval::<crate::Function>()
                .unwrap(),
        )
        .unwrap();
        table.set_metatable(Some(meta)).unwrap();
    }
    table
}

fn assert_readonly(error: Error) {
    assert!(
        error.to_string().contains("Attempt to modify a readonly table"),
        "{error}"
    );
}

fn assert_original(table: &Table) {
    assert_eq!(table.raw_get::<i64>(1).unwrap(), 2);
    assert_eq!(table.raw_get::<i64>(2).unwrap(), 1);
    assert_eq!(table.raw_len(), 2);
    assert_eq!(table.raw_get::<i64>("original").unwrap(), 17);
    assert!(table.raw_get::<Value>("new").unwrap().is_nil());
}

#[test]
fn native_readonly_covers_lua_writes_rawset_mutators_metatables_and_redirects() {
    for limit in [0, 16 * 1024 * 1024] {
        for metatable in [false, true] {
            let lua = vm(limit);
            let table = target(&lua, metatable);
            lua.globals().set("protected", table.clone()).unwrap();
            table.set_readonly(true);
            assert!(table.is_readonly());
            for code in [
                "protected.new = 9",
                "protected.original = 99",
                "protected.original = nil",
                "protected[1] = 99",
                "rawset(protected, 'original', 99)",
                "rawset(protected, 1, 99)",
                "setmetatable(protected, {})",
                "setmetatable(protected, nil)",
                "table.insert(protected, 1, 99)",
                "table.remove(protected, 1)",
                "table.sort(protected)",
                "local alias = protected; alias.original = 99",
                "local t=setmetatable({}, {__newindex=protected}); t.new=9",
                "local t=setmetatable({}, {__newindex=function() rawset(protected,'new',9) end}); t.new=9",
            ] {
                assert_readonly(lua.load(code).exec().unwrap_err());
                assert_original(&table);
                assert_eq!(
                    lua.load("return protected.original + 1").eval::<i64>().unwrap(),
                    18
                );
                assert!(lua.globals().get::<Value>("seen").unwrap().is_nil());
            }
            let caught: (bool, String) = lua
                .load("return pcall(rawset,protected,'original',99)")
                .eval()
                .unwrap();
            assert!(!caught.0);
            assert!(caught.1.contains("Attempt to modify a readonly table"));
            table.set_readonly(false);
            assert!(!table.is_readonly());
            table.raw_set("original", 99).unwrap();
            assert_eq!(table.raw_get::<i64>("original").unwrap(), 99);
        }
    }
}

#[test]
fn native_readonly_rust_writes_return_errors_without_unwinding_across_ffi() {
    type Write = (&'static str, fn(&Table) -> Result<()>);
    let writes: &[Write] = &[
        ("set", |t| t.set("original", 99)),
        ("raw_set", |t| t.raw_set("original", 99)),
        ("raw_seti", |t| t.raw_seti(1, 99)),
        ("push", |t| t.push(99)),
        ("raw_push", |t| t.raw_push(99)),
        ("pop", |t| t.pop::<Value>().map(drop)),
        ("raw_pop", |t| t.raw_pop::<Value>().map(drop)),
        ("raw_insert", |t| t.raw_insert(1, 99)),
        ("raw_remove_index", |t| t.raw_remove(1)),
        ("raw_remove_key", |t| t.raw_remove("original")),
        ("clear", |t| t.clear()),
        ("set_metatable", |t| t.set_metatable(None)),
    ];
    for limit in [0, 16 * 1024 * 1024] {
        for metatable in [false, true] {
            let lua = vm(limit);
            let table = target(&lua, metatable);
            table.set_readonly(true);
            for (name, write) in writes {
                let before = unsafe { ffi::lua_gettop(lua.lock().state()) };
                assert_readonly(write(&table).unwrap_err());
                let after = unsafe { ffi::lua_gettop(lua.lock().state()) };
                assert_eq!(before, after, "{name}: stack must be restored");
                assert_original(&table);
                assert_eq!(lua.load("return 42").eval::<i64>().unwrap(), 42);
            }
        }
    }
}

struct FreezeOnConversion(Table);
impl IntoLua for FreezeOnConversion {
    fn into_lua(self, _lua: &Lua) -> Result<Value> {
        self.0.set_readonly(true);
        Ok(Value::Integer(99))
    }
}

#[test]
fn native_readonly_preflight_follows_user_value_conversion() {
    let lua = vm(0);
    let table = target(&lua, false);
    let writes: &[fn(&Table) -> Result<()>] = &[
        |t: &Table| t.raw_set("original", FreezeOnConversion(t.clone())),
        |t: &Table| t.raw_seti(1, FreezeOnConversion(t.clone())),
        |t: &Table| t.raw_push(FreezeOnConversion(t.clone())),
    ];
    for write in writes {
        table.set_readonly(false);
        assert_readonly(write(&table).unwrap_err());
        assert_original(&table);
    }
}

#[test]
fn native_readonly_setlist_bytecode_checks_actual_target_before_writing() {
    let lua = vm(0);
    let table = target(&lua, false);
    lua.globals().set("protected", table.clone()).unwrap();
    let function = lua
        .load("local target=protected; local values={3,4}; return values,target")
        .into_function()
        .unwrap();
    let mut binary = function.dump(false);
    assert_eq!(&binary[..4], ffi::LUA_SIGNATURE);
    assert_eq!(binary[7] as usize, size_of::<i32>());
    assert_eq!(binary[8] as usize, size_of::<usize>());
    assert_eq!(binary[9] as usize, size_of::<u32>());
    let source_len = usize::from_ne_bytes(binary[12..12 + size_of::<usize>()].try_into().unwrap());
    let code_length_offset = 12 + size_of::<usize>() + source_len + 2 * size_of::<i32>() + 4;
    let count = i32::from_ne_bytes(
        binary[code_length_offset..code_length_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let code = code_length_offset + 4;
    let first = u32::from_ne_bytes(binary[code..code + 4].try_into().unwrap());
    assert_eq!(first & 63, 5, "GETGLOBAL loads protected into register zero");
    assert_eq!((first >> 6) & 255, 0);
    let mut replaced = 0;
    for index in 0..count {
        let offset = code + index * 4;
        let instruction = u32::from_ne_bytes(binary[offset..offset + 4].try_into().unwrap());
        if instruction & 63 == 10 {
            // OP_NEWTABLE -> OP_MOVE from the existing target.
            let target_register = (instruction >> 6) & 255;
            binary[offset..offset + 4].copy_from_slice(&(target_register << 6).to_ne_bytes());
            replaced += 1;
        }
    }
    assert_eq!(replaced, 1);
    let crafted = lua.load(binary).into_function().unwrap();
    let actual: Table = crafted.call(()).unwrap();
    assert_eq!(actual.to_pointer(), table.to_pointer());
    assert_eq!(table.raw_get::<i64>(1).unwrap(), 3);
    table.raw_set(1, 2).unwrap();
    table.raw_set(2, 1).unwrap();
    table.set_readonly(true);
    assert_readonly(crafted.call::<Value>(()).unwrap_err());
    assert_original(&table);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn native_readonly_keeps_host_bind_and_async_bytecode_caches_working() {
    let lua = vm(16 * 1024 * 1024);
    let table = target(&lua, false);
    lua.globals().set("protected", table).unwrap();
    let function: crate::Function = lua.load("return function(a,b) return a+b end").eval().unwrap();
    assert_eq!(function.bind(20).unwrap().call::<i64>(22).unwrap(), 42);
    lua.globals()
        .set(
            "host",
            lua.create_async_function(|_, value: i64| async move { Ok(value + 1) })
                .unwrap(),
        )
        .unwrap();
    lua.globals().set_readonly(true);
    for _ in 0..2 {
        assert_eq!(lua.load("return host(41)").eval_async::<i64>().await.unwrap(), 42);
    }
}

#[test]
fn raw_table_invalid_keys_keep_native_errors_and_stack_balance_with_any_memory_limit() {
    for limit in [0, 16 * 1024 * 1024] {
        for metatable in [false, true] {
            let lua = vm(limit);
            let table = target(&lua, metatable);
            for (key, expected) in [
                (Value::Nil, "table index is nil"),
                (Value::Number(f64::NAN), "table index is NaN"),
            ] {
                let operations: &[fn(&Table, Value) -> Result<()>] = &[
                    |t: &Table, key: Value| t.raw_set(key, 99),
                    |t: &Table, key: Value| t.set(key, 99),
                    |t: &Table, key: Value| t.raw_remove(key),
                ];
                for operation in operations {
                    let before = unsafe { ffi::lua_gettop(lua.lock().state()) };
                    let error = operation(&table, key.clone()).unwrap_err();
                    assert!(error.to_string().contains(expected), "{error}");
                    assert_eq!(unsafe { ffi::lua_gettop(lua.lock().state()) }, before);
                    assert_original(&table);
                    assert_eq!(lua.load("return 42").eval::<i64>().unwrap(), 42);
                }
            }
        }
    }
}

#[test]
fn table_constructor_invalid_keys_return_errors_and_drop_the_original_iterator() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Entries {
        values: std::vec::IntoIter<(Value, Value)>,
        dropped: Arc<AtomicUsize>,
    }
    impl Iterator for Entries {
        type Item = (Value, Value);
        fn next(&mut self) -> Option<Self::Item> {
            self.values.next()
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            self.values.size_hint()
        }
    }
    impl Drop for Entries {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    for limit in [0, 16 * 1024 * 1024] {
        let lua = vm(limit);
        let table = target(&lua, false);
        for (key, expected) in [
            (Value::Nil, "table index is nil"),
            (Value::Number(f64::NAN), "table index is NaN"),
        ] {
            let dropped = Arc::new(AtomicUsize::new(0));
            let entries = Entries {
                values: vec![(Value::Integer(1), Value::Integer(7)), (key, Value::Integer(99))].into_iter(),
                dropped: Arc::clone(&dropped),
            };
            let before = unsafe { ffi::lua_gettop(lua.lock().state()) };
            let error = lua.create_table_from(entries).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(dropped.load(Ordering::Relaxed), 1);
            assert_eq!(unsafe { ffi::lua_gettop(lua.lock().state()) }, before);
            assert_original(&table);
            assert_eq!(lua.load("return 42").eval::<i64>().unwrap(), 42);
        }
    }
}
