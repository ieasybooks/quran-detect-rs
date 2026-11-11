//! The repository README's Rust examples are run as doctests of the library README.

fn rust_blocks(markdown: &str) -> Vec<&str> {
    markdown
        .split("```rust\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect()
}

#[test]
fn repository_readme_examples_are_tested_with_the_library() {
    let repository = include_str!("../README.md");
    let library = include_str!("../crates/quran-detect/README.md");
    let blocks = rust_blocks(repository);
    assert_ne!(blocks.len(), 0);
    for block in blocks {
        assert!(library.contains(block), "untested README example:\n{block}");
    }
}
