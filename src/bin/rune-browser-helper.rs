//! Chromium's helper process (renderer, GPU, utility). Bundled as "Rune Browser Helper*.app".
use cef::{args::Args, *};

fn main() {
    let args = Args::new();
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };
    let loader = library_loader::LibraryLoader::new(&std::env::current_exe().expect("helper path"), true);
    assert!(loader.load(), "Chromium Embedded Framework failed to load");
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    execute_process(Some(args.as_main_args()), None::<&mut App>, std::ptr::null_mut());
}
