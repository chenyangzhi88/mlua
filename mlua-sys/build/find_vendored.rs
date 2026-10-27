#![allow(dead_code)]

pub fn probe_lua() {
    #[cfg(feature = "lua55")]
    let artifacts = lua_src::Build::new().build(lua_src::Lua55);

    #[cfg(feature = "lua54")]
    let artifacts = lua_src::Build::new().build(lua_src::Lua54);

    #[cfg(feature = "lua53")]
    let artifacts = lua_src::Build::new().build(lua_src::Lua53);

    #[cfg(feature = "lua52")]
    let artifacts = lua_src::Build::new().build(lua_src::Lua52);

    #[cfg(feature = "lua51")]
    build_lua51();

    #[cfg(feature = "luajit")]
    let artifacts = luajit_src::Build::new()
        .lua52compat(cfg!(feature = "luajit52"))
        .build();

    #[cfg(feature = "luau")]
    let artifacts = luau0_src::Build::new()
        .enable_codegen(cfg!(feature = "luau-codegen"))
        .set_max_cstack_size(1000000)
        .set_vector_size(if cfg!(feature = "luau-vector4") { 4 } else { 3 })
        .build();

    #[cfg(not(feature = "lua51"))]
    artifacts.print_cargo_metadata();
}

// The product's Lua51 backend keeps the existing 5.1.5 inputs and build flags,
// with Redis' native readonly checks. Other backends use their upstream builder.
#[cfg(feature = "lua51")]
fn build_lua51() {
    use std::path::PathBuf;
    use std::{env, fs};

    const SOURCES: &[&str] = &[
        "lapi.c",
        "lauxlib.c",
        "lbaselib.c",
        "lcode.c",
        "ldblib.c",
        "ldebug.c",
        "ldo.c",
        "ldump.c",
        "lfunc.c",
        "lgc.c",
        "linit.c",
        "liolib.c",
        "llex.c",
        "lmathlib.c",
        "lmem.c",
        "loadlib.c",
        "lobject.c",
        "lopcodes.c",
        "loslib.c",
        "lparser.c",
        "lstate.c",
        "lstring.c",
        "lstrlib.c",
        "ltable.c",
        "ltablib.c",
        "ltm.c",
        "lundump.c",
        "lvm.c",
        "lzio.c",
    ];
    let target = env::var("TARGET").expect("Cargo sets TARGET");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets manifest dir"));
    let mut source = manifest.join("vendor/lua51");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let lib = output.join("lib");
    let include = output.join("include");
    println!("cargo:rerun-if-changed={}", source.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("vendor/lua51.sources.json").display()
    );

    let mut compiler = cc::Build::new();
    compiler.warnings(false).cargo_metadata(false).target(&target);
    let mut extra_libs = Vec::new();
    match target.as_str() {
        target if target.contains("linux") || target.ends_with("bsd") || target.contains("apple-darwin") => {
            compiler.define("LUA_USE_LINUX", None);
        }
        target if target.contains("apple-ios") => {
            compiler.define("LUA_USE_POSIX", None);
        }
        target if target.contains("windows") => {
            compiler.define("LUA_USE_WINDOWS", None);
        }
        target if target.ends_with("emscripten") => {
            compiler
                .define("LUA_USE_POSIX", None)
                .cpp(true)
                .flag("-fexceptions");
            let cpp_source = output.join("cpp_source");
            fs::create_dir_all(&cpp_source).expect("create Lua51 C++ source directory");
            for entry in fs::read_dir(&source).expect("read controlled Lua51 sources") {
                let entry = entry.expect("read Lua51 source entry");
                let name = entry.file_name();
                let mut contents = fs::read(entry.path()).expect("read Lua51 source");
                if name
                    .to_str()
                    .is_some_and(|name| ["lauxlib.h", "lua.h", "lualib.h"].contains(&name))
                {
                    contents.splice(0..0, b"extern \"C\" {\n".to_vec());
                    contents.extend(b"\n}".iter());
                }
                fs::write(cpp_source.join(name), contents).expect("write Lua51 C++ source");
            }
            source = cpp_source;
        }
        target if target.contains("wasi") => {
            compiler
                .define("LUA_USE_POSIX", None)
                .define("_WASI_EMULATED_SIGNAL", None);
            compiler
                .flag("-mllvm")
                .flag("-wasm-enable-eh")
                .flag("-mllvm")
                .flag("-wasm-use-legacy-eh=false")
                .flag("-mllvm")
                .flag("-wasm-enable-sjlj");
            extra_libs.extend(["wasi-emulated-signal", "setjmp"]);
        }
        _ => panic!("don't know how to build Lua for {target}"),
    }
    if cfg!(debug_assertions) {
        compiler.define("LUA_USE_APICHECK", None).debug(true);
    }
    compiler
        .include(&source)
        .flag_if_supported("-fno-common")
        .out_dir(&lib);
    for filename in SOURCES {
        compiler.file(source.join(filename));
    }
    compiler.compile("lua5.1");
    fs::create_dir_all(&include).expect("create Lua51 include directory");
    for filename in ["lauxlib.h", "lua.h", "luaconf.h", "lualib.h"] {
        fs::copy(source.join(filename), include.join(filename)).expect("copy Lua51 public header");
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static:-bundle=lua5.1");
    for library in extra_libs {
        println!("cargo:rustc-link-lib=static:-bundle={library}");
    }
}
