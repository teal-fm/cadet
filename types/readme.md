## Types
Rust lexicons for teal.fm and others.

### Generate lexicons
You will need to download, modify, and build the 'lexgen' tool from https://github.com/sugyan/atrium.

- Clone the repository: `git clone https://github.com/sugyan/atrium.git`
- Modify the `lexgen` tool
  - Add a new namespace (fm.teal)
```rs
  let results = genapi(
          &args.lexdir,
          &args.outdir,
          &[
              ("com.atproto", None),
              ("app.bsky", Some("namespace-appbsky")),
              ("chat.bsky", Some("namespace-chatbsky")),
              ("tools.ozone", Some("namespace-toolsozone")),
+              ("fm.teal", Some("namespace-fmteal")), // add this line
            ],
      )?;
```
- Build the `lexgen` tool by cding into `atrium/lexicons/lexgen` and using `cargo build --release`
- Optionally rename (by default, the tool will be located in `atrium/lexicons/lexgen/target/release` and named `main`. I like to name it `lexgen-rs`.)
