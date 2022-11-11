use std::env;
extern crate cbindgen;

trait AddPkg {
    fn add_pkg_config(&mut self, pkg: pkg_config::Library) -> &mut Self;
}
impl AddPkg for cc::Build {
    fn add_pkg_config(&mut self, pkg: pkg_config::Library) -> &mut Self {
        for p in pkg.include_paths.into_iter() {
            self.flag("-isystem").flag(p.to_str().unwrap());
        }
        for p in pkg.link_paths.into_iter() {
            self.flag(&format!("-L{:?}", p));
        }
        for p in pkg.libs.into_iter() {
            self.flag(&format!("-l{}", p));
        }
        for p in pkg.framework_paths.into_iter() {
            self.flag(&format!("-F{:?}", p));
        }
        for p in pkg.frameworks.into_iter() {
            self.flag(&format!("-framework {}", p));
        }
        self
    }
}

fn main() {
    #[cfg(test)]
    {
        return;
    }

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("nix_otel_plugin.h");

    println!("cargo:rerun-if-changed=plugin.cpp");
    let nix_expr = pkg_config::Config::new()
        .atleast_version("2.1.1")
        .probe("nix-expr")
        .unwrap();
    let nix_store = pkg_config::Config::new()
        .atleast_version("2.1.1")
        .probe("nix-store")
        .unwrap();
    let nix_main = pkg_config::Config::new()
        .atleast_version("2.1.1")
        .probe("nix-main")
        .unwrap();

    let nix_ver = nix_expr.version.clone();

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .opt_level(2)
        .shared_flag(true)
        .flag("-std=c++17")
        .add_pkg_config(nix_expr)
        .add_pkg_config(nix_store)
        .add_pkg_config(nix_main)
        .cargo_metadata(false)
        .file("plugin.cpp");

    // HACK: For some reason, rustc doesn't link libc++ on macOS by itself even
    // though cc-rs has been told cpp(true). So we force it.
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=c++");
    }

    let mut parts = nix_ver.split('.').map(str::parse);
    let major: u32 = parts.next().unwrap().unwrap();
    let minor = parts.next().unwrap().unwrap();

    // Indicate that we need to patch around an API change with macros
    if (major, minor) >= (2, 4) {
        build.define("NIX_2_4_0", None);
    }
    if (major, minor) >= (2, 6) {
        build.define("NIX_2_6_0", None);
    }
    if (major, minor) >= (2, 9) {
        build.define("NIX_2_9_0", None);
    }

    println!("cargo:rustc-link-lib=static:+whole-archive=nix_otel_plugin");
    println!(
        "cargo:rustc-link-search=native={}",
        env::var("OUT_DIR").unwrap()
    );

    build.compile("nix_otel_plugin");
}
