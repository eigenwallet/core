# Contribution guidelines

Thank you for wanting to contribute to this project!

## Contributing code

There are a couple of things we are going to look out for in PRs and knowing them upfront is going to reduce the number of times we will be going back and forth, making things more efficient.

0. **Read and comply with our [AI Policy](AI_POLICY.md)**
1. We have CI checks in place that validate formatting and code style.
   Make sure the branch is building with `--all-features` and `--all-targets` without errors
   and all tests are passed.
2. All text document (`CHANGELOG.md`, `README.md`, etc) should follow the [semantic linebreaks](https://sembr.org/) specification.
3. We strive for atomic commits with good commit messages.
   As an inspiration, read [this](https://chris.beams.io/posts/git-commit/) blogpost.
   An atomic commit is a cohesive diff with formatting checks, linter and build passing.
   Ideally, all tests are passing as well but we acknowledge that this is not always possible depending on the change you are making.
4. If you are making any user visible changes, include a changelog entry.

## Contributing issues

When contributing a feature request, please focus on your _problem_ as much as possible.
It is okay to include ideas on how the feature should be implemented but they should be 2nd nature of your request.

## Code style

### General

 - File structure
   - The content of each file should be ordered in terms of importance / level of abstraction
   - Public `struct`s, `enum`s and important constants should be at the top
   - `impl` blocks should be below the type declarations
   - Both the type declaration part and the implementation part of the file should be internally ordered by level of abstraction/ importance
   - For example, `fn main` should always be at least at the top of the implementation 
 - Prefer early returns over nested `if`/`match` statements
 - Don't use fallback values or silent failures

### Rust

 - Use `cargo fmt` for formatting
 - Make use of the powerful `if let` and `let ... else` pattern to enable early returns
 - Make use of anyhows `.context` method and the `?` operator for concise error reporting

