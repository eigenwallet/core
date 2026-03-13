 - Read CONTRIBUTING.md and the code style guidelines!
 - When asked about libp2p, check if a rust-libp2p folder exists which contains the cloned rust libp2p codebase. Read through to figure out what the best response it. If its a question about best practice when implementing protocols read @rust-libp2p/protocols/ specificially.
 - Never do `cargo clean`. Building `monero-sys` takes ages, and cleaning the build cache will cause a full rebuilt (horrible).
   `cargo clean` has never fixed a build problem.
 - Before suggesting a change, always give at least a short (1 sentence) summary of which function you are editing and why.
 - When being asked to add something, check whether there is a similar thing already implemented, the architecture of which you can follow.
   For example, when asked to add a new Tauri command, check out how other tauri commands are implemented and what conventions they follow.
 - 

 - Think about seperation of concerns. This has many facets. But the most ofen there are questions like:
   "Which part of the code should decide how to handle this situation". In the context of an error, the solution is:
   - Never use fallback values. They lead to
     - swallowed errors
     - breaking invariances
     - breaking other implicit assumptions
     - destroy any meaning the value might have had.
     Instead, if an error/invlaid state is encountered, the error should be propagated.
     This is most often correctly done by using anyhow's "Context" and the question mark operator`.context("Failed to <foo>")?`.
   - Keep error handling simple: it is basically never wrong to just propagate the error using `?` and maybe add some basic context.

  Other facetts of seperation of concern include:
   - should this function need to have access to this <implementation detail>?
   - should this function decide a parameter itself or just take an argument?

  We follow the principle of LEAST SURPRISE. Take a step back, and come back with a fresh view. Then ask yourself: "would I expect this function to do <X>?".
  If not, then don't do it. 

- coding style tips:
  - keep the code succint. Prefer `if let` and `let ... else` to `match` whenever possible.
  - avoid nesting if possible.
  - prefer early returns to nesting.

- Docker tests: We have an extended test suite that simulates a whole blockchain environment for the purpose of testing swaps end to end.
  The docker tests are located in `swap/tests` and can be executed using `just docker_test <name>`. Get a list of all docker tests by `just list-docker-tests`.
- If you changed something could possibly affect the success of a swap, make sure to run the integration tests that could possibly be affected.
  Be very liberal with assuming what might be affected.
- If not explicitly instructed yet, ask the user whether you should add {unit, integration} tests if you just added / changed some behaviour/code
- The docker tests are long (multiple minutes) and produce tens of thousands of log messages.
  Don't try to read all of that output, it will fill you context up before finishing
  the initialization.
  Instead, spawn them as a background-task (each as it's own).
  Then you can simply check in on the current status by checking it's output every minute or so.
  If you are claude, use claude codes native background task system and read from the `/tmp/claude/tasks/foo/output.tmp` pipe file, or whatever the path is.
  If you are not claude, then do the thing that best accomplishises this.

- Before claiming you finished, make sure everything compiles (`cargo c --all-features`).
  Also all tests (`cargo c --tests`) and all targets (`cargo c --all-targets`) must compile.


  
