/// Embeds what a running process cannot know from itself: which build it is.
///
/// `BUILD_NUMBER` is the Dockerfile argument release CI sets to the GitHub
/// run number, and `cargo:rustc-env` is the only channel whose change
/// rebuilds the crate — `option_env!` alone would freeze whatever value the
/// first cache-warm build had. Outside CI the variable is absent, which is
/// honest: the process then reports `local`.
fn main() {
    println!("cargo:rerun-if-env-changed=BUILD_NUMBER");
    println!("cargo:rerun-if-changed=build.rs");
    let build = std::env::var("BUILD_NUMBER").unwrap_or_else(|_| "local".to_owned());
    println!("cargo:rustc-env=TUCANO_BUILD={build}");
    let rustc =
        std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg("--version")
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=TUCANO_TOOLCHAIN={rustc}");
}
