#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (Vec<(&str, u8)>, Vec<&str>)| {
    let (tree, paths) = data;

    let mut matcher = matchit::Router::new();

    for (key, item) in tree {
        let _ = matcher.insert(key, item);
    }

    for path in paths {
        if let Ok(m) = matcher.at(&path) {
            let _ = m.params.len();
            let _ = m.params.get("x");
            let _ = m.params.get("y");
        }
        let _ = matcher.fix_path(&path);
    }
});
