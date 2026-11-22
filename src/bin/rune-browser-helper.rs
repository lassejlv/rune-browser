//! Chromium's helper process (renderer, GPU, utility). Bundled as "Rune Browser Helper*.app" on
//! macOS; beside the app on Linux.
use cef::{args::Args, *};

fn main() {
    let args = Args::new();
    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };
    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().expect("helper path"), true);
        assert!(loader.load(), "Chromium Embedded Framework failed to load");
        loader
    };
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    std::process::exit(execute_process(Some(args.as_main_args()), None::<&mut App>, std::ptr::null_mut()));
}
